//! Task-free reachability generation and status transitions.
//!
//! Network ownership lives in the coordinator added by the mapping runtime.
//! This module keeps eligibility and stale-result fencing deterministic.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rstorrent_engine::port_mapping::upnp::{
    UpnpDiscoveryConfig, UpnpError, UpnpMapping, UpnpStage, discover_igd_v2,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::advertised_endpoint::AdvertisedPeerEndpointSelector;
use crate::diagnostics::{DiagnosticSeverity, category};
use crate::settings::{
    ClientSettings, ListenerPolicy, ListenerStatus, PortMappingFailureStage, PortMappingMechanism,
    PortMappingPolicy, PortMappingStatus, SettingsDomainGeneration,
};
use crate::views::ViewHub;

const DELETE_TIMEOUT: Duration = Duration::from_secs(5);
const RENEWAL_RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReachabilityEvent {
    Discovering,
    Mapping,
    Mapped {
        external_address: Ipv4Addr,
        external_port: u16,
        lease_seconds: u32,
    },
    Failed {
        stage: PortMappingFailureStage,
        detail: String,
    },
    RenewalFailed {
        external_address: Ipv4Addr,
        external_port: u16,
        detail: String,
    },
    Stopping,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ReachabilityOwnerCounts {
    pub tasks: usize,
    pub mappings: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UncertainMappingLease {
    pub external_address: Ipv4Addr,
    pub external_port: u16,
    pub expires_at: Instant,
    pub detail: String,
}

impl UncertainMappingLease {
    pub(crate) fn remaining_lease_seconds(&self, now: Instant) -> u32 {
        let remaining = self.expires_at.saturating_duration_since(now);
        if remaining.is_zero() {
            return 0;
        }
        u32::try_from(remaining.as_secs().saturating_add(1)).unwrap_or(u32::MAX)
    }
}

#[derive(Debug, Default)]
struct ReachabilityRunOutcome {
    uncertain_mapping: Option<UncertainMappingLease>,
    error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ReachabilityGenerationShutdown {
    pub terminal: ReachabilityOwnerCounts,
    pub uncertain_mapping: Option<UncertainMappingLease>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct ReachabilityCounters {
    tasks: AtomicUsize,
    mappings: AtomicUsize,
}

#[derive(Debug)]
struct MappingTaskContext {
    views: ViewHub,
    endpoint_selector: AdvertisedPeerEndpointSelector,
    cancellation: CancellationToken,
    counters: Arc<ReachabilityCounters>,
    settings_generation: SettingsDomainGeneration,
    discovery_config: Option<UpnpDiscoveryConfig>,
}

#[derive(Debug)]
pub(crate) struct ReachabilityCoordinator {
    cancellation: CancellationToken,
    task: Option<JoinHandle<ReachabilityRunOutcome>>,
    counters: Arc<ReachabilityCounters>,
    endpoint_selector: AdvertisedPeerEndpointSelector,
}

impl ReachabilityCoordinator {
    pub(crate) fn start(
        settings: &ClientSettings,
        listener_status: &ListenerStatus,
        views: ViewHub,
        endpoint_selector: AdvertisedPeerEndpointSelector,
        settings_generation: SettingsDomainGeneration,
    ) -> Self {
        let generation = endpoint_selector.begin_mapping_generation();
        let state = ReachabilityState::new(generation, settings, listener_status);
        let _ = views.set_port_mapping_status_for(settings_generation, state.status().clone());
        let cancellation = CancellationToken::new();
        let counters = Arc::new(ReachabilityCounters::default());
        let task = state.local_endpoint().map(|local_endpoint| {
            counters.tasks.store(1, Ordering::Release);
            let task_cancellation = cancellation.clone();
            let task_counters = counters.clone();
            let task_endpoint_selector = endpoint_selector.clone();
            let task_views = views.clone();
            tokio::spawn(async move {
                let result = run_mapping(
                    state,
                    local_endpoint,
                    MappingTaskContext {
                        views: task_views,
                        endpoint_selector: task_endpoint_selector,
                        cancellation: task_cancellation,
                        counters: task_counters.clone(),
                        settings_generation,
                        discovery_config: None,
                    },
                )
                .await;
                task_counters.tasks.store(0, Ordering::Release);
                result
            })
        });
        Self {
            cancellation,
            task,
            counters,
            endpoint_selector,
        }
    }

    pub(crate) fn owner_counts(&self) -> ReachabilityOwnerCounts {
        ReachabilityOwnerCounts {
            tasks: self.counters.tasks.load(Ordering::Acquire),
            mappings: self.counters.mappings.load(Ordering::Acquire),
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.task.as_ref().is_some_and(JoinHandle::is_finished)
    }

    pub(crate) async fn shutdown(mut self) -> Result<ReachabilityOwnerCounts, String> {
        let shutdown = self.shutdown_inner(true).await?;
        let mut errors = shutdown.error.into_iter().collect::<Vec<_>>();
        if let Some(uncertain) = shutdown.uncertain_mapping {
            errors.push(format!(
                "{}; external endpoint {}:{} may remain for {} seconds",
                uncertain.detail,
                uncertain.external_address,
                uncertain.external_port,
                uncertain.remaining_lease_seconds(Instant::now()),
            ));
        }
        if errors.is_empty() {
            Ok(shutdown.terminal)
        } else {
            Err(errors.join("; "))
        }
    }

    pub(crate) async fn shutdown_generation(
        mut self,
    ) -> Result<ReachabilityGenerationShutdown, String> {
        self.shutdown_inner(false).await
    }

    async fn shutdown_inner(
        &mut self,
        final_application_shutdown: bool,
    ) -> Result<ReachabilityGenerationShutdown, String> {
        if final_application_shutdown {
            self.endpoint_selector.stop();
        }
        self.cancellation.cancel();
        let outcome = if let Some(task) = self.task.take() {
            match task.await {
                Ok(outcome) => outcome,
                Err(error) if error.is_cancelled() => ReachabilityRunOutcome::default(),
                Err(error) => return Err(format!("reachability task join: {error}")),
            }
        } else {
            ReachabilityRunOutcome::default()
        };
        let terminal = self.owner_counts();
        if terminal != ReachabilityOwnerCounts::default() {
            return Err(format!(
                "reachability owners remain: tasks={},mappings={}",
                terminal.tasks, terminal.mappings
            ));
        }
        Ok(ReachabilityGenerationShutdown {
            terminal,
            uncertain_mapping: outcome.uncertain_mapping,
            error: outcome.error,
        })
    }
}

impl Drop for ReachabilityCoordinator {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

async fn run_mapping(
    mut state: ReachabilityState,
    local_endpoint: SocketAddrV4,
    context: MappingTaskContext,
) -> ReachabilityRunOutcome {
    let MappingTaskContext {
        views,
        endpoint_selector,
        cancellation,
        counters,
        settings_generation,
        discovery_config,
    } = context;
    if let Err(error) = publish(
        &mut state,
        &views,
        settings_generation,
        ReachabilityEvent::Discovering,
    ) {
        return ReachabilityRunOutcome {
            error: Some(error),
            ..ReachabilityRunOutcome::default()
        };
    }
    let config = match discovery_config
        .map(Ok)
        .unwrap_or_else(|| UpnpDiscoveryConfig::new(*local_endpoint.ip()))
    {
        Ok(config) => config,
        Err(error) => {
            return ReachabilityRunOutcome {
                error: publish_failure(&mut state, &views, settings_generation, error).err(),
                ..ReachabilityRunOutcome::default()
            };
        }
    };
    let gateway = match discover_igd_v2(config, &cancellation).await {
        Ok(gateway) => gateway,
        Err(error) => {
            return ReachabilityRunOutcome {
                error: publish_failure(&mut state, &views, settings_generation, error).err(),
                ..ReachabilityRunOutcome::default()
            };
        }
    };
    if let Err(error) = publish(
        &mut state,
        &views,
        settings_generation,
        ReachabilityEvent::Mapping,
    ) {
        return ReachabilityRunOutcome {
            error: Some(error),
            ..ReachabilityRunOutcome::default()
        };
    }
    let mut mapping = match gateway
        .create_mapping(local_endpoint.port(), &cancellation)
        .await
    {
        Ok(mapping) => mapping,
        Err(error) => {
            return ReachabilityRunOutcome {
                error: publish_failure(&mut state, &views, settings_generation, error).err(),
                ..ReachabilityRunOutcome::default()
            };
        }
    };
    counters.mappings.store(1, Ordering::Release);
    let mut run_error = publish_mapped(
        &mut state,
        &views,
        &endpoint_selector,
        &mapping,
        settings_generation,
    )
    .err();
    let mut lease_deadline = Instant::now()
        .checked_add(Duration::from_secs(u64::from(mapping.lease_seconds)))
        .unwrap_or_else(|| {
            run_error = Some("UPnP mapping lease deadline overflow".to_owned());
            Instant::now()
        });
    let mut renewal_delay = mapping.renewal_delay();
    while run_error.is_none() {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            _ = tokio::time::sleep(renewal_delay) => {
                if endpoint_selector.expire(Instant::now())
                    && let Err(error) = publish_selected_endpoint(&endpoint_selector, &views)
                {
                    run_error = Some(error);
                    continue;
                }
                match gateway.renew_mapping(&mut mapping, &cancellation).await {
                    Ok(()) => {
                        if let Err(error) = publish_mapped(
                            &mut state,
                            &views,
                            &endpoint_selector,
                            &mapping,
                            settings_generation,
                        ) {
                            run_error = Some(error);
                            continue;
                        }
                        let Some(deadline) = Instant::now()
                            .checked_add(Duration::from_secs(u64::from(mapping.lease_seconds)))
                        else {
                            run_error = Some("UPnP mapping lease deadline overflow".to_owned());
                            continue;
                        };
                        lease_deadline = deadline;
                        renewal_delay = mapping.renewal_delay();
                    }
                    Err(error) => {
                        if let Err(publish_error) = publish(
                            &mut state,
                            &views,
                            settings_generation,
                            ReachabilityEvent::RenewalFailed {
                                external_address: mapping.external_address,
                                external_port: mapping.external_port,
                                detail: error.detail().to_owned(),
                            },
                        ) {
                            run_error = Some(publish_error);
                            continue;
                        }
                        if endpoint_selector.renewal_failed(
                            state.generation(),
                            error.detail().to_owned(),
                            Instant::now(),
                        ) && let Err(publish_error) =
                            publish_selected_endpoint(&endpoint_selector, &views)
                        {
                            run_error = Some(publish_error);
                            continue;
                        }
                        record_failure_diagnostic(&views, &error);
                        let remaining =
                            lease_deadline.saturating_duration_since(Instant::now());
                        renewal_delay = if remaining.is_zero() {
                            RENEWAL_RETRY_DELAY
                        } else {
                            RENEWAL_RETRY_DELAY.min(remaining)
                        };
                    }
                }
            }
        }
    }
    if let Err(error) = publish(
        &mut state,
        &views,
        settings_generation,
        ReachabilityEvent::Stopping,
    ) && run_error.is_none()
    {
        run_error = Some(error);
    }
    let cleanup_cancellation = CancellationToken::new();
    let delete = tokio::time::timeout(
        DELETE_TIMEOUT,
        gateway.delete_mapping(&mapping, &cleanup_cancellation),
    )
    .await;
    counters.mappings.store(0, Ordering::Release);
    let uncertain_mapping = match delete {
        Ok(Ok(())) => None,
        Ok(Err(error)) => {
            record_failure_diagnostic(&views, &error);
            Some(UncertainMappingLease {
                external_address: mapping.external_address,
                external_port: mapping.external_port,
                expires_at: lease_deadline,
                detail: format!("delete UPnP mapping: {}", error.detail()),
            })
        }
        Err(_) => Some(UncertainMappingLease {
            external_address: mapping.external_address,
            external_port: mapping.external_port,
            expires_at: lease_deadline,
            detail: "delete UPnP mapping: cleanup deadline elapsed".to_owned(),
        }),
    };
    ReachabilityRunOutcome {
        uncertain_mapping,
        error: run_error,
    }
}

fn publish_mapped(
    state: &mut ReachabilityState,
    views: &ViewHub,
    endpoint_selector: &AdvertisedPeerEndpointSelector,
    mapping: &UpnpMapping,
    settings_generation: SettingsDomainGeneration,
) -> Result<(), String> {
    let current = publish(
        state,
        views,
        settings_generation,
        ReachabilityEvent::Mapped {
            external_address: mapping.external_address,
            external_port: mapping.external_port,
            lease_seconds: mapping.lease_seconds,
        },
    )?;
    if !current {
        return Ok(());
    }
    endpoint_selector.mapping_verified(
        state.generation(),
        SocketAddrV4::new(mapping.external_address, mapping.external_port),
        Duration::from_secs(u64::from(mapping.lease_seconds)),
        Instant::now(),
    );
    publish_selected_endpoint(endpoint_selector, views)
}

fn publish_failure(
    state: &mut ReachabilityState,
    views: &ViewHub,
    settings_generation: SettingsDomainGeneration,
    error: UpnpError,
) -> Result<(), String> {
    record_failure_diagnostic(views, &error);
    publish(
        state,
        views,
        settings_generation,
        ReachabilityEvent::Failed {
            stage: failure_stage(error.stage()),
            detail: error.detail().to_owned(),
        },
    )
    .map(|_| ())
}

fn publish(
    state: &mut ReachabilityState,
    views: &ViewHub,
    settings_generation: SettingsDomainGeneration,
    event: ReachabilityEvent,
) -> Result<bool, String> {
    if state.apply(state.generation(), event) {
        return views
            .set_port_mapping_status_for(settings_generation, state.status().clone())
            .map_err(|error| error.to_string());
    }
    Ok(true)
}

fn publish_selected_endpoint(
    endpoint_selector: &AdvertisedPeerEndpointSelector,
    views: &ViewHub,
) -> Result<(), String> {
    views
        .set_advertised_peer_endpoint(endpoint_selector.status(Instant::now()))
        .map_err(|error| error.to_string())
}

fn record_failure_diagnostic(views: &ViewHub, error: &UpnpError) {
    let stage = failure_stage_name(error.stage());
    let _ = views.record_diagnostic(
        DiagnosticSeverity::Warning,
        category::DISCOVERY_REACHABILITY,
        "upnp_mapping_failed",
        None,
        "Automatic incoming TCP mapping failed; the local listener remains available",
        &[("stage", stage), ("detail", error.detail())],
    );
}

fn failure_stage(stage: UpnpStage) -> PortMappingFailureStage {
    match stage {
        UpnpStage::Discovery => PortMappingFailureStage::Discovery,
        UpnpStage::Description => PortMappingFailureStage::Description,
        UpnpStage::ExternalAddress => PortMappingFailureStage::ExternalAddress,
        UpnpStage::Add => PortMappingFailureStage::Add,
        UpnpStage::Verify => PortMappingFailureStage::Verify,
        UpnpStage::Renewal => PortMappingFailureStage::Renewal,
        UpnpStage::Delete => PortMappingFailureStage::Delete,
    }
}

fn failure_stage_name(stage: UpnpStage) -> &'static str {
    match stage {
        UpnpStage::Discovery => "discovery",
        UpnpStage::Description => "description",
        UpnpStage::ExternalAddress => "external_address",
        UpnpStage::Add => "add",
        UpnpStage::Verify => "verify",
        UpnpStage::Renewal => "renewal",
        UpnpStage::Delete => "delete",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReachabilityState {
    generation: u64,
    local_endpoint: Option<SocketAddrV4>,
    status: PortMappingStatus,
    stopping: bool,
}

impl ReachabilityState {
    pub(crate) fn new(
        generation: u64,
        settings: &ClientSettings,
        listener_status: &ListenerStatus,
    ) -> Self {
        let local_endpoint = eligible_local_endpoint(settings, listener_status);
        let status = if settings.port_mapping == PortMappingPolicy::Disabled {
            PortMappingStatus::Disabled
        } else if local_endpoint.is_some() {
            PortMappingStatus::Discovering
        } else {
            PortMappingStatus::Ineligible
        };
        Self {
            generation,
            local_endpoint,
            status,
            stopping: false,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn local_endpoint(&self) -> Option<SocketAddrV4> {
        self.local_endpoint
    }

    pub(crate) fn status(&self) -> &PortMappingStatus {
        &self.status
    }

    pub(crate) fn apply(&mut self, generation: u64, event: ReachabilityEvent) -> bool {
        if generation != self.generation || self.stopping {
            return false;
        }
        let Some(local_endpoint) = self.local_endpoint else {
            return false;
        };
        let next = match event {
            ReachabilityEvent::Discovering => PortMappingStatus::Discovering,
            ReachabilityEvent::Mapping if matches!(self.status, PortMappingStatus::Discovering) => {
                PortMappingStatus::Mapping
            }
            ReachabilityEvent::Mapped {
                external_address,
                external_port,
                lease_seconds,
            } if matches!(
                self.status,
                PortMappingStatus::Mapping
                    | PortMappingStatus::Mapped { .. }
                    | PortMappingStatus::RenewalFailed { .. }
            ) =>
            {
                PortMappingStatus::Mapped {
                    mechanism: PortMappingMechanism::UpnpIgdV2,
                    local_address: local_endpoint.ip().to_string(),
                    local_port: local_endpoint.port(),
                    external_address: external_address.to_string(),
                    external_port,
                    lease_seconds,
                }
            }
            ReachabilityEvent::Failed { stage, detail } => {
                PortMappingStatus::Failed { stage, detail }
            }
            ReachabilityEvent::RenewalFailed {
                external_address,
                external_port,
                detail,
            } if matches!(self.status, PortMappingStatus::Mapped { .. }) => {
                PortMappingStatus::RenewalFailed {
                    external_address: external_address.to_string(),
                    external_port,
                    detail,
                }
            }
            ReachabilityEvent::Stopping => {
                self.stopping = true;
                PortMappingStatus::Stopping
            }
            _ => return false,
        };
        if self.status == next {
            return false;
        }
        self.status = next;
        true
    }
}

fn eligible_local_endpoint(
    settings: &ClientSettings,
    listener_status: &ListenerStatus,
) -> Option<SocketAddrV4> {
    if settings.port_mapping != PortMappingPolicy::Upnp
        || !matches!(
            settings.listener,
            ListenerPolicy::AutomaticLocalNetwork | ListenerPolicy::FixedLocalNetwork { .. }
        )
    {
        return None;
    }
    let ListenerStatus::Listening { address, port } = listener_status else {
        return None;
    };
    let address = address.parse::<Ipv4Addr>().ok()?;
    if *port == 0
        || address.is_unspecified()
        || address.is_loopback()
        || address.is_multicast()
        || address == Ipv4Addr::BROADCAST
    {
        return None;
    }
    Some(SocketAddrV4::new(address, *port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{SocketAddr, SocketAddrV4};
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};

    use crate::advertised_endpoint::AdvertisedPeerEndpointState;
    use crate::control::ServiceSnapshot;
    use crate::settings::{SettingsConvergenceModel, SettingsDomain};

    fn eligible_settings() -> ClientSettings {
        ClientSettings {
            listener: ListenerPolicy::AutomaticLocalNetwork,
            port_mapping: PortMappingPolicy::Upnp,
            ..ClientSettings::default()
        }
    }

    fn listening() -> ListenerStatus {
        ListenerStatus::Listening {
            address: "192.168.50.12".to_owned(),
            port: 41_234,
        }
    }

    #[test]
    fn policy_and_observed_listener_jointly_determine_eligibility() {
        let disabled = ReachabilityState::new(1, &ClientSettings::default(), &listening());
        assert_eq!(disabled.status(), &PortMappingStatus::Disabled);

        let loopback = ClientSettings {
            listener: ListenerPolicy::AutomaticLoopback,
            port_mapping: PortMappingPolicy::Upnp,
            ..ClientSettings::default()
        };
        assert_eq!(
            ReachabilityState::new(2, &loopback, &listening()).status(),
            &PortMappingStatus::Ineligible
        );
        assert_eq!(
            ReachabilityState::new(
                3,
                &eligible_settings(),
                &ListenerStatus::Listening {
                    address: "127.0.0.1".to_owned(),
                    port: 41_234,
                },
            )
            .status(),
            &PortMappingStatus::Ineligible
        );

        let eligible = ReachabilityState::new(4, &eligible_settings(), &listening());
        assert_eq!(eligible.status(), &PortMappingStatus::Discovering);
        assert_eq!(
            eligible.local_endpoint(),
            Some(SocketAddrV4::new(Ipv4Addr::new(192, 168, 50, 12), 41_234))
        );
    }

    #[test]
    fn transitions_are_ordered_and_stale_generations_are_fenced() {
        let mut state = ReachabilityState::new(7, &eligible_settings(), &listening());
        assert!(!state.apply(6, ReachabilityEvent::Mapping));
        assert!(!state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(7, ReachabilityEvent::Mapping));
        assert!(state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(
            7,
            ReachabilityEvent::RenewalFailed {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                detail: "gateway did not answer".to_owned(),
            },
        ));
        assert!(state.apply(
            7,
            ReachabilityEvent::Mapped {
                external_address: Ipv4Addr::new(203, 0, 113, 10),
                external_port: 48_001,
                lease_seconds: 3_600,
            },
        ));
        assert!(state.apply(7, ReachabilityEvent::Stopping));
        assert!(!state.apply(7, ReachabilityEvent::Discovering));
        assert_eq!(state.status(), &PortMappingStatus::Stopping);
        assert_eq!(state.generation(), 7);
    }

    #[test]
    fn uncertain_mapping_blocks_until_its_finite_lease_expires() {
        let now = Instant::now();
        let mapping = UncertainMappingLease {
            external_address: Ipv4Addr::new(203, 0, 113, 10),
            external_port: 48_001,
            expires_at: now + Duration::from_millis(1_500),
            detail: "delete verification failed".to_owned(),
        };
        assert_eq!(mapping.remaining_lease_seconds(now), 2);
        assert_eq!(
            mapping.remaining_lease_seconds(now + Duration::from_millis(1_499)),
            1,
        );
        assert_eq!(
            mapping.remaining_lease_seconds(now + Duration::from_millis(1_500)),
            0,
        );
    }

    #[tokio::test]
    async fn scripted_delete_failure_reports_one_finite_uncertain_lease() {
        let (config, transcript, udp_task, http_task) = scripted_delete_failure_gateway().await;
        let listener = ListenerStatus::Listening {
            address: Ipv4Addr::LOCALHOST.to_string(),
            port: 42_000,
        };
        let selector = AdvertisedPeerEndpointSelector::new(&listener);
        let mapping_generation = selector.begin_mapping_generation();
        let state = ReachabilityState {
            generation: mapping_generation,
            local_endpoint: Some(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42_000)),
            status: PortMappingStatus::Discovering,
            stopping: false,
        };
        let mut convergence = SettingsConvergenceModel::default();
        let attempt = convergence
            .begin(eligible_settings())
            .expect("settings attempt");
        let settings_generation = attempt.domain(SettingsDomain::PortMapping);
        let views = ViewHub::new(&ServiceSnapshot {
            profile_id: "scripted-reachability".to_owned(),
            revision: "0".to_owned(),
            storage: Default::default(),
            client_settings: eligible_settings(),
            torrents: Vec::new(),
        })
        .expect("scripted reachability views");
        views
            .set_client_settings_mapping_generation(settings_generation)
            .expect("install mapping generation");
        let cancellation = CancellationToken::new();
        let counters = Arc::new(ReachabilityCounters::default());
        let mapping_task = tokio::spawn(run_mapping(
            state,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42_000),
            MappingTaskContext {
                views,
                endpoint_selector: selector.clone(),
                cancellation: cancellation.clone(),
                counters: counters.clone(),
                settings_generation,
                discovery_config: Some(config),
            },
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if matches!(
                    selector.current(),
                    AdvertisedPeerEndpointState::Mapped { .. }
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("scripted mapping reaches verified state");
        assert_eq!(counters.mappings.load(Ordering::Acquire), 1);

        cancellation.cancel();
        let outcome = mapping_task.await.expect("join scripted mapping");
        let uncertain = outcome
            .uncertain_mapping
            .expect("failed delete retains uncertain lease");
        assert_eq!(uncertain.external_address, Ipv4Addr::new(203, 0, 113, 10));
        assert_eq!(uncertain.external_port, 42_000);
        assert!(uncertain.remaining_lease_seconds(Instant::now()) <= 1);
        assert!(uncertain.detail.contains("delete UPnP mapping"));
        assert_eq!(counters.mappings.load(Ordering::Acquire), 0);
        udp_task.await.expect("join scripted SSDP");
        http_task.await.expect("join scripted HTTP");
        assert_eq!(
            transcript.lock().unwrap().as_slice(),
            [
                "GET /root.xml",
                "GET /wan.xml",
                "GetExternalIPAddress",
                "GetSpecificPortMappingEntry",
                "AddPortMapping",
                "GetSpecificPortMappingEntry",
                "DeletePortMapping",
            ]
        );
    }

    async fn scripted_delete_failure_gateway() -> (
        UpnpDiscoveryConfig,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = listener.local_addr().unwrap().port();
        let ssdp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let SocketAddr::V4(ssdp_endpoint) = ssdp.local_addr().unwrap() else {
            unreachable!();
        };
        let transcript = Arc::new(Mutex::new(Vec::new()));
        let http_transcript = transcript.clone();
        let http_task = tokio::spawn(async move {
            let mut mapped = false;
            for _ in 0..7 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut stream).await;
                let first_line = request.lines().next().unwrap_or_default();
                let (status, content_type, body, event) = if first_line
                    .starts_with("GET /root.xml ")
                {
                    (
                        "200 OK",
                        "text/xml",
                        device_description(http_port),
                        "GET /root.xml",
                    )
                } else if first_line.starts_with("GET /wan.xml ") {
                    (
                        "200 OK",
                        "application/xml",
                        scpd_description(),
                        "GET /wan.xml",
                    )
                } else {
                    let action = soap_action(&request);
                    let (status, body) = match action {
                        "GetExternalIPAddress" => (
                            "200 OK",
                            soap_response(
                                action,
                                "<NewExternalIPAddress>203.0.113.10</NewExternalIPAddress>",
                            ),
                        ),
                        "GetSpecificPortMappingEntry" if mapped => (
                            "200 OK",
                            soap_response(
                                action,
                                "<NewInternalPort>42000</NewInternalPort><NewInternalClient>127.0.0.1</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>RSTorrent</NewPortMappingDescription><NewLeaseDuration>1</NewLeaseDuration>",
                            ),
                        ),
                        "GetSpecificPortMappingEntry" => (
                            "500 Internal Server Error",
                            soap_fault(714, "NoSuchEntryInArray"),
                        ),
                        "AddPortMapping" => {
                            mapped = true;
                            ("200 OK", soap_response(action, ""))
                        }
                        "DeletePortMapping" => {
                            ("500 Internal Server Error", soap_fault(501, "DeleteFailed"))
                        }
                        other => panic!("unexpected scripted action {other}"),
                    };
                    (status, "text/xml; charset=utf-8", body, action)
                };
                http_transcript.lock().unwrap().push(event.to_owned());
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let udp_task = tokio::spawn(async move {
            let mut request = [0_u8; 1_024];
            let (length, peer) = ssdp.recv_from(&mut request).await.unwrap();
            let request = std::str::from_utf8(&request[..length]).unwrap();
            assert!(request.contains("ST: upnp:rootdevice\r\n"));
            let response = format!(
                "HTTP/1.1 200 OK\r\nLOCATION: http://127.0.0.1:{http_port}/root.xml\r\nUSN: uuid:scripted::upnp:rootdevice\r\n\r\n"
            );
            ssdp.send_to(response.as_bytes(), peer).await.unwrap();
        });
        (
            UpnpDiscoveryConfig::scripted_for_testing(Ipv4Addr::LOCALHOST, ssdp_endpoint),
            transcript,
            udp_task,
            http_task,
        )
    }

    async fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4_096];
        let header_end = loop {
            let length = stream.read(&mut buffer).await.unwrap();
            assert_ne!(length, 0, "request ended before headers");
            bytes.extend_from_slice(&buffer[..length]);
            if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let length = stream.read(&mut buffer).await.unwrap();
            assert_ne!(length, 0, "request ended before body");
            bytes.extend_from_slice(&buffer[..length]);
        }
        String::from_utf8(bytes).unwrap()
    }

    fn soap_action(request: &str) -> &str {
        request
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("soapaction")
                    .then(|| value.trim().trim_matches('"').rsplit('#').next().unwrap())
            })
            .expect("SOAPAction header")
    }

    fn device_description(port: u16) -> String {
        format!(
            "<?xml version=\"1.0\"?><root><URLBase>http://127.0.0.1:{port}/</URLBase><device><serviceList><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:2</serviceType><controlURL>/control</controlURL><SCPDURL>/wan.xml</SCPDURL></service></serviceList></device></root>"
        )
    }

    fn scpd_description() -> String {
        "<scpd><actionList><action><name>GetExternalIPAddress</name></action><action><name>GetSpecificPortMappingEntry</name></action><action><name>AddPortMapping</name></action><action><name>DeletePortMapping</name></action></actionList></scpd>".to_owned()
    }

    fn soap_response(action: &str, arguments: &str) -> String {
        format!(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:WANIPConnection:2\">{arguments}</u:{action}Response></s:Body></s:Envelope>"
        )
    }

    fn soap_fault(code: u16, description: &str) -> String {
        format!(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><detail><UPnPError><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"
        )
    }
}

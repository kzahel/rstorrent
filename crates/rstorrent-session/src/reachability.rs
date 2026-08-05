//! Task-free reachability generation and status transitions.
//!
//! Network ownership lives in the coordinator added by the mapping runtime.
//! This module keeps eligibility and stale-result fencing deterministic.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
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
    PortMappingPolicy, PortMappingStatus,
};
use crate::views::ViewHub;

const DELETE_TIMEOUT: Duration = Duration::from_secs(5);
const RENEWAL_RETRY_DELAY: Duration = Duration::from_secs(60);
static NEXT_REACHABILITY_GENERATION: AtomicU64 = AtomicU64::new(1);

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

#[derive(Debug, Default)]
struct ReachabilityCounters {
    tasks: AtomicUsize,
    mappings: AtomicUsize,
}

#[derive(Debug)]
pub(crate) struct ReachabilityCoordinator {
    cancellation: CancellationToken,
    task: Option<JoinHandle<Result<(), String>>>,
    counters: Arc<ReachabilityCounters>,
    endpoint_selector: AdvertisedPeerEndpointSelector,
}

impl ReachabilityCoordinator {
    pub(crate) fn start(
        settings: &ClientSettings,
        listener_status: &ListenerStatus,
        views: ViewHub,
        endpoint_selector: AdvertisedPeerEndpointSelector,
    ) -> Self {
        let generation = NEXT_REACHABILITY_GENERATION.fetch_add(1, Ordering::Relaxed);
        let state = ReachabilityState::new(generation, settings, listener_status);
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
                    task_views,
                    task_endpoint_selector,
                    task_cancellation,
                    task_counters.clone(),
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

    pub(crate) async fn shutdown(mut self) -> Result<ReachabilityOwnerCounts, String> {
        self.endpoint_selector.stop();
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            match task.await {
                Ok(result) => result?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(format!("reachability task join: {error}")),
            }
        }
        let terminal = self.owner_counts();
        if terminal != ReachabilityOwnerCounts::default() {
            return Err(format!(
                "reachability owners remain: tasks={},mappings={}",
                terminal.tasks, terminal.mappings
            ));
        }
        Ok(terminal)
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
    views: ViewHub,
    endpoint_selector: AdvertisedPeerEndpointSelector,
    cancellation: CancellationToken,
    counters: Arc<ReachabilityCounters>,
) -> Result<(), String> {
    publish(&mut state, &views, ReachabilityEvent::Discovering)?;
    let config = match UpnpDiscoveryConfig::new(*local_endpoint.ip()) {
        Ok(config) => config,
        Err(error) => {
            publish_failure(&mut state, &views, error)?;
            return Ok(());
        }
    };
    let gateway = match discover_igd_v2(config, &cancellation).await {
        Ok(gateway) => gateway,
        Err(error) => {
            publish_failure(&mut state, &views, error)?;
            return Ok(());
        }
    };
    publish(&mut state, &views, ReachabilityEvent::Mapping)?;
    let mut mapping = match gateway
        .create_mapping(local_endpoint.port(), &cancellation)
        .await
    {
        Ok(mapping) => mapping,
        Err(error) => {
            publish_failure(&mut state, &views, error)?;
            return Ok(());
        }
    };
    counters.mappings.store(1, Ordering::Release);
    publish_mapped(&mut state, &views, &endpoint_selector, &mapping)?;
    let mut lease_deadline = Instant::now()
        .checked_add(Duration::from_secs(u64::from(mapping.lease_seconds)))
        .ok_or_else(|| "UPnP mapping lease deadline overflow".to_owned())?;
    let mut renewal_delay = mapping.renewal_delay();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            _ = tokio::time::sleep(renewal_delay) => {
                if endpoint_selector.expire(Instant::now()) {
                    publish_selected_endpoint(&endpoint_selector, &views)?;
                }
                match gateway.renew_mapping(&mut mapping, &cancellation).await {
                    Ok(()) => {
                        publish_mapped(&mut state, &views, &endpoint_selector, &mapping)?;
                        lease_deadline = Instant::now()
                            .checked_add(Duration::from_secs(u64::from(mapping.lease_seconds)))
                            .ok_or_else(|| "UPnP mapping lease deadline overflow".to_owned())?;
                        renewal_delay = mapping.renewal_delay();
                    }
                    Err(error) => {
                        publish(
                            &mut state,
                            &views,
                            ReachabilityEvent::RenewalFailed {
                                external_address: mapping.external_address,
                                external_port: mapping.external_port,
                                detail: error.detail().to_owned(),
                            },
                        )?;
                        if endpoint_selector.renewal_failed(
                            state.generation(),
                            error.detail().to_owned(),
                            Instant::now(),
                        ) {
                            publish_selected_endpoint(&endpoint_selector, &views)?;
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
    publish(&mut state, &views, ReachabilityEvent::Stopping)?;
    if endpoint_selector.stop() {
        publish_selected_endpoint(&endpoint_selector, &views)?;
    }
    let cleanup_cancellation = CancellationToken::new();
    let delete = tokio::time::timeout(
        DELETE_TIMEOUT,
        gateway.delete_mapping(&mapping, &cleanup_cancellation),
    )
    .await;
    counters.mappings.store(0, Ordering::Release);
    match delete {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            record_failure_diagnostic(&views, &error);
            Err(format!("delete UPnP mapping: {}", error.detail()))
        }
        Err(_) => Err("delete UPnP mapping: cleanup deadline elapsed".to_owned()),
    }
}

fn publish_mapped(
    state: &mut ReachabilityState,
    views: &ViewHub,
    endpoint_selector: &AdvertisedPeerEndpointSelector,
    mapping: &UpnpMapping,
) -> Result<(), String> {
    publish(
        state,
        views,
        ReachabilityEvent::Mapped {
            external_address: mapping.external_address,
            external_port: mapping.external_port,
            lease_seconds: mapping.lease_seconds,
        },
    )?;
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
    error: UpnpError,
) -> Result<(), String> {
    record_failure_diagnostic(views, &error);
    publish(
        state,
        views,
        ReachabilityEvent::Failed {
            stage: failure_stage(error.stage()),
            detail: error.detail().to_owned(),
        },
    )
}

fn publish(
    state: &mut ReachabilityState,
    views: &ViewHub,
    event: ReachabilityEvent,
) -> Result<(), String> {
    if state.apply(state.generation(), event) {
        views
            .set_port_mapping_status(state.status().clone())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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
}

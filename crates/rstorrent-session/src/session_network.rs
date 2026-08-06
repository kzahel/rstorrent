//! Stable session-network composition and joined lifetime ownership.

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rstorrent_engine::dht::{DhtConfig, DhtError, DhtService, DhtSnapshot};
use rstorrent_engine::{
    ByteMetricSink, DiscoveryAdvertisementError, DiscoveryAdvertisementHandle,
    DiscoveryAdvertisementService, IncomingPeerAcceptor, IncomingPeerError, IncomingPeerHandle,
    IncomingPeerRuntime, IncomingPeerServiceConfig, IncomingPeerServiceSnapshot, NetworkConfig,
    PeerBudget, SessionSocketConfig, SessionSocketError, SessionSocketSet, SessionUdpError,
    SessionUdpHandle, SessionUdpService,
};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::advertised_endpoint::AdvertisedPeerEndpointSelector;
use crate::dht_views::{DhtObservationRuntime, inspection_view};
use crate::diagnostics::{DiagnosticSeverity, category};
use crate::incoming_seeding::IncomingSeeding;
use crate::reachability::{
    ReachabilityCoordinator, ReachabilityGenerationShutdown, UncertainMappingLease,
};
use crate::settings::{
    AdvertisedPeerEndpointStatus, ClientSettings, ClientSettingsApplicationState,
    ClientSettingsDegradedReason, ClientSettingsRuntimeView, EffectiveListenerSettings,
    HttpsServerAuthenticationPolicy, ListenerBindFailureReason, ListenerPolicy, ListenerStatus,
    PortMappingPolicy, SessionUdpStatus, SettingsAttempt, SettingsConvergenceModel, SettingsDomain,
    SettingsDomainGeneration, classify_listener_bind_failure,
};
use crate::views::{DhtInspectionView, ViewHub};

const PEER_RECONFIGURATION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
struct PendingPeerLimit {
    generation: SettingsDomainGeneration,
    deadline: tokio::time::Instant,
}

#[derive(Clone, Debug)]
struct PendingMappingExpiry {
    attempt: SettingsAttempt,
    deadline: Instant,
}

fn classify_session_socket_bind_failure(error: &SessionSocketError) -> Option<ListenerStatus> {
    let kind = match error {
        SessionSocketError::Bind { source, .. }
        | SessionSocketError::LocalAddress { source, .. } => source.kind(),
        SessionSocketError::LocalNetworkAddress(_) => io::ErrorKind::AddrNotAvailable,
        SessionSocketError::InvalidPreferredPort(_)
        | SessionSocketError::InvalidFixedPort(_)
        | SessionSocketError::InvalidUdpFallbackAddress => return None,
    };
    Some(classify_listener_bind_failure(&io::Error::new(
        kind,
        error.to_string(),
    )))
}

#[derive(Clone, Debug)]
pub(crate) struct SessionNetworkConfig {
    pub settings: ClientSettings,
    pub network: NetworkConfig,
    pub dht: DhtConfig,
    pub initial_dht_snapshot: Option<DhtSnapshot>,
    pub byte_metric_sink: Arc<dyn ByteMetricSink>,
    pub upload_read_jobs: usize,
    pub incoming_handshake_timeout: Duration,
    pub incoming_peer_activity_timeout: Duration,
    pub incoming_keepalive_interval: Duration,
    pub incoming_no_request_timeout: Duration,
    pub incoming_inactivity_timeout: Duration,
    pub peer_budget_max_open_files_for_testing: Option<usize>,
}

#[derive(Debug)]
pub(crate) enum SessionNetworkError {
    Configuration(String),
    Dht(DhtError),
    Incoming(IncomingPeerError),
    SessionSocket(SessionSocketError),
    SessionUdp(SessionUdpError),
    Discovery(DiscoveryAdvertisementError),
}

impl fmt::Display for SessionNetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(formatter, "session network: {message}"),
            Self::Dht(error) => write!(formatter, "{error}"),
            Self::Incoming(error) => write!(formatter, "{error}"),
            Self::SessionSocket(error) => write!(formatter, "{error}"),
            Self::SessionUdp(error) => write!(formatter, "{error}"),
            Self::Discovery(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for SessionNetworkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Dht(error) => Some(error),
            Self::Incoming(error) => Some(error),
            Self::SessionSocket(error) => Some(error),
            Self::SessionUdp(error) => Some(error),
            Self::Discovery(error) => Some(error),
            Self::Configuration(_) => None,
        }
    }
}

impl From<DhtError> for SessionNetworkError {
    fn from(error: DhtError) -> Self {
        Self::Dht(error)
    }
}

impl From<IncomingPeerError> for SessionNetworkError {
    fn from(error: IncomingPeerError) -> Self {
        Self::Incoming(error)
    }
}

impl From<SessionSocketError> for SessionNetworkError {
    fn from(error: SessionSocketError) -> Self {
        Self::SessionSocket(error)
    }
}

impl From<SessionUdpError> for SessionNetworkError {
    fn from(error: SessionUdpError) -> Self {
        Self::SessionUdp(error)
    }
}

impl From<DiscoveryAdvertisementError> for SessionNetworkError {
    fn from(error: DiscoveryAdvertisementError) -> Self {
        Self::Discovery(error)
    }
}

#[derive(Debug)]
pub(crate) struct SessionNetworkShutdown {
    pub dht_snapshot: Option<DhtSnapshot>,
    pub dht_error: Option<DhtError>,
    pub join_error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct SessionNetworkRuntime {
    settings: ClientSettings,
    effective_peer_connection_limit: u32,
    listener_status: ListenerStatus,
    session_udp_status: SessionUdpStatus,
    advertised_endpoint: AdvertisedPeerEndpointSelector,
    peer_budget: PeerBudget,
    incoming_handle: IncomingPeerHandle,
    incoming_seeding: IncomingSeeding,
    session_udp_handle: SessionUdpHandle,
    discovery_handle: DiscoveryAdvertisementHandle,
    listener_active: Arc<AtomicBool>,
    pending_owner: Option<SessionNetworkOwner>,
    settings_sender: Option<watch::Sender<Option<SettingsAttempt>>>,
    reconciliation_cancellation: CancellationToken,
    reconciliation_task: Option<JoinHandle<SessionNetworkOwner>>,
    convergence: Arc<Mutex<SettingsConvergenceModel>>,
    initial_mapping_generation: SettingsDomainGeneration,
    initial_tracker_https_authentication: Option<HttpsServerAuthenticationPolicy>,
    initial_tracker_https_application: ClientSettingsApplicationState,
    views: Option<ViewHub>,
}

#[derive(Debug)]
struct SessionNetworkOwner {
    effective_settings: ClientSettings,
    effective_listener: Option<EffectiveListenerSettings>,
    listener_status: ListenerStatus,
    session_udp_status: SessionUdpStatus,
    dht_bind_address: std::net::SocketAddr,
    incoming_handshake_timeout: Duration,
    advertised_endpoint: AdvertisedPeerEndpointSelector,
    peer_budget: PeerBudget,
    incoming_runtime: Option<IncomingPeerRuntime>,
    incoming_acceptor: Option<IncomingPeerAcceptor>,
    incoming_seeding: IncomingSeeding,
    session_udp: Option<SessionUdpService>,
    dht: Option<DhtService>,
    dht_observations: Option<DhtObservationRuntime>,
    discovery_advertisement: Option<DiscoveryAdvertisementService>,
    reachability: Option<ReachabilityCoordinator>,
    listener_active: Arc<AtomicBool>,
    uncertain_mapping: Option<UncertainMappingLease>,
    mapping_runtime_error: Option<String>,
    effective_tracker_https_authentication: Option<HttpsServerAuthenticationPolicy>,
}

impl SessionNetworkRuntime {
    pub(crate) async fn start(config: SessionNetworkConfig) -> Result<Self, SessionNetworkError> {
        let SessionNetworkConfig {
            settings,
            network,
            mut dht,
            initial_dht_snapshot,
            byte_metric_sink,
            upload_read_jobs,
            incoming_handshake_timeout,
            incoming_peer_activity_timeout,
            incoming_keepalive_interval,
            incoming_no_request_timeout,
            incoming_inactivity_timeout,
            peer_budget_max_open_files_for_testing,
        } = config;
        let mut peer_budget_config = settings.peer_budget_config();
        if let Some(maximum) = peer_budget_max_open_files_for_testing {
            peer_budget_config.max_open_files = maximum;
        }
        let effective_peer_connection_limit = u32::try_from(peer_budget_config.effective_limit())
            .map_err(|_| {
            SessionNetworkError::Configuration(
                "effective peer connection limit cannot be represented".to_owned(),
            )
        })?;
        let peer_budget = PeerBudget::new(peer_budget_config);
        let mut incoming_config = IncomingPeerServiceConfig::new(settings.incoming_bootstrap())
            .with_peer_budget(peer_budget.clone());
        incoming_config.upload_scheduler = settings.upload_scheduler_config();
        incoming_config.upload_read_jobs = upload_read_jobs;
        incoming_config.handshake_timeout = incoming_handshake_timeout;
        incoming_config.peer_activity_timeout = incoming_peer_activity_timeout;
        incoming_config.keepalive_interval = incoming_keepalive_interval;
        incoming_config.no_request_timeout = incoming_no_request_timeout;
        incoming_config.inactivity_timeout = incoming_inactivity_timeout;
        incoming_config.peer_id = network.peer_id;
        incoming_config.byte_metric_sink = Some(byte_metric_sink.clone());

        dht.network_policy = network.policy;
        dht.initial_snapshot = initial_dht_snapshot;
        dht.byte_metric_sink = Some(byte_metric_sink);
        let dht_bind_address = dht.bind_address;
        let socket_config = SessionSocketConfig::new(
            settings.incoming_bootstrap(),
            settings.preferred_listen_port,
            dht.bind_address,
        );
        let mut listener_failure = None;
        let socket_set = match SessionSocketSet::bind(socket_config).await {
            Ok(sockets) => sockets,
            Err(error)
                if !matches!(
                    incoming_config.bootstrap,
                    rstorrent_engine::IncomingTcpBootstrap::Disabled
                ) =>
            {
                let Some(failure) = classify_session_socket_bind_failure(&error) else {
                    return Err(error.into());
                };
                listener_failure = Some(failure);
                SessionSocketSet::bind(SessionSocketConfig::new(
                    rstorrent_engine::IncomingTcpBootstrap::Disabled,
                    settings.preferred_listen_port,
                    dht.bind_address,
                ))
                .await?
            }
            Err(error) => return Err(error.into()),
        };
        let tcp_address = socket_set.tcp_address();
        let udp_address = socket_set.udp_address();
        let coordinated_with_tcp = socket_set.ports_match();
        let (tcp_listener, udp_socket) = socket_set.into_parts();
        let (session_udp, dht_transport) = SessionUdpService::start(udp_socket)?;
        let session_udp_handle = session_udp.handle();
        let incoming_runtime = match IncomingPeerRuntime::start(incoming_config) {
            Ok(runtime) => runtime,
            Err(error) => {
                drop(dht_transport);
                let _ = session_udp.shutdown().await;
                return Err(error.into());
            }
        };
        let (incoming_acceptor, listener_status) = match tcp_listener {
            Some(listener) => match incoming_runtime.start_acceptor(
                settings.incoming_bootstrap(),
                listener,
                incoming_handshake_timeout,
            ) {
                Ok(acceptor) => (
                    Some(acceptor),
                    ListenerStatus::Listening {
                        address: tcp_address
                            .expect("bound TCP listener has an observed address")
                            .ip()
                            .to_string(),
                        port: tcp_address
                            .expect("bound TCP listener has an observed address")
                            .port(),
                    },
                ),
                Err(error) => {
                    drop(dht_transport);
                    let _ = incoming_runtime.shutdown().await;
                    let _ = session_udp.shutdown().await;
                    return Err(error.into());
                }
            },
            None => (None, listener_failure.unwrap_or(ListenerStatus::Disabled)),
        };
        let session_udp_status = SessionUdpStatus::Bound {
            address: udp_address.ip().to_string(),
            port: udp_address.port(),
            coordinated_with_tcp,
        };
        let mut incoming_runtime = Some(incoming_runtime);
        let mut incoming_acceptor = incoming_acceptor;
        let mut session_udp = Some(session_udp);
        let dht = match DhtService::start_with_transport(dht, dht_transport).await {
            Ok(dht) => dht,
            Err(error) => {
                if let Some(acceptor) = incoming_acceptor.take() {
                    let _ = acceptor.shutdown().await;
                }
                if let Some(runtime) = incoming_runtime.take() {
                    let _ = runtime.shutdown().await;
                }
                if let Some(udp) = session_udp.take() {
                    let _ = udp.shutdown().await;
                }
                return Err(error.into());
            }
        };
        let advertised_endpoint = AdvertisedPeerEndpointSelector::new(&listener_status);
        let discovery_advertisement =
            DiscoveryAdvertisementService::start_with_https_authentication(
                network,
                advertised_endpoint.subscribe_wire(),
                dht.handle(),
                settings.tracker_https_authentication(),
            )?;
        let initial_tracker_https_authentication = discovery_advertisement
            .initial_https_authentication()
            .map(HttpsServerAuthenticationPolicy::from_engine);
        let initial_tracker_https_application = discovery_advertisement
            .initial_https_error()
            .map_or(ClientSettingsApplicationState::Applied, |detail| {
                ClientSettingsApplicationState::Degraded {
                    reason: ClientSettingsDegradedReason::TrackerHttpsAuthenticationFailed,
                    detail: detail.to_owned(),
                }
            });
        let discovery_handle = discovery_advertisement.handle();
        let incoming_seeding = IncomingSeeding::new(
            incoming_runtime
                .as_ref()
                .expect("incoming runtime exists after startup")
                .handle(),
        );
        let incoming_handle = incoming_runtime
            .as_ref()
            .expect("incoming runtime exists after startup")
            .handle();
        let listener_active = Arc::new(AtomicBool::new(incoming_acceptor.is_some()));
        let effective_listener = if matches!(
            listener_status,
            ListenerStatus::Disabled | ListenerStatus::Listening { .. }
        ) {
            Some(EffectiveListenerSettings::from_settings(&settings))
        } else {
            None
        };
        let pending_owner = SessionNetworkOwner {
            effective_settings: settings.clone(),
            effective_listener,
            listener_status: listener_status.clone(),
            session_udp_status: session_udp_status.clone(),
            dht_bind_address,
            incoming_handshake_timeout,
            advertised_endpoint: advertised_endpoint.clone(),
            peer_budget: peer_budget.clone(),
            incoming_runtime,
            incoming_acceptor,
            incoming_seeding: incoming_seeding.clone(),
            session_udp,
            dht: Some(dht),
            dht_observations: None,
            discovery_advertisement: Some(discovery_advertisement),
            reachability: None,
            listener_active: listener_active.clone(),
            uncertain_mapping: None,
            mapping_runtime_error: None,
            effective_tracker_https_authentication: initial_tracker_https_authentication,
        };
        let mut convergence = SettingsConvergenceModel::default();
        let initial_attempt = convergence
            .begin(settings.clone())
            .expect("initial client-settings generation is available");
        for domain in [
            SettingsDomain::Transport,
            SettingsDomain::PortMapping,
            SettingsDomain::PeerConnections,
            SettingsDomain::UploadSlots,
        ] {
            assert!(convergence.apply(
                initial_attempt.domain(domain),
                ClientSettingsApplicationState::Applied,
            ));
        }
        assert!(convergence.apply(
            initial_attempt.domain(SettingsDomain::TrackerHttpsAuthentication),
            initial_tracker_https_application.clone(),
        ));
        let initial_mapping_generation = initial_attempt.domain(SettingsDomain::PortMapping);
        Ok(Self {
            settings,
            effective_peer_connection_limit,
            listener_status,
            session_udp_status,
            advertised_endpoint,
            peer_budget,
            incoming_handle,
            incoming_seeding,
            session_udp_handle,
            discovery_handle,
            listener_active,
            pending_owner: Some(pending_owner),
            settings_sender: None,
            reconciliation_cancellation: CancellationToken::new(),
            reconciliation_task: None,
            convergence: Arc::new(Mutex::new(convergence)),
            initial_mapping_generation,
            initial_tracker_https_authentication,
            initial_tracker_https_application,
            views: None,
        })
    }

    pub(crate) fn initial_dht_view(&self) -> DhtInspectionView {
        let observations = self
            .pending_owner
            .as_ref()
            .and_then(|owner| owner.dht.as_ref())
            .expect("DHT exists before reconciliation starts")
            .subscribe_observations();
        inspection_view(&observations.borrow())
    }

    pub(crate) fn initial_settings_view(&self) -> ClientSettingsRuntimeView {
        let mut session_udp_status = self.session_udp_status.clone();
        if let SessionUdpStatus::Bound {
            address,
            port,
            coordinated_with_tcp: _,
        } = &mut session_udp_status
        {
            let observed = self.session_udp_handle.local_address();
            *address = observed.ip().to_string();
            *port = observed.port();
        }
        let mut view = ClientSettingsRuntimeView::from_started(
            self.settings.clone(),
            self.settings.clone(),
            self.effective_peer_connection_limit,
            self.listener_status.clone(),
            session_udp_status,
            self.advertised_endpoint.status(Instant::now()),
        );
        view.effective_tracker_https_server_authentication =
            self.initial_tracker_https_authentication;
        view.tracker_https_authentication_application =
            self.initial_tracker_https_application.clone();
        view
    }

    pub(crate) fn attach_views(&mut self, views: ViewHub) {
        views
            .set_client_settings_mapping_generation(self.initial_mapping_generation)
            .expect("view hub accepts initial client-settings generation");
        let observations = self
            .pending_owner
            .as_ref()
            .and_then(|owner| owner.dht.as_ref())
            .expect("DHT exists when views attach")
            .subscribe_observations();
        self.pending_owner
            .as_mut()
            .expect("session network owner exists when views attach")
            .dht_observations = Some(DhtObservationRuntime::start(observations, views.clone()));
        let preferred_port = self.settings.preferred_listen_port.to_string();
        let tcp_endpoint = match &self.listener_status {
            ListenerStatus::Listening { address, port } => format!("{address}:{port}"),
            ListenerStatus::Disabled | ListenerStatus::BindFailed { .. } => "disabled".to_owned(),
        };
        let (udp_endpoint, coordinated) = match &self.session_udp_status {
            SessionUdpStatus::Bound {
                address,
                port,
                coordinated_with_tcp,
            } => (
                format!("{address}:{port}"),
                coordinated_with_tcp.to_string(),
            ),
            SessionUdpStatus::Unavailable => ("unavailable".to_owned(), "false".to_owned()),
        };
        let _ = views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::PEER_CONNECTION,
            "session_listen_sockets_bound",
            None,
            "Session listen sockets resolved to concrete runtime endpoints",
            &[
                ("preferred_port", &preferred_port),
                ("tcp_endpoint", &tcp_endpoint),
                ("udp_endpoint", &udp_endpoint),
                ("coordinated", &coordinated),
            ],
        );
        if let ListenerStatus::BindFailed { reason, detail } = &self.listener_status {
            let reason = match reason {
                ListenerBindFailureReason::AddressInUse => "address_in_use",
                ListenerBindFailureReason::PermissionDenied => "permission_denied",
                ListenerBindFailureReason::AddressUnavailable => "address_unavailable",
                ListenerBindFailureReason::Other => "other",
            };
            let _ = views.record_diagnostic(
                DiagnosticSeverity::Warning,
                category::PEER_CONNECTION,
                "incoming_listener_bind_failed",
                None,
                "Incoming listener could not start; settings remain available",
                &[("reason", reason), ("detail", detail)],
            );
        }
        if self.initial_tracker_https_authentication
            == Some(HttpsServerAuthenticationPolicy::Disabled)
        {
            let _ = views.record_diagnostic(
                DiagnosticSeverity::Warning,
                category::TRACKER_ANNOUNCE,
                "tracker_https_authentication_disabled",
                None,
                "HTTPS tracker server authentication is disabled",
                &[("settings_generation", "startup"), ("policy", "disabled")],
            );
        }
        self.views = Some(views);
    }

    pub(crate) fn start_reachability(&mut self, views: ViewHub) {
        let mut owner = self
            .pending_owner
            .take()
            .expect("session network owner starts reconciliation once");
        debug_assert!(owner.reachability.is_none());
        owner.reachability = Some(ReachabilityCoordinator::start(
            &owner.effective_settings,
            &owner.listener_status,
            views.clone(),
            owner.advertised_endpoint.clone(),
            self.initial_mapping_generation,
        ));
        let (settings_sender, settings_receiver) = watch::channel(None);
        let cancellation = self.reconciliation_cancellation.clone();
        let convergence = self.convergence.clone();
        self.reconciliation_task = Some(tokio::spawn(run_session_network(
            owner,
            settings_receiver,
            convergence,
            cancellation,
            views,
        )));
        self.settings_sender = Some(settings_sender);
    }

    pub(crate) fn submit_settings(
        &mut self,
        settings: ClientSettings,
    ) -> Result<(), SessionNetworkError> {
        let attempt = self
            .convergence
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .begin(settings.clone())
            .map_err(|error| SessionNetworkError::Configuration(error.to_string()))?;
        self.views
            .as_ref()
            .expect("views attach before settings submission")
            .begin_client_settings_attempt(attempt.domain(SettingsDomain::PortMapping), settings)
            .map_err(|error| SessionNetworkError::Configuration(error.to_string()))?;
        self.settings_sender
            .as_ref()
            .ok_or_else(|| {
                SessionNetworkError::Configuration("settings reconciler is not running".to_owned())
            })?
            .send_replace(Some(attempt));
        Ok(())
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.settings_sender.take();
        self.reconciliation_cancellation.cancel();
    }

    pub(crate) fn peer_budget(&self) -> PeerBudget {
        self.peer_budget.clone()
    }

    pub(crate) fn incoming_seeding(&self) -> IncomingSeeding {
        self.incoming_seeding.clone()
    }

    pub(crate) fn advertised_endpoint(&self) -> AdvertisedPeerEndpointSelector {
        self.advertised_endpoint.clone()
    }

    pub(crate) fn discovery_handle(&self) -> DiscoveryAdvertisementHandle {
        self.discovery_handle.clone()
    }

    pub(crate) fn incoming_peer_snapshot(&self) -> Option<IncomingPeerServiceSnapshot> {
        self.listener_active
            .load(Ordering::Acquire)
            .then(|| self.incoming_handle.snapshot())
    }

    #[cfg(test)]
    pub(crate) fn session_udp_snapshot(&self) -> rstorrent_engine::SessionUdpSnapshot {
        self.session_udp_handle.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn session_udp_generation(&self) -> u64 {
        self.session_udp_handle.generation()
    }

    pub(crate) async fn shutdown(mut self, views: &ViewHub) -> SessionNetworkShutdown {
        self.begin_shutdown();
        if let Some(task) = self.reconciliation_task.take() {
            return match task.await {
                Ok(owner) => owner.shutdown(views).await,
                Err(error) => SessionNetworkShutdown {
                    dht_snapshot: None,
                    dht_error: None,
                    join_error: Some(format!("session network reconciler: {error}")),
                },
            };
        }
        self.pending_owner
            .take()
            .expect("pending owner exists before reconciliation starts")
            .shutdown(views)
            .await
    }
}

impl Drop for SessionNetworkRuntime {
    fn drop(&mut self) {
        self.settings_sender.take();
        self.reconciliation_cancellation.cancel();
        if let Some(task) = self.reconciliation_task.take() {
            task.abort();
        }
        self.incoming_seeding.stop();
    }
}

async fn run_session_network(
    mut owner: SessionNetworkOwner,
    mut settings: watch::Receiver<Option<SettingsAttempt>>,
    convergence: Arc<Mutex<SettingsConvergenceModel>>,
    cancellation: CancellationToken,
    views: ViewHub,
) -> SessionNetworkOwner {
    let mut peer_limit_pending = None;
    let mut mapping_expiry_pending: Option<PendingMappingExpiry> = None;
    let mut peer_poll = tokio::time::interval(Duration::from_millis(25));
    peer_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut mapping_poll = tokio::time::interval(Duration::from_millis(100));
    mapping_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => break,
            changed = settings.changed() => {
                if changed.is_err() {
                    break;
                }
                let attempt = settings.borrow_and_update().clone();
                if let Some(attempt) = attempt {
                    (peer_limit_pending, mapping_expiry_pending) = owner
                        .reconcile(attempt, &convergence, &views, &cancellation)
                        .await;
                }
            }
            _ = peer_poll.tick(), if peer_limit_pending.is_some() => {
                let pending = peer_limit_pending.expect("guarded pending generation");
                if !is_current(&convergence, pending.generation) {
                    peer_limit_pending = None;
                } else if owner.peer_budget.within_absolute_limit() {
                    publish_peer_connections(
                        &convergence,
                        pending.generation,
                        owner.peer_budget.snapshot().effective_limit,
                        ClientSettingsApplicationState::Applied,
                        &views,
                    );
                    peer_limit_pending = None;
                } else if tokio::time::Instant::now() >= pending.deadline {
                    publish_peer_connections(
                        &convergence,
                        pending.generation,
                        owner.peer_budget.snapshot().effective_limit,
                        ClientSettingsApplicationState::Degraded {
                            reason: ClientSettingsDegradedReason::PeerConnectionConvergenceFailed,
                            detail: "peer owners did not release cancelled permits before the convergence deadline".to_owned(),
                        },
                        &views,
                    );
                    peer_limit_pending = None;
                }
            }
            _ = mapping_poll.tick(), if mapping_expiry_pending.is_some() => {
                let pending = mapping_expiry_pending
                    .as_ref()
                    .expect("guarded pending mapping expiry");
                let generation = pending.attempt.domain(SettingsDomain::PortMapping);
                if !is_current(&convergence, generation) {
                    mapping_expiry_pending = None;
                } else if Instant::now() >= pending.deadline {
                    let attempt = pending.attempt.clone();
                    mapping_expiry_pending = owner
                        .reconcile_mapping(&attempt, false, &convergence, &views)
                        .await
                        .map(|deadline| PendingMappingExpiry { attempt, deadline });
                }
            }
        }
    }
    owner
}

impl SessionNetworkOwner {
    fn record_reachability_shutdown(
        &mut self,
        result: Result<ReachabilityGenerationShutdown, String>,
    ) {
        match result {
            Ok(shutdown) => {
                debug_assert_eq!(shutdown.terminal, Default::default());
                if let Some(error) = shutdown.error {
                    self.mapping_runtime_error = Some(error);
                }
                if let Some(uncertain_mapping) = shutdown.uncertain_mapping {
                    self.uncertain_mapping = Some(uncertain_mapping);
                }
            }
            Err(error) => self.mapping_runtime_error = Some(error),
        }
    }

    async fn reconcile(
        &mut self,
        attempt: SettingsAttempt,
        convergence: &Arc<Mutex<SettingsConvergenceModel>>,
        views: &ViewHub,
        cancellation: &CancellationToken,
    ) -> (Option<PendingPeerLimit>, Option<PendingMappingExpiry>) {
        let tracker_https_generation = attempt.domain(SettingsDomain::TrackerHttpsAuthentication);
        let tracker_https_state = self
            .discovery_advertisement
            .as_ref()
            .expect("discovery advertisement exists during reconciliation")
            .handle()
            .replace_https_authentication(attempt.settings.tracker_https_authentication())
            .await;
        match tracker_https_state {
            Ok(()) => {
                let effective = attempt.settings.tracker_https_server_authentication;
                self.effective_settings.tracker_https_server_authentication = effective;
                self.effective_tracker_https_authentication = Some(effective);
                publish_tracker_https_authentication(
                    convergence,
                    tracker_https_generation,
                    self.effective_tracker_https_authentication,
                    ClientSettingsApplicationState::Applied,
                    views,
                );
                if effective == HttpsServerAuthenticationPolicy::Disabled {
                    let generation = tracker_https_generation.attempt().to_string();
                    let _ = views.record_diagnostic(
                        DiagnosticSeverity::Warning,
                        category::TRACKER_ANNOUNCE,
                        "tracker_https_authentication_disabled",
                        None,
                        "HTTPS tracker server authentication is disabled",
                        &[("settings_generation", &generation), ("policy", "disabled")],
                    );
                }
            }
            Err(error) => publish_tracker_https_authentication(
                convergence,
                tracker_https_generation,
                self.effective_tracker_https_authentication,
                ClientSettingsApplicationState::Degraded {
                    reason: ClientSettingsDegradedReason::TrackerHttpsAuthenticationFailed,
                    detail: error.to_string(),
                },
                views,
            ),
        }

        let peer_generation = attempt.domain(SettingsDomain::PeerConnections);
        let peer = self.peer_budget.reconfigure(
            usize::try_from(attempt.settings.peer_connection_limit)
                .expect("validated peer limit fits usize"),
        );
        self.effective_settings.peer_connection_limit = attempt.settings.peer_connection_limit;
        let peer_state = if peer.within_limit {
            ClientSettingsApplicationState::Applied
        } else {
            ClientSettingsApplicationState::Applying
        };
        publish_peer_connections(
            convergence,
            peer_generation,
            peer.effective_limit,
            peer_state,
            views,
        );

        let upload_generation = attempt.domain(SettingsDomain::UploadSlots);
        self.incoming_runtime
            .as_ref()
            .expect("incoming runtime exists during reconciliation")
            .reconfigure_upload_slots(usize::from(attempt.settings.upload_slots));
        self.effective_settings.upload_slots = attempt.settings.upload_slots;
        if apply_state(
            convergence,
            upload_generation,
            ClientSettingsApplicationState::Applied,
        )
        .is_some()
        {
            let slots = attempt.settings.upload_slots;
            let _ = views.update_client_settings_runtime_for(upload_generation, |runtime| {
                runtime.effective_upload_slots = slots;
                runtime.upload_slots_application = ClientSettingsApplicationState::Applied;
            });
        }

        let transport_changed = self
            .reconcile_transport(&attempt, convergence, views, cancellation)
            .await;
        let mapping_expiry = self
            .reconcile_mapping(&attempt, transport_changed, convergence, views)
            .await;
        let peer_limit = (!peer.within_limit).then(|| PendingPeerLimit {
            generation: peer_generation,
            deadline: tokio::time::Instant::now() + PEER_RECONFIGURATION_TIMEOUT,
        });
        let mapping_expiry =
            mapping_expiry.map(|deadline| PendingMappingExpiry { attempt, deadline });
        (peer_limit, mapping_expiry)
    }

    async fn reconcile_transport(
        &mut self,
        attempt: &SettingsAttempt,
        convergence: &Arc<Mutex<SettingsConvergenceModel>>,
        views: &ViewHub,
        cancellation: &CancellationToken,
    ) -> bool {
        let generation = attempt.domain(SettingsDomain::Transport);
        let desired = EffectiveListenerSettings::from_settings(&attempt.settings);
        if !transport_rebind_required(
            self.effective_listener.as_ref(),
            &desired,
            &self.listener_status,
        ) {
            self.effective_listener = Some(desired.clone());
            self.effective_settings.listener = desired.listener;
            self.effective_settings.preferred_listen_port = desired.preferred_listen_port;
            publish_transport(
                convergence,
                generation,
                Some(desired),
                self.listener_status.clone(),
                self.session_udp_status.clone(),
                self.advertised_endpoint.status(Instant::now()),
                ClientSettingsApplicationState::Applied,
                views,
            );
            return false;
        }

        let candidate = SessionSocketSet::bind(SessionSocketConfig::new(
            attempt.settings.incoming_bootstrap(),
            attempt.settings.preferred_listen_port,
            self.dht_bind_address,
        ))
        .await;
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(error) => {
                publish_transport(
                    convergence,
                    generation,
                    self.effective_listener.clone(),
                    self.listener_status.clone(),
                    self.session_udp_status.clone(),
                    self.advertised_endpoint.status(Instant::now()),
                    ClientSettingsApplicationState::Degraded {
                        reason: ClientSettingsDegradedReason::TransportBindFailed,
                        detail: error.to_string(),
                    },
                    views,
                );
                return false;
            }
        };
        if cancellation.is_cancelled() || !is_current(convergence, generation) {
            return false;
        }

        let tcp_address = candidate.tcp_address();
        let udp_address = candidate.udp_address();
        let coordinated_with_tcp = candidate.ports_match();
        let (tcp_listener, udp_socket) = candidate.into_parts();
        self.advertised_endpoint
            .replace_listener(&ListenerStatus::Disabled);
        let _ = views.set_advertised_peer_endpoint(self.advertised_endpoint.status(Instant::now()));
        if let Some(reachability) = self.reachability.take() {
            self.record_reachability_shutdown(reachability.shutdown_generation().await);
        }
        if cancellation.is_cancelled() || !is_current(convergence, generation) {
            self.advertised_endpoint
                .replace_listener(&self.listener_status);
            let _ =
                views.set_advertised_peer_endpoint(self.advertised_endpoint.status(Instant::now()));
            self.reachability = Some(ReachabilityCoordinator::start(
                &self.effective_settings,
                &self.listener_status,
                views.clone(),
                self.advertised_endpoint.clone(),
                attempt.domain(SettingsDomain::PortMapping),
            ));
            return false;
        }

        let candidate_acceptor = match tcp_listener {
            Some(listener) => match self
                .incoming_runtime
                .as_ref()
                .expect("incoming runtime exists during handover")
                .start_acceptor(
                    attempt.settings.incoming_bootstrap(),
                    listener,
                    self.incoming_handshake_timeout,
                ) {
                Ok(acceptor) => Some(acceptor),
                Err(error) => {
                    self.advertised_endpoint
                        .replace_listener(&self.listener_status);
                    let _ = views.set_advertised_peer_endpoint(
                        self.advertised_endpoint.status(Instant::now()),
                    );
                    self.reachability = Some(ReachabilityCoordinator::start(
                        &self.effective_settings,
                        &self.listener_status,
                        views.clone(),
                        self.advertised_endpoint.clone(),
                        attempt.domain(SettingsDomain::PortMapping),
                    ));
                    publish_transport(
                        convergence,
                        generation,
                        self.effective_listener.clone(),
                        self.listener_status.clone(),
                        self.session_udp_status.clone(),
                        self.advertised_endpoint.status(Instant::now()),
                        ClientSettingsApplicationState::Degraded {
                            reason: ClientSettingsDegradedReason::TransportHandoverFailed,
                            detail: error.to_string(),
                        },
                        views,
                    );
                    return false;
                }
            },
            None => None,
        };

        let udp_result = self
            .session_udp
            .as_mut()
            .expect("session UDP exists during handover")
            .replace_socket(udp_socket)
            .await;
        let listener_status = tcp_address.map_or(ListenerStatus::Disabled, |address| {
            ListenerStatus::Listening {
                address: address.ip().to_string(),
                port: address.port(),
            }
        });
        let session_udp_status = SessionUdpStatus::Bound {
            address: udp_address.ip().to_string(),
            port: udp_address.port(),
            coordinated_with_tcp,
        };
        let previous_acceptor = std::mem::replace(&mut self.incoming_acceptor, candidate_acceptor);
        if self.incoming_acceptor.is_none() {
            self.incoming_runtime
                .as_ref()
                .expect("incoming runtime exists during listener disable")
                .disable_listener();
        }
        self.listener_active
            .store(self.incoming_acceptor.is_some(), Ordering::Release);
        self.listener_status = listener_status;
        self.session_udp_status = session_udp_status;
        self.effective_listener = Some(desired.clone());
        self.effective_settings.listener = desired.listener;
        self.effective_settings.preferred_listen_port = desired.preferred_listen_port;
        self.advertised_endpoint
            .replace_listener(&self.listener_status);
        if let Some(acceptor) = previous_acceptor
            && let Err(error) = acceptor.shutdown().await
        {
            let detail = format!("retire previous incoming acceptor: {error}");
            publish_transport(
                convergence,
                generation,
                Some(desired),
                self.listener_status.clone(),
                self.session_udp_status.clone(),
                self.advertised_endpoint.status(Instant::now()),
                ClientSettingsApplicationState::Degraded {
                    reason: ClientSettingsDegradedReason::TransportHandoverFailed,
                    detail,
                },
                views,
            );
            return true;
        }
        let state = match udp_result {
            Ok(()) => ClientSettingsApplicationState::Applied,
            Err(error) => ClientSettingsApplicationState::Degraded {
                reason: ClientSettingsDegradedReason::TransportHandoverFailed,
                detail: format!("retire previous UDP generation: {error}"),
            },
        };
        publish_transport(
            convergence,
            generation,
            Some(desired),
            self.listener_status.clone(),
            self.session_udp_status.clone(),
            self.advertised_endpoint.status(Instant::now()),
            state,
            views,
        );
        true
    }

    async fn reconcile_mapping(
        &mut self,
        attempt: &SettingsAttempt,
        transport_changed: bool,
        convergence: &Arc<Mutex<SettingsConvergenceModel>>,
        views: &ViewHub,
    ) -> Option<Instant> {
        let generation = attempt.domain(SettingsDomain::PortMapping);
        let policy_changed = self.effective_settings.port_mapping != attempt.settings.port_mapping;
        let reachability_finished = self
            .reachability
            .as_ref()
            .is_some_and(ReachabilityCoordinator::is_finished);
        if !policy_changed
            && !transport_changed
            && self.uncertain_mapping.is_none()
            && self.mapping_runtime_error.is_none()
            && !reachability_finished
        {
            publish_mapping(
                convergence,
                generation,
                self.effective_settings.port_mapping,
                ClientSettingsApplicationState::Applied,
                views,
            );
            return None;
        }
        if self.reachability.is_some() {
            self.advertised_endpoint
                .replace_listener(&ListenerStatus::Disabled);
            let _ =
                views.set_advertised_peer_endpoint(self.advertised_endpoint.status(Instant::now()));
            if let Some(reachability) = self.reachability.take() {
                self.record_reachability_shutdown(reachability.shutdown_generation().await);
            }
        }
        self.advertised_endpoint
            .replace_listener(&self.listener_status);
        let _ = views.set_advertised_peer_endpoint(self.advertised_endpoint.status(Instant::now()));
        let now = Instant::now();
        if self
            .uncertain_mapping
            .as_ref()
            .is_some_and(|mapping| mapping.remaining_lease_seconds(now) == 0)
        {
            self.uncertain_mapping = None;
        }
        if let Some(mapping) = &self.uncertain_mapping {
            let remaining_lease_seconds = mapping.remaining_lease_seconds(now);
            let detail = format!(
                "{}; the prior external lease may remain for {remaining_lease_seconds} seconds",
                mapping.detail,
            );
            let _ = views.set_port_mapping_status_for(
                generation,
                crate::PortMappingStatus::CleanupFailed {
                    external_address: mapping.external_address.to_string(),
                    external_port: mapping.external_port,
                    remaining_lease_seconds,
                    detail: detail.clone(),
                },
            );
            publish_mapping(
                convergence,
                generation,
                self.effective_settings.port_mapping,
                ClientSettingsApplicationState::Degraded {
                    reason: ClientSettingsDegradedReason::PortMappingCleanupFailed,
                    detail,
                },
                views,
            );
            return Some(mapping.expires_at);
        }
        if let Some(error) = self.mapping_runtime_error.take() {
            publish_mapping(
                convergence,
                generation,
                self.effective_settings.port_mapping,
                ClientSettingsApplicationState::Degraded {
                    reason: ClientSettingsDegradedReason::PortMappingCleanupFailed,
                    detail: error,
                },
                views,
            );
            return None;
        }
        self.effective_settings.port_mapping = attempt.settings.port_mapping;
        self.reachability = Some(ReachabilityCoordinator::start(
            &self.effective_settings,
            &self.listener_status,
            views.clone(),
            self.advertised_endpoint.clone(),
            generation,
        ));
        publish_mapping(
            convergence,
            generation,
            self.effective_settings.port_mapping,
            ClientSettingsApplicationState::Applied,
            views,
        );
        None
    }

    async fn shutdown(mut self, views: &ViewHub) -> SessionNetworkShutdown {
        let mut join_error = None;
        if let Some(discovery) = self.discovery_advertisement.take() {
            match discovery.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts = format!(
                        "tasks={},registrations={},tracker_operations={},tracker_high_water={},dht_operations={},dht_high_water={},queue_high_water={}",
                        terminal.tasks,
                        terminal.registrations,
                        terminal.tracker_operations,
                        terminal.tracker_operations_high_water,
                        terminal.dht_operations,
                        terminal.dht_operations_high_water,
                        terminal.command_queue_high_water,
                    );
                    let _ = views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_PEER,
                        "discovery_advertisement_service_stopped",
                        None,
                        "Discovery and advertisement service stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => {
                    remember_error(&mut join_error, format!("discovery advertisement: {error}"))
                }
            }
        }
        if let Some(reachability) = self.reachability.take() {
            match reachability.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts =
                        format!("tasks={},mappings={}", terminal.tasks, terminal.mappings);
                    let _ = views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_REACHABILITY,
                        "reachability_coordinator_stopped",
                        None,
                        "Incoming reachability coordinator stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => remember_error(
                    &mut join_error,
                    format!("reachability coordinator: {error}"),
                ),
            }
        }
        if let Some(mapping) = self.uncertain_mapping.take() {
            remember_error(
                &mut join_error,
                format!(
                    "uncertain UPnP mapping {}:{} may remain for {} seconds: {}",
                    mapping.external_address,
                    mapping.external_port,
                    mapping.remaining_lease_seconds(Instant::now()),
                    mapping.detail,
                ),
            );
        }
        if let Some(error) = self.mapping_runtime_error.take() {
            remember_error(&mut join_error, format!("reachability generation: {error}"));
        }
        self.incoming_seeding.stop();
        if let Some(acceptor) = self.incoming_acceptor.take()
            && let Err(error) = acceptor.shutdown().await
        {
            remember_error(&mut join_error, format!("incoming acceptor: {error}"));
        }
        self.listener_active.store(false, Ordering::Release);
        if let Some(runtime) = self.incoming_runtime.take() {
            match runtime.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts = format!(
                        "pending={},established={},reads={},registrations={}",
                        terminal.pending,
                        terminal.established,
                        terminal.reads,
                        terminal.registrations
                    );
                    let _ = views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::PEER_CONNECTION,
                        "incoming_peer_service_stopped",
                        None,
                        "Incoming peer runtime stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => {
                    remember_error(&mut join_error, format!("incoming peer runtime: {error}"))
                }
            }
        }
        let (dht_snapshot, dht_error) = match self.dht.take() {
            Some(dht) => match dht.shutdown().await {
                Ok(snapshot) => (Some(snapshot), None),
                Err(error) => (None, Some(error)),
            },
            None => (None, None),
        };
        if let Some(udp) = self.session_udp.take() {
            match udp.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts = format!(
                        "tasks={},queued={},dropped={}",
                        terminal.tasks, terminal.queued, terminal.datagrams_dropped
                    );
                    let _ = views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_DHT,
                        "session_udp_service_stopped",
                        None,
                        "Session UDP service stopped with joined ownership",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => {
                    remember_error(&mut join_error, format!("session UDP service: {error}"))
                }
            }
        }
        if let Some(observations) = self.dht_observations.take() {
            match observations.join().await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => remember_error(
                    &mut join_error,
                    format!("DHT observation forwarder: {error}"),
                ),
                Err(error) => remember_error(
                    &mut join_error,
                    format!("DHT observation forwarder: {error}"),
                ),
            }
        }
        SessionNetworkShutdown {
            dht_snapshot,
            dht_error,
            join_error,
        }
    }
}

fn transport_rebind_required(
    effective: Option<&EffectiveListenerSettings>,
    desired: &EffectiveListenerSettings,
    listener_status: &ListenerStatus,
) -> bool {
    let Some(effective) = effective else {
        return true;
    };
    if effective.listener != desired.listener {
        let ListenerStatus::Listening { port, .. } = listener_status else {
            return true;
        };
        let same_scope_adoption = match (effective.listener, desired.listener) {
            (ListenerPolicy::AutomaticLoopback, ListenerPolicy::FixedLoopback { port: fixed })
            | (
                ListenerPolicy::AutomaticLocalNetwork,
                ListenerPolicy::FixedLocalNetwork { port: fixed },
            ) => fixed == *port,
            (ListenerPolicy::FixedLoopback { .. }, ListenerPolicy::AutomaticLoopback)
            | (ListenerPolicy::FixedLocalNetwork { .. }, ListenerPolicy::AutomaticLocalNetwork) => {
                desired.preferred_listen_port == *port
            }
            _ => false,
        };
        return !same_scope_adoption;
    }
    matches!(
        desired.listener,
        ListenerPolicy::AutomaticLoopback | ListenerPolicy::AutomaticLocalNetwork
    ) && effective.preferred_listen_port != desired.preferred_listen_port
}

fn is_current(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
) -> bool {
    convergence
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_current(generation)
}

fn apply_state(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
    state: ClientSettingsApplicationState,
) -> Option<ClientSettingsApplicationState> {
    let mut convergence = convergence
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !convergence.is_current(generation) {
        return None;
    }
    convergence.apply(generation, state);
    Some(convergence.state(generation.domain()).clone())
}

fn publish_peer_connections(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
    effective_limit: usize,
    state: ClientSettingsApplicationState,
    views: &ViewHub,
) {
    let Some(state) = apply_state(convergence, generation, state) else {
        return;
    };
    let effective_limit = u32::try_from(effective_limit).unwrap_or(u32::MAX);
    let _ = views.update_client_settings_runtime_for(generation, |runtime| {
        runtime.effective_peer_connection_limit = effective_limit;
        runtime.peer_connections_application = state;
    });
}

#[allow(clippy::too_many_arguments)]
fn publish_transport(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
    effective_listener: Option<EffectiveListenerSettings>,
    listener_status: ListenerStatus,
    session_udp_status: SessionUdpStatus,
    advertised_endpoint: AdvertisedPeerEndpointStatus,
    state: ClientSettingsApplicationState,
    views: &ViewHub,
) {
    let Some(state) = apply_state(convergence, generation, state) else {
        return;
    };
    let _ = views.update_client_settings_runtime_for(generation, |runtime| {
        runtime.effective_listener = effective_listener;
        runtime.listener_status = listener_status;
        runtime.session_udp_status = session_udp_status;
        runtime.advertised_peer_endpoint = advertised_endpoint;
        runtime.transport_application = state;
    });
}

fn publish_mapping(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
    effective_policy: PortMappingPolicy,
    state: ClientSettingsApplicationState,
    views: &ViewHub,
) {
    let Some(state) = apply_state(convergence, generation, state) else {
        return;
    };
    let _ = views.update_client_settings_runtime_for(generation, |runtime| {
        runtime.effective_port_mapping = effective_policy;
        runtime.port_mapping_application = state;
    });
}

fn publish_tracker_https_authentication(
    convergence: &Arc<Mutex<SettingsConvergenceModel>>,
    generation: SettingsDomainGeneration,
    effective_policy: Option<HttpsServerAuthenticationPolicy>,
    state: ClientSettingsApplicationState,
    views: &ViewHub,
) {
    let Some(state) = apply_state(convergence, generation, state) else {
        return;
    };
    let _ = views.update_client_settings_runtime_for(generation, |runtime| {
        runtime.effective_tracker_https_server_authentication = effective_policy;
        runtime.tracker_https_authentication_application = state;
    });
}

fn remember_error(slot: &mut Option<String>, error: String) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

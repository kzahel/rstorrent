//! Stable session-network composition and joined lifetime ownership.

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rstorrent_engine::dht::{DhtConfig, DhtError, DhtService, DhtSnapshot};
use rstorrent_engine::{
    ByteMetricSink, DiscoveryAdvertisementHandle, DiscoveryAdvertisementService,
    IncomingPeerAcceptor, IncomingPeerError, IncomingPeerRuntime, IncomingPeerServiceConfig,
    IncomingPeerServiceSnapshot, NetworkConfig, PeerBudget, SessionSocketConfig,
    SessionSocketError, SessionSocketSet, SessionUdpError, SessionUdpService,
};

use crate::advertised_endpoint::AdvertisedPeerEndpointSelector;
use crate::dht_views::{DhtObservationRuntime, inspection_view};
use crate::diagnostics::{DiagnosticSeverity, category};
use crate::incoming_seeding::IncomingSeeding;
use crate::reachability::ReachabilityCoordinator;
use crate::settings::{
    ClientSettings, ClientSettingsRuntimeView, ListenerBindFailureReason, ListenerStatus,
    SessionUdpStatus, classify_listener_bind_failure,
};
use crate::views::{DhtInspectionView, ViewHub};

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
}

impl fmt::Display for SessionNetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(formatter, "session network: {message}"),
            Self::Dht(error) => write!(formatter, "{error}"),
            Self::Incoming(error) => write!(formatter, "{error}"),
            Self::SessionSocket(error) => write!(formatter, "{error}"),
            Self::SessionUdp(error) => write!(formatter, "{error}"),
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
    incoming_runtime: Option<IncomingPeerRuntime>,
    incoming_acceptor: Option<IncomingPeerAcceptor>,
    incoming_seeding: Option<IncomingSeeding>,
    session_udp: Option<SessionUdpService>,
    dht: Option<DhtService>,
    dht_observations: Option<DhtObservationRuntime>,
    discovery_advertisement: Option<DiscoveryAdvertisementService>,
    discovery_handle: DiscoveryAdvertisementHandle,
    reachability: Option<ReachabilityCoordinator>,
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
        let discovery_advertisement = DiscoveryAdvertisementService::start(
            network,
            advertised_endpoint.subscribe_wire(),
            dht.handle(),
        );
        let discovery_handle = discovery_advertisement.handle();
        let incoming_seeding = IncomingSeeding::new(
            incoming_runtime
                .as_ref()
                .expect("incoming runtime exists after startup")
                .handle(),
        );
        Ok(Self {
            settings,
            effective_peer_connection_limit,
            listener_status,
            session_udp_status,
            advertised_endpoint,
            peer_budget,
            incoming_runtime,
            incoming_acceptor,
            incoming_seeding: Some(incoming_seeding),
            session_udp,
            dht: Some(dht),
            dht_observations: None,
            discovery_advertisement: Some(discovery_advertisement),
            discovery_handle,
            reachability: None,
        })
    }

    pub(crate) fn initial_dht_view(&self) -> DhtInspectionView {
        let observations = self
            .dht
            .as_ref()
            .expect("DHT exists before session-network shutdown")
            .subscribe_observations();
        inspection_view(&observations.borrow())
    }

    pub(crate) fn initial_settings_view(&self) -> ClientSettingsRuntimeView {
        ClientSettingsRuntimeView::from_started(
            self.settings.clone(),
            self.settings.clone(),
            self.effective_peer_connection_limit,
            self.listener_status.clone(),
            self.session_udp_status.clone(),
            self.advertised_endpoint.status(Instant::now()),
        )
    }

    pub(crate) fn attach_views(&mut self, views: ViewHub) {
        let observations = self
            .dht
            .as_ref()
            .expect("DHT exists when views attach")
            .subscribe_observations();
        self.dht_observations = Some(DhtObservationRuntime::start(observations, views.clone()));
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
    }

    pub(crate) fn start_reachability(&mut self, views: ViewHub) {
        debug_assert!(self.reachability.is_none());
        self.reachability = Some(ReachabilityCoordinator::start(
            &self.settings,
            &self.listener_status,
            views,
            self.advertised_endpoint.clone(),
        ));
    }

    pub(crate) fn peer_budget(&self) -> PeerBudget {
        self.peer_budget.clone()
    }

    pub(crate) fn incoming_seeding(&self) -> IncomingSeeding {
        self.incoming_seeding
            .as_ref()
            .expect("incoming seeding exists before shutdown")
            .clone()
    }

    pub(crate) fn advertised_endpoint(&self) -> AdvertisedPeerEndpointSelector {
        self.advertised_endpoint.clone()
    }

    pub(crate) fn discovery_handle(&self) -> DiscoveryAdvertisementHandle {
        self.discovery_handle.clone()
    }

    pub(crate) fn incoming_peer_snapshot(&self) -> Option<IncomingPeerServiceSnapshot> {
        self.incoming_acceptor.as_ref()?;
        self.incoming_runtime
            .as_ref()
            .map(IncomingPeerRuntime::snapshot)
    }

    #[cfg(test)]
    pub(crate) fn session_udp_snapshot(&self) -> rstorrent_engine::SessionUdpSnapshot {
        self.session_udp
            .as_ref()
            .expect("session UDP exists before shutdown")
            .snapshot()
    }

    #[cfg(test)]
    pub(crate) fn reachability_owner_counts(&self) -> crate::reachability::ReachabilityOwnerCounts {
        self.reachability
            .as_ref()
            .expect("reachability exists after views attach")
            .owner_counts()
    }

    pub(crate) async fn shutdown(mut self, views: &ViewHub) -> SessionNetworkShutdown {
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
        if let Some(incoming) = self.incoming_seeding.take() {
            incoming.stop();
        }
        if let Some(acceptor) = self.incoming_acceptor.take()
            && let Err(error) = acceptor.shutdown().await
        {
            remember_error(&mut join_error, format!("incoming acceptor: {error}"));
        }
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

fn remember_error(slot: &mut Option<String>, error: String) {
    if slot.is_none() {
        *slot = Some(error);
    }
}

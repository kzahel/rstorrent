use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rstorrent_engine::dht::{DhtConfig, DhtError, DhtService};
use rstorrent_engine::{
    DEFAULT_INCOMING_HANDSHAKE_TIMEOUT, DEFAULT_INCOMING_INACTIVITY_TIMEOUT,
    DEFAULT_INCOMING_KEEPALIVE_INTERVAL, DEFAULT_INCOMING_NO_REQUEST_TIMEOUT,
    DEFAULT_INCOMING_PEER_ACTIVITY_TIMEOUT, DEFAULT_PEER_ID, DEFAULT_STORAGE_FILE_LIMIT,
    DEFAULT_UPLOAD_READ_JOBS, DescriptorFile, DescriptorStorage, DescriptorStoragePlan,
    DiscoveryAdvertisementError, DiscoveryAdvertisementHandle, DiscoveryAdvertisementRegistration,
    DiscoveryAdvertisementService, DiskCheckpointStage, DownloadActivityEvent,
    DownloadActivitySink, DownloadCheckpointSink, DownloadControl, DownloadError,
    DownloadResourceLimits, IncomingPeerError, IncomingPeerService, IncomingPeerServiceConfig,
    IncomingPeerServiceSnapshot, NetworkConfig, PathPublicationStage, PeerBudget,
    PlatformStorageClient, PlatformStorageFailureKind, PlatformStorageSpec, PreparedFileHash,
    PublicationShape, ResumableMagnetDownloadConfig, ResumeArtifactState, ResumedStorage,
    SessionSocketConfig, SessionSocketError, SessionSocketSet, SessionUdpError, SessionUdpService,
    StorageFilePool, StorageFilePoolSnapshot, TorrentPrivacy, TrackerConfig, TrackerEndpoint,
    TrackerSource, TrackerTransport, download_magnet_metadata_with_external_discovery,
    plan_descriptor_storage, resume_magnet_to_descriptors_with_control, resume_magnet_with_control,
    torrent_storage_paths, verify_prepared_descriptors, verify_prepared_platform_files,
};
use rstorrent_protocol::magnet::{MAX_TRACKER_URL_LENGTH, UdpTrackerUrl};
use rstorrent_protocol::metainfo::{
    BEP9_METAINFO_LIMITS, DURABLE_METAINFO_LIMITS, Metainfo, MetainfoError,
};
use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
use tokio::task::JoinHandle;

use crate::advertised_endpoint::AdvertisedPeerEndpointSelector;
use crate::control::{
    AddTorrentBytesRequest, Command, ErrorCode, FilePriority, RemovalDataPolicy, RemovalState,
    RequestEnvelope, ResponseEnvelope, ResponseOutcome, StorageState, TorrentState,
};
use crate::dht_views::{DhtObservationRuntime, inspection_view};
use crate::diagnostics::{
    DiagnosticCategory, DiagnosticDraft, DiagnosticField, DiagnosticSeverity, DiagnosticSubject,
    category,
};
use crate::file_views::FileProgressModel;
use crate::have::HaveState;
use crate::incoming_seeding::{IncomingSeeding, IncomingSeedingError, SeedReconcileOutcome};
use crate::reachability::ReachabilityCoordinator;
use crate::settings::{
    ClientSettingsRuntimeView, ListenerBindFailureReason, ListenerStatus, SessionUdpStatus,
    StorageRootSnapshot, classify_listener_bind_failure,
};
use crate::speed::{PreparedSpeedHistory, SessionSpeedRecorder, SpeedHistoryRuntime};
use crate::store::{
    ConfiguredStorageRoot, ManagedArtifactState, PreparedFileRecord, RemovalRecord, ResumeRecord,
    SessionStore, StorageRootLocation, StoreError, StoredStorageRoot, StoredTracker,
    StoredTrackerSource, StoredTrackerTransport, prepare_torrent_bytes,
};
use crate::torrent_runtime::{ActiveDownload, TorrentRuntime, TorrentRuntimeHandle};
use crate::tracker_views::TrackerViewModel;

fn parse_durable_metainfo(raw_info: &[u8]) -> Result<Metainfo, MetainfoError> {
    Metainfo::from_info_bytes_with_limits(raw_info, DURABLE_METAINFO_LIMITS)
}

fn parse_peer_metainfo(raw_info: &[u8]) -> Result<Metainfo, MetainfoError> {
    Metainfo::from_info_bytes_with_limits(raw_info, BEP9_METAINFO_LIMITS)
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

fn operational_trackers(
    trackers: &[StoredTracker],
) -> Result<Vec<TrackerConfig>, ApplicationError> {
    trackers
        .iter()
        .filter_map(|tracker| {
            if tracker.url.len() > MAX_TRACKER_URL_LENGTH
                || !tracker.url.is_ascii()
                || tracker
                    .url
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte == b' ')
                || tracker.url.contains('#')
            {
                return None;
            }
            let endpoint = match tracker.transport {
                StoredTrackerTransport::Udp => {
                    TrackerEndpoint::Udp(UdpTrackerUrl::from_metainfo_url(&tracker.url)?)
                }
                StoredTrackerTransport::Http | StoredTrackerTransport::Https => {
                    TrackerEndpoint::from_http_url(&tracker.url)?
                }
            };
            if !matches!(
                (tracker.transport, endpoint.transport()),
                (StoredTrackerTransport::Udp, TrackerTransport::Udp)
                    | (StoredTrackerTransport::Http, TrackerTransport::Http)
                    | (StoredTrackerTransport::Https, TrackerTransport::Https)
            ) {
                return None;
            }
            Ok(TrackerConfig {
                url: tracker.url.clone(),
                endpoint,
                tier: tracker.tier,
                position: tracker.position,
                source: match tracker.source {
                    StoredTrackerSource::Magnet => TrackerSource::Magnet,
                    StoredTrackerSource::Metainfo => TrackerSource::Metainfo,
                },
            })
            .into()
        })
        .collect()
}

fn allocate_application_peer_id() -> Result<[u8; 20], ApplicationError> {
    let mut peer_id = DEFAULT_PEER_ID;
    getrandom::fill(&mut peer_id[8..])
        .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
    Ok(peer_id)
}
use crate::views::{
    DurableTorrentViewState, ProgressInputs, SubscriptionError, SubscriptionSpec, TorrentActivity,
    VIEW_SET_REAPER_INTERVAL_MILLIS, ViewHub, ViewSetLeaseReaper, ViewSubscription,
    ranges_from_pieces,
};
use crate::{
    OpenViewSetRequest, OpenViewSetResponse, UpdateViewSetRequest, ViewSet, ViewSetError,
    ViewSetOwner,
};

#[derive(Clone, Debug)]
pub struct ApplicationConfig {
    pub persistence: ApplicationPersistence,
    pub profile_id: String,
    pub storage_roots: Vec<ConfiguredStorageRoot>,
    pub network: NetworkConfig,
    pub download_resource_limits: DownloadResourceLimits,
    pub dht: DhtConfig,
    pub upload_read_jobs: usize,
    pub incoming_handshake_timeout: Duration,
    pub incoming_peer_activity_timeout: Duration,
    pub incoming_keepalive_interval: Duration,
    pub incoming_no_request_timeout: Duration,
    pub incoming_inactivity_timeout: Duration,
    pub view_set_lease: Duration,
    pub view_set_reaper_interval: Duration,
    #[doc(hidden)]
    pub peer_budget_max_open_files_for_testing: Option<usize>,
    #[doc(hidden)]
    pub storage_write_delay_for_testing: Duration,
    #[doc(hidden)]
    pub storage_write_concurrency_for_testing: usize,
    #[doc(hidden)]
    pub storage_hash_concurrency_for_testing: usize,
    #[doc(hidden)]
    pub checkpoint_sync_delay_for_testing: Duration,
    #[doc(hidden)]
    pub checkpoint_commit_delay_for_testing: Duration,
    #[doc(hidden)]
    pub checkpoint_stage_trace_for_testing: bool,
    #[doc(hidden)]
    pub publication_delay_stage_for_testing: Option<PathPublicationStage>,
    #[doc(hidden)]
    pub publication_delay_for_testing: Duration,
    #[doc(hidden)]
    pub publication_stage_trace_for_testing: bool,
    #[doc(hidden)]
    pub platform_storage_client: Option<PlatformStorageClient>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationPersistence {
    Durable { profile_root: PathBuf },
    Ephemeral,
}

impl ApplicationPersistence {
    pub fn durable_profile_root(&self) -> Option<&Path> {
        match self {
            Self::Durable { profile_root } => Some(profile_root),
            Self::Ephemeral => None,
        }
    }

    fn diagnostic_name(&self) -> &'static str {
        match self {
            Self::Durable { .. } => "durable",
            Self::Ephemeral => "ephemeral",
        }
    }
}

impl ApplicationConfig {
    pub fn new(
        profile_root: PathBuf,
        profile_id: String,
        storage_roots: Vec<ConfiguredStorageRoot>,
        network: NetworkConfig,
    ) -> Self {
        Self::with_persistence(
            ApplicationPersistence::Durable { profile_root },
            profile_id,
            storage_roots,
            network,
        )
    }

    pub fn ephemeral(
        profile_id: String,
        storage_roots: Vec<ConfiguredStorageRoot>,
        network: NetworkConfig,
    ) -> Self {
        Self::with_persistence(
            ApplicationPersistence::Ephemeral,
            profile_id,
            storage_roots,
            network,
        )
    }

    fn with_persistence(
        persistence: ApplicationPersistence,
        profile_id: String,
        storage_roots: Vec<ConfiguredStorageRoot>,
        network: NetworkConfig,
    ) -> Self {
        let dht = DhtConfig::for_network(network.policy);
        Self {
            persistence,
            profile_id,
            storage_roots,
            network,
            download_resource_limits: DownloadResourceLimits::DESKTOP,
            dht,
            upload_read_jobs: DEFAULT_UPLOAD_READ_JOBS,
            incoming_handshake_timeout: DEFAULT_INCOMING_HANDSHAKE_TIMEOUT,
            incoming_peer_activity_timeout: DEFAULT_INCOMING_PEER_ACTIVITY_TIMEOUT,
            incoming_keepalive_interval: DEFAULT_INCOMING_KEEPALIVE_INTERVAL,
            incoming_no_request_timeout: DEFAULT_INCOMING_NO_REQUEST_TIMEOUT,
            incoming_inactivity_timeout: DEFAULT_INCOMING_INACTIVITY_TIMEOUT,
            view_set_lease: Duration::from_millis(crate::views::VIEW_SET_LEASE_MILLIS),
            view_set_reaper_interval: Duration::from_millis(VIEW_SET_REAPER_INTERVAL_MILLIS),
            peer_budget_max_open_files_for_testing: None,
            storage_write_delay_for_testing: Duration::ZERO,
            storage_write_concurrency_for_testing: 4,
            storage_hash_concurrency_for_testing: 4,
            checkpoint_sync_delay_for_testing: Duration::ZERO,
            checkpoint_commit_delay_for_testing: Duration::ZERO,
            checkpoint_stage_trace_for_testing: false,
            publication_delay_stage_for_testing: None,
            publication_delay_for_testing: Duration::ZERO,
            publication_stage_trace_for_testing: false,
            platform_storage_client: None,
        }
    }

    pub fn durable_profile_root(&self) -> Option<&Path> {
        self.persistence.durable_profile_root()
    }
}

#[derive(Debug)]
enum ApplicationTaskReport {
    Metadata,
    Download,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformRemovalPlan {
    pub operation_id: String,
    pub torrent_id: String,
    pub storage_root: String,
    pub name: String,
}

#[derive(Debug)]
pub struct ApplicationService {
    store: Arc<Mutex<SessionStore>>,
    storage_roots: Arc<BTreeMap<String, StorageRootLocation>>,
    network: NetworkConfig,
    download_resource_limits: DownloadResourceLimits,
    storage_write_delay_for_testing: Duration,
    storage_write_concurrency_for_testing: usize,
    storage_hash_concurrency_for_testing: usize,
    checkpoint_sync_delay_for_testing: Duration,
    checkpoint_commit_delay_for_testing: Duration,
    checkpoint_stage_trace_for_testing: bool,
    publication_delay_stage_for_testing: Option<PathPublicationStage>,
    publication_delay_for_testing: Duration,
    publication_stage_trace_for_testing: bool,
    storage_file_pool: StorageFilePool,
    peer_budget: PeerBudget,
    incoming_seeding: Option<IncomingSeeding>,
    advertised_endpoint: AdvertisedPeerEndpointSelector,
    discovery_advertisement: Option<DiscoveryAdvertisementService>,
    discovery_handle: DiscoveryAdvertisementHandle,
    reachability: Option<ReachabilityCoordinator>,
    incoming_service: Option<IncomingPeerService>,
    session_udp: Option<SessionUdpService>,
    torrent_runtimes: BTreeMap<String, TorrentRuntime>,
    active_torrent: Option<String>,
    next_torrent_generation: u64,
    dht: Option<DhtService>,
    dht_observations: Option<DhtObservationRuntime>,
    speed_recorder: Arc<SessionSpeedRecorder>,
    speed_history: Option<SpeedHistoryRuntime>,
    views: ViewHub,
    view_set_reaper: Option<ViewSetLeaseReaper>,
}

impl ApplicationService {
    pub async fn open(config: ApplicationConfig) -> Result<Self, ApplicationError> {
        if config.network.peer_connect_timeout.is_zero() {
            return Err(ApplicationError::Configuration(
                "peer connect timeout must be nonzero".to_owned(),
            ));
        }
        if config.network.peer_io_timeout.is_zero() {
            return Err(ApplicationError::Configuration(
                "peer I/O timeout must be nonzero".to_owned(),
            ));
        }
        let network = if config.network.peer_id == DEFAULT_PEER_ID {
            config.network.with_peer_id(allocate_application_peer_id()?)
        } else {
            config.network
        };
        if config.view_set_lease.is_zero()
            || config.view_set_reaper_interval.is_zero()
            || config.view_set_reaper_interval > config.view_set_lease
        {
            return Err(ApplicationError::Configuration(
                "view-set lease timing must be nonzero and the reaper interval cannot exceed the lease"
                    .to_owned(),
            ));
        }
        if config.storage_write_delay_for_testing > Duration::from_secs(10)
            || config.checkpoint_sync_delay_for_testing > Duration::from_secs(60)
            || config.checkpoint_commit_delay_for_testing > Duration::from_secs(60)
            || config.publication_delay_for_testing > Duration::from_secs(60)
        {
            return Err(ApplicationError::Configuration(
                "test storage delay exceeds its fixed maximum".to_owned(),
            ));
        }
        if !(1..=8).contains(&config.storage_write_concurrency_for_testing)
            || !(1..=8).contains(&config.storage_hash_concurrency_for_testing)
        {
            return Err(ApplicationError::Configuration(
                "test storage concurrency must be between 1 and 8".to_owned(),
            ));
        }
        let mut configured_root_ids = BTreeMap::new();
        for root in &config.storage_roots {
            if configured_root_ids
                .insert(root.id.clone(), root.location.clone())
                .is_some()
            {
                return Err(ApplicationError::Configuration(format!(
                    "storage root {} is duplicated",
                    root.id
                )));
            }
            if let StorageRootLocation::Path(path) = &root.location {
                std::fs::create_dir_all(path).map_err(|source| ApplicationError::Io {
                    operation: "create configured storage root",
                    source,
                })?;
            }
        }
        let store = match &config.persistence {
            ApplicationPersistence::Durable { profile_root } => {
                SessionStore::open(profile_root, &config.profile_id, &config.storage_roots)?
            }
            ApplicationPersistence::Ephemeral => {
                SessionStore::open_ephemeral(&config.profile_id, &config.storage_roots)?
            }
        };
        let snapshot = store.snapshot()?;
        let active_client_settings = snapshot.client_settings.clone();
        let mut peer_budget_config = active_client_settings.peer_budget_config();
        if let Some(maximum) = config.peer_budget_max_open_files_for_testing {
            peer_budget_config.max_open_files = maximum;
        }
        let effective_peer_connection_limit = u32::try_from(peer_budget_config.effective_limit())
            .map_err(|_| {
            ApplicationError::Configuration(
                "effective peer connection limit cannot be represented".to_owned(),
            )
        })?;
        let storage_roots = available_storage_roots(store.storage_roots()?);
        let (initial_dht_snapshot, dht_state_warning) = match store.load_dht_snapshot() {
            Ok(snapshot) => (snapshot, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let speed = match &config.persistence {
            ApplicationPersistence::Durable { profile_root } => {
                PreparedSpeedHistory::open_durable(profile_root)
            }
            ApplicationPersistence::Ephemeral => PreparedSpeedHistory::open_ephemeral()
                .map_err(|error| ApplicationError::Persistence(error.to_string()))?,
        };
        let speed_recorder = speed.recorder.clone();
        let storage_file_pool =
            StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, config.platform_storage_client)
                .map_err(|error| ApplicationError::Configuration(error.to_owned()))?;
        let peer_budget = PeerBudget::new(peer_budget_config);
        let mut incoming_config =
            IncomingPeerServiceConfig::new(active_client_settings.incoming_bootstrap())
                .with_peer_budget(peer_budget.clone());
        incoming_config.upload_scheduler = active_client_settings.upload_scheduler_config();
        incoming_config.upload_read_jobs = config.upload_read_jobs;
        incoming_config.handshake_timeout = config.incoming_handshake_timeout;
        incoming_config.peer_activity_timeout = config.incoming_peer_activity_timeout;
        incoming_config.keepalive_interval = config.incoming_keepalive_interval;
        incoming_config.no_request_timeout = config.incoming_no_request_timeout;
        incoming_config.inactivity_timeout = config.incoming_inactivity_timeout;
        incoming_config.peer_id = network.peer_id;
        incoming_config.byte_metric_sink = Some(speed_recorder.clone());
        let mut dht_config = config.dht;
        dht_config.network_policy = network.policy;
        dht_config.initial_snapshot = initial_dht_snapshot;
        dht_config.byte_metric_sink = Some(speed_recorder.clone());
        let socket_config = SessionSocketConfig::new(
            active_client_settings.incoming_bootstrap(),
            active_client_settings.preferred_listen_port,
            dht_config.bind_address,
        );
        let mut listener_failure = None;
        let socket_set = match SessionSocketSet::bind(socket_config).await {
            Ok(sockets) => sockets,
            Err(error)
                if !matches!(
                    incoming_config.bootstrap,
                    crate::IncomingTcpBootstrap::Disabled
                ) =>
            {
                let Some(failure) = classify_session_socket_bind_failure(&error) else {
                    return Err(error.into());
                };
                listener_failure = Some(failure);
                SessionSocketSet::bind(SessionSocketConfig::new(
                    crate::IncomingTcpBootstrap::Disabled,
                    active_client_settings.preferred_listen_port,
                    dht_config.bind_address,
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
        let (mut incoming_service, listener_status) = match tcp_listener {
            Some(listener) => match IncomingPeerService::start(incoming_config, listener) {
                Ok(service) => (
                    Some(service),
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
                    session_udp.shutdown().await?;
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
        let mut session_udp = Some(session_udp);
        let dht = match DhtService::start_with_transport(dht_config, dht_transport).await {
            Ok(dht) => dht,
            Err(error) => {
                if let Some(incoming) = incoming_service.take() {
                    incoming.shutdown().await?;
                }
                if let Some(udp) = session_udp.take() {
                    udp.shutdown().await?;
                }
                return Err(error.into());
            }
        };
        let dht_observation_receiver = dht.subscribe_observations();
        let initial_dht_view = inspection_view(&dht_observation_receiver.borrow());
        let advertised_endpoint = AdvertisedPeerEndpointSelector::new(&listener_status);
        let discovery_advertisement = DiscoveryAdvertisementService::start(
            network,
            advertised_endpoint.subscribe_wire(),
            dht.handle(),
        );
        let discovery_handle = discovery_advertisement.handle();
        let runtime_client_settings = ClientSettingsRuntimeView::from_started(
            snapshot.client_settings.clone(),
            active_client_settings.clone(),
            effective_peer_connection_limit,
            listener_status.clone(),
            session_udp_status,
            advertised_endpoint.status(std::time::Instant::now()),
        );
        let views = ViewHub::new_with_runtime_views(
            &snapshot,
            config.view_set_lease,
            speed.history.clone(),
            initial_dht_view,
            runtime_client_settings,
        )?;
        let dht_observations =
            DhtObservationRuntime::start(dht_observation_receiver, views.clone());
        let speed_history = speed.start(views.clone());
        let view_set_reaper =
            ViewSetLeaseReaper::start(views.clone(), config.view_set_reaper_interval);
        let incoming_seeding = incoming_service
            .as_ref()
            .map(|incoming| IncomingSeeding::new(incoming.handle()));
        let mut torrent_runtimes = BTreeMap::new();
        let mut next_torrent_generation = 1_u64;
        for torrent in &snapshot.torrents {
            let generation = next_torrent_generation;
            next_torrent_generation = next_torrent_generation.checked_add(1).ok_or_else(|| {
                ApplicationError::Configuration("torrent runtime generation overflow".to_owned())
            })?;
            let runtime = TorrentRuntime::new(
                torrent.torrent_id.clone(),
                generation,
                views.clone(),
                advertised_endpoint.clone(),
            )
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            torrent_runtimes.insert(torrent.torrent_id.clone(), runtime);
        }
        let mut service = Self {
            store: Arc::new(Mutex::new(store)),
            storage_roots: Arc::new(storage_roots),
            network,
            download_resource_limits: config.download_resource_limits,
            storage_write_delay_for_testing: config.storage_write_delay_for_testing,
            storage_write_concurrency_for_testing: config.storage_write_concurrency_for_testing,
            storage_hash_concurrency_for_testing: config.storage_hash_concurrency_for_testing,
            checkpoint_sync_delay_for_testing: config.checkpoint_sync_delay_for_testing,
            checkpoint_commit_delay_for_testing: config.checkpoint_commit_delay_for_testing,
            checkpoint_stage_trace_for_testing: config.checkpoint_stage_trace_for_testing,
            publication_delay_stage_for_testing: config.publication_delay_stage_for_testing,
            publication_delay_for_testing: config.publication_delay_for_testing,
            publication_stage_trace_for_testing: config.publication_stage_trace_for_testing,
            storage_file_pool,
            peer_budget,
            incoming_seeding,
            advertised_endpoint,
            discovery_advertisement: Some(discovery_advertisement),
            discovery_handle,
            reachability: None,
            incoming_service,
            session_udp,
            torrent_runtimes,
            active_torrent: None,
            next_torrent_generation,
            dht: Some(dht),
            dht_observations: Some(dht_observations),
            speed_recorder,
            speed_history: Some(speed_history),
            views,
            view_set_reaper: Some(view_set_reaper),
        };
        service.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_SESSION,
            "application_opened",
            None,
            "Application profile opened",
            &[
                ("profile", &config.profile_id),
                ("network_policy", network.policy.as_str()),
                ("persistence_mode", config.persistence.diagnostic_name()),
            ],
        )?;
        let preferred_port = active_client_settings.preferred_listen_port.to_string();
        let tcp_endpoint =
            tcp_address.map_or_else(|| "disabled".to_owned(), |address| address.to_string());
        let udp_endpoint = udp_address.to_string();
        let coordinated = coordinated_with_tcp.to_string();
        service.views.record_diagnostic(
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
        )?;
        if let Some(detail) = dht_state_warning {
            service.views.record_diagnostic(
                DiagnosticSeverity::Warning,
                category::DISCOVERY_DHT,
                "dht_state_rejected",
                None,
                "Saved DHT state was rejected; using cold bootstrap",
                &[("detail", &detail)],
            )?;
        }
        if let ListenerStatus::BindFailed { reason, detail } = &listener_status {
            let reason = match reason {
                ListenerBindFailureReason::AddressInUse => "address_in_use",
                ListenerBindFailureReason::PermissionDenied => "permission_denied",
                ListenerBindFailureReason::AddressUnavailable => "address_unavailable",
                ListenerBindFailureReason::Other => "other",
            };
            service.views.record_diagnostic(
                DiagnosticSeverity::Warning,
                category::PEER_CONNECTION,
                "incoming_listener_bind_failed",
                None,
                "Incoming loopback listener could not start; settings remain available",
                &[("reason", reason), ("detail", detail)],
            )?;
        }
        service.refresh_views()?;
        service.restore_removals().await?;
        service.restore_running().await?;
        service.reconcile_incoming_catalog().await?;
        service.reconcile_discovery_catalog().await?;
        service.refresh_views()?;
        service.reachability = Some(ReachabilityCoordinator::start(
            &active_client_settings,
            &listener_status,
            service.views.clone(),
            service.advertised_endpoint.clone(),
        ));
        Ok(service)
    }

    fn active_runtime(&self) -> Option<&TorrentRuntime> {
        self.active_torrent
            .as_ref()
            .and_then(|torrent_id| self.torrent_runtimes.get(torrent_id))
    }

    fn active_download(&self) -> Option<(&str, &ActiveDownload)> {
        self.active_runtime().and_then(|runtime| {
            runtime
                .active_download()
                .map(|active| (runtime.torrent_id(), active))
        })
    }

    fn ensure_torrent_runtime(
        &mut self,
        torrent_id: &str,
    ) -> Result<&mut TorrentRuntime, ApplicationError> {
        if !self.torrent_runtimes.contains_key(torrent_id) {
            let generation = self.next_torrent_generation;
            self.next_torrent_generation =
                self.next_torrent_generation.checked_add(1).ok_or_else(|| {
                    ApplicationError::Configuration(
                        "torrent runtime generation overflow".to_owned(),
                    )
                })?;
            let runtime = TorrentRuntime::new(
                torrent_id.to_owned(),
                generation,
                self.views.clone(),
                self.advertised_endpoint.clone(),
            )
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            self.torrent_runtimes.insert(torrent_id.to_owned(), runtime);
        }
        Ok(self
            .torrent_runtimes
            .get_mut(torrent_id)
            .expect("torrent runtime exists after insertion"))
    }

    fn torrent_peers(
        &mut self,
        torrent_id: &str,
    ) -> Result<rstorrent_engine::TorrentPeerHandle, ApplicationError> {
        Ok(self.ensure_torrent_runtime(torrent_id)?.peers())
    }

    fn install_active_download(
        &mut self,
        torrent_id: &str,
        active: ActiveDownload,
    ) -> Result<(), ApplicationError> {
        if let Some(active_torrent) = &self.active_torrent {
            return Err(ApplicationError::Busy(active_torrent.clone()));
        }
        self.ensure_torrent_runtime(torrent_id)?
            .set_active_download(active);
        self.active_torrent = Some(torrent_id.to_owned());
        Ok(())
    }

    fn take_active_download(&mut self) -> Option<(String, ActiveDownload)> {
        let torrent_id = self.active_torrent.take()?;
        let active = self
            .torrent_runtimes
            .get_mut(&torrent_id)
            .and_then(TorrentRuntime::take_active_download)
            .expect("active torrent points to an active download");
        Some((torrent_id, active))
    }

    pub async fn dispatch(
        &mut self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, ApplicationError> {
        self.reap_finished().await?;
        let command = request.command.clone();
        let file_priority_changed = match &command {
            Command::SetFilePriority {
                torrent_id,
                file_indices,
                priority,
            } => self
                .store_mut()?
                .load_resume(&torrent_id.to_ascii_lowercase())
                .map(|resume| {
                    file_indices.iter().any(|file_index| {
                        let skipped = resume.skip_files.binary_search(file_index).is_ok();
                        skipped != matches!(priority, FilePriority::Skip)
                    })
                })
                .unwrap_or(true),
            Command::SetFilePriorityRanges { .. } => true,
            _ => false,
        };
        let selected_root = match &command {
            Command::AddMagnet { storage_root, .. }
            | Command::SetDefaultStorageRoot { storage_root } => Some(storage_root.as_str()),
            _ => None,
        };
        if let Some(storage_root) = selected_root
            && !self.storage_roots.contains_key(storage_root)
        {
            let snapshot = self.store_mut()?.snapshot()?;
            let known = snapshot
                .storage
                .roots
                .iter()
                .any(|root| root.root_id == storage_root);
            return Ok(ResponseEnvelope::error(
                request.request_id,
                self.store_mut()?.revision()?,
                if known {
                    ErrorCode::StorageNeedsRepair
                } else {
                    ErrorCode::UnknownStorageRoot
                },
                if known {
                    format!("storage root {storage_root} is unavailable and needs repair")
                } else {
                    format!("storage root {storage_root} is not configured")
                },
            ));
        }
        if let Some((active_torrent, _)) = self.active_download() {
            let active_torrent = active_torrent.to_owned();
            let target = match &command {
                Command::AddMagnet { magnet, .. } => {
                    rstorrent_protocol::magnet::Magnet::parse(magnet)
                        .ok()
                        .map(|magnet| encode_info_hash(magnet.info_hash))
                }
                Command::Resume { torrent_id } => Some(torrent_id.to_ascii_lowercase()),
                Command::ForceRecheck { torrent_id } => Some(torrent_id.to_ascii_lowercase()),
                Command::SetFilePriority { torrent_id, .. }
                | Command::SetFilePriorityRanges { torrent_id, .. } => {
                    Some(torrent_id.to_ascii_lowercase())
                }
                _ => None,
            };
            if target.is_some_and(|target| target != active_torrent) {
                return Ok(ResponseEnvelope::error(
                    request.request_id,
                    self.store_mut()?.revision()?,
                    ErrorCode::Busy,
                    format!("torrent {} already owns the download slot", active_torrent),
                ));
            }
        }
        let incoming_fence = match &command {
            Command::Pause { torrent_id }
            | Command::ForceRecheck { torrent_id }
            | Command::Archive { torrent_id }
            | Command::RemoveTorrent { torrent_id, .. } => Some(torrent_id.to_ascii_lowercase()),
            Command::SetFilePriority { torrent_id, .. }
            | Command::SetFilePriorityRanges { torrent_id, .. }
                if file_priority_changed =>
            {
                Some(torrent_id.to_ascii_lowercase())
            }
            _ => None,
        };
        if let Some(torrent_id) = incoming_fence.as_deref() {
            self.stop_discovery_torrent(torrent_id).await?;
            self.unregister_incoming(torrent_id).await?;
        }
        let revision_before = self.store_mut()?.revision()?;
        let durable_result = {
            let mut store = self.store_mut()?;
            store.handle_durable(&request)
        };
        let response = match durable_result {
            Ok(response) => response,
            Err(error) => {
                if error.is_resource_limit() {
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Error,
                        category::STORAGE_IO,
                        "application_state_resource_limit",
                        None,
                        "Application state reached its configured resource limit",
                        &[],
                    );
                }
                if let Some(torrent_id) = incoming_fence.as_deref() {
                    self.reconcile_incoming_torrent(torrent_id).await?;
                    self.reconcile_discovery_torrent(torrent_id).await?;
                }
                return Err(error.into());
            }
        };
        if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
            if let Some(torrent_id) = incoming_fence.as_deref() {
                self.reconcile_incoming_torrent(torrent_id).await?;
                self.reconcile_discovery_torrent(torrent_id).await?;
            }
            return Ok(response);
        }
        let durable_mutation_applied = response.revision.parse::<u64>().map_err(|_| {
            ApplicationError::Configuration(
                "durable response contains an invalid revision".to_owned(),
            )
        })? > revision_before;
        self.refresh_views()?;
        self.reconcile_discovery_catalog().await?;

        let shutting_down = matches!(&command, Command::Shutdown);
        match command {
            Command::AddMagnet { magnet, .. } => {
                let torrent_id = rstorrent_protocol::magnet::Magnet::parse(&magnet)
                    .map(|magnet| encode_info_hash(magnet.info_hash))
                    .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_added",
                    Some(&torrent_id),
                    "Torrent added to the session",
                    &[],
                )?;
                self.start_if_possible(&torrent_id).await?;
            }
            Command::Resume { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_resumed",
                    Some(&torrent_id),
                    "Torrent resume requested",
                    &[],
                )?;
                self.start_if_possible(&torrent_id).await?;
            }
            Command::Pause { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_paused",
                    Some(&torrent_id),
                    "Torrent pause requested",
                    &[],
                )?;
                self.pause(&torrent_id).await?;
            }
            Command::ForceRecheck { torrent_id } => {
                if !durable_mutation_applied {
                    self.reconcile_incoming_torrent(&torrent_id.to_ascii_lowercase())
                        .await?;
                    self.reconcile_discovery_torrent(&torrent_id.to_ascii_lowercase())
                        .await?;
                    return Ok(response);
                }
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.pause(&torrent_id).await?;
                self.refresh_views()?;
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::INTEGRITY_HASH,
                    "force_recheck_requested",
                    Some(&torrent_id),
                    "Managed content force recheck requested",
                    &[],
                )?;
                self.start_recheck_if_possible(&torrent_id).await?;
            }
            Command::SetFilePriority { torrent_id, .. }
            | Command::SetFilePriorityRanges { torrent_id, .. } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::STORAGE_IO,
                    "file_priority_changed",
                    Some(&torrent_id),
                    "Torrent file priority changed",
                    &[],
                )?;
                if file_priority_changed {
                    self.pause(&torrent_id).await?;
                    self.refresh_views()?;
                    self.start_if_possible(&torrent_id).await?;
                }
            }
            Command::Archive { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_archived",
                    Some(&torrent_id),
                    "Torrent archived",
                    &[],
                )?;
                self.reconcile_incoming_torrent(&torrent_id).await?;
            }
            Command::RestoreArchive { torrent_id } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_archive_restored",
                    Some(&torrent_id),
                    "Torrent restored from archive",
                    &[],
                )?;
                self.reconcile_incoming_torrent(&torrent_id).await?;
            }
            Command::RemoveTorrent { torrent_id, .. } => {
                let torrent_id = torrent_id.to_ascii_lowercase();
                self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "torrent_removal_started",
                    Some(&torrent_id),
                    "Torrent removal started",
                    &[],
                )?;
                self.drive_removal(&torrent_id).await?;
            }
            Command::RemoveStorageRoot { .. } => {
                self.reload_storage_roots()?;
            }
            Command::SetDefaultStorageRoot { .. }
            | Command::SetShowAddOptions { .. }
            | Command::SetClientSettings { .. } => {}
            Command::Shutdown => {
                self.shutdown().await?;
            }
            Command::Snapshot => {}
        }
        if !shutting_down {
            self.reconcile_discovery_catalog().await?;
        }
        Ok(response)
    }

    pub async fn add_torrent_bytes(
        &mut self,
        request: AddTorrentBytesRequest,
        source: Vec<u8>,
    ) -> Result<ResponseEnvelope, ApplicationError> {
        self.reap_finished().await?;
        if !self.storage_roots.contains_key(&request.storage_root) {
            let snapshot = self.store_mut()?.snapshot()?;
            let known = snapshot
                .storage
                .roots
                .iter()
                .any(|root| root.root_id == request.storage_root);
            return Ok(ResponseEnvelope::error(
                request.request_id,
                self.store_mut()?.revision()?,
                if known {
                    ErrorCode::StorageNeedsRepair
                } else {
                    ErrorCode::UnknownStorageRoot
                },
                if known {
                    format!(
                        "storage root {} is unavailable and needs repair",
                        request.storage_root
                    )
                } else {
                    format!("storage root {} is not configured", request.storage_root)
                },
            ));
        }

        let prepare_request = request.clone();
        let prepared = match tokio::task::spawn_blocking(move || {
            prepare_torrent_bytes(&prepare_request, source)
        })
        .await
        .map_err(|error| ApplicationError::Join(error.to_string()))?
        {
            Ok(prepared) => prepared,
            Err((code, message)) => {
                return Ok(ResponseEnvelope::error(
                    request.request_id,
                    self.store_mut()?.revision()?,
                    code,
                    message,
                ));
            }
        };
        let torrent_id = prepared.torrent_id();
        if let Some((active_torrent, _)) = self.active_download()
            && active_torrent != torrent_id
        {
            let active_torrent = active_torrent.to_owned();
            return Ok(ResponseEnvelope::error(
                request.request_id,
                self.store_mut()?.revision()?,
                ErrorCode::Busy,
                format!("torrent {} already owns the download slot", active_torrent),
            ));
        }

        let durable_result = self
            .store_mut()?
            .handle_prepared_torrent_bytes(&request, &prepared);
        let response = match durable_result {
            Ok(response) => response,
            Err(error) => {
                if error.is_resource_limit() {
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Error,
                        category::STORAGE_IO,
                        "application_state_resource_limit",
                        None,
                        "Application state reached its configured resource limit",
                        &[],
                    );
                }
                return Err(error.into());
            }
        };
        if !matches!(response.outcome, ResponseOutcome::Success { .. }) {
            return Ok(response);
        }
        self.refresh_views()?;
        self.reconcile_discovery_catalog().await?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "torrent_bytes_added",
            Some(&torrent_id),
            "Torrent metainfo bytes added to the session",
            &[],
        )?;
        self.start_if_possible(&torrent_id).await?;
        self.reconcile_discovery_torrent(&torrent_id).await?;
        Ok(response)
    }

    pub fn revision(&self) -> Result<u64, ApplicationError> {
        Ok(self.store_mut()?.revision()?)
    }

    pub fn storage_snapshot(&self) -> Result<crate::StorageSettingsSnapshot, ApplicationError> {
        Ok(self.store_mut()?.snapshot()?.storage)
    }

    pub fn incoming_peer_snapshot(&self) -> Option<IncomingPeerServiceSnapshot> {
        self.incoming_service
            .as_ref()
            .map(IncomingPeerService::snapshot)
    }

    pub fn suggested_storage_root_path(
        &self,
        repair_root: Option<&str>,
    ) -> Result<Option<PathBuf>, ApplicationError> {
        let store = self.store_mut()?;
        let snapshot = store.snapshot()?;
        let roots = store.storage_roots()?;
        if let Some(root_id) = repair_root {
            let root = snapshot
                .storage
                .roots
                .iter()
                .find(|root| root.root_id == root_id)
                .ok_or_else(|| {
                    ApplicationError::Configuration(format!(
                        "storage root {root_id} is not configured"
                    ))
                })?;
            if root.availability == crate::StorageRootAvailability::Available {
                return Err(ApplicationError::Configuration(
                    "an available root cannot be re-selected; torrent relocation is not implemented"
                        .to_owned(),
                ));
            }
        }
        let preferred = repair_root
            .map(str::to_owned)
            .or(snapshot.storage.default_root);
        let candidate = preferred
            .as_deref()
            .and_then(|root_id| roots.iter().find(|root| root.id == root_id))
            .or_else(|| {
                roots.iter().find(|root| {
                    matches!(&root.location, StorageRootLocation::Path(path) if path.is_dir())
                })
            });
        Ok(candidate.and_then(|root| match &root.location {
            StorageRootLocation::Path(path) if path.is_dir() => Some(path.clone()),
            StorageRootLocation::Path(path) => path
                .ancestors()
                .skip(1)
                .find(|ancestor| ancestor.is_dir())
                .map(Path::to_path_buf),
            StorageRootLocation::PlatformCapability => None,
        }))
    }

    pub fn install_path_storage_root(
        &mut self,
        selected_path: &Path,
    ) -> Result<StorageRootSnapshot, ApplicationError> {
        let path = validate_selected_directory(selected_path)?;
        let label = storage_root_label(&path);
        let root_id = self.allocate_storage_root_id()?;
        let (_, installed_id) = self
            .store_mut()?
            .install_path_storage_root(&root_id, &label, &path)?;
        self.reload_storage_roots()?;
        self.refresh_views()?;
        self.storage_snapshot()?
            .roots
            .into_iter()
            .find(|root| root.root_id == installed_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(
                    "installed storage root is missing from the durable snapshot".to_owned(),
                )
            })
    }

    pub fn repair_path_storage_root(
        &mut self,
        root_id: &str,
        selected_path: &Path,
    ) -> Result<StorageRootSnapshot, ApplicationError> {
        let snapshot = self.storage_snapshot()?;
        let root = snapshot
            .roots
            .iter()
            .find(|root| root.root_id == root_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(format!("storage root {root_id} is not configured"))
            })?;
        if root.availability == crate::StorageRootAvailability::Available {
            return Err(ApplicationError::Configuration(
                "an available root cannot be re-selected; torrent relocation is not implemented"
                    .to_owned(),
            ));
        }
        if let Some((active_torrent, _)) = self.active_download() {
            let active_torrent = active_torrent.to_owned();
            let resume = self.store_mut()?.load_resume(&active_torrent)?;
            if resume.storage_root == root_id {
                return Err(ApplicationError::Busy(active_torrent));
            }
        }
        let path = validate_selected_directory(selected_path)?;
        let label = storage_root_label(&path);
        self.store_mut()?
            .repair_path_storage_root(root_id, &label, &path)?;
        self.reload_storage_roots()?;
        self.refresh_views()?;
        self.storage_snapshot()?
            .roots
            .into_iter()
            .find(|root| root.root_id == root_id)
            .ok_or_else(|| {
                ApplicationError::Configuration(
                    "repaired storage root is missing from the durable snapshot".to_owned(),
                )
            })
    }

    fn allocate_storage_root_id(&self) -> Result<String, ApplicationError> {
        let existing = self
            .store_mut()?
            .storage_roots()?
            .into_iter()
            .map(|root| root.id)
            .collect::<std::collections::BTreeSet<_>>();
        for _ in 0..4 {
            let mut bytes = [0_u8; 16];
            getrandom::fill(&mut bytes)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let mut id = String::with_capacity(37);
            id.push_str("root_");
            for byte in bytes {
                use std::fmt::Write as _;
                write!(&mut id, "{byte:02x}").expect("writing to String cannot fail");
            }
            if !existing.contains(&id) {
                return Ok(id);
            }
        }
        Err(ApplicationError::Configuration(
            "could not allocate a unique storage root ID".to_owned(),
        ))
    }

    fn reload_storage_roots(&mut self) -> Result<(), ApplicationError> {
        let roots = self.store_mut()?.storage_roots()?;
        self.storage_roots = Arc::new(available_storage_roots(roots));
        Ok(())
    }

    pub fn api_hello(&self) -> crate::ApiHello {
        let mut hello = crate::ApiHello::default();
        hello.limits.lease_millis = self.views.view_set_lease().as_millis().to_string();
        hello
    }

    pub fn subscribe(&self, spec: SubscriptionSpec) -> Result<ViewSubscription, ApplicationError> {
        Ok(self.views.subscribe(spec)?)
    }

    pub fn open_view_set(
        &self,
        owner: ViewSetOwner,
        request: OpenViewSetRequest,
    ) -> Result<OpenViewSetResponse, ViewSetError> {
        self.views.open_view_set(owner, request)
    }

    pub fn update_view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
        request: UpdateViewSetRequest,
    ) -> Result<(), ViewSetError> {
        self.views.update_view_set(owner, view_set_id, request)
    }

    pub fn view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
    ) -> Result<ViewSet, ViewSetError> {
        self.views.view_set(owner, view_set_id)
    }

    pub fn close_view_set(
        &self,
        owner: &ViewSetOwner,
        view_set_id: &str,
    ) -> Result<(), ViewSetError> {
        self.views.close_view_set(owner, view_set_id)
    }

    pub fn close_view_sets(&self) {
        self.views.close_all_view_sets();
    }

    pub async fn descriptor_storage_plan(
        &mut self,
        torrent_id: &str,
    ) -> Result<DescriptorStoragePlan, ApplicationError> {
        self.reap_finished().await?;
        let resume = self.load_resume_conservative(&torrent_id.to_ascii_lowercase())?;
        if !matches!(
            self.storage_roots.get(&resume.storage_root),
            Some(StorageRootLocation::PlatformCapability)
        ) {
            return Err(ApplicationError::Configuration(
                "torrent does not use a platform storage root".to_owned(),
            ));
        }
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = parse_durable_metainfo(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        require_publication_name(resume.publication_name.as_deref(), &metainfo)?;
        let skip_files = resume
            .skip_files
            .into_iter()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
        plan_descriptor_storage(&metainfo, &skip_files, &[])
            .map_err(|error| ApplicationError::Configuration(error.to_string()))
    }

    pub async fn start_with_descriptors(
        &mut self,
        torrent_id: &str,
        descriptors: DescriptorStorage,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        if let Some((active_torrent, _)) = self.active_download() {
            return Err(ApplicationError::Busy(active_torrent.to_owned()));
        }
        let resume = self.load_resume_conservative(&torrent_id)?;
        if !resume.desired_running
            || matches!(
                resume.state,
                TorrentState::Paused | TorrentState::Complete | TorrentState::AwaitingPublication
            )
        {
            return Ok(());
        }
        if resume.state == TorrentState::NeedsRepair {
            return Err(ApplicationError::Configuration(format!(
                "torrent cannot accept storage in state {}",
                resume.state.as_str()
            )));
        }
        if !matches!(
            self.storage_roots.get(&resume.storage_root),
            Some(StorageRootLocation::PlatformCapability)
        ) {
            return Err(ApplicationError::Configuration(
                "torrent does not use a platform storage root".to_owned(),
            ));
        }
        let artifact_state = resume_artifact_state(&resume)?;
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = parse_durable_metainfo(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        require_publication_name(resume.publication_name.as_deref(), &metainfo)?;
        let initialize_storage = resume.storage_state == StorageState::None;
        let skip_files = resume
            .skip_files
            .into_iter()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
        let verified_pieces = resume
            .have
            .as_ref()
            .map_or_else(Vec::new, |have| have.pieces().to_vec());
        let torrent_peers = self.torrent_peers(&torrent_id)?;
        let config = ResumableMagnetDownloadConfig {
            magnet: resume.magnet,
            storage_root: PathBuf::new(),
            network: self.network,
            peer_budget: self.peer_budget.clone(),
            torrent_peers: Some(torrent_peers),
            resource_limits: self.download_resource_limits,
            skip_files,
            verified_info: Some(raw_info),
            verified_pieces,
            artifact_state,
            download_missing: true,
            dht: None,
            udp_trackers: Some(Vec::new()),
        };
        let checkpoints: Arc<dyn DownloadCheckpointSink> = Arc::new(StoreCheckpointSink {
            store: self.store.clone(),
            storage_roots: self.storage_roots.clone(),
            torrent_id: torrent_id.clone(),
            views: self.views.clone(),
        });
        let control = self.download_control(&torrent_id);
        let task_control = control.clone();
        let operation = async move {
            resume_magnet_to_descriptors_with_control(
                config,
                descriptors,
                initialize_storage,
                checkpoints,
                task_control,
            )
            .await
            .map(|_| ApplicationTaskReport::Download)
        };
        let task = self.spawn_supervised_task(&torrent_id, operation)?;
        self.install_active_download(&torrent_id, ActiveDownload { control, task })?;
        Ok(())
    }

    pub async fn prepared_files(
        &mut self,
        torrent_id: &str,
    ) -> Result<Vec<PreparedFileRecord>, ApplicationError> {
        self.reap_finished().await?;
        Ok(self
            .store_mut()?
            .load_prepared_files(&torrent_id.to_ascii_lowercase())?)
    }

    pub fn storage_file_pool_snapshot(&self) -> StorageFilePoolSnapshot {
        self.storage_file_pool.snapshot()
    }

    pub async fn prepare_platform_publication(
        &mut self,
        torrent_id: &str,
    ) -> Result<String, ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let resume = self.load_resume_conservative(&torrent_id)?;
        if resume.state != TorrentState::AwaitingPublication
            || !matches!(
                self.storage_roots.get(&resume.storage_root),
                Some(StorageRootLocation::PlatformCapability)
            )
        {
            return Err(ApplicationError::Configuration(
                "torrent is not awaiting platform publication".to_owned(),
            ));
        }
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = parse_durable_metainfo(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        self.storage_file_pool.invalidate_storage(&torrent_id);
        Ok(metainfo.name)
    }

    pub async fn confirm_platform_publication(
        &mut self,
        torrent_id: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let resume = self.load_resume_conservative(&torrent_id)?;
        if resume.state == TorrentState::Complete && resume.storage_state == StorageState::Published
        {
            return Ok(());
        }
        if resume.state != TorrentState::AwaitingPublication
            || !matches!(
                self.storage_roots.get(&resume.storage_root),
                Some(StorageRootLocation::PlatformCapability)
            )
        {
            return Err(ApplicationError::Configuration(
                "torrent is not awaiting platform publication".to_owned(),
            ));
        }
        let raw_info = resume.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = parse_durable_metainfo(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        let expected = self
            .store_mut()?
            .load_prepared_files(&torrent_id)?
            .into_iter()
            .map(|file| PreparedFileHash {
                file_index: file.file_index,
                length: file.length,
                sha1: file.sha1,
            })
            .collect::<Vec<_>>();
        if expected.is_empty() {
            return Err(ApplicationError::Configuration(
                "torrent has no prepared publication manifest".to_owned(),
            ));
        }
        verify_prepared_platform_files(
            &PlatformStorageSpec {
                pool: self.storage_file_pool.clone(),
                root_id: resume.storage_root,
                storage_id: torrent_id.clone(),
                publication_shape: PublicationShape::from_metainfo(&metainfo),
                publication_name: metainfo.name.clone(),
                namespace_generation: 1,
                managed: true,
                published: true,
            },
            &metainfo,
            &expected,
        )
        .await
        .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        self.store_mut()?.begin_published_recheck(&torrent_id)?;
        self.storage_file_pool.invalidate_storage(&torrent_id);
        self.refresh_views()?;
        self.start_recheck_if_possible(&torrent_id).await?;
        Ok(())
    }

    pub async fn confirm_descriptor_publication(
        &mut self,
        torrent_id: &str,
        descriptors: Vec<DescriptorFile>,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let resume = self.load_resume_conservative(&torrent_id)?;
        if resume.state == TorrentState::Complete && resume.storage_state == StorageState::Published
        {
            return Ok(());
        }
        let prepared = self.store_mut()?.load_prepared_files(&torrent_id)?;
        let expected = prepared
            .into_iter()
            .map(|file| PreparedFileHash {
                file_index: file.file_index,
                length: file.length,
                sha1: file.sha1,
            })
            .collect::<Vec<_>>();
        if expected.is_empty() {
            return Err(ApplicationError::Configuration(
                "torrent has no prepared publication manifest".to_owned(),
            ));
        }
        verify_prepared_descriptors(descriptors, &expected)
            .await
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        self.store_mut()?
            .confirm_prepared_publication(&torrent_id)?;
        self.refresh_views()?;
        Ok(())
    }

    pub async fn mark_storage_unavailable(
        &mut self,
        torrent_id: &str,
        message: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        if self.active_torrent.as_deref() == Some(torrent_id.as_str()) {
            return Err(ApplicationError::Busy(torrent_id));
        }
        self.store_mut()?
            .mark_awaiting_storage(&torrent_id, Some(message))?;
        self.storage_file_pool.invalidate_storage(&torrent_id);
        self.refresh_views()?;
        Ok(())
    }

    pub async fn prepare_platform_storage_replacement(
        &mut self,
        root_id: &str,
    ) -> Result<Option<String>, ApplicationError> {
        self.reap_finished().await?;
        if !matches!(
            self.storage_roots.get(root_id),
            Some(StorageRootLocation::PlatformCapability)
        ) {
            return Err(ApplicationError::Configuration(format!(
                "storage root {root_id} is not a platform capability"
            )));
        }
        let restart = if let Some((active_torrent, _)) = self.active_download() {
            let torrent_id = active_torrent.to_owned();
            let resume = self.load_resume_conservative(&torrent_id)?;
            (resume.storage_root == root_id).then_some(torrent_id)
        } else {
            None
        };
        if let Some(torrent_id) = restart.as_deref() {
            self.pause(torrent_id).await?;
        }
        self.storage_file_pool.invalidate_all();
        Ok(restart)
    }

    pub async fn platform_removal_plan(
        &mut self,
        torrent_id: &str,
    ) -> Result<PlatformRemovalPlan, ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.policy != RemovalDataPolicy::DeleteManaged
            || removal.state != RemovalState::AwaitingPlatform
            || !matches!(
                self.storage_roots.get(&removal.storage_root),
                Some(StorageRootLocation::PlatformCapability)
            )
        {
            return Err(ApplicationError::Configuration(
                "torrent is not awaiting platform data removal".to_owned(),
            ));
        }
        let raw_info = removal.raw_info.ok_or_else(|| {
            ApplicationError::Configuration("torrent metadata is not available".to_owned())
        })?;
        let metainfo = parse_durable_metainfo(&raw_info)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        if let Some(publication_name) = removal.publication_name.as_deref() {
            require_publication_name(Some(publication_name), &metainfo)?;
        }
        self.storage_file_pool.invalidate_storage(&torrent_id);
        let plan = plan_descriptor_storage(&metainfo, &[], &[])
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        Ok(PlatformRemovalPlan {
            operation_id: removal.operation_id,
            torrent_id,
            storage_root: removal.storage_root,
            name: plan.name,
        })
    }

    pub async fn confirm_platform_removal(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.state != RemovalState::AwaitingPlatform || removal.operation_id != operation_id {
            return Err(ApplicationError::Configuration(
                "platform removal confirmation is stale".to_owned(),
            ));
        }
        self.prepare_torrent_runtime_removal(&torrent_id)?;
        self.store_mut()?
            .finalize_removal(&torrent_id, operation_id)?;
        self.torrent_runtimes.remove(&torrent_id);
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::PLATFORM_ADAPTER,
            "torrent_removal_completed",
            Some(&torrent_id),
            "Platform-managed torrent data and catalog entry removed",
            &[],
        )?;
        Ok(())
    }

    pub async fn fail_platform_removal(
        &mut self,
        torrent_id: &str,
        operation_id: &str,
        message: &str,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        let torrent_id = torrent_id.to_ascii_lowercase();
        let removal = self.store_mut()?.load_removal(&torrent_id)?;
        if removal.state != RemovalState::AwaitingPlatform || removal.operation_id != operation_id {
            return Err(ApplicationError::Configuration(
                "platform removal failure is stale".to_owned(),
            ));
        }
        self.store_mut()?.set_removal_state(
            &torrent_id,
            operation_id,
            RemovalState::Failed,
            Some(message),
        )?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Error,
            category::PLATFORM_ADAPTER,
            "torrent_removal_failed",
            Some(&torrent_id),
            "Platform-managed torrent data could not be removed",
            &[("detail", message)],
        )?;
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), ApplicationError> {
        let mut active_join_error = None;
        let mut shutdown_error = None;
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
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_PEER,
                        "discovery_advertisement_service_stopped",
                        None,
                        "Discovery and advertisement service stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => {
                    active_join_error = Some(format!("discovery advertisement: {error}"));
                }
            }
        }
        if let Some(reachability) = self.reachability.take() {
            match reachability.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts =
                        format!("tasks={},mappings={}", terminal.tasks, terminal.mappings);
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_REACHABILITY,
                        "reachability_coordinator_stopped",
                        None,
                        "Incoming reachability coordinator stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => {
                    active_join_error = Some(format!("reachability coordinator: {error}"));
                }
            }
        }
        if let Some(incoming) = self.incoming_seeding.take() {
            incoming.stop();
        }
        if let Some((_, active)) = self.active_download() {
            active.control.cancel();
        }
        if let Some(incoming) = self.incoming_service.take() {
            match incoming.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts = format!(
                        "pending={},established={},reads={},registrations={}",
                        terminal.pending,
                        terminal.established,
                        terminal.reads,
                        terminal.registrations
                    );
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::PEER_CONNECTION,
                        "incoming_peer_service_stopped",
                        None,
                        "Incoming peer service stopped with joined owners",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) => active_join_error = Some(format!("incoming peer service: {error}")),
            }
        }
        if let Some((_, active)) = self.take_active_download() {
            match active.task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if active_join_error.is_none() => active_join_error = Some(error),
                Err(error) if error.is_cancelled() => {}
                Err(error) if active_join_error.is_none() => {
                    active_join_error = Some(error.to_string());
                }
                Ok(Err(_)) | Err(_) => {}
            }
        }
        for runtime in self.torrent_runtimes.values_mut() {
            if let Err(error) = runtime.handle().forget_seed_registration()
                && active_join_error.is_none()
            {
                active_join_error = Some(format!("torrent seed registration: {error}"));
            }
            if let Err(error) = runtime.publish_inactive()
                && active_join_error.is_none()
            {
                active_join_error = Some(format!("torrent peer state: {error}"));
            }
            runtime.deactivate_peer_events();
        }
        self.torrent_runtimes.clear();
        if let Err(error) = self.storage_file_pool.shutdown().await
            && active_join_error.is_none()
        {
            active_join_error = Some(format!("storage file pool: {error}"));
        }
        if let Some(dht) = self.dht.take() {
            match dht.shutdown().await {
                Ok(snapshot) => {
                    if let Err(error) = self
                        .store_mut()
                        .and_then(|mut store| store.save_dht_snapshot(snapshot).map_err(Into::into))
                    {
                        shutdown_error = Some(error);
                    }
                }
                Err(error) => shutdown_error = Some(error.into()),
            }
        }
        if let Some(udp) = self.session_udp.take() {
            match udp.shutdown().await {
                Ok(terminal) => {
                    let terminal_counts = format!(
                        "tasks={},queued={},dropped={}",
                        terminal.tasks, terminal.queued, terminal.datagrams_dropped
                    );
                    let _ = self.views.record_diagnostic(
                        DiagnosticSeverity::Info,
                        category::DISCOVERY_DHT,
                        "session_udp_service_stopped",
                        None,
                        "Session UDP service stopped with joined ownership",
                        &[("terminal_counts", &terminal_counts)],
                    );
                }
                Err(error) if active_join_error.is_none() => {
                    active_join_error = Some(format!("session UDP service: {error}"));
                }
                Err(_) => {}
            }
        }
        if let Some(observations) = self.dht_observations.take() {
            match observations.join().await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if active_join_error.is_none() => {
                    active_join_error = Some(format!("DHT observation forwarder: {error}"));
                }
                Err(error) if active_join_error.is_none() => {
                    active_join_error = Some(format!("DHT observation forwarder: {error}"));
                }
                Ok(Err(_)) | Err(_) => {}
            }
        }
        if let Some(speed_history) = self.speed_history.take()
            && let Err(error) = speed_history.shutdown().await
            && active_join_error.is_none()
        {
            active_join_error = Some(format!("speed history: {error}"));
        }
        let _ = self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "application_shutdown",
            None,
            "Application shutdown completed",
            &[],
        );
        if let Some(mut reaper) = self.view_set_reaper.take()
            && let Err(error) = reaper.shutdown().await
            && active_join_error.is_none()
        {
            active_join_error = Some(format!("view-set lease reaper: {error}"));
        }
        self.close_view_sets();
        if let Some(error) = shutdown_error {
            return Err(error);
        }
        if let Some(error) = active_join_error {
            return Err(ApplicationError::Join(error));
        }
        Ok(())
    }

    fn store_mut(&self) -> Result<MutexGuard<'_, SessionStore>, ApplicationError> {
        self.store
            .lock()
            .map_err(|_| ApplicationError::StorePoisoned)
    }

    async fn restore_running(&mut self) -> Result<(), ApplicationError> {
        let torrent_ids = self
            .store_mut()?
            .snapshot()?
            .torrents
            .into_iter()
            .filter(|torrent| !matches!(torrent.state, TorrentState::Paused))
            .map(|torrent| torrent.torrent_id)
            .collect::<Vec<_>>();
        for torrent_id in torrent_ids {
            let (should_start, force_recheck) = match self.load_resume_conservative(&torrent_id) {
                Ok(resume) => (
                    resume.desired_running
                        || resume.raw_info.is_none()
                        || resume.state == TorrentState::Checking,
                    resume.raw_info.is_some() && resume.state == TorrentState::Checking,
                ),
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(&torrent_id, &error.to_string())?;
                    continue;
                }
            };
            if should_start {
                if force_recheck {
                    self.start_recheck_if_possible(&torrent_id).await?;
                } else {
                    self.start_if_possible(&torrent_id).await?;
                }
                break;
            }
        }
        Ok(())
    }

    async fn restore_removals(&mut self) -> Result<(), ApplicationError> {
        let removals = self.store_mut()?.load_removals()?;
        for removal in removals {
            if removal.state == RemovalState::Pending {
                self.drive_removal(&removal.torrent_id).await?;
            }
        }
        Ok(())
    }

    async fn drive_removal(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        let removal = match self.store_mut()?.load_removal(torrent_id) {
            Ok(removal) => removal,
            Err(StoreError::UnknownTorrent(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if removal.state != RemovalState::Pending {
            return Ok(());
        }
        if let Err(error) = self.pause(torrent_id).await {
            return self.fail_removal(&removal, &error.to_string());
        }
        match removal.policy {
            RemovalDataPolicy::Keep => self.complete_removal(&removal),
            RemovalDataPolicy::DeleteManaged if removal.raw_info.is_none() => {
                self.complete_removal(&removal)
            }
            RemovalDataPolicy::DeleteManaged => {
                match self.storage_roots.get(&removal.storage_root).cloned() {
                    Some(StorageRootLocation::Path(root)) => {
                        let publication_shape = match removal
                            .raw_info
                            .as_deref()
                            .map(parse_durable_metainfo)
                            .transpose()
                        {
                            Ok(Some(metainfo)) => PublicationShape::from_metainfo(&metainfo),
                            Ok(None) => PublicationShape::Tree,
                            Err(error) => {
                                return self.fail_removal(&removal, &error.to_string());
                            }
                        };
                        let owned_torrent_id = torrent_id.to_owned();
                        let publication_name = removal.publication_name.clone();
                        match tokio::task::spawn_blocking(move || {
                            delete_path_artifacts(
                                &root,
                                &owned_torrent_id,
                                publication_name.as_deref(),
                                publication_shape,
                                removal.storage_state,
                                removal.managed_artifacts,
                            )
                        })
                        .await
                        {
                            Ok(Ok(())) => self.complete_removal(&removal),
                            Ok(Err(error)) => self.fail_removal(&removal, &error.to_string()),
                            Err(error) => self.fail_removal(
                                &removal,
                                &format!("managed-data cleanup task failed: {error}"),
                            ),
                        }
                    }
                    Some(StorageRootLocation::PlatformCapability) => {
                        self.store_mut()?.set_removal_state(
                            torrent_id,
                            &removal.operation_id,
                            RemovalState::AwaitingPlatform,
                            None,
                        )?;
                        self.refresh_views()
                    }
                    None => self.fail_removal(
                        &removal,
                        "configured storage root is unavailable for removal",
                    ),
                }
            }
        }
    }

    fn complete_removal(&mut self, removal: &RemovalRecord) -> Result<(), ApplicationError> {
        self.prepare_torrent_runtime_removal(&removal.torrent_id)?;
        self.store_mut()?
            .finalize_removal(&removal.torrent_id, &removal.operation_id)?;
        self.torrent_runtimes.remove(&removal.torrent_id);
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "torrent_removal_completed",
            Some(&removal.torrent_id),
            "Torrent removed from the session",
            &[],
        )?;
        Ok(())
    }

    fn prepare_torrent_runtime_removal(
        &mut self,
        torrent_id: &str,
    ) -> Result<(), ApplicationError> {
        if self.active_torrent.as_deref() == Some(torrent_id) {
            return Err(ApplicationError::Busy(torrent_id.to_owned()));
        }
        let Some(runtime) = self.torrent_runtimes.get(torrent_id) else {
            return Ok(());
        };
        if runtime.handle().has_seed_registration() {
            return Err(ApplicationError::Configuration(format!(
                "torrent {torrent_id} still owns an incoming seed registration"
            )));
        }
        runtime
            .publish_inactive()
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        runtime.deactivate_peer_events();
        Ok(())
    }

    fn fail_removal(&self, removal: &RemovalRecord, message: &str) -> Result<(), ApplicationError> {
        self.store_mut()?.set_removal_state(
            &removal.torrent_id,
            &removal.operation_id,
            RemovalState::Failed,
            Some(message),
        )?;
        self.refresh_views()?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Error,
            category::STORAGE_IO,
            "torrent_removal_failed",
            Some(&removal.torrent_id),
            "Torrent data could not be removed",
            &[("detail", message)],
        )?;
        Ok(())
    }

    async fn start_if_possible(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        self.start_if_possible_with_mode(torrent_id, false).await
    }

    async fn start_recheck_if_possible(
        &mut self,
        torrent_id: &str,
    ) -> Result<(), ApplicationError> {
        self.start_if_possible_with_mode(torrent_id, true).await
    }

    async fn start_if_possible_with_mode(
        &mut self,
        torrent_id: &str,
        force_recheck: bool,
    ) -> Result<(), ApplicationError> {
        self.reap_finished().await?;
        self.unregister_incoming(torrent_id).await?;
        if let Some((active_torrent, _)) = self.active_download() {
            if active_torrent == torrent_id {
                return Ok(());
            }
            return Err(ApplicationError::Busy(active_torrent.to_owned()));
        }
        let torrent_peers = self.torrent_peers(torrent_id)?;
        let resume = match self.load_resume_conservative(torrent_id) {
            Ok(resume) => resume,
            Err(error) => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, &error.to_string())?;
                return Ok(());
            }
        };
        if resume.state == TorrentState::NeedsRepair
            || (!force_recheck
                && resume.raw_info.is_some()
                && (!resume.desired_running || resume.state == TorrentState::Paused))
        {
            return Ok(());
        }
        if let Some(raw_info) = &resume.raw_info {
            let metainfo = match parse_durable_metainfo(raw_info) {
                Ok(metainfo) => metainfo,
                Err(error) => {
                    self.store_mut()?
                        .mark_needs_repair(torrent_id, &error.to_string())?;
                    return Ok(());
                }
            };
            if encode_info_hash(metainfo.info_hash) != torrent_id {
                self.store_mut()?.mark_needs_repair(
                    torrent_id,
                    "stored metadata does not match torrent identity",
                )?;
                return Ok(());
            }
            if resume.publication_name.as_deref() != Some(metainfo.name.as_str()) {
                self.store_mut()?.mark_needs_repair(
                    torrent_id,
                    "stored publication name is missing or does not match verified metadata",
                )?;
                return Ok(());
            }
        }
        let root = match self.storage_roots.get(&resume.storage_root).cloned() {
            Some(root) => root,
            None => {
                self.store_mut()?
                    .mark_needs_repair(torrent_id, "configured storage root is unavailable")?;
                return Ok(());
            }
        };
        let platform_root = matches!(root, StorageRootLocation::PlatformCapability);
        if platform_root {
            if resume.state == TorrentState::AwaitingPublication {
                return Ok(());
            }
            if resume.raw_info.is_none() {
                let checkpoints = Arc::new(StoreCheckpointSink {
                    store: self.store.clone(),
                    storage_roots: self.storage_roots.clone(),
                    torrent_id: torrent_id.to_owned(),
                    views: self.views.clone(),
                });
                let control = self.download_control(torrent_id);
                let task_control = control.clone();
                let magnet = resume.magnet.clone();
                let continue_downloading = resume.desired_running;
                let skip_files = resume
                    .skip_files
                    .iter()
                    .map(|index| *index as usize)
                    .collect::<Vec<_>>();
                let root_id = resume.storage_root.clone();
                let storage_id = torrent_id.to_owned();
                let storage_pool = self.storage_file_pool.clone();
                let resource_limits = self.download_resource_limits;
                let network = self.network;
                let peer_budget = self.peer_budget.clone();
                let operation = async move {
                    let raw_info = download_magnet_metadata_with_external_discovery(
                        magnet.clone(),
                        network,
                        task_control.clone(),
                        peer_budget.clone(),
                        torrent_peers.clone(),
                    )
                    .await?;
                    checkpoints
                        .metadata_verified(&raw_info)
                        .map_err(DownloadError::Checkpoint)?;
                    if !continue_downloading {
                        return Ok(ApplicationTaskReport::Metadata);
                    }
                    let metainfo =
                        parse_peer_metainfo(&raw_info).map_err(DownloadError::Metainfo)?;
                    task_control.set_platform_storage(PlatformStorageSpec {
                        pool: storage_pool,
                        root_id,
                        storage_id,
                        publication_shape: PublicationShape::from_metainfo(&metainfo),
                        publication_name: metainfo.name,
                        namespace_generation: 0,
                        managed: false,
                        published: false,
                    });
                    resume_magnet_with_control(
                        ResumableMagnetDownloadConfig {
                            magnet,
                            storage_root: PathBuf::new(),
                            network,
                            peer_budget,
                            torrent_peers: Some(torrent_peers),
                            resource_limits,
                            skip_files,
                            verified_info: Some(raw_info),
                            verified_pieces: Vec::new(),
                            artifact_state: ResumeArtifactState::None,
                            download_missing: true,
                            dht: None,
                            udp_trackers: Some(Vec::new()),
                        },
                        checkpoints,
                        task_control,
                    )
                    .await
                    .map(|_| ApplicationTaskReport::Download)
                };
                let task = self.spawn_supervised_task(torrent_id, operation)?;
                self.install_active_download(torrent_id, ActiveDownload { control, task })?;
                return Ok(());
            }
        }
        let root_path = match &root {
            StorageRootLocation::Path(root) => root.clone(),
            StorageRootLocation::PlatformCapability => PathBuf::new(),
        };
        if !platform_root
            && resume.storage_state == StorageState::None
            && let Some(raw_info) = resume.raw_info.as_ref()
        {
            let metainfo = parse_durable_metainfo(raw_info)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let paths = torrent_storage_paths(&root_path, &metainfo.name, metainfo.info_hash)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let collision = [
                ("output", paths.output),
                ("staging output", paths.staging),
                ("part file", paths.part),
            ]
            .into_iter()
            .find(|(_, path)| std::fs::symlink_metadata(path).is_ok());
            if let Some((artifact, path)) = collision {
                self.store_mut()?.mark_needs_repair(
                    torrent_id,
                    &format!("{artifact} already exists: {}", path.display()),
                )?;
                self.refresh_views()?;
                return Ok(());
            }
        }
        if resume.raw_info.is_none() && !resume.desired_running {
            let checkpoints = Arc::new(StoreCheckpointSink {
                store: self.store.clone(),
                storage_roots: self.storage_roots.clone(),
                torrent_id: torrent_id.to_owned(),
                views: self.views.clone(),
            });
            let control = self.download_control(torrent_id);
            let task_control = control.clone();
            let magnet = resume.magnet;
            let network = self.network;
            let peer_budget = self.peer_budget.clone();
            let operation = async move {
                let raw_info = download_magnet_metadata_with_external_discovery(
                    magnet,
                    network,
                    task_control,
                    peer_budget,
                    torrent_peers,
                )
                .await?;
                checkpoints
                    .metadata_verified(&raw_info)
                    .map_err(DownloadError::Checkpoint)?;
                Ok(ApplicationTaskReport::Metadata)
            };
            let task = self.spawn_supervised_task(torrent_id, operation)?;
            self.install_active_download(torrent_id, ActiveDownload { control, task })?;
            return Ok(());
        }
        let skip_files = resume
            .skip_files
            .iter()
            .map(|index| {
                usize::try_from(*index).map_err(|_| {
                    ApplicationError::Configuration("file selection index overflow".to_owned())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let verified_pieces = resume
            .have
            .as_ref()
            .map_or_else(Vec::new, |have| have.pieces().to_vec());
        let artifact_state = resume_artifact_state(&resume)?;
        let config = ResumableMagnetDownloadConfig {
            magnet: resume.magnet,
            storage_root: root_path,
            network: self.network,
            peer_budget: self.peer_budget.clone(),
            torrent_peers: Some(torrent_peers),
            resource_limits: self.download_resource_limits,
            skip_files,
            verified_info: resume.raw_info,
            verified_pieces,
            artifact_state,
            download_missing: resume.desired_running,
            dht: None,
            udp_trackers: Some(Vec::new()),
        };
        let checkpoints: Arc<dyn DownloadCheckpointSink> = Arc::new(StoreCheckpointSink {
            store: self.store.clone(),
            storage_roots: self.storage_roots.clone(),
            torrent_id: torrent_id.to_owned(),
            views: self.views.clone(),
        });
        let control = self.download_control(torrent_id);
        if platform_root {
            let raw_info = config
                .verified_info
                .as_ref()
                .expect("platform content start requires verified metadata");
            let metainfo = parse_durable_metainfo(raw_info)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            control.set_platform_storage(PlatformStorageSpec {
                pool: self.storage_file_pool.clone(),
                root_id: resume.storage_root.clone(),
                storage_id: torrent_id.to_owned(),
                publication_shape: PublicationShape::from_metainfo(&metainfo),
                publication_name: metainfo.name,
                namespace_generation: u64::from(resume.storage_state == StorageState::Published),
                managed: resume.storage_state != StorageState::None,
                published: resume.storage_state == StorageState::Published,
            });
        }
        let task_control = control.clone();
        let operation = async move {
            resume_magnet_with_control(config, checkpoints, task_control)
                .await
                .map(|_| ApplicationTaskReport::Download)
        };
        let task = self.spawn_supervised_task(torrent_id, operation)?;
        self.install_active_download(torrent_id, ActiveDownload { control, task })?;
        Ok(())
    }

    fn download_control(&self, torrent_id: &str) -> DownloadControl {
        let control = DownloadControl::new();
        control.set_storage_file_pool(self.storage_file_pool.clone());
        control.set_storage_write_delay(self.storage_write_delay_for_testing);
        control
            .set_storage_execution_limits_for_testing(
                self.storage_write_concurrency_for_testing,
                self.storage_hash_concurrency_for_testing,
            )
            .expect("application configuration validated diagnostic storage limits");
        control.set_checkpoint_sync_delay_for_testing(self.checkpoint_sync_delay_for_testing);
        control.set_checkpoint_commit_delay_for_testing(self.checkpoint_commit_delay_for_testing);
        control.set_activity_sink(self.view_activity_sink(torrent_id));
        control.set_byte_metric_sink(self.speed_recorder.clone());
        control
    }

    fn spawn_supervised_task<F>(
        &self,
        torrent_id: &str,
        operation: F,
    ) -> Result<JoinHandle<Result<(), String>>, ApplicationError>
    where
        F: Future<Output = Result<ApplicationTaskReport, DownloadError>> + Send + 'static,
    {
        self.views.set_progress_inputs(
            torrent_id,
            ProgressInputs {
                task_active: true,
                ..ProgressInputs::default()
            },
        )?;
        self.views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::LIFECYCLE_TORRENT,
            "engine_task_started",
            Some(torrent_id),
            "Engine task started",
            &[],
        )?;
        let store = self.store.clone();
        let storage_roots = self.storage_roots.clone();
        let incoming_seeding = self.incoming_seeding.clone();
        let storage_file_pool = self.storage_file_pool.clone();
        let torrent_runtime = self
            .torrent_runtimes
            .get(torrent_id)
            .expect("torrent runtime exists before its operation starts")
            .handle();
        let discovery_handle = self.discovery_handle.clone();
        let views = self.views.clone();
        let torrent_id = torrent_id.to_owned();
        Ok(tokio::spawn(async move {
            let outcome = operation.await;
            let result = handle_task_outcome(&store, &storage_roots, &views, &torrent_id, outcome);
            let reconcile = reconcile_completed_seed(
                &store,
                &storage_roots,
                &views,
                incoming_seeding.as_ref(),
                &storage_file_pool,
                &torrent_runtime,
                &torrent_id,
            )
            .await;
            let advertise = reconcile_completed_advertisement(
                &store,
                &views,
                &discovery_handle,
                &torrent_runtime,
                &torrent_id,
            )
            .await;
            match (result, reconcile, advertise) {
                (Ok(()), Ok(()), Ok(())) => Ok(()),
                (Err(error), _, _) | (Ok(()), Err(error), _) | (Ok(()), Ok(()), Err(error)) => {
                    Err(error)
                }
            }
        }))
    }

    fn load_resume_conservative(&self, torrent_id: &str) -> Result<ResumeRecord, ApplicationError> {
        let mut store = self.store_mut()?;
        match store.load_resume(torrent_id) {
            Ok(resume) => Ok(resume),
            Err(StoreError::Have(_)) => {
                store.reset_have_from_metadata(torrent_id)?;
                Ok(store.load_resume(torrent_id)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn pause(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        self.unregister_incoming(torrent_id).await?;
        if self.active_torrent.as_deref() != Some(torrent_id) {
            return self.reconcile_incoming_torrent(torrent_id).await;
        }
        let (_, active) = self
            .take_active_download()
            .expect("matching active task exists");
        active.control.cancel_when_safe();
        let result = match active.task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ApplicationError::Join(error)),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        };
        self.reconcile_incoming_torrent(torrent_id).await?;
        result
    }

    async fn reap_finished(&mut self) -> Result<(), ApplicationError> {
        if self
            .active_download()
            .is_none_or(|(_, active)| !active.task.is_finished())
        {
            return Ok(());
        }
        let (torrent_id, active) = self
            .take_active_download()
            .expect("finished active task exists");
        let result = match active.task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(ApplicationError::Join(error)),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(ApplicationError::Join(error.to_string())),
        };
        self.reconcile_incoming_torrent(&torrent_id).await?;
        result
    }

    async fn unregister_incoming(&mut self, torrent_id: &str) -> Result<(), ApplicationError> {
        let runtime = self
            .torrent_runtimes
            .get(torrent_id)
            .map(TorrentRuntime::handle);
        if let (Some(incoming), Some(runtime)) = (&self.incoming_seeding, runtime) {
            runtime
                .unregister_seed(incoming)
                .await
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        }
        Ok(())
    }

    async fn reconcile_incoming_torrent(
        &mut self,
        torrent_id: &str,
    ) -> Result<(), ApplicationError> {
        self.ensure_torrent_runtime(torrent_id)?;
        let runtime = self
            .torrent_runtimes
            .get(torrent_id)
            .expect("torrent runtime exists during seed reconciliation")
            .handle();
        let Some(incoming) = self.incoming_seeding.clone() else {
            runtime
                .publish_inactive()
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            return Ok(());
        };
        let prepared = {
            let store = self.store_mut()?;
            seed_reconcile_inputs(&store, &self.storage_roots, torrent_id)?
        };
        let Some(prepared) = prepared else {
            runtime
                .unregister_seed(&incoming)
                .await
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            runtime
                .publish_inactive()
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            return Ok(());
        };
        let active = self.active_torrent.as_deref() == Some(torrent_id);
        let outcome = runtime
            .reconcile_seed(
                &incoming,
                &prepared.0,
                prepared.1,
                prepared.2.as_ref(),
                active,
                &self.storage_file_pool,
            )
            .await
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
        if let Some(outcome) = outcome {
            record_seed_reconcile(&self.views, torrent_id, &outcome)?;
        }
        Ok(())
    }

    async fn reconcile_incoming_catalog(&mut self) -> Result<(), ApplicationError> {
        if self.incoming_seeding.is_none() {
            return Ok(());
        }
        let torrent_ids = self
            .store_mut()?
            .snapshot()?
            .torrents
            .into_iter()
            .map(|torrent| torrent.torrent_id)
            .collect::<Vec<_>>();
        for torrent_id in torrent_ids {
            self.reconcile_incoming_torrent(&torrent_id).await?;
        }
        Ok(())
    }

    async fn reconcile_discovery_torrent(
        &mut self,
        torrent_id: &str,
    ) -> Result<(), ApplicationError> {
        self.ensure_torrent_runtime(torrent_id)?;
        let (resume, catalog_eligible) = {
            let store = self.store_mut()?;
            let resume = match store.load_resume(torrent_id) {
                Ok(resume) => resume,
                Err(StoreError::UnknownTorrent(_)) => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            let snapshot = store.snapshot()?;
            let eligible = snapshot.torrents.iter().any(|torrent| {
                torrent.torrent_id == torrent_id
                    && !torrent.archived
                    && torrent.removal_state.is_none()
            });
            (resume, eligible)
        };
        let runtime = self
            .torrent_runtimes
            .get(torrent_id)
            .expect("torrent runtime exists during discovery reconciliation");
        if !catalog_eligible {
            return self.stop_discovery_torrent(torrent_id).await;
        }
        let handle = runtime.handle();
        let counters = handle.tracker_counters();
        let (privacy, left) = tracker_metadata_state(&resume)?;
        let metadata_discovery_active =
            resume.raw_info.is_none() && self.active_torrent.as_deref() == Some(torrent_id);
        counters.set_left(left);
        let registration = DiscoveryAdvertisementRegistration {
            generation: runtime.generation(),
            info_hash: crate::control::decode_info_hash(torrent_id).ok_or_else(|| {
                ApplicationError::Configuration("invalid torrent identity".to_owned())
            })?,
            trackers: operational_trackers(&resume.trackers)?,
            desired_running: resume.desired_running || metadata_discovery_active,
            complete: resume.state == TorrentState::Complete,
            incoming_registered: handle.has_seed_registration(),
            privacy,
            counters,
            peers: runtime.peers(),
            activity_sink: self.view_activity_sink(torrent_id),
        };
        self.discovery_handle
            .upsert(registration)
            .await
            .map_err(|error| {
                ApplicationError::Configuration(format!("discovery advertisement: {error}"))
            })
    }

    async fn reconcile_discovery_catalog(&mut self) -> Result<(), ApplicationError> {
        let torrent_ids = self
            .store_mut()?
            .snapshot()?
            .torrents
            .into_iter()
            .map(|torrent| torrent.torrent_id)
            .collect::<Vec<_>>();
        for torrent_id in torrent_ids {
            self.reconcile_discovery_torrent(&torrent_id).await?;
        }
        Ok(())
    }

    async fn stop_discovery_torrent(&self, torrent_id: &str) -> Result<(), ApplicationError> {
        let Some(runtime) = self.torrent_runtimes.get(torrent_id) else {
            return Ok(());
        };
        let info_hash = crate::control::decode_info_hash(torrent_id).ok_or_else(|| {
            ApplicationError::Configuration("invalid torrent identity".to_owned())
        })?;
        self.discovery_handle
            .remove(info_hash, runtime.generation())
            .await
            .map_err(|error| {
                ApplicationError::Configuration(format!("discovery advertisement: {error}"))
            })
    }

    fn refresh_views(&self) -> Result<(), ApplicationError> {
        let (snapshot, durable) = {
            let store = self.store_mut()?;
            durable_view_state(&store, &self.storage_roots)?
        };
        self.views.replace_durable(&snapshot, &durable)?;
        Ok(())
    }

    fn view_activity_sink(&self, torrent_id: &str) -> Arc<dyn DownloadActivitySink> {
        Arc::new(ViewActivitySink {
            torrent_id: torrent_id.to_owned(),
            views: self.views.clone(),
            trace_checkpoint_stages: self.checkpoint_stage_trace_for_testing,
            trace_publication_stages: self.publication_stage_trace_for_testing,
            publication_delay_stage: self.publication_delay_stage_for_testing,
            publication_delay: self.publication_delay_for_testing,
            last_checkpoint_stage: Mutex::new(None),
        })
    }
}

fn tracker_metadata_state(
    resume: &ResumeRecord,
) -> Result<(TorrentPrivacy, u64), ApplicationError> {
    let Some(raw_info) = &resume.raw_info else {
        return Ok((
            TorrentPrivacy::Unknown,
            rstorrent_engine::UNKNOWN_METADATA_LEFT_BYTES,
        ));
    };
    let Ok(metainfo) = parse_durable_metainfo(raw_info) else {
        // Discovery must not make startup less conservative than the storage
        // recovery path. Corrupt metadata has no trustworthy privacy or size
        // state, so retain tracker-only premetadata behavior and suppress DHT.
        return Ok((
            TorrentPrivacy::Unknown,
            rstorrent_engine::UNKNOWN_METADATA_LEFT_BYTES,
        ));
    };
    let privacy = if metainfo.private {
        TorrentPrivacy::Private
    } else {
        TorrentPrivacy::Public
    };
    if resume.state == TorrentState::Complete {
        return Ok((privacy, 0));
    }
    let layout = TorrentLayout::from_metainfo(&metainfo);
    let skipped = resume
        .skip_files
        .iter()
        .map(|index| usize::try_from(*index))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApplicationError::Configuration("file index overflow".to_owned()))?;
    let selection = FileSelection::new(&layout, &skipped)
        .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
    let verified = resume.have.as_ref().map(HaveState::pieces);
    let mut left = 0_u64;
    for piece_index in 0..layout.piece_count() {
        if verified.is_some_and(|pieces| pieces.get(piece_index).copied().unwrap_or(false)) {
            continue;
        }
        let piece_index = u32::try_from(piece_index)
            .map_err(|_| ApplicationError::Configuration("piece index overflow".to_owned()))?;
        for range in layout
            .request_ranges(piece_index, &selection)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?
        {
            left = left.saturating_add(u64::from(range.length));
        }
    }
    Ok((privacy, left))
}

impl Drop for ApplicationService {
    fn drop(&mut self) {
        if let Some((_, active)) = self.active_download() {
            active.control.cancel();
        }
    }
}

fn delete_path_artifacts(
    root: &Path,
    torrent_id: &str,
    publication_name: Option<&str>,
    publication_shape: PublicationShape,
    storage_state: StorageState,
    managed_artifacts: ManagedArtifactState,
) -> Result<(), ApplicationError> {
    match managed_artifacts {
        ManagedArtifactState::None => {}
        ManagedArtifactState::Legacy => {
            let output = root.join(torrent_id);
            let staging = rstorrent_engine::selective_staging_path(&output)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let part = rstorrent_engine::selective_part_path(&output)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            remove_managed_artifact(&output, PublicationShape::Tree)?;
            remove_managed_artifact(&staging, PublicationShape::Tree)?;
            remove_managed_file(&part)?;
        }
        ManagedArtifactState::Staging | ManagedArtifactState::Published => {
            let publication_name = publication_name.ok_or_else(|| {
                ApplicationError::Configuration(
                    "managed storage has no durable publication name".to_owned(),
                )
            })?;
            let info_hash = crate::control::decode_info_hash(torrent_id).ok_or_else(|| {
                ApplicationError::Configuration("invalid torrent identity".to_owned())
            })?;
            let paths = torrent_storage_paths(root, publication_name, info_hash)
                .map_err(|error| ApplicationError::Configuration(error.to_string()))?;
            let output_exists = std::fs::symlink_metadata(&paths.output).is_ok();
            let staging_exists = std::fs::symlink_metadata(&paths.staging).is_ok();
            let ambiguous_both = matches!(
                (storage_state, managed_artifacts),
                (StorageState::Prepared, ManagedArtifactState::Staging)
                    | (StorageState::Published, ManagedArtifactState::Published)
            );
            if output_exists && staging_exists && ambiguous_both {
                return Err(ApplicationError::Configuration(
                    "managed publication has both staging and final artifacts".to_owned(),
                ));
            }
            match (storage_state, managed_artifacts) {
                (StorageState::Prepared, ManagedArtifactState::Staging) => {
                    if output_exists {
                        remove_managed_artifact(&paths.output, publication_shape)?;
                    } else {
                        remove_managed_artifact(&paths.staging, publication_shape)?;
                    }
                }
                (StorageState::Published, ManagedArtifactState::Published) => {
                    if staging_exists {
                        return Err(ApplicationError::Configuration(
                            "published managed storage has only a staging artifact".to_owned(),
                        ));
                    }
                    remove_managed_artifact(&paths.output, publication_shape)?;
                }
                (_, ManagedArtifactState::Staging) => {
                    remove_managed_artifact(&paths.staging, publication_shape)?;
                }
                (_, ManagedArtifactState::Published) => {
                    remove_managed_artifact(&paths.output, publication_shape)?;
                }
                (_, ManagedArtifactState::None | ManagedArtifactState::Legacy) => {
                    unreachable!("managed artifact state was matched above")
                }
            }
            remove_managed_file(&paths.part)?;
        }
    }
    Ok(())
}

fn remove_managed_artifact(
    path: &Path,
    publication_shape: PublicationShape,
) -> Result<(), ApplicationError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ApplicationError::Io {
                operation: "inspect managed torrent directory",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink()
        || match publication_shape {
            PublicationShape::File => !metadata.is_file(),
            PublicationShape::Tree => !metadata.is_dir(),
        }
    {
        return Err(ApplicationError::Configuration(format!(
            "managed torrent artifact has an unexpected type: {}",
            path.display()
        )));
    }
    match publication_shape {
        PublicationShape::File => {
            std::fs::remove_file(path).map_err(|source| ApplicationError::Io {
                operation: "remove managed torrent file",
                source,
            })
        }
        PublicationShape::Tree => {
            std::fs::remove_dir_all(path).map_err(|source| ApplicationError::Io {
                operation: "remove managed torrent directory",
                source,
            })
        }
    }
}

fn remove_managed_file(path: &Path) -> Result<(), ApplicationError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(ApplicationError::Io {
                operation: "inspect managed torrent part file",
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ApplicationError::Configuration(format!(
            "managed part-file path is a directory: {}",
            path.display()
        )));
    }
    std::fs::remove_file(path).map_err(|source| ApplicationError::Io {
        operation: "remove managed torrent part file",
        source,
    })
}

async fn reconcile_completed_seed(
    store: &Arc<Mutex<SessionStore>>,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
    views: &ViewHub,
    incoming: Option<&IncomingSeeding>,
    storage_file_pool: &StorageFilePool,
    runtime: &TorrentRuntimeHandle,
    torrent_id: &str,
) -> Result<(), String> {
    let Some(incoming) = incoming else {
        return runtime
            .publish_inactive()
            .map_err(|error| error.to_string());
    };
    let prepared = {
        let store = store
            .lock()
            .map_err(|_| "session store lock is poisoned".to_owned())?;
        seed_reconcile_inputs(&store, storage_roots, torrent_id)
            .map_err(|error| error.to_string())?
    };
    let Some(prepared) = prepared else {
        runtime
            .unregister_seed(incoming)
            .await
            .map_err(|error| error.to_string())?;
        return runtime
            .publish_inactive()
            .map_err(|error| error.to_string());
    };
    let outcome = runtime
        .reconcile_seed(
            incoming,
            &prepared.0,
            prepared.1,
            prepared.2.as_ref(),
            false,
            storage_file_pool,
        )
        .await
        .map_err(|error| error.to_string())?;
    if let Some(outcome) = outcome {
        record_seed_reconcile(views, torrent_id, &outcome).map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn reconcile_completed_advertisement(
    store: &Arc<Mutex<SessionStore>>,
    views: &ViewHub,
    discovery: &DiscoveryAdvertisementHandle,
    runtime: &TorrentRuntimeHandle,
    torrent_id: &str,
) -> Result<(), String> {
    let (resume, catalog_eligible) = {
        let store = store
            .lock()
            .map_err(|_| "session store lock is poisoned".to_owned())?;
        let resume = match store.load_resume(torrent_id) {
            Ok(resume) => resume,
            Err(StoreError::UnknownTorrent(_)) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        let snapshot = store.snapshot().map_err(|error| error.to_string())?;
        let eligible = snapshot.torrents.iter().any(|torrent| {
            torrent.torrent_id == torrent_id && !torrent.archived && torrent.removal_state.is_none()
        });
        (resume, eligible)
    };
    if !catalog_eligible {
        return discovery
            .remove(
                crate::control::decode_info_hash(torrent_id)
                    .ok_or_else(|| "invalid torrent identity".to_owned())?,
                runtime.generation(),
            )
            .await
            .or_else(|error| match error {
                DiscoveryAdvertisementError::OwnerStopped => Ok(()),
                error => Err(error),
            })
            .map_err(|error| error.to_string());
    }
    let counters = runtime.tracker_counters();
    let (privacy, left) = tracker_metadata_state(&resume).map_err(|error| error.to_string())?;
    counters.set_left(left);
    discovery
        .upsert(DiscoveryAdvertisementRegistration {
            generation: runtime.generation(),
            info_hash: crate::control::decode_info_hash(torrent_id)
                .ok_or_else(|| "invalid torrent identity".to_owned())?,
            trackers: operational_trackers(&resume.trackers).map_err(|error| error.to_string())?,
            desired_running: resume.desired_running,
            complete: resume.state == TorrentState::Complete,
            incoming_registered: runtime.has_seed_registration(),
            privacy,
            counters,
            peers: runtime.peers(),
            activity_sink: tracker_activity_sink(torrent_id, views),
        })
        .await
        .or_else(|error| match error {
            DiscoveryAdvertisementError::OwnerStopped => Ok(()),
            error => Err(error),
        })
        .map_err(|error| error.to_string())
}

fn tracker_activity_sink(torrent_id: &str, views: &ViewHub) -> Arc<dyn DownloadActivitySink> {
    Arc::new(ViewActivitySink {
        torrent_id: torrent_id.to_owned(),
        views: views.clone(),
        trace_checkpoint_stages: false,
        trace_publication_stages: false,
        publication_delay_stage: None,
        publication_delay: Duration::ZERO,
        last_checkpoint_stage: Mutex::new(None),
    })
}

type SeedReconcileInputs = (ResumeRecord, bool, Option<StorageRootLocation>);

fn seed_reconcile_inputs(
    store: &SessionStore,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
    torrent_id: &str,
) -> Result<Option<SeedReconcileInputs>, StoreError> {
    let resume = match store.load_resume(torrent_id) {
        Ok(resume) => resume,
        Err(StoreError::UnknownTorrent(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let snapshot = store.snapshot()?;
    let catalog_eligible = snapshot.torrents.iter().any(|torrent| {
        torrent.torrent_id == torrent_id && !torrent.archived && torrent.removal_state.is_none()
    });
    let root = storage_roots.get(&resume.storage_root).cloned();
    Ok(Some((resume, catalog_eligible, root)))
}

fn record_seed_reconcile(
    views: &ViewHub,
    torrent_id: &str,
    outcome: &SeedReconcileOutcome,
) -> Result<(), SubscriptionError> {
    match outcome {
        SeedReconcileOutcome::Registered => views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::PEER_CONNECTION,
            "incoming_seed_registered",
            Some(torrent_id),
            "Completed torrent registered for incoming seeding",
            &[],
        ),
        SeedReconcileOutcome::Unregistered => views.record_diagnostic(
            DiagnosticSeverity::Info,
            category::PEER_CONNECTION,
            "incoming_seed_unregistered",
            Some(torrent_id),
            "Torrent removed from incoming seeding",
            &[],
        ),
        SeedReconcileOutcome::Unavailable(detail) => views.record_diagnostic(
            DiagnosticSeverity::Warning,
            category::PEER_CONNECTION,
            "incoming_seed_unavailable",
            Some(torrent_id),
            "Completed torrent could not be registered for incoming seeding",
            &[("detail", detail)],
        ),
        SeedReconcileOutcome::AlreadyRegistered | SeedReconcileOutcome::Ineligible(_) => Ok(()),
    }
}

fn handle_task_outcome(
    store: &Arc<Mutex<SessionStore>>,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
    views: &ViewHub,
    torrent_id: &str,
    outcome: Result<ApplicationTaskReport, DownloadError>,
) -> Result<(), String> {
    views
        .clear_disk_runtime(torrent_id)
        .map_err(|error| error.to_string())?;
    views
        .clear_piece_runtime(torrent_id)
        .map_err(|error| error.to_string())?;
    match outcome {
        Ok(report) => {
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            let operation = match report {
                ApplicationTaskReport::Metadata => "metadata",
                ApplicationTaskReport::Download => "download",
            };
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_completed",
                    Some(torrent_id),
                    "Engine task completed",
                    &[("operation", operation)],
                )
                .map_err(|error| error.to_string())
        }
        Err(DownloadError::Cancelled) => {
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_cancelled",
                    Some(torrent_id),
                    "Engine task stopped at a cancellation point",
                    &[],
                )
                .map_err(|error| error.to_string())
        }
        Err(DownloadError::NetworkDisabled) => {
            views
                .set_progress_inputs(
                    torrent_id,
                    ProgressInputs {
                        network_disabled: true,
                        ..ProgressInputs::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_PEER,
                    "network_disabled",
                    Some(torrent_id),
                    "Outbound networking is disabled by application policy",
                    &[],
                )
                .map_err(|error| error.to_string())
        }
        Err(error) if is_discovery_exhaustion(&error) => {
            views
                .set_progress_inputs(
                    torrent_id,
                    ProgressInputs {
                        discovery_exhausted: true,
                        ..ProgressInputs::default()
                    },
                )
                .map_err(|error| error.to_string())?;
            let detail = error.to_string();
            if matches!(error, DownloadError::NoUsableTrackerAddress) {
                views
                    .record_diagnostic(
                        DiagnosticSeverity::Warning,
                        category::TRACKER_ANNOUNCE,
                        "tracker_address_rejected",
                        Some(torrent_id),
                        "Tracker supplied no address allowed by the current network policy",
                        &[("detail", &detail)],
                    )
                    .map_err(|error| error.to_string())?;
            }
            views
                .record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::DISCOVERY_PEER,
                    "discovery_exhausted",
                    Some(torrent_id),
                    "No enabled discovery source can currently supply an eligible peer",
                    &[("detail", &detail)],
                )
                .map_err(|error| error.to_string())
        }
        Err(DownloadError::SelectiveStorage(error))
            if error.platform_failure_kind()
                == Some(PlatformStorageFailureKind::GrantUnavailable) =>
        {
            let detail = error.to_string();
            {
                let mut store = store
                    .lock()
                    .map_err(|_| "session store lock is poisoned".to_owned())?;
                store
                    .mark_awaiting_storage(torrent_id, Some(&detail))
                    .map_err(|error| error.to_string())?;
                let (snapshot, durable) =
                    durable_view_state(&store, storage_roots).map_err(|error| error.to_string())?;
                drop(store);
                views
                    .replace_durable(&snapshot, &durable)
                    .map_err(|error| error.to_string())?;
            }
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::PLATFORM_ADAPTER,
                    "platform_storage_grant_unavailable",
                    Some(torrent_id),
                    "Platform storage grant is unavailable",
                    &[("detail", &detail)],
                )
                .map_err(|error| error.to_string())
        }
        Err(error) => {
            let cleanup_failed = matches!(&error, DownloadError::PeerCleanup { .. });
            let detail = error.to_string();
            {
                let mut store = store
                    .lock()
                    .map_err(|_| "session store lock is poisoned".to_owned())?;
                if matches!(error, DownloadError::SelectiveStorage(_)) {
                    store
                        .mark_needs_repair(torrent_id, &detail)
                        .map_err(|error| error.to_string())?;
                } else {
                    store
                        .mark_error(torrent_id, &detail)
                        .map_err(|error| error.to_string())?;
                }
                let (snapshot, durable) =
                    durable_view_state(&store, storage_roots).map_err(|error| error.to_string())?;
                drop(store);
                views
                    .replace_durable(&snapshot, &durable)
                    .map_err(|error| error.to_string())?;
            }
            views
                .set_progress_inputs(torrent_id, ProgressInputs::default())
                .map_err(|error| error.to_string())?;
            views
                .record_diagnostic(
                    DiagnosticSeverity::Error,
                    category::LIFECYCLE_TORRENT,
                    "engine_task_failed",
                    Some(torrent_id),
                    "Engine task failed",
                    &[("detail", &detail)],
                )
                .map_err(|error| error.to_string())?;
            if cleanup_failed { Err(detail) } else { Ok(()) }
        }
    }
}

fn is_discovery_exhaustion(error: &DownloadError) -> bool {
    matches!(
        error,
        DownloadError::NetworkPolicyDenied { .. }
            | DownloadError::UdpTracker(_)
            | DownloadError::NoUsablePeer
            | DownloadError::NoUsableTrackerAddress
            | DownloadError::UdpTrackerResponseTooLarge { .. }
            | DownloadError::UdpTrackerTimedOut { .. }
            | DownloadError::NetworkTimedOut { .. }
            | DownloadError::PeerClosed
            | DownloadError::PeerTimedOut { .. }
    )
}

#[derive(Debug)]
struct StoreCheckpointSink {
    store: Arc<Mutex<SessionStore>>,
    storage_roots: Arc<BTreeMap<String, StorageRootLocation>>,
    torrent_id: String,
    views: ViewHub,
}

impl StoreCheckpointSink {
    fn store(&self) -> Result<MutexGuard<'_, SessionStore>, String> {
        self.store
            .lock()
            .map_err(|_| "session store lock is poisoned".to_owned())
    }

    fn refresh(&self) -> Result<(), String> {
        let (snapshot, durable) = {
            let store = self.store()?;
            durable_view_state(&store, &self.storage_roots).map_err(|error| error.to_string())?
        };
        self.views
            .replace_durable(&snapshot, &durable)
            .map_err(|error| error.to_string())
    }
}

impl DownloadCheckpointSink for StoreCheckpointSink {
    fn metadata_verified(&self, raw_info: &[u8]) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_metadata(&self.torrent_id, raw_info)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::METADATA_EXCHANGE,
                "metadata_verified",
                Some(&self.torrent_id),
                "Torrent metadata verified",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn storage_prepared(&self, storage: ResumedStorage) -> Result<(), String> {
        let storage_state = match storage {
            ResumedStorage::Created | ResumedStorage::Staging => StorageState::Staging,
            ResumedStorage::Published => StorageState::Published,
        };
        self.store().and_then(|mut store| {
            store
                .mark_storage_prepared(&self.torrent_id, storage_state)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "storage_prepared",
                Some(&self.torrent_id),
                "Torrent storage prepared",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn recheck_started(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .begin_recheck(&self.torrent_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::INTEGRITY_HASH,
                "recheck_started",
                Some(&self.torrent_id),
                "Managed content recheck started",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn have_rechecked(&self, verified_pieces: &[bool]) -> Result<(), String> {
        let resume = self.store().and_then(|store| {
            store
                .load_resume(&self.torrent_id)
                .map_err(|error| error.to_string())
        })?;
        let have = resume
            .have
            .ok_or_else(|| "metadata checkpoint did not create have state".to_owned())?;
        let replacement = HaveState::from_pieces(have.info_hash(), verified_pieces.to_vec())
            .map_err(|error| error.to_string())?;
        self.store().and_then(|mut store| {
            store
                .complete_recheck(&self.torrent_id, &replacement)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::INTEGRITY_HASH,
                "have_rechecked",
                Some(&self.torrent_id),
                "Existing piece state rechecked",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn pieces_durable(&self, piece_indices: &[usize]) -> Result<(), String> {
        if piece_indices.is_empty() {
            return Err("durable piece batch must be nonempty".to_owned());
        }
        let mut durable_indices = piece_indices.to_vec();
        durable_indices.sort_unstable();
        durable_indices.dedup();
        let view_indices = durable_indices
            .iter()
            .copied()
            .map(|piece_index| {
                u32::try_from(piece_index)
                    .map_err(|_| "durable piece index overflows u32".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let revision = self.store().and_then(|mut store| {
            store
                .record_pieces(&self.torrent_id, &durable_indices)
                .map_err(|error| error.to_string())
        })?;
        self.views
            .record_pieces_durable(&self.torrent_id, &view_indices, revision)
            .map_err(|error| error.to_string())?;
        let piece_count = durable_indices.len().to_string();
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Debug,
                category::PIECE_BLOCK,
                "pieces_durable",
                Some(&self.torrent_id),
                "Verified pieces became durable",
                &[("piece_count", &piece_count)],
            )
            .map_err(|error| error.to_string())
    }

    fn descriptor_prepared(&self, files: &[PreparedFileHash]) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .record_prepared_files(&self.torrent_id, files)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "publication_prepared",
                Some(&self.torrent_id),
                "Payload files prepared for publication",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn publication_prepared(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .mark_publication_prepared(&self.torrent_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::STORAGE_IO,
                "path_publication_prepared",
                Some(&self.torrent_id),
                "Path publication intent became durable",
                &[],
            )
            .map_err(|error| error.to_string())
    }

    fn published(&self) -> Result<(), String> {
        self.store().and_then(|mut store| {
            store
                .mark_complete(&self.torrent_id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })?;
        self.refresh()?;
        self.views
            .record_diagnostic(
                DiagnosticSeverity::Info,
                category::LIFECYCLE_TORRENT,
                "torrent_completed",
                Some(&self.torrent_id),
                "Torrent completed",
                &[],
            )
            .map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
struct ViewActivitySink {
    torrent_id: String,
    views: ViewHub,
    trace_checkpoint_stages: bool,
    trace_publication_stages: bool,
    publication_delay_stage: Option<PathPublicationStage>,
    publication_delay: Duration,
    last_checkpoint_stage: Mutex<Option<DiskCheckpointStage>>,
}

const fn checkpoint_stage_name(stage: DiskCheckpointStage) -> &'static str {
    match stage {
        DiskCheckpointStage::Idle => "idle",
        DiskCheckpointStage::Syncing => "syncing",
        DiskCheckpointStage::Committing => "committing",
        DiskCheckpointStage::Error => "error",
    }
}

const fn publication_stage_name(stage: PathPublicationStage) -> &'static str {
    match stage {
        PathPublicationStage::IntentDurable => "intent_durable",
        PathPublicationStage::Renamed => "renamed",
        PathPublicationStage::NamespaceDurable => "namespace_durable",
    }
}

fn piece_diagnostic_context(
    event: &DownloadActivityEvent,
) -> (Vec<DiagnosticSubject>, Vec<DiagnosticField>) {
    let (piece_index, attempt) = match event {
        DownloadActivityEvent::PieceStarted {
            piece_index,
            attempt,
            ..
        } => (*piece_index, Some(*attempt)),
        DownloadActivityEvent::BlockRequested { piece_index, .. }
        | DownloadActivityEvent::BlockReceived { piece_index, .. }
        | DownloadActivityEvent::BlockStored { piece_index, .. }
        | DownloadActivityEvent::PieceHashing { piece_index }
        | DownloadActivityEvent::PieceVerified { piece_index } => (*piece_index, None),
        _ => return (Vec::new(), Vec::new()),
    };
    let fields = match event {
        DownloadActivityEvent::PieceStarted { piece_length, .. } => {
            vec![DiagnosticField::bytes(
                "piece_length",
                u64::from(*piece_length),
            )]
        }
        DownloadActivityEvent::BlockRequested { begin, length, .. }
        | DownloadActivityEvent::BlockReceived { begin, length, .. }
        | DownloadActivityEvent::BlockStored { begin, length, .. } => vec![
            DiagnosticField::bytes("block_offset", u64::from(*begin)),
            DiagnosticField::bytes("block_length", u64::from(*length)),
        ],
        _ => Vec::new(),
    };
    (
        vec![DiagnosticSubject::Piece {
            piece_index,
            attempt,
        }],
        fields,
    )
}

impl DownloadActivitySink for ViewActivitySink {
    fn record(&self, event: DownloadActivityEvent) {
        if let DownloadActivityEvent::PathPublicationStage(stage) = &event {
            if self.publication_delay_stage == Some(*stage) && !self.publication_delay.is_zero() {
                eprintln!("publication_gate={}", publication_stage_name(*stage));
                std::thread::sleep(self.publication_delay);
            } else if self.trace_publication_stages && self.publication_delay_stage.is_none() {
                eprintln!("publication_stage={}", publication_stage_name(*stage));
            }
            return;
        }
        if let DownloadActivityEvent::PeerConnections { captured_at, peers } = &event {
            let _ = self.views.record_peer_connections(
                &self.torrent_id,
                *captured_at,
                peers.as_slice(),
            );
            return;
        }
        if let DownloadActivityEvent::PeerRegistryState { active, snapshot } = &event {
            let _ =
                self.views
                    .record_peer_registry_state(&self.torrent_id, *active, snapshot.as_ref());
            return;
        }
        if let DownloadActivityEvent::TrackerState(snapshot) = &event {
            let _ = self.views.record_tracker_state(&self.torrent_id, snapshot);
            return;
        }
        if let DownloadActivityEvent::StorageState(snapshot) = &event {
            if self.trace_checkpoint_stages {
                let mut previous = self
                    .last_checkpoint_stage
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if *previous != Some(snapshot.checkpoint_stage) {
                    *previous = Some(snapshot.checkpoint_stage);
                    eprintln!(
                        "checkpoint_stage={} dirty_pieces={} dirty_bytes={} batches_completed={}",
                        checkpoint_stage_name(snapshot.checkpoint_stage),
                        snapshot.checkpoint_dirty_pieces,
                        snapshot.checkpoint_dirty_bytes,
                        snapshot.checkpoint_batches_completed,
                    );
                }
            }
            let _ = self.views.record_disk_runtime(&self.torrent_id, snapshot);
            let _ = self
                .views
                .record_piece_runtime(&self.torrent_id, &snapshot.pieces);
            return;
        }
        let piece_activity = match &event {
            DownloadActivityEvent::MetadataVerified { .. } => {
                return self.record_discovery_event(event);
            }
            DownloadActivityEvent::PieceStarted {
                piece_index,
                piece_length,
                attempt,
            } => TorrentActivity::PieceStarted {
                piece_index: *piece_index,
                piece_length: *piece_length,
                attempt: *attempt,
            },
            DownloadActivityEvent::BlockRequested {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockRequested {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::BlockReceived {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockReceived {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::BlockStored {
                piece_index,
                begin,
                length,
            } => TorrentActivity::BlockStored {
                piece_index: *piece_index,
                begin: *begin,
                length: *length,
            },
            DownloadActivityEvent::PieceVerified { piece_index } => {
                TorrentActivity::PieceVerified {
                    piece_index: *piece_index,
                }
            }
            DownloadActivityEvent::PieceHashFailed { piece_index, .. } => {
                TorrentActivity::PieceHashFailed {
                    piece_index: *piece_index,
                }
            }
            DownloadActivityEvent::PieceHashing { piece_index } => TorrentActivity::PieceHashing {
                piece_index: *piece_index,
            },
            _ => return self.record_discovery_event(event),
        };
        let _ = self.views.record_activity(&self.torrent_id, piece_activity);
        if matches!(event, DownloadActivityEvent::PieceHashFailed { .. }) {
            return self.record_discovery_event(event);
        }
        let (severity, category, code, summary) = match event {
            DownloadActivityEvent::PieceStarted { .. } => (
                DiagnosticSeverity::Debug,
                category::SCHEDULER_REQUEST,
                "piece_started",
                "Piece transfer started",
            ),
            DownloadActivityEvent::BlockRequested { .. } => (
                DiagnosticSeverity::Trace,
                category::PEER_PROTOCOL,
                "block_requested",
                "Piece block requested",
            ),
            DownloadActivityEvent::BlockReceived { .. } => (
                DiagnosticSeverity::Trace,
                category::PEER_PROTOCOL,
                "block_received",
                "Piece block received",
            ),
            DownloadActivityEvent::BlockStored { .. } => (
                DiagnosticSeverity::Trace,
                category::STORAGE_IO,
                "block_stored",
                "Piece block stored",
            ),
            DownloadActivityEvent::PieceHashing { .. } => (
                DiagnosticSeverity::Debug,
                category::INTEGRITY_HASH,
                "piece_hashing",
                "Piece hash verification started",
            ),
            DownloadActivityEvent::PieceVerified { .. } => (
                DiagnosticSeverity::Info,
                category::INTEGRITY_HASH,
                "piece_verified",
                "Piece hash verified",
            ),
            _ => unreachable!("discovery events returned before piece diagnostics"),
        };
        let diagnostic_torrent_id = self.torrent_id.clone();
        let _ =
            self.views
                .record_diagnostic_lazy(severity, category, Some(&self.torrent_id), || {
                    let (subjects, fields) = piece_diagnostic_context(&event);
                    DiagnosticDraft {
                        severity,
                        category: DiagnosticCategory::from_static(category),
                        code: code.to_owned(),
                        torrent_id: Some(diagnostic_torrent_id),
                        message: summary.to_owned(),
                        subjects,
                        fields,
                    }
                });
    }
}

impl ViewActivitySink {
    fn record_structured(
        &self,
        severity: DiagnosticSeverity,
        category: &'static str,
        code: &'static str,
        message: &'static str,
        subjects: Vec<DiagnosticSubject>,
        fields: Vec<DiagnosticField>,
    ) {
        let _ = self.views.record_structured_diagnostic(DiagnosticDraft {
            severity,
            category: DiagnosticCategory::from_static(category),
            code: code.to_owned(),
            torrent_id: Some(self.torrent_id.clone()),
            message: message.to_owned(),
            subjects,
            fields,
        });
    }

    fn record_discovery_event(&self, event: DownloadActivityEvent) {
        match event {
            DownloadActivityEvent::MetadataVerified {
                total_length,
                piece_length,
                piece_count,
                file_count,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::METADATA_EXCHANGE,
                    "metadata_verified",
                    "Torrent metadata verified",
                    Vec::new(),
                    vec![
                        DiagnosticField::bytes("total_length", total_length),
                        DiagnosticField::bytes("piece_length", u64::from(piece_length)),
                        DiagnosticField::count(
                            "piece_count",
                            u64::try_from(piece_count).unwrap_or(u64::MAX),
                        ),
                        DiagnosticField::count(
                            "file_count",
                            u64::try_from(file_count).unwrap_or(u64::MAX),
                        ),
                    ],
                );
            }
            DownloadActivityEvent::TrackerAnnounceStarted {
                tracker,
                tier,
                attempt,
                event,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let announce_event = format!("{event:?}").to_ascii_lowercase();
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_started",
                    "Contacting tracker",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("tier", u64::from(tier)),
                        DiagnosticField::count("attempt", u64::from(attempt)),
                        DiagnosticField::text("event", announce_event),
                    ],
                );
            }
            DownloadActivityEvent::TrackerUdpRetransmitted { tracker, operation } => {
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::TRACKER_ANNOUNCE,
                    "tracker_udp_retransmitted",
                    Some(&self.torrent_id),
                    "UDP tracker request retransmitted after silence",
                    &[("tracker", &tracker), ("operation", operation)],
                );
            }
            DownloadActivityEvent::TrackerAnnounceFailed {
                tracker,
                failures,
                retry_in_seconds,
                detail,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Warning,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_failed",
                    "Tracker announce failed temporarily",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("failures", u64::from(failures)),
                        DiagnosticField::duration_millis(
                            "retry_in",
                            retry_in_seconds.saturating_mul(1_000),
                        ),
                        DiagnosticField::text("detail", detail),
                    ],
                );
            }
            DownloadActivityEvent::TrackerFallbackSelected { tracker, tier } => {
                let tier = tier.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_fallback_selected",
                    Some(&self.torrent_id),
                    "Trying another tracker in the tier",
                    &[("tracker", &tracker), ("tier", &tier)],
                );
            }
            DownloadActivityEvent::TrackerRetryScheduled {
                tracker,
                retry_in_seconds,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let retry = retry_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_retry_scheduled",
                    Some(&self.torrent_id),
                    "Tracker discovery will retry automatically",
                    &[("tracker", &tracker), ("retry_seconds", &retry)],
                );
            }
            DownloadActivityEvent::TrackerReannounceScheduled {
                tracker,
                announce_in_seconds,
            } => {
                let announce = announce_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::TRACKER_ANNOUNCE,
                    "tracker_reannounce_scheduled",
                    Some(&self.torrent_id),
                    "Tracker accepted the announce interval",
                    &[("tracker", &tracker), ("announce_seconds", &announce)],
                );
            }
            DownloadActivityEvent::TrackerAnnounceSucceeded {
                tracker,
                peer_count,
                interval_seconds,
            } => {
                self.record_structured(
                    DiagnosticSeverity::Info,
                    category::TRACKER_ANNOUNCE,
                    "tracker_announce_succeeded",
                    "Tracker announce succeeded",
                    vec![DiagnosticSubject::Tracker {
                        tracker_id: tracker,
                    }],
                    vec![
                        DiagnosticField::count("peers", u64::from(peer_count)),
                        DiagnosticField::duration_millis(
                            "announce_interval",
                            interval_seconds.saturating_mul(1_000),
                        ),
                    ],
                );
            }
            DownloadActivityEvent::TrackerWarning { tracker, detail } => {
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::TRACKER_ANNOUNCE,
                    "tracker_warning",
                    Some(&self.torrent_id),
                    "Tracker announce succeeded with a warning",
                    &[("tracker", &tracker), ("detail", &detail)],
                );
            }
            DownloadActivityEvent::TrackerPeersUnavailable {
                tracker,
                peer_count,
            } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let peers = peer_count.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::PEER_CONNECTION,
                    "tracker_peers_unavailable",
                    Some(&self.torrent_id),
                    "Tracker response has no currently eligible peer",
                    &[("tracker", &tracker), ("peers", &peers)],
                );
            }
            DownloadActivityEvent::DhtLookupStarted => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_lookup_started",
                    Some(&self.torrent_id),
                    "Searching the distributed hash table for peers",
                    &[],
                );
            }
            DownloadActivityEvent::DhtLookupSucceeded { peer_count } => {
                let peers = peer_count.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_lookup_succeeded",
                    Some(&self.torrent_id),
                    "DHT lookup returned peers",
                    &[("peers", &peers)],
                );
            }
            DownloadActivityEvent::DhtAnnounceCompleted {
                port,
                token_nodes,
                announces_sent,
                announces_succeeded,
                announces_failed,
            } => {
                self.record_structured(
                    if announces_failed == 0 {
                        DiagnosticSeverity::Info
                    } else {
                        DiagnosticSeverity::Warning
                    },
                    category::DISCOVERY_DHT,
                    "dht_announce_completed",
                    "DHT peer announcement traversal completed",
                    Vec::new(),
                    vec![
                        DiagnosticField::count("port", u64::from(port)),
                        DiagnosticField::count("token_nodes", u64::from(token_nodes)),
                        DiagnosticField::count("announces_sent", u64::from(announces_sent)),
                        DiagnosticField::count(
                            "announces_succeeded",
                            u64::from(announces_succeeded),
                        ),
                        DiagnosticField::count("announces_failed", u64::from(announces_failed)),
                    ],
                );
            }
            DownloadActivityEvent::DhtLookupFailed { detail } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, false, true);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::DISCOVERY_DHT,
                    "dht_lookup_failed",
                    Some(&self.torrent_id),
                    "DHT lookup ended without a peer and may be retried",
                    &[("detail", &detail)],
                );
            }
            DownloadActivityEvent::DhtRetryScheduled { retry_in_seconds } => {
                let retry = retry_in_seconds.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_retry_scheduled",
                    Some(&self.torrent_id),
                    "DHT lookup will retry after bounded backoff",
                    &[("retry_in_seconds", &retry)],
                );
            }
            DownloadActivityEvent::DhtDisabledForPrivateTorrent => {
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::DISCOVERY_DHT,
                    "dht_disabled_private_torrent",
                    Some(&self.torrent_id),
                    "Verified private metadata disabled decentralized discovery",
                    &[],
                );
            }
            DownloadActivityEvent::PeerDialStarted { peer: _ } => {
                let _ = self
                    .views
                    .set_discovery_activity(&self.torrent_id, true, false);
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Info,
                    category::PEER_CONNECTION,
                    "peer_dial_started",
                    Some(&self.torrent_id),
                    "Connecting to discovered peer",
                    &[],
                );
            }
            DownloadActivityEvent::SwarmState(snapshot) => {
                let connected = snapshot.connected_peers.to_string();
                let pending = snapshot.pending_dials.to_string();
                let unchoked = snapshot.unchoked_peers.to_string();
                let missing = snapshot.missing_blocks.to_string();
                let requested = snapshot.requested_blocks.to_string();
                let writing = snapshot.writing_blocks.to_string();
                let reserved = snapshot.outstanding_request_bytes.to_string();
                let oldest = snapshot
                    .oldest_request_age_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let next_expiry = snapshot
                    .next_request_expiry_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let next_replacement = snapshot
                    .next_replacement_seconds
                    .map_or_else(|| "none".to_owned(), |value| value.to_string());
                let reason = snapshot
                    .no_request_reason
                    .map_or_else(|| "requestable".to_owned(), |reason| format!("{reason:?}"));
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Debug,
                    category::PERFORMANCE_BACKPRESSURE,
                    "swarm_state_changed",
                    Some(&self.torrent_id),
                    "Torrent request state changed",
                    &[
                        ("connected_peers", &connected),
                        ("pending_dials", &pending),
                        ("unchoked_peers", &unchoked),
                        ("missing_blocks", &missing),
                        ("requested_blocks", &requested),
                        ("writing_blocks", &writing),
                        ("outstanding_request_bytes", &reserved),
                        ("oldest_request_seconds", &oldest),
                        ("next_expiry_seconds", &next_expiry),
                        ("next_replacement_seconds", &next_replacement),
                        ("no_request_reason", &reason),
                    ],
                );
            }
            DownloadActivityEvent::PieceHashFailed {
                piece_index,
                contributor_count,
                failed_bytes,
            } => {
                let piece_index = piece_index.to_string();
                let contributors = contributor_count.to_string();
                let failed_bytes = failed_bytes.to_string();
                let _ = self.views.record_diagnostic(
                    DiagnosticSeverity::Warning,
                    category::INTEGRITY_HASH,
                    "piece_hash_failed",
                    Some(&self.torrent_id),
                    "Piece hash failed; retrying the entire piece",
                    &[
                        ("piece_index", &piece_index),
                        ("contributors", &contributors),
                        ("failed_bytes", &failed_bytes),
                    ],
                );
            }
            DownloadActivityEvent::PieceStarted { .. }
            | DownloadActivityEvent::BlockRequested { .. }
            | DownloadActivityEvent::BlockReceived { .. }
            | DownloadActivityEvent::BlockStored { .. }
            | DownloadActivityEvent::PieceHashing { .. }
            | DownloadActivityEvent::PieceVerified { .. } => {
                unreachable!("piece events are handled before discovery events")
            }
            DownloadActivityEvent::PeerConnections { .. } => {
                unreachable!("peer projections are handled before diagnostic events")
            }
            DownloadActivityEvent::PeerRegistryState { .. } => {
                unreachable!("swarm projections are handled before diagnostic events")
            }
            DownloadActivityEvent::TrackerState(_) => {
                unreachable!("tracker projections are handled before diagnostic events")
            }
            DownloadActivityEvent::StorageState(_) => {
                unreachable!("disk projections are handled before diagnostic events")
            }
            DownloadActivityEvent::PathPublicationStage(_) => {
                unreachable!("publication stages are handled before diagnostic events")
            }
        }
    }
}

fn available_storage_roots(roots: Vec<StoredStorageRoot>) -> BTreeMap<String, StorageRootLocation> {
    roots
        .into_iter()
        .filter(|root| match &root.location {
            StorageRootLocation::Path(path) => path.is_dir() && std::fs::read_dir(path).is_ok(),
            StorageRootLocation::PlatformCapability => true,
        })
        .map(|root| (root.id, root.location))
        .collect()
}

fn validate_selected_directory(path: &Path) -> Result<PathBuf, ApplicationError> {
    let path = std::fs::canonicalize(path).map_err(|source| ApplicationError::Io {
        operation: "resolve selected storage root",
        source,
    })?;
    if !path.is_dir() {
        return Err(ApplicationError::Configuration(
            "selected storage root is not a directory".to_owned(),
        ));
    }
    std::fs::read_dir(&path).map_err(|source| ApplicationError::Io {
        operation: "read selected storage root",
        source,
    })?;
    Ok(path)
}

fn storage_root_label(path: &Path) -> String {
    let mut label = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or_else(|| path.to_str())
        .unwrap_or("Download folder")
        .to_owned();
    if label.len() > crate::control::MAX_ROOT_LABEL_LENGTH {
        label.truncate(label.floor_char_boundary(crate::control::MAX_ROOT_LABEL_LENGTH));
    }
    label
}

fn durable_view_state(
    store: &SessionStore,
    storage_roots: &BTreeMap<String, StorageRootLocation>,
) -> Result<
    (
        crate::ServiceSnapshot,
        BTreeMap<String, DurableTorrentViewState>,
    ),
    ApplicationError,
> {
    let snapshot = store.snapshot()?;
    let mut durable = BTreeMap::new();
    for torrent in &snapshot.torrents {
        let Ok(resume) = store.load_resume(&torrent.torrent_id) else {
            durable.insert(
                torrent.torrent_id.clone(),
                DurableTorrentViewState {
                    display_name: None,
                    verified: Vec::new(),
                    files: None,
                    trackers: TrackerViewModel::default(),
                },
            );
            continue;
        };
        let verified_pieces = resume.have.as_ref().map_or(&[][..], |have| have.pieces());
        let verified_indices = verified_pieces
            .iter()
            .enumerate()
            .filter(|(_, verified)| **verified)
            .map(|(index, _)| u32::try_from(index))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ApplicationError::Configuration("piece index overflows u32".to_owned()))?;
        let metainfo = resume
            .raw_info
            .as_deref()
            .and_then(|raw_info| parse_durable_metainfo(raw_info).ok());
        let display_name = metainfo.as_ref().map(|metainfo| metainfo.name.clone());
        let files = if let Some(metainfo) = metainfo.as_ref() {
            let filesystem_content_base = filesystem_content_base(
                storage_roots.get(&resume.storage_root),
                &torrent.torrent_id,
                resume.publication_name.as_deref(),
            )?;
            FileProgressModel::new(
                metainfo,
                &resume.skip_files,
                &verified_indices,
                filesystem_content_base,
            )
            .ok()
        } else {
            None
        };
        let trackers = TrackerViewModel::from_trackers(&resume.trackers);
        durable.insert(
            torrent.torrent_id.clone(),
            DurableTorrentViewState {
                display_name,
                verified: ranges_from_pieces(verified_pieces),
                files,
                trackers,
            },
        );
    }
    Ok((snapshot, durable))
}

fn filesystem_content_base(
    storage_root: Option<&StorageRootLocation>,
    torrent_id: &str,
    publication_name: Option<&str>,
) -> Result<Option<String>, ApplicationError> {
    let Some(StorageRootLocation::Path(root)) = storage_root else {
        return Ok(None);
    };
    let root = if root.is_absolute() {
        root.clone()
    } else {
        std::env::current_dir()
            .map_err(|source| ApplicationError::Io {
                operation: "resolve storage root",
                source,
            })?
            .join(root)
    };
    let path = if let Some(publication_name) = publication_name {
        let info_hash = crate::control::decode_info_hash(torrent_id).ok_or_else(|| {
            ApplicationError::Configuration("invalid torrent identity".to_owned())
        })?;
        torrent_storage_paths(&root, publication_name, info_hash)
            .map_err(|error| ApplicationError::Configuration(error.to_string()))?
            .output
    } else {
        root.join(torrent_id)
    };
    path.into_os_string()
        .into_string()
        .map(Some)
        .map_err(|_| ApplicationError::Configuration("storage path is not UTF-8".to_owned()))
}

fn require_publication_name(
    publication_name: Option<&str>,
    metainfo: &Metainfo,
) -> Result<(), ApplicationError> {
    if publication_name == Some(metainfo.name.as_str()) {
        Ok(())
    } else {
        Err(ApplicationError::Configuration(
            "stored publication name is missing or does not match verified metadata".to_owned(),
        ))
    }
}

fn resume_artifact_state(resume: &ResumeRecord) -> Result<ResumeArtifactState, ApplicationError> {
    match (resume.storage_state, resume.managed_artifacts) {
        (StorageState::None, ManagedArtifactState::None) => Ok(ResumeArtifactState::None),
        (StorageState::Staging, ManagedArtifactState::Staging) => Ok(ResumeArtifactState::Staging),
        (
            StorageState::Prepared,
            ManagedArtifactState::Staging | ManagedArtifactState::Published,
        ) => Ok(ResumeArtifactState::Publishing),
        (StorageState::Published, ManagedArtifactState::Published) => {
            Ok(ResumeArtifactState::Published)
        }
        _ => Err(ApplicationError::Configuration(
            "stored payload ownership and storage state are inconsistent".to_owned(),
        )),
    }
}

#[derive(Debug)]
pub enum ApplicationError {
    Store(StoreError),
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    Configuration(String),
    Persistence(String),
    Busy(String),
    Join(String),
    StorePoisoned,
    Subscription(SubscriptionError),
    Dht(DhtError),
    Incoming(IncomingPeerError),
    SessionSocket(SessionSocketError),
    SessionUdp(SessionUdpError),
    IncomingSeeding(String),
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
            Self::Configuration(message) => {
                write!(formatter, "application configuration: {message}")
            }
            Self::Persistence(message) => write!(formatter, "application persistence: {message}"),
            Self::Busy(torrent_id) => {
                write!(
                    formatter,
                    "torrent {torrent_id} already owns the download slot"
                )
            }
            Self::Join(message) => write!(formatter, "engine task join: {message}"),
            Self::StorePoisoned => write!(formatter, "session store lock is poisoned"),
            Self::Subscription(error) => write!(formatter, "{error}"),
            Self::Dht(error) => write!(formatter, "{error}"),
            Self::Incoming(error) => write!(formatter, "{error}"),
            Self::SessionSocket(error) => write!(formatter, "{error}"),
            Self::SessionUdp(error) => write!(formatter, "{error}"),
            Self::IncomingSeeding(error) => write!(formatter, "incoming seeding: {error}"),
        }
    }
}

impl Error for ApplicationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::Subscription(error) => Some(error),
            Self::Dht(error) => Some(error),
            Self::Incoming(error) => Some(error),
            Self::SessionSocket(error) => Some(error),
            Self::SessionUdp(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreError> for ApplicationError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<DhtError> for ApplicationError {
    fn from(error: DhtError) -> Self {
        Self::Dht(error)
    }
}

impl From<IncomingPeerError> for ApplicationError {
    fn from(error: IncomingPeerError) -> Self {
        Self::Incoming(error)
    }
}

impl From<SessionSocketError> for ApplicationError {
    fn from(error: SessionSocketError) -> Self {
        Self::SessionSocket(error)
    }
}

impl From<SessionUdpError> for ApplicationError {
    fn from(error: SessionUdpError) -> Self {
        Self::SessionUdp(error)
    }
}

impl From<IncomingSeedingError> for ApplicationError {
    fn from(error: IncomingSeedingError) -> Self {
        Self::IncomingSeeding(error.to_string())
    }
}

impl From<SubscriptionError> for ApplicationError {
    fn from(error: SubscriptionError) -> Self {
        Self::Subscription(error)
    }
}

fn encode_info_hash(info_hash: [u8; 20]) -> String {
    let mut output = String::with_capacity(40);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in info_hash {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub fn application_error_response(
    request_id: String,
    revision: u64,
    error: &ApplicationError,
) -> ResponseEnvelope {
    let code = match error {
        ApplicationError::Busy(_) => ErrorCode::Busy,
        ApplicationError::Configuration(_) => ErrorCode::InvalidRequest,
        ApplicationError::Store(error) if error.is_resource_limit() => ErrorCode::ResourceLimit,
        ApplicationError::Store(StoreError::UnknownTorrent(_)) => ErrorCode::UnknownTorrent,
        ApplicationError::Store(StoreError::DurableState(_) | StoreError::Have(_)) => {
            ErrorCode::InvalidDurableState
        }
        _ => ErrorCode::Internal,
    };
    ResponseEnvelope::error(request_id, revision, code, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use rstorrent_engine::dht::BootstrapNode;
    use rstorrent_engine::{
        ByteMetric, ByteMetricSink, DEFAULT_PEER_ID, DownloadError, NetworkConfig, NetworkPolicy,
        PublicationShape, torrent_storage_paths,
    };
    use rstorrent_protocol::dht::{
        DhtEndpoint, DhtIp, Message as DhtMessage, NodeId, decode_message as decode_dht,
        encode_response as encode_dht_response,
    };
    use rstorrent_protocol::metadata::{
        MetadataMessage, encode_extension_handshake, encode_metadata_data, parse_metadata_message,
    };
    use rstorrent_protocol::peer_wire::{
        EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX, FrameDecoder,
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake,
        encode_handshake_with_reserved, encode_message,
    };
    use rusqlite::Connection;
    use sha1::{Digest, Sha1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};

    use super::{
        ApplicationConfig, ApplicationService, ManagedArtifactState, delete_path_artifacts,
        handle_task_outcome,
    };
    use crate::{
        AddTorrentBytesRequest, CONTROL_VERSION, CatalogPageRequest, ClientSettings, Command,
        ConfiguredStorageRoot, DeliveryPolicy, DhtLifecycleView, DiagnosticFilter,
        DiagnosticProfile, DiagnosticSeverity, FilePriority, ListenerBindFailureReason,
        ListenerPolicy, ListenerStatus, OpenViewSetOptions, OpenViewSetRequest, PeerDirection,
        PeerFlagView, PeerLifecycle, PeerRole, PeerSourceView, PeerTransportKind, PeerView,
        ProgressDisposition, ProgressReason, RemovalDataPolicy, RemovalState, RequestEnvelope,
        ResponseOutcome, SessionStore, StorageState, SubscriptionSpec, SwarmCatalogState,
        SwarmPeerState, SwarmPeerView, TorrentState, TrackerConnectionFamilyView,
        TrackerSecurityView, TrackerView, ViewDeliveryPolicy, ViewPatch, ViewProjection,
        ViewSelector, ViewSetError, ViewSetOwner, ViewSetUpdate, ViewSnapshot, ViewSpec,
        ViewUpdatePayload,
    };

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        let sequence = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-application-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn config(root: &Path) -> ApplicationConfig {
        ApplicationConfig::new(
            root.join("profile"),
            "test".to_owned(),
            vec![ConfiguredStorageRoot::path(
                "downloads",
                root.join("payload"),
            )],
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(5),
            ),
        )
    }

    fn persist_client_settings(config: &ApplicationConfig, settings: ClientSettings) {
        let mut store = SessionStore::open(
            config
                .durable_profile_root()
                .expect("test configuration is durable"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open settings fixture store");
        let response = store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "configure-client-settings".to_owned(),
                expected_revision: None,
                command: Command::SetClientSettings { settings },
            })
            .expect("configure client settings");
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
    }

    fn add_request(request_id: &str, torrent_id: &str) -> RequestEnvelope {
        RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            command: Command::AddMagnet {
                magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe=127.0.0.1:1"),
                storage_root: "downloads".to_owned(),
                start_content: true,
                skip_files: Vec::new(),
            },
        }
    }

    fn multi_file_info() -> Vec<u8> {
        let mut info = b"d5:filesld6:lengthi4e4:pathl5:a.bineed6:lengthi4e4:pathl5:b.bineee4:name5:multi12:piece lengthi4e6:pieces40:".to_vec();
        info.extend_from_slice(&[b'a'; 20]);
        info.extend_from_slice(&[b'b'; 20]);
        info.push(b'e');
        info
    }

    fn single_file_info(name: &str, payload: &[u8], piece_length: usize) -> Vec<u8> {
        let hashes = payload
            .chunks(piece_length)
            .flat_map(|piece| Sha1::digest(piece).to_vec())
            .collect::<Vec<_>>();
        let mut info = format!(
            "d6:lengthi{}e4:name{}:{}12:piece lengthi{}e6:pieces{}:",
            payload.len(),
            name.len(),
            name,
            piece_length,
            hashes.len()
        )
        .into_bytes();
        info.extend_from_slice(&hashes);
        info.push(b'e');
        info
    }

    fn torrent_bytes_request(
        request_id: &str,
        source: &[u8],
        start_content: bool,
    ) -> AddTorrentBytesRequest {
        AddTorrentBytesRequest {
            version: CONTROL_VERSION,
            request_id: request_id.to_owned(),
            expected_revision: None,
            storage_root: "downloads".to_owned(),
            start_content,
            selection: crate::FileSelectionIntent::All,
            source_length: source.len() as u32,
        }
    }

    async fn read_peer_message(
        stream: &mut tokio::net::TcpStream,
        decoder: &mut FrameDecoder,
        pending: &mut std::collections::VecDeque<PeerMessage>,
    ) -> PeerMessage {
        loop {
            if let Some(message) = pending.pop_front() {
                return message;
            }
            let mut bytes = [0_u8; 1024];
            let length = stream.read(&mut bytes).await.expect("read peer message");
            assert_ne!(length, 0, "peer closed before expected message");
            pending.extend(decoder.push(&bytes[..length]).expect("decode peer message"));
        }
    }

    async fn wait_for_seed_registrations(service: &ApplicationService, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if service
                    .incoming_peer_snapshot()
                    .is_some_and(|snapshot| snapshot.registrations == expected)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("incoming seed reconciliation deadline");
    }

    async fn client_settings_runtime(
        service: &ApplicationService,
    ) -> crate::ClientSettingsRuntimeView {
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe to client settings");
        let update = subscription
            .next_update()
            .await
            .expect("initial client settings snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::TorrentList {
                client_settings, ..
            },
        } = update.payload
        else {
            panic!("expected torrent-list client settings snapshot");
        };
        client_settings
    }

    async fn torrent_peer_views(service: &ApplicationService, torrent_id: &str) -> Vec<PeerView> {
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::Torrent {
                    torrent_id: torrent_id.to_owned(),
                },
                projection: ViewProjection::Peers,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe to torrent peers");
        let update = subscription
            .next_update()
            .await
            .expect("initial torrent peer snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::Peers { peers, .. },
        } = update.payload
        else {
            panic!("expected torrent peer snapshot");
        };
        peers
    }

    async fn torrent_tracker_views(
        service: &ApplicationService,
        torrent_id: &str,
    ) -> Vec<TrackerView> {
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::Torrent {
                    torrent_id: torrent_id.to_owned(),
                },
                projection: ViewProjection::Trackers,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: Some(CatalogPageRequest::default()),
            })
            .expect("subscribe to torrent trackers");
        let update = subscription
            .next_update()
            .await
            .expect("initial torrent tracker snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::Trackers { trackers, .. },
        } = update.payload
        else {
            panic!("expected torrent tracker snapshot");
        };
        trackers
    }

    async fn torrent_swarm_view(
        service: &ApplicationService,
        torrent_id: &str,
    ) -> (SwarmCatalogState, Vec<SwarmPeerView>) {
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::Torrent {
                    torrent_id: torrent_id.to_owned(),
                },
                projection: ViewProjection::Swarm,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe to torrent swarm");
        let update = subscription
            .next_update()
            .await
            .expect("initial torrent swarm snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::Swarm { state, peers, .. },
        } = update.payload
        else {
            panic!("expected torrent swarm snapshot");
        };
        (state, peers)
    }

    async fn connect_application_seed(
        service: &ApplicationService,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
    ) -> (
        tokio::net::TcpStream,
        FrameDecoder,
        std::collections::VecDeque<PeerMessage>,
    ) {
        connect_application_seed_with_extensions(service, info_hash, peer_id, false).await
    }

    async fn connect_application_seed_with_extensions(
        service: &ApplicationService,
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        supports_extensions: bool,
    ) -> (
        tokio::net::TcpStream,
        FrameDecoder,
        std::collections::VecDeque<PeerMessage>,
    ) {
        let address = service
            .incoming_peer_snapshot()
            .expect("incoming service enabled")
            .listen_address;
        let mut stream = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect application seed");
        let mut reserved = [0; 8];
        if supports_extensions {
            reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
        }
        stream
            .write_all(&encode_handshake_with_reserved(
                info_hash, peer_id, reserved,
            ))
            .await
            .expect("send seed handshake");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read seed handshake");
        decode_handshake(&handshake, info_hash).expect("valid seed handshake");
        let mut decoder = FrameDecoder::new();
        let mut pending = std::collections::VecDeque::new();
        assert_eq!(
            read_peer_message(&mut stream, &mut decoder, &mut pending).await,
            PeerMessage::Bitfield(vec![0b1100_0000])
        );
        if supports_extensions {
            assert!(matches!(
                read_peer_message(&mut stream, &mut decoder, &mut pending).await,
                PeerMessage::Extended { id: 0, .. }
            ));
        }
        (stream, decoder, pending)
    }

    async fn spawn_metadata_peer(raw_info: Vec<u8>) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata peer");
        let address = listener.local_addr().expect("metadata peer address");
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept metadata peer");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read metadata handshake");
            let handshake =
                decode_handshake(&handshake, info_hash).expect("metadata handshake identity");
            assert!(handshake.supports_extensions());
            let mut reserved = [0; 8];
            reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
            stream
                .write_all(&encode_handshake_with_reserved(
                    info_hash,
                    *b"-RS-EPHEMERAL-000000",
                    reserved,
                ))
                .await
                .expect("send metadata handshake");
            let mut decoder = FrameDecoder::new();
            let mut pending = std::collections::VecDeque::new();
            assert!(matches!(
                read_peer_message(&mut stream, &mut decoder, &mut pending).await,
                PeerMessage::Extended { id: 0, .. }
            ));
            stream
                .write_all(
                    &encode_message(&PeerMessage::Extended {
                        id: 0,
                        payload: encode_extension_handshake(Some(raw_info.len())),
                    })
                    .expect("encode extension handshake"),
                )
                .await
                .expect("send extension handshake");
            let PeerMessage::Extended {
                id: 1,
                payload: request,
            } = read_peer_message(&mut stream, &mut decoder, &mut pending).await
            else {
                panic!("metadata request expected");
            };
            assert_eq!(
                parse_metadata_message(&request).expect("parse metadata request"),
                MetadataMessage::Request { piece: 0 }
            );
            stream
                .write_all(
                    &encode_message(&PeerMessage::Extended {
                        id: 1,
                        payload: encode_metadata_data(0, raw_info.len(), &raw_info)
                            .expect("encode metadata"),
                    })
                    .expect("encode metadata message"),
                )
                .await
                .expect("send metadata");
            let mut tail = [0; 32];
            match stream.read(&mut tail).await {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                outcome => panic!("metadata connection did not close: {outcome:?}"),
            }
        });
        (address, task)
    }

    async fn serve_single_piece_peer(listener: TcpListener, info_hash: [u8; 20], payload: Vec<u8>) {
        let (mut stream, _) = listener.accept().await.expect("accept content peer");
        let mut handshake = [0; HANDSHAKE_LENGTH];
        stream
            .read_exact(&mut handshake)
            .await
            .expect("read content handshake");
        decode_handshake(&handshake, info_hash).expect("content handshake identity");
        stream
            .write_all(&encode_handshake(info_hash, *b"-RS-HTTP-IPV6-000000"))
            .await
            .expect("send content handshake");
        stream
            .write_all(&encode_message(&PeerMessage::Bitfield(vec![0x80])).expect("bitfield"))
            .await
            .expect("send content bitfield");
        let mut decoder = FrameDecoder::new();
        let mut pending = std::collections::VecDeque::new();
        loop {
            match read_peer_message(&mut stream, &mut decoder, &mut pending).await {
                PeerMessage::Interested => {
                    stream
                        .write_all(&encode_message(&PeerMessage::Unchoke).expect("unchoke"))
                        .await
                        .expect("send unchoke");
                }
                PeerMessage::Request(request) => {
                    assert_eq!(request.index, 0);
                    let begin = usize::try_from(request.begin).expect("request begin");
                    let length = usize::try_from(request.length).expect("request length");
                    let end = begin.checked_add(length).expect("request end");
                    stream
                        .write_all(
                            &encode_message(&PeerMessage::Piece {
                                index: 0,
                                begin: request.begin,
                                block: payload[begin..end].to_vec(),
                            })
                            .expect("piece"),
                        )
                        .await
                        .expect("send piece");
                    if end == payload.len() {
                        return;
                    }
                }
                PeerMessage::KeepAlive | PeerMessage::NotInterested => {}
                message => panic!("unexpected content message: {message:?}"),
            }
        }
    }

    async fn read_http_request<S>(stream: &mut S) -> String
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1_024];
            let length = stream.read(&mut chunk).await.expect("read HTTP request");
            assert_ne!(length, 0, "HTTP request ended before headers");
            request.extend_from_slice(&chunk[..length]);
            assert!(request.len() <= 16 * 1_024, "HTTP request is bounded");
        }
        String::from_utf8(request).expect("HTTP request is ASCII")
    }

    fn untrusted_tls_acceptor() -> tokio_rustls::TlsAcceptor {
        use base64::Engine as _;
        use std::sync::Arc;
        use tokio_rustls::rustls::ServerConfig;
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

        const CERT: &str = concat!(
            "MIIDETCCAfmgAwIBAgIUaGe1xp9QWaOq2j9MBVoYJQHxQjUwDQYJKoZIhvcNAQELBQAwGDEW",
            "MBQGA1UEAwwNd3JvbmcuZXhhbXBsZTAeFw0yNjA4MDUxODMxMDNaFw0zNjA4MDIxODMxMDNa",
            "MBgxFjAUBgNVBAMMDXdyb25nLmV4YW1wbGUwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEK",
            "AoIBAQDfQV9T9lOopJtynnl+v0lzgusiA5zeYE20tSlD04EtdJ8SakjrEC1cbfGQN9azRSNA",
            "oVKdazKOQpGib+cwbXd4snw/CE2qKVZ6j5grp8QZvSqm8gjHwp3WwlfVbmOJPXLsSjvCU36j",
            "qI5s5VWfWiPAxDYklfUn4hz4aR5oabP+poMgsXxq411UEclqr+s6fv1TVPO95hT9CeTyNXtN",
            "1T1Wq1tVMLm3ULI7oGVLhdZJGEyLLdricnIda+YOZEeoUslfzuV3rQMQJyDRWdajNdpbPJL/",
            "nBzVTAaGpe+O+JIj0hP1jdDNzdODTJ6b0JfrngB9mDYKGw4tnaLz8i0Tcpc/AgMBAAGjUzBR",
            "MB0GA1UdDgQWBBQ5BQU9ptEzvoQqvmi6fnGKh3GdMDAfBgNVHSMEGDAWgBQ5BQU9ptEzvoQq",
            "vmi6fnGKh3GdMDAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQBfpwTOEGer",
            "XsgIPC5kVDTdQI0d5XmVuAbxCWQZCsRbrn65hW3E+uaWr2W7SMUAczYpWi3W/Q+YWXz/F19",
            "3IBVLUL7zohyQP06vYIsQUAgKpWDwPMHTtzLKAy2wtn50Y83CR/FMEXQpSFsFxZEO7rupEAL",
            "E3oA/jzaApcIuqMOJbGFcrsuRy3HVS6g+T1wabFZGts9XdXmHoXc6zXL8fxUVvdI4Sdl39co",
            "nd7TayWPpdy+pD//z0qsoTkHEuKwd9dgiIU2bg7kh7AilXQM9ncze4IxkZyQfM87YAb3bMG",
            "Nwrd/DjbA2yWPWUmGV4+laMESgYDrXRFemNrNgif1v4eM5"
        );
        const KEY: &str = concat!(
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDfQV9T9lOopJtynnl+v0lz",
            "gusiA5zeYE20tSlD04EtdJ8SakjrEC1cbfGQN9azRSNAoVKdazKOQpGib+cwbXd4snw/CE2q",
            "KVZ6j5grp8QZvSqm8gjHwp3WwlfVbmOJPXLsSjvCU36jqI5s5VWfWiPAxDYklfUn4hz4aR5",
            "oabP+poMgsXxq411UEclqr+s6fv1TVPO95hT9CeTyNXtN1T1Wq1tVMLm3ULI7oGVLhdZJGE",
            "yLLdricnIda+YOZEeoUslfzuV3rQMQJyDRWdajNdpbPJL/nBzVTAaGpe+O+JIj0hP1jdDNzd",
            "ODTJ6b0JfrngB9mDYKGw4tnaLz8i0Tcpc/AgMBAAECggEASQLPgp1diZrfbVIPSJiVFE4d",
            "yF9nF0BmWTEfwBs0tSFc/kA8/YaqVv5rj+7662CyYSoA4xNSEr0JdJZlBGzgM9wnDtQP1hSz",
            "v9wi9y/jzUkUYElp/q4SQVAIOnfh3Fl4snaqaWg1057FiS5M3JK1e46PaFKUPIlRURnLhHkB",
            "EMdWNcCozrzihkLoJ5rTBQSGdmMFThGoF5MaK0MN4AxU1o0rWbI8GVnua4Cm4FuR3Eaqqsb",
            "b0mUl1JWykLo7FoO60tWCiD6mv5bhKNtkFpMMHBSWj9W9jX0n5pprsnfSYh0I8WHQKRBwqF",
            "KAxz3RDpo7LKS9RM1eqmos4FsW+L68IQKBgQD6TB/LeQa6kk45T5lJ6coABm1x66bves1L/",
            "8/PSE8XV3gBobSs4jG00bd9Mh1ulmEzd1oey62F5Dky/z4o6Fs/uj03dZjynsc0I9D3oykKB",
            "w7XkKdb7F5UYCVxAR818/bKZM1gbWqkWoo6pRA9bj8pNzvrRhbb8Rj959ZJtxOf4QKBgQDk",
            "V4Yg0u3pCtvlEp0x8+v4WbiQIrqgaJ6UwAAwpt4XS2yW6YU2x813McUXsqf3CiYCUurhxtZP",
            "q50llvXFZXKpzO8fekR5q7vgSFu9UrplCsDqkTcWqyGaGNuFBfr8TzSyc2zKkhrpWLn36sy/",
            "jxMgprk+uaQV2+CVdyHBQuWbHwKBgBLugRUl0VF5UXtaPvDtQv8ffVW5ikXg1vhhn/lAseL",
            "FFemhroXJEhNoLWXFzZ4Yt79pzqI3q6dN7NmjnrL/aC94ybqRJYFsawrRjrO8XpVIlWHOqi",
            "n0xenB3/MdL5woGMmUOEiL3h4STxRCeej7lsFqURjpkz8NjGNgDsBCnbRhAoGBALA0bje0LY",
            "0xKQE7fPyIO2bZbZgkhJm2QfGNvFfO3QFi3bgTGg5s3rwFNw+TeRQky7HtZH2337d5OfpA5",
            "QVfxL0NfNVwl5jAkml/zPNq/JVuV/Jq/vTKOFLerb+YHtdHE+ZFNgWX+5ZoNpH+qeOEuADx",
            "R3AE9386vrL4TJ8DTYWH AoGAb7l64MdxbuMZnzRpYMOg4n8aUQOD9QxX9WKtVwbcCfBskW",
            "p44v/NxFnLaJjqdg9zioAL8hL1PO1z0ya5mvAYkCV6ti80T8JZi144iV7yaciKXorCcvNqX",
            "ljY3cEmel5PhjJTLCn1+cVe/seZJwrrd5zP5Jlb+jvZxUQIzUmkAXw="
        );
        let cert = base64::engine::general_purpose::STANDARD
            .decode(CERT)
            .expect("certificate DER");
        let key = base64::engine::general_purpose::STANDARD
            .decode(KEY.replace(' ', ""))
            .expect("private key DER");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .expect("TLS server config");
        tokio_rustls::TlsAcceptor::from(Arc::new(config))
    }

    fn only_peers6_http_response(peer: SocketAddr) -> Vec<u8> {
        let IpAddr::V6(address) = peer.ip() else {
            panic!("fixture peer must be IPv6");
        };
        let mut body = b"d8:intervali900e6:peers618:".to_vec();
        body.extend_from_slice(&address.octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response
    }

    fn compact_ipv4_http_response(peer: SocketAddr) -> Vec<u8> {
        let IpAddr::V4(address) = peer.ip() else {
            panic!("fixture peer must be IPv4");
        };
        let mut body = b"d8:intervali900e5:peers6:".to_vec();
        body.extend_from_slice(&address.octets());
        body.extend_from_slice(&peer.port().to_be_bytes());
        body.push(b'e');
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(&body);
        response
    }

    async fn serve_tracker_stream<S>(mut stream: S, peer: SocketAddr) -> (String, bool)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let request = read_http_request(&mut stream).await;
        let stopped = request.contains("&event=stopped ");
        stream
            .write_all(&only_peers6_http_response(peer))
            .await
            .expect("write tracker response");
        stream.shutdown().await.expect("close tracker response");
        (request, stopped)
    }

    fn percent_encode_magnet_value(value: &str) -> String {
        let mut encoded = String::with_capacity(value.len() * 3);
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for byte in value.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                encoded.push(char::from(byte));
            } else {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
        encoded
    }

    fn dht_endpoint(address: SocketAddr) -> DhtEndpoint {
        let port = address.port();
        match address.ip() {
            IpAddr::V4(address) => DhtEndpoint::new(DhtIp::V4(address.octets()), port),
            IpAddr::V6(address) => DhtEndpoint::new(DhtIp::V6(address.octets()), port),
        }
    }

    async fn run_tracker_only_peers6_application_transfer(https: bool) {
        let root = test_root(if https {
            "https-tracker-ipv6-transfer"
        } else {
            "http-tracker-ipv6-transfer"
        });
        let payload = b"hash verified HTTP tracker IPv6 payload".to_vec();
        let raw_info = single_file_info("tracker-ipv6.bin", &payload, payload.len());
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);

        let peer_listener = match TcpListener::bind("[::1]:0").await {
            Ok(listener) => listener,
            Err(_) => return,
        };
        let peer_address = peer_listener.local_addr().expect("IPv6 peer address");
        let peer_task = tokio::spawn(serve_single_piece_peer(
            peer_listener,
            info_hash,
            payload.clone(),
        ));

        let tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind HTTP tracker");
        let scheme = if https { "https" } else { "http" };
        let tracker_url = format!(
            "{scheme}://{}/announce?passkey=fixture",
            tracker.local_addr().expect("tracker address")
        );
        let tls_acceptor = https.then(untrusted_tls_acceptor);
        let (announce_sender, mut announce_receiver) =
            tokio::sync::mpsc::unbounded_channel::<String>();
        let tracker_task = tokio::spawn(async move {
            let mut requests = Vec::new();
            loop {
                let (stream, _) = tracker.accept().await.expect("accept tracker announce");
                let (request, stopped) = if let Some(acceptor) = tls_acceptor.as_ref() {
                    let stream = acceptor.accept(stream).await.expect("TLS handshake");
                    serve_tracker_stream(stream, peer_address).await
                } else {
                    serve_tracker_stream(stream, peer_address).await
                };
                announce_sender
                    .send(request.clone())
                    .expect("report tracker request");
                requests.push(request);
                if stopped {
                    return requests;
                }
            }
        });

        let configuration = config(&root);
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open fixture store");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-http-tracker-ipv6".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!(
                        "magnet:?xt=urn:btih:{torrent_id}&tr={}",
                        percent_encode_magnet_value(&tracker_url)
                    ),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add tracker-only magnet");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record verified metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open application");
        let first_announce = tokio::time::timeout(Duration::from_secs(2), announce_receiver.recv())
            .await
            .expect("first tracker announce deadline")
            .expect("tracker server ended before first announce");
        assert!(first_announce.contains("&event=started "));
        let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
            let mut sequence = 0_u64;
            loop {
                let response = service
                    .dispatch(RequestEnvelope {
                        version: CONTROL_VERSION,
                        request_id: format!("tracker-transfer-{sequence}"),
                        expected_revision: None,
                        command: Command::Snapshot,
                    })
                    .await
                    .expect("snapshot transfer");
                let ResponseOutcome::Success { snapshot } = response.outcome else {
                    panic!("snapshot should succeed");
                };
                if snapshot.torrents[0].state == TorrentState::Complete {
                    return snapshot;
                }
                sequence = sequence.saturating_add(1);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(_) => {
                let response = service
                    .dispatch(RequestEnvelope {
                        version: CONTROL_VERSION,
                        request_id: "tracker-transfer-timeout".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    })
                    .await
                    .expect("timeout snapshot");
                let ResponseOutcome::Success { snapshot } = response.outcome else {
                    panic!("timeout snapshot should succeed");
                };
                let peers = torrent_peer_views(&service, &torrent_id).await;
                let trackers = torrent_tracker_views(&service, &torrent_id).await;
                panic!(
                    "tracker IPv6 transfer deadline; torrent={:?}; peers={peers:?}; trackers={trackers:?}; peer_finished={}; tracker_finished={}",
                    snapshot.torrents[0],
                    peer_task.is_finished(),
                    tracker_task.is_finished()
                );
            }
        };
        assert_eq!(snapshot.torrents[0].verified_piece_count, 1);
        assert_eq!(
            fs::read(root.join("payload/tracker-ipv6.bin")).expect("published payload"),
            payload
        );
        let trackers = torrent_tracker_views(&service, &torrent_id).await;
        assert_eq!(trackers.len(), 1);
        assert_eq!(trackers[0].last_peer_count, Some(1));
        assert_eq!(
            trackers[0].last_connection_family,
            Some(TrackerConnectionFamilyView::Ipv4)
        );
        assert_eq!(
            trackers[0].security,
            if https {
                TrackerSecurityView::EncryptedUnauthenticated
            } else {
                TrackerSecurityView::Unencrypted
            }
        );

        service.shutdown().await.expect("shutdown application");
        drop(service);
        peer_task.await.expect("IPv6 peer task");
        let requests = tracker_task.await.expect("HTTP tracker task");
        assert!(
            requests
                .iter()
                .any(|request| request.contains("&event=started "))
        );
        assert!(
            requests
                .iter()
                .any(|request| request.contains("&event=stopped "))
        );
        assert!(
            requests
                .iter()
                .all(|request| request.starts_with("GET /announce?passkey=fixture&"))
        );
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn http_tracker_only_peers6_completes_hash_verified_application_transfer() {
        run_tracker_only_peers6_application_transfer(false).await;
    }

    #[tokio::test]
    async fn unauthenticated_https_tracker_completes_hash_verified_application_transfer() {
        run_tracker_only_peers6_application_transfer(true).await;
    }

    #[tokio::test]
    async fn metadata_only_add_activates_tracker_until_metadata_is_verified() {
        let root = test_root("metadata-only-tracker");
        let raw_info = multi_file_info();
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let (peer, peer_task) = spawn_metadata_peer(raw_info).await;

        let tracker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata tracker");
        let tracker_url = format!(
            "http://{}/announce",
            tracker.local_addr().expect("tracker address")
        );
        let response = compact_ipv4_http_response(peer);
        let (announce_sender, mut announce_receiver) =
            tokio::sync::mpsc::unbounded_channel::<String>();
        let tracker_task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = tracker.accept().await.expect("accept tracker announce");
                let request = read_http_request(&mut stream).await;
                stream
                    .write_all(&response)
                    .await
                    .expect("write tracker response");
                stream.shutdown().await.expect("close tracker response");
                let stopped = request.contains("&event=stopped ");
                announce_sender
                    .send(request)
                    .expect("report tracker request");
                if stopped {
                    break;
                }
            }
        });

        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open application");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "metadata-only-tracker-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!(
                        "magnet:?xt=urn:btih:{torrent_id}&tr={}",
                        percent_encode_magnet_value(&tracker_url)
                    ),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .await
            .expect("add metadata-only tracker magnet");

        let started = tokio::time::timeout(Duration::from_secs(2), announce_receiver.recv())
            .await
            .expect("started announce deadline")
            .expect("tracker ended before started announce");
        assert!(started.contains("&event=started "), "{started}");

        let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
            for sequence in 0_u64..500 {
                let response = service
                    .dispatch(RequestEnvelope {
                        version: CONTROL_VERSION,
                        request_id: format!("metadata-only-tracker-snapshot-{sequence}"),
                        expected_revision: None,
                        command: Command::Snapshot,
                    })
                    .await
                    .expect("metadata snapshot");
                let ResponseOutcome::Success { snapshot } = response.outcome else {
                    panic!("snapshot should succeed");
                };
                if snapshot.torrents[0].metadata_available {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("metadata did not become available");
        })
        .await
        .expect("metadata-only tracker deadline");
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        assert_eq!(snapshot.torrents[0].storage_state, StorageState::None);

        let stopped = tokio::time::timeout(Duration::from_secs(2), announce_receiver.recv())
            .await
            .expect("stopped announce deadline")
            .expect("tracker ended before stopped announce");
        assert!(stopped.contains("&event=stopped "), "{stopped}");
        let trackers = torrent_tracker_views(&service, &torrent_id).await;
        assert_eq!(trackers.len(), 1);
        assert_eq!(trackers[0].total_attempts, 2);
        assert_eq!(
            trackers[0].last_connection_family,
            Some(TrackerConnectionFamilyView::Ipv4)
        );

        let paths = torrent_storage_paths(&root.join("payload"), "multi", info_hash)
            .expect("storage paths");
        assert!(!paths.output.exists());
        assert!(!paths.staging.exists());
        assert!(!paths.part.exists());

        peer_task.await.expect("metadata peer task");
        tracker_task.await.expect("metadata tracker task");
        service.shutdown().await.expect("shutdown application");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn application_shutdown_closes_view_sets_and_wakes_waiters() {
        let root = test_root("view-set-shutdown");
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open application");
        let owner = ViewSetOwner::trusted("test-owner");
        let opened = service
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![ViewSpec::TorrentList {
                        view_id: "library".to_owned(),
                        delivery: ViewDeliveryPolicy::default(),
                    }],
                    options: OpenViewSetOptions::default(),
                },
            )
            .expect("open view set");
        let view_set = service
            .view_set(&owner, &opened.view_set_id)
            .expect("view set handle");
        let cursor = opened.initial.cursor;
        let waiter = tokio::spawn(async move { view_set.next_updates(&cursor, 20_000).await });
        tokio::task::yield_now().await;

        service.shutdown().await.expect("shutdown");
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter timed out")
            .expect("waiter task");
        assert_eq!(result, Err(ViewSetError::Closed));
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn torrent_bytes_metadata_only_add_restarts_without_payload_artifacts() {
        let root = test_root("torrent-bytes-metadata-only");
        let raw_info = multi_file_info();
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let mut source = b"d4:info".to_vec();
        source.extend_from_slice(&raw_info);
        source.push(b'e');
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open application");

        let response = service
            .add_torrent_bytes(
                torrent_bytes_request("bytes-metadata-only", &source, false),
                source,
            )
            .await
            .expect("add torrent bytes");
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        let resume = service
            .store_mut()
            .expect("store")
            .load_resume(&torrent_id)
            .expect("load imported torrent");
        assert_eq!(resume.raw_info.as_deref(), Some(raw_info.as_slice()));
        assert_eq!(resume.state, TorrentState::Paused);
        assert!(!resume.desired_running);
        let paths = torrent_storage_paths(&root.join("payload"), "multi", info_hash)
            .expect("storage paths");
        assert!(!paths.output.exists());
        assert!(!paths.staging.exists());
        assert!(!paths.part.exists());
        service.shutdown().await.expect("shutdown application");
        drop(service);

        let mut reopened = ApplicationService::open(config(&root))
            .await
            .expect("reopen application");
        let resume = reopened
            .store_mut()
            .expect("store")
            .load_resume(&torrent_id)
            .expect("restart imported torrent");
        assert_eq!(resume.raw_info.as_deref(), Some(raw_info.as_slice()));
        assert_eq!(resume.state, TorrentState::Paused);
        assert!(!paths.output.exists());
        assert!(!paths.staging.exists());
        assert!(!paths.part.exists());
        reopened.shutdown().await.expect("shutdown reopened");
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn metadata_only_add_verifies_metadata_without_content_artifacts() {
        let root = test_root("metadata-only-add");
        let raw_info = multi_file_info();
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind metadata peer");
        let address = listener.local_addr().expect("metadata peer address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept metadata peer");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read metadata handshake");
            let handshake =
                decode_handshake(&handshake, info_hash).expect("metadata handshake identity");
            assert!(handshake.supports_extensions());
            let mut reserved = [0; 8];
            reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
            stream
                .write_all(&encode_handshake_with_reserved(
                    info_hash,
                    *b"-RS-META-ONLY-000000",
                    reserved,
                ))
                .await
                .expect("send metadata handshake");
            let mut decoder = FrameDecoder::new();
            let mut pending = std::collections::VecDeque::new();
            assert!(matches!(
                read_peer_message(&mut stream, &mut decoder, &mut pending).await,
                PeerMessage::Extended { id: 0, .. }
            ));
            stream
                .write_all(
                    &encode_message(&PeerMessage::Extended {
                        id: 0,
                        payload: encode_extension_handshake(Some(raw_info.len())),
                    })
                    .expect("encode extension handshake"),
                )
                .await
                .expect("send extension handshake");
            let PeerMessage::Extended {
                id: 1,
                payload: request,
            } = read_peer_message(&mut stream, &mut decoder, &mut pending).await
            else {
                panic!("metadata request expected");
            };
            assert_eq!(
                parse_metadata_message(&request).expect("parse metadata request"),
                MetadataMessage::Request { piece: 0 }
            );
            stream
                .write_all(
                    &encode_message(&PeerMessage::Extended {
                        id: 1,
                        payload: encode_metadata_data(0, raw_info.len(), &raw_info)
                            .expect("encode metadata"),
                    })
                    .expect("encode metadata message"),
                )
                .await
                .expect("send metadata");
            let mut tail = [0; 32];
            match stream.read(&mut tail).await {
                Ok(0) => {}
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                outcome => panic!("metadata connection did not close: {outcome:?}"),
            }
        });

        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open application");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "metadata-only-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe={address}"),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .await
            .expect("add metadata-only torrent");
        let snapshot = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut ready = None;
            for sequence in 0..100 {
                tokio::task::yield_now().await;
                let response = service
                    .dispatch(RequestEnvelope {
                        version: CONTROL_VERSION,
                        request_id: format!("metadata-snapshot-{sequence}"),
                        expected_revision: None,
                        command: Command::Snapshot,
                    })
                    .await
                    .expect("metadata snapshot");
                let ResponseOutcome::Success { snapshot } = response.outcome else {
                    panic!("snapshot failed");
                };
                if snapshot.torrents[0].metadata_available {
                    ready = Some(snapshot);
                    break;
                }
            }
            ready.expect("metadata did not become available")
        })
        .await
        .expect("metadata-only add timed out");
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        assert_eq!(snapshot.torrents[0].storage_state, StorageState::None);
        let paths = torrent_storage_paths(&root.join("payload"), "multi", info_hash)
            .expect("plan content paths");
        assert!(!paths.output.exists());
        assert!(!paths.staging.exists());
        assert!(!paths.part.exists());
        tokio::time::timeout(std::time::Duration::from_secs(1), peer_task)
            .await
            .expect("metadata peer did not join")
            .expect("metadata peer task");
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn ephemeral_application_owns_loopback_state_until_joined_shutdown() {
        let root = test_root("ephemeral-loopback");
        let payload_root = root.join("payload");
        fs::create_dir_all(&payload_root).expect("pre-provision payload root");
        let absent_profile = root.join("profile-was-never-configured");
        let raw_info = multi_file_info();
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let (address, peer_task) = spawn_metadata_peer(raw_info).await;
        let application_config = ApplicationConfig::ephemeral(
            "ephemeral-test".to_owned(),
            vec![ConfiguredStorageRoot::path(
                "downloads",
                payload_root.clone(),
            )],
            NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(5),
            ),
        );
        let mut service = ApplicationService::open(application_config.clone())
            .await
            .expect("open ephemeral application");
        assert!(!absent_profile.exists());
        let usage = service
            .store
            .lock()
            .expect("session store")
            .page_usage()
            .expect("session page usage");
        assert!(usage.page_count <= usage.maximum_page_count);

        let diagnostics = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: Some(DiagnosticFilter::default()),
                catalog_page: None,
            })
            .expect("diagnostic subscription")
            .next_update()
            .await
            .expect("diagnostic snapshot");
        assert!(
            serde_json::to_string(&diagnostics)
                .expect("encode diagnostics")
                .contains("ephemeral")
        );

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "ephemeral-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe={address}"),
                    storage_root: "downloads".to_owned(),
                    start_content: false,
                    skip_files: Vec::new(),
                },
            })
            .await
            .expect("add ephemeral metadata-only torrent");
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                tokio::task::yield_now().await;
                let response = service
                    .dispatch(RequestEnvelope {
                        version: CONTROL_VERSION,
                        request_id: "ephemeral-wait".to_owned(),
                        expected_revision: None,
                        command: Command::Snapshot,
                    })
                    .await
                    .expect("ephemeral snapshot");
                let ResponseOutcome::Success { snapshot } = response.outcome else {
                    panic!("snapshot failed");
                };
                if snapshot.torrents[0].metadata_available {
                    break;
                }
            }
        })
        .await
        .expect("ephemeral metadata timed out");
        tokio::time::timeout(std::time::Duration::from_secs(1), peer_task)
            .await
            .expect("ephemeral peer did not join")
            .expect("ephemeral peer task");

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "ephemeral-selection".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriority {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![1],
                    priority: FilePriority::Skip,
                },
            })
            .await
            .expect("change ephemeral selection");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "ephemeral-setting".to_owned(),
                expected_revision: None,
                command: Command::SetShowAddOptions { show: false },
            })
            .await
            .expect("change ephemeral setting");
        service
            .speed_recorder
            .record(ByteMetric::PayloadReceived, 4096);

        let owner = ViewSetOwner::trusted("ephemeral-client");
        let opened = service
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![ViewSpec::TorrentList {
                        view_id: "library".to_owned(),
                        delivery: ViewDeliveryPolicy::default(),
                    }],
                    options: OpenViewSetOptions::default(),
                },
            )
            .expect("open ephemeral view set");
        service
            .close_view_set(&owner, &opened.view_set_id)
            .expect("close last ephemeral view set");
        let snapshot = service
            .store
            .lock()
            .expect("session store")
            .snapshot()
            .expect("snapshot after detachment");
        assert_eq!(snapshot.torrents[0].skip_files, vec![1]);
        assert!(!snapshot.storage.show_add_options);
        assert!(!absent_profile.exists());
        assert_eq!(
            fs::read_dir(&payload_root)
                .expect("read payload root")
                .count(),
            0
        );

        service.shutdown().await.expect("joined ephemeral shutdown");
        assert!(
            service
                .store
                .lock()
                .expect("session store after shutdown")
                .load_dht_snapshot()
                .expect("load in-memory DHT snapshot")
                .is_some()
        );
        drop(service);
        assert!(!absent_profile.exists());
        assert_eq!(
            fs::read_dir(&payload_root)
                .expect("read payload root")
                .count(),
            0
        );

        let mut fresh = ApplicationService::open(application_config)
            .await
            .expect("open fresh ephemeral application");
        let fresh_snapshot = fresh
            .store
            .lock()
            .expect("fresh session store")
            .snapshot()
            .expect("fresh snapshot");
        assert_eq!(fresh_snapshot.revision, "0");
        assert!(fresh_snapshot.torrents.is_empty());
        assert!(fresh_snapshot.storage.show_add_options);
        assert!(
            fresh
                .store
                .lock()
                .expect("fresh session store")
                .load_dht_snapshot()
                .expect("load fresh DHT snapshot")
                .is_none()
        );
        fresh.shutdown().await.expect("shutdown fresh application");
        drop(fresh);
        fs::remove_dir_all(root).expect("remove ephemeral test root");
    }

    #[tokio::test]
    async fn ephemeral_application_opens_and_closes_offline_without_a_profile() {
        let root = test_root("ephemeral-offline");
        let payload_root = root.join("payload");
        fs::create_dir_all(&payload_root).expect("pre-provision payload root");
        let absent_profile = root.join("profile-was-never-configured");
        let mut application_config = ApplicationConfig::ephemeral(
            "offline-ephemeral".to_owned(),
            vec![ConfiguredStorageRoot::path(
                "downloads",
                payload_root.clone(),
            )],
            NetworkConfig::new(
                NetworkPolicy::Offline,
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(1),
            ),
        );
        application_config.dht.bootstrap_nodes.clear();
        let mut service = ApplicationService::open(application_config)
            .await
            .expect("open offline ephemeral application");
        assert_eq!(service.revision().expect("offline revision"), 0);
        assert!(!absent_profile.exists());
        service
            .shutdown()
            .await
            .expect("shutdown offline ephemeral application");
        drop(service);
        assert!(!absent_profile.exists());
        assert_eq!(
            fs::read_dir(&payload_root)
                .expect("read payload root")
                .count(),
            0
        );
        fs::remove_dir_all(root).expect("remove offline ephemeral test root");
    }

    async fn answer_dht_query(router: &UdpSocket) -> SocketAddr {
        let mut packet = [0_u8; 1024];
        let (length, client) = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            router.recv_from(&mut packet),
        )
        .await
        .expect("DHT query timed out")
        .expect("receive DHT query");
        let DhtMessage::Query(query) = decode_dht(&packet[..length]).expect("decode DHT query")
        else {
            panic!("DHT query expected");
        };
        let response = encode_dht_response(
            &query.transaction,
            NodeId([9; 20]),
            &[],
            &[],
            None,
            dht_endpoint(client),
        )
        .expect("encode DHT response");
        router
            .send_to(&response, client)
            .await
            .expect("send DHT response");
        client
    }

    async fn local_network_ipv4() -> Option<Ipv4Addr> {
        let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
        probe
            .connect((Ipv4Addr::new(239, 255, 255, 250), 1_900))
            .await
            .ok()?;
        let SocketAddr::V4(address) = probe.local_addr().ok()? else {
            return None;
        };
        let address = *address.ip();
        (!address.is_unspecified()
            && !address.is_loopback()
            && !address.is_multicast()
            && !address.is_broadcast())
        .then_some(address)
    }

    async fn available_port_on(address: Ipv4Addr) -> Option<u16> {
        for _ in 0..16 {
            let tcp = TcpListener::bind((address, 0)).await.ok()?;
            let port = tcp.local_addr().ok()?.port();
            if UdpSocket::bind((address, port)).await.is_ok() {
                return Some(port);
            }
        }
        None
    }

    #[tokio::test]
    async fn application_coordinates_tcp_and_dht_udp_endpoints() {
        let root = test_root("coordinated-session-sockets");
        let preferred_probe = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind preferred-port probe");
        let preferred_port = preferred_probe.local_addr().unwrap().port();
        let preferred_udp_probe = UdpSocket::bind((Ipv4Addr::LOCALHOST, preferred_port))
            .await
            .expect("bind preferred UDP-port probe");
        drop(preferred_probe);
        drop(preferred_udp_probe);
        let router = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind DHT router");
        let router_address = router.local_addr().expect("DHT router address");
        let mut configuration = config(&root);
        configuration.dht.bootstrap_nodes = vec![BootstrapNode::Address(router_address)];
        persist_client_settings(
            &configuration,
            ClientSettings {
                listener: ListenerPolicy::AutomaticLoopback,
                preferred_listen_port: preferred_port,
                ..ClientSettings::default()
            },
        );
        let mut application = ApplicationService::open(configuration)
            .await
            .expect("open coordinated application");
        let runtime = client_settings_runtime(&application).await;
        let ListenerStatus::Listening {
            address: tcp_address,
            port: tcp_port,
        } = runtime.listener_status
        else {
            panic!("coordinated TCP listener must be active");
        };
        let crate::SessionUdpStatus::Bound {
            address: udp_address,
            port: udp_port,
            coordinated_with_tcp,
        } = runtime.session_udp_status
        else {
            panic!("session UDP endpoint must be active");
        };
        assert_eq!(tcp_address, "127.0.0.1");
        assert_eq!(udp_address, tcp_address);
        assert_eq!(tcp_port, preferred_port);
        assert_eq!(udp_port, tcp_port);
        assert!(coordinated_with_tcp);

        let observed_dht_source = answer_dht_query(&router).await;
        assert_eq!(observed_dht_source.ip().to_string(), udp_address);
        assert_eq!(observed_dht_source.port(), udp_port);
        let tcp = TcpStream::connect((tcp_address.as_str(), tcp_port))
            .await
            .expect("connect observed TCP endpoint");
        drop(tcp);
        assert_eq!(
            application
                .session_udp
                .as_ref()
                .expect("session UDP owner")
                .snapshot()
                .task_high_water,
            1
        );
        application.shutdown().await.expect("joined shutdown");
        assert!(application.session_udp.is_none());
        drop(application);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn application_coordinates_local_network_endpoints_when_available() {
        let Some(local_address) = local_network_ipv4().await else {
            return;
        };
        let Some(preferred_port) = available_port_on(local_address).await else {
            return;
        };
        let root = test_root("coordinated-local-network-sockets");
        let router = UdpSocket::bind((local_address, 0))
            .await
            .expect("bind local-network DHT router");
        let router_address = router.local_addr().expect("DHT router address");
        let network = NetworkConfig::new(
            NetworkPolicy::Online,
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(5),
        );
        let mut configuration = ApplicationConfig::new(
            root.join("profile"),
            "test".to_owned(),
            vec![ConfiguredStorageRoot::path(
                "downloads",
                root.join("payload"),
            )],
            network,
        );
        configuration.dht.bootstrap_nodes = vec![BootstrapNode::Address(router_address)];
        persist_client_settings(
            &configuration,
            ClientSettings {
                listener: ListenerPolicy::AutomaticLocalNetwork,
                preferred_listen_port: preferred_port,
                ..ClientSettings::default()
            },
        );
        let mut application = ApplicationService::open(configuration)
            .await
            .expect("open local-network application");
        let runtime = client_settings_runtime(&application).await;
        let ListenerStatus::Listening { address, port } = runtime.listener_status else {
            panic!("local-network TCP listener must be active");
        };
        let crate::SessionUdpStatus::Bound {
            address: udp_address,
            port: udp_port,
            coordinated_with_tcp,
        } = runtime.session_udp_status
        else {
            panic!("local-network session UDP endpoint must be active");
        };
        assert_eq!(address, local_address.to_string());
        assert_eq!(udp_address, address);
        assert_eq!(port, preferred_port);
        assert_eq!(udp_port, port);
        assert!(coordinated_with_tcp);

        let observed_dht_source = answer_dht_query(&router).await;
        assert_eq!(
            observed_dht_source,
            SocketAddr::from((local_address, udp_port))
        );
        let tcp = TcpStream::connect((local_address, port))
            .await
            .expect("connect local-network TCP endpoint");
        drop(tcp);
        application.shutdown().await.expect("joined shutdown");
        drop(application);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn application_shutdown_persists_and_revalidates_warm_dht_node() {
        let root = test_root("dht-warm-restart");
        let router = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind DHT router");
        let router_address = router.local_addr().expect("DHT router address");
        let mut first_config = config(&root);
        first_config.dht.bootstrap_nodes = vec![BootstrapNode::Address(router_address)];
        let mut first = ApplicationService::open(first_config)
            .await
            .expect("open first application");
        answer_dht_query(&router).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        first.shutdown().await.expect("first shutdown");
        drop(first);

        let mut persisted = config(&root);
        persisted.dht.bootstrap_nodes.clear();
        let store = SessionStore::open(
            persisted
                .durable_profile_root()
                .expect("durable profile root"),
            &persisted.profile_id,
            &persisted.storage_roots,
        )
        .expect("open persisted store");
        let snapshot = store
            .load_dht_snapshot()
            .expect("load DHT snapshot")
            .expect("saved DHT snapshot");
        assert_eq!(snapshot.nodes_v4[0].address, dht_endpoint(router_address));
        drop(store);

        let mut second = ApplicationService::open(persisted)
            .await
            .expect("open second application");
        answer_dht_query(&router).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        second.shutdown().await.expect("second shutdown");
        drop(second);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn application_shutdown_joins_terminal_dht_view_forwarding() {
        let root = test_root("dht-terminal-view");
        let mut application = ApplicationService::open(config(&root))
            .await
            .expect("open application");
        let subscription = application
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::SessionDht,
                projection: ViewProjection::Dht,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe to DHT view");
        subscription
            .next_update()
            .await
            .expect("initial DHT snapshot");

        application.shutdown().await.expect("shutdown application");

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let update = subscription
                    .next_update()
                    .await
                    .expect("terminal DHT update");
                if let ViewUpdatePayload::Patch {
                    patch: ViewPatch::SessionDht { inspection },
                } = update.payload
                    && inspection.lifecycle == DhtLifecycleView::Inactive
                {
                    assert_eq!(inspection.active_transactions, 0);
                    assert_eq!(inspection.active_lookups, 0);
                    assert!(inspection.lookups.is_empty());
                    break;
                }
            }
        })
        .await
        .expect("terminal DHT view timed out");
        drop(application);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn command_mutation_publishes_a_typed_view_patch() {
        let root = test_root("view-patch");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe");
        assert!(matches!(
            subscription.next_update().await.expect("initial").payload,
            ViewUpdatePayload::Snapshot { .. }
        ));

        service
            .dispatch(add_request("add", torrent_id))
            .await
            .expect("add");
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            subscription.next_update(),
        )
        .await
        .expect("view update timed out")
        .expect("view update");
        assert_eq!(update.base_revision, "0");
        assert_eq!(update.revision, "1");
        let ViewUpdatePayload::Patch { patch } = update.payload else {
            panic!("mutation must publish a patch");
        };
        assert!(
            serde_json::to_string(&patch)
                .expect("serialize patch")
                .contains(torrent_id)
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn client_settings_mutation_publishes_configured_restart_state() {
        let root = test_root("client-settings-view-patch");
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        let subscription = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 4096,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("subscribe");
        let initial = subscription.next_update().await.expect("initial");
        let ViewUpdatePayload::Snapshot {
            snapshot:
                ViewSnapshot::TorrentList {
                    client_settings: initial,
                    ..
                },
        } = initial.payload
        else {
            panic!("expected settings snapshot");
        };
        assert!(!initial.restart_required);

        let configured = ClientSettings {
            listener: ListenerPolicy::AutomaticLoopback,
            preferred_listen_port: 6_881,
            port_mapping: crate::PortMappingPolicy::Disabled,
            peer_connection_limit: 321,
            upload_slots: 3,
        };
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "set-client-settings-view".to_owned(),
                expected_revision: Some("0".to_owned()),
                command: Command::SetClientSettings {
                    settings: configured.clone(),
                },
            })
            .await
            .expect("set settings");
        let update = subscription.next_update().await.expect("settings patch");
        let ViewUpdatePayload::Patch {
            patch:
                ViewPatch::TorrentList {
                    client_settings: Some(runtime),
                    ..
                },
        } = update.payload
        else {
            panic!("expected settings replacement patch");
        };
        assert_eq!(runtime.configured, configured);
        assert_eq!(runtime.active, ClientSettings::default());
        assert!(runtime.restart_required);

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "revert-client-settings-view".to_owned(),
                expected_revision: Some("1".to_owned()),
                command: Command::SetClientSettings {
                    settings: ClientSettings::default(),
                },
            })
            .await
            .expect("revert settings to active values");
        let update = subscription.next_update().await.expect("revert patch");
        let ViewUpdatePayload::Patch {
            patch:
                ViewPatch::TorrentList {
                    client_settings: Some(runtime),
                    ..
                },
        } = update.payload
        else {
            panic!("expected reverted settings replacement patch");
        };
        assert_eq!(runtime.configured, ClientSettings::default());
        assert_eq!(runtime.active, ClientSettings::default());
        assert!(!runtime.restart_required);

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    async fn wait_for_torrent_state(
        service: &mut ApplicationService,
        torrent_id: &str,
        expected: TorrentState,
        label: &str,
    ) {
        for sequence in 0..200 {
            let response = service
                .dispatch(RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: format!("{label}-snapshot-{sequence}"),
                    expected_revision: None,
                    command: Command::Snapshot,
                })
                .await
                .expect("poll torrent state");
            let ResponseOutcome::Success { snapshot } = response.outcome else {
                panic!("state poll must succeed");
            };
            let torrent = snapshot
                .torrents
                .iter()
                .find(|torrent| torrent.torrent_id == torrent_id)
                .expect("polled torrent");
            if torrent.state == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("torrent {torrent_id} did not reach {expected:?}");
    }

    #[tokio::test]
    async fn complete_startup_and_force_recheck_rebuild_have_without_network() {
        let root = test_root("complete-force-recheck");
        let configuration = config(&root);
        let payload = (0..32_768)
            .map(|offset| ((offset * 31 + offset / 13) & 0xff) as u8)
            .collect::<Vec<_>>();
        let raw_info = single_file_info("complete.bin", &payload, 16_384);
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-complete-recheck", &torrent_id))
            .expect("add complete torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record complete metadata");
        store
            .record_pieces(&torrent_id, &[0, 1])
            .expect("record old complete have");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record published ownership");
        store.mark_complete(&torrent_id).expect("mark complete");
        drop(store);
        let output = root.join("payload/complete.bin");
        fs::create_dir_all(output.parent().expect("payload parent")).expect("create payload root");
        fs::write(&output, &payload).expect("write complete publication");

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open complete application");
        wait_for_torrent_state(
            &mut service,
            &torrent_id,
            TorrentState::Complete,
            "startup-recheck",
        )
        .await;
        assert_eq!(
            service
                .store_mut()
                .expect("store")
                .load_resume(&torrent_id)
                .expect("startup result")
                .have
                .expect("startup have")
                .pieces(),
            &[true, true]
        );

        let force = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "force-complete-recheck".to_owned(),
            expected_revision: None,
            command: Command::ForceRecheck {
                torrent_id: torrent_id.clone(),
            },
        };
        service
            .dispatch(force.clone())
            .await
            .expect("force recheck");
        wait_for_torrent_state(
            &mut service,
            &torrent_id,
            TorrentState::Complete,
            "forced-recheck",
        )
        .await;
        let revision = service
            .store_mut()
            .expect("store")
            .revision()
            .expect("completed revision");
        service.dispatch(force).await.expect("replay force recheck");
        assert_eq!(
            service
                .store_mut()
                .expect("store")
                .revision()
                .expect("replayed revision"),
            revision,
            "replaying one request must not start another generation"
        );
        assert_eq!(fs::read(&output).expect("read rechecked output"), payload);

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn startup_restarts_interrupted_check_even_with_paused_intent() {
        let root = test_root("paused-interrupted-recheck");
        let configuration = config(&root);
        let payload = (0..32_768)
            .map(|offset| ((offset * 23 + offset / 11) & 0xff) as u8)
            .collect::<Vec<_>>();
        let raw_info = single_file_info("paused-check.bin", &payload, 16_384);
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-paused-check", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record metadata");
        store
            .record_pieces(&torrent_id, &[0, 1])
            .expect("record old have");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record published ownership");
        store.mark_complete(&torrent_id).expect("mark complete");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-interrupted-check".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .expect("persist paused intent");
        store
            .begin_recheck(&torrent_id)
            .expect("begin interrupted check");
        drop(store);
        let output = root.join("payload/paused-check.bin");
        fs::create_dir_all(output.parent().expect("payload parent")).expect("create payload root");
        fs::write(&output, &payload).expect("write published payload");

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("restart application");
        wait_for_torrent_state(
            &mut service,
            &torrent_id,
            TorrentState::Complete,
            "paused-interrupted-check",
        )
        .await;
        let resume = service
            .store_mut()
            .expect("store")
            .load_resume(&torrent_id)
            .expect("completed resume");
        assert!(!resume.desired_running);
        assert_eq!(
            resume.have.expect("replacement have").pieces(),
            &[true, true]
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn paused_force_recheck_clears_corruption_without_starting_repair() {
        let root = test_root("paused-force-recheck");
        let configuration = config(&root);
        let payload = (0..32_768)
            .map(|offset| ((offset * 17 + offset / 7) & 0xff) as u8)
            .collect::<Vec<_>>();
        let raw_info = single_file_info("paused.bin", &payload, 16_384);
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-paused-recheck", &torrent_id))
            .expect("add paused torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record paused metadata");
        store
            .record_pieces(&torrent_id, &[0, 1])
            .expect("record stale complete have");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record published ownership");
        store.mark_complete(&torrent_id).expect("mark complete");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-before-force".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .expect("persist paused intent");
        drop(store);
        let output = root.join("payload/paused.bin");
        fs::create_dir_all(output.parent().expect("payload parent")).expect("create payload root");
        let mut corrupt = payload.clone();
        corrupt[16_384] ^= 0xff;
        fs::write(&output, &corrupt).expect("write corrupt publication");

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open paused application");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "force-paused-recheck".to_owned(),
                expected_revision: None,
                command: Command::ForceRecheck {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("force paused recheck");
        wait_for_torrent_state(
            &mut service,
            &torrent_id,
            TorrentState::Paused,
            "paused-recheck",
        )
        .await;
        assert_eq!(
            service
                .store_mut()
                .expect("store")
                .load_resume(&torrent_id)
                .expect("paused result")
                .have
                .expect("paused have")
                .pieces(),
            &[true, false]
        );
        assert_eq!(
            fs::read(&output).expect("read unrepaired publication"),
            corrupt,
            "paused intent must not admit repair writes"
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn pause_is_durable_and_prevents_restart() {
        let root = test_root("pause");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        service
            .dispatch(add_request("add", torrent_id))
            .await
            .expect("add");
        let paused = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .await
            .expect("pause");
        let ResponseOutcome::Success { snapshot } = paused.outcome else {
            panic!("pause should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        service.shutdown().await.expect("shutdown");
        drop(service);

        let mut reopened = ApplicationService::open(config(&root))
            .await
            .expect("reopen service");
        let snapshot = reopened
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = snapshot.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        reopened.shutdown().await.expect("shutdown reopened");
        drop(reopened);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn pause_joins_content_peer_before_view_set_removal() {
        let root = test_root("pause-peer-cleanup");
        let configuration = config(&root);
        let payload = b"view";
        let mut raw_info =
            b"d5:filesld6:lengthi4e4:pathl4:testeee4:name4:root12:piece lengthi4e6:pieces20:"
                .to_vec();
        raw_info.extend_from_slice(&Sha1::digest(payload));
        raw_info.push(b'e');
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind paused content peer");
        let address = listener.local_addr().expect("content peer address");
        let peer_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept content peer");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read content handshake");
            decode_handshake(&handshake, info_hash).expect("content handshake identity");
            stream
                .write_all(&encode_handshake(info_hash, *b"-RS-APP-PAUSE-000000"))
                .await
                .expect("send content handshake");
            stream
                .write_all(&encode_message(&PeerMessage::Bitfield(vec![0x80])).expect("bitfield"))
                .await
                .expect("send content bitfield");
            let mut buffer = [0; 128];
            loop {
                if stream.read(&mut buffer).await.expect("wait for pause") == 0 {
                    break;
                }
            }
        });

        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-paused-content".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe={address}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add content torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record content metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open content service");
        let owner = ViewSetOwner::trusted("pause-peer-owner");
        let opened = service
            .open_view_set(
                owner.clone(),
                OpenViewSetRequest {
                    views: vec![
                        ViewSpec::TorrentPeers {
                            view_id: "peers".to_owned(),
                            torrent_id: torrent_id.clone(),
                            delivery: ViewDeliveryPolicy::default(),
                        },
                        ViewSpec::TorrentSummary {
                            view_id: "summary".to_owned(),
                            torrent_id: torrent_id.clone(),
                            delivery: ViewDeliveryPolicy::default(),
                        },
                    ],
                    options: OpenViewSetOptions::default(),
                },
            )
            .expect("open peer view set");
        let view_set = service
            .view_set(&owner, &opened.view_set_id)
            .expect("peer view set handle");
        let mut cursor = opened.initial.cursor.clone();
        let mut active_peer = opened
            .initial
            .updates
            .iter()
            .find_map(|update| match update {
                ViewSetUpdate::Snapshot {
                    snapshot: ViewSnapshot::Peers { peers, .. },
                    ..
                } => peers
                    .iter()
                    .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                    .map(|peer| peer.connection_id.clone()),
                _ => None,
            });
        if active_peer.is_none() {
            active_peer = tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    let batch = view_set
                        .next_updates(&cursor, 1_000)
                        .await
                        .expect("connected peer update");
                    cursor = batch.cursor.clone();
                    if let Some(connection_id) =
                        batch.updates.iter().find_map(|update| match update {
                            ViewSetUpdate::Snapshot {
                                snapshot: ViewSnapshot::Peers { peers, .. },
                                ..
                            } => peers
                                .iter()
                                .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                                .map(|peer| peer.connection_id.clone()),
                            ViewSetUpdate::Patch {
                                patch: ViewPatch::Peers { upsert, .. },
                                ..
                            } => upsert
                                .iter()
                                .find(|peer| peer.lifecycle == crate::PeerLifecycle::Connected)
                                .map(|peer| peer.connection_id.clone()),
                            _ => None,
                        })
                    {
                        break Some(connection_id);
                    }
                }
            })
            .await
            .expect("content peer never became visible");
        }
        let active_peer = active_peer.expect("connected content peer row");

        let paused = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-content".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("pause content torrent");
        let ResponseOutcome::Success { snapshot } = paused.outcome else {
            panic!("pause should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::Paused);
        tokio::time::timeout(std::time::Duration::from_secs(1), peer_task)
            .await
            .expect("content peer did not close before pause receipt")
            .expect("content peer task");

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            let mut removed = false;
            let mut summary_is_terminal = false;
            while !removed || !summary_is_terminal {
                let batch = view_set
                    .next_updates(&cursor, 1_000)
                    .await
                    .expect("terminal peer updates");
                cursor = batch.cursor.clone();
                for update in batch.updates {
                    match update {
                        ViewSetUpdate::Patch {
                            patch: ViewPatch::Peers { removed: ids, .. },
                            ..
                        } => removed |= ids.contains(&active_peer),
                        ViewSetUpdate::Patch {
                            patch:
                                ViewPatch::Torrent {
                                    torrent: Some(torrent),
                                },
                            ..
                        }
                        | ViewSetUpdate::Snapshot {
                            snapshot:
                                ViewSnapshot::Torrent {
                                    torrent: Some(torrent),
                                },
                            ..
                        } => {
                            summary_is_terminal |= torrent.active_peer_connections == 0
                                && torrent.payload_download_rate_bytes == "0";
                        }
                        _ => {}
                    }
                }
            }
        })
        .await
        .expect("pause did not publish terminal peer views");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn file_priority_joins_generation_idles_all_skip_and_restarts_normal() {
        let root = test_root("file-priority-generation");
        let configuration = config(&root);
        let raw_info = multi_file_info();
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind content peer");
        let address = listener.local_addr().expect("content peer address");
        let (accepted_sender, mut accepted_receiver) = tokio::sync::mpsc::unbounded_channel();
        let peer_task = tokio::spawn(async move {
            for generation in 0..2 {
                let (mut stream, _) = listener.accept().await.expect("accept content peer");
                let mut handshake = [0; HANDSHAKE_LENGTH];
                stream
                    .read_exact(&mut handshake)
                    .await
                    .expect("read content handshake");
                decode_handshake(&handshake, info_hash).expect("content handshake identity");
                stream
                    .write_all(&encode_handshake(info_hash, *b"-RS-FILE-PRIO-000000"))
                    .await
                    .expect("send content handshake");
                stream
                    .write_all(
                        &encode_message(&PeerMessage::Bitfield(vec![0xc0])).expect("bitfield"),
                    )
                    .await
                    .expect("send content bitfield");
                accepted_sender
                    .send(generation)
                    .expect("report accepted generation");
                let mut buffer = [0; 128];
                loop {
                    match stream.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error)
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::ConnectionReset
                                    | std::io::ErrorKind::BrokenPipe
                            ) =>
                        {
                            break;
                        }
                        Err(error) => panic!("wait for joined generation: {error}"),
                    }
                }
            }
        });

        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "add-file-priority".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!("magnet:?xt=urn:btih:{torrent_id}&x.pe={address}"),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .expect("add content torrent");
        store
            .record_metadata(&torrent_id, &raw_info)
            .expect("record content metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open content service");
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), accepted_receiver.recv())
                .await
                .expect("first generation did not connect"),
            Some(0)
        );
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "skip-every-file".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriority {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![0, 1],
                    priority: FilePriority::Skip,
                },
            })
            .await
            .expect("skip all files");
        assert!(service.active_download().is_none());
        let idle = service
            .load_resume_conservative(&torrent_id)
            .expect("load all-skipped state");
        assert!(idle.desired_running);
        assert_eq!(idle.state, TorrentState::Paused);
        assert_eq!(idle.skip_files, vec![0, 1]);

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "restore-normal-file".to_owned(),
                expected_revision: None,
                command: Command::SetFilePriority {
                    torrent_id: torrent_id.clone(),
                    file_indices: vec![1],
                    priority: FilePriority::Normal,
                },
            })
            .await
            .expect("restore normal file");
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), accepted_receiver.recv())
                .await
                .expect("replacement generation did not connect"),
            Some(1)
        );
        let paths = torrent_storage_paths(&root.join("payload"), "multi", info_hash)
            .expect("plan storage paths");
        assert!(!paths.part.exists());
        service.shutdown().await.expect("shutdown");
        tokio::time::timeout(std::time::Duration::from_secs(1), peer_task)
            .await
            .expect("peer generations did not join")
            .expect("peer task");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn owner_cleanup_failure_is_not_accepted_as_joined_pause() {
        let root = test_root("pause-cleanup-failure");
        let configuration = config(&root);
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &configuration.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-cleanup-failure", torrent_id))
            .expect("add cleanup-failure torrent");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-cleanup-failure".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.to_owned(),
                },
            })
            .expect("persist paused intent");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open paused service");
        let result = handle_task_outcome(
            &service.store,
            &service.storage_roots,
            &service.views,
            torrent_id,
            Err(DownloadError::PeerCleanup {
                failure: "download cancelled".to_owned(),
                cleanup: "one peer connection remains".to_owned(),
            }),
        );
        assert!(
            result
                .expect_err("cleanup failure must propagate")
                .contains("one peer connection remains")
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn delete_managed_removal_joins_and_deletes_only_owned_path_artifacts() {
        let root = test_root("remove-path");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            config.durable_profile_root().expect("durable profile root"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-path", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record published storage ownership");
        drop(store);

        let payload = root.join("payload");
        let output = payload.join("test");
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        let sibling = payload.join("keep-me");
        fs::create_dir_all(&payload).expect("create payload root");
        fs::write(&output, b"payload").expect("write output");
        fs::write(&part, b"parts").expect("write part file");
        fs::write(&sibling, b"sibling").expect("write sibling");

        let mut service = ApplicationService::open(config)
            .await
            .expect("open service");
        let request = RequestEnvelope {
            version: CONTROL_VERSION,
            request_id: "remove-with-data".to_owned(),
            expected_revision: None,
            command: Command::RemoveTorrent {
                torrent_id: torrent_id.clone(),
                data: RemovalDataPolicy::DeleteManaged,
            },
        };
        service.dispatch(request.clone()).await.expect("remove");
        assert!(!output.exists());
        assert!(!staging.exists());
        assert!(!part.exists());
        assert_eq!(fs::read(&sibling).expect("preserved sibling"), b"sibling");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );
        service.dispatch(request).await.expect("idempotent replay");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn staging_cleanup_preserves_an_unowned_final_destination() {
        let root = test_root("staging-cleanup-conflict");
        let payload = root.join("payload");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let output = payload.join("named");
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&output).expect("create conflicting output");
        fs::write(output.join("preserve"), b"unowned").expect("write conflicting output");
        fs::create_dir_all(&staging).expect("create owned staging");
        fs::write(staging.join("partial"), b"owned").expect("write staging");
        fs::write(&part, b"owned").expect("write part");

        delete_path_artifacts(
            &payload,
            torrent_id,
            Some("named"),
            PublicationShape::Tree,
            StorageState::Staging,
            ManagedArtifactState::Staging,
        )
        .expect("clean staging-owned artifacts");

        assert_eq!(
            fs::read(output.join("preserve")).expect("preserve conflicting output"),
            b"unowned"
        );
        assert!(!staging.exists());
        assert!(!part.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn publishing_cleanup_deletes_whichever_owned_side_survived() {
        for final_visible in [false, true] {
            let root = test_root(if final_visible {
                "publishing-cleanup-final"
            } else {
                "publishing-cleanup-staging"
            });
            let payload = root.join("payload");
            let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
            let owned = if final_visible {
                payload.join("named")
            } else {
                payload.join(format!(".{torrent_id}.rstorrent-staging"))
            };
            let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
            fs::create_dir_all(&owned).expect("create owned publication side");
            fs::write(owned.join("payload"), b"owned").expect("write owned payload");
            fs::write(&part, b"owned").expect("write owned part");

            delete_path_artifacts(
                &payload,
                torrent_id,
                Some("named"),
                PublicationShape::Tree,
                StorageState::Prepared,
                ManagedArtifactState::Staging,
            )
            .expect("clean publishing-owned artifacts");

            assert!(!owned.exists());
            assert!(!part.exists());
            fs::remove_dir_all(root).expect("remove test root");
        }
    }

    #[test]
    fn publishing_cleanup_fails_closed_when_both_sides_exist() {
        let root = test_root("publishing-cleanup-ambiguous");
        let payload = root.join("payload");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let output = payload.join("named");
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        fs::create_dir_all(&output).expect("create final");
        fs::create_dir_all(&staging).expect("create staging");

        let error = delete_path_artifacts(
            &payload,
            torrent_id,
            Some("named"),
            PublicationShape::Tree,
            StorageState::Prepared,
            ManagedArtifactState::Staging,
        )
        .expect_err("ambiguous artifacts must be preserved");

        assert!(error.to_string().contains("both staging and final"));
        assert!(output.exists());
        assert!(staging.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn published_single_file_cleanup_uses_the_recorded_file_shape() {
        let root = test_root("published-single-cleanup");
        let payload = root.join("payload");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let output = payload.join("named.bin");
        fs::create_dir_all(&payload).expect("create payload root");
        fs::write(&output, b"owned").expect("write owned final file");

        delete_path_artifacts(
            &payload,
            torrent_id,
            Some("named.bin"),
            PublicationShape::File,
            StorageState::Published,
            ManagedArtifactState::Published,
        )
        .expect("clean published single file");

        assert!(!output.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[cfg(unix)]
    #[test]
    fn managed_cleanup_preserves_a_replacement_symlink() {
        use std::os::unix::fs::symlink;

        let root = test_root("cleanup-symlink");
        let payload = root.join("payload");
        let outside = root.join("outside");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        fs::create_dir_all(&payload).expect("create payload root");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(outside.join("preserve"), b"foreign").expect("write outside file");
        symlink(&outside, &staging).expect("replace staging with symlink");

        let error = delete_path_artifacts(
            &payload,
            torrent_id,
            Some("named"),
            PublicationShape::Tree,
            StorageState::Staging,
            ManagedArtifactState::Staging,
        )
        .expect_err("replacement symlink must fail closed");

        assert!(error.to_string().contains("unexpected type"));
        assert!(
            fs::symlink_metadata(&staging)
                .expect("preserved link")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read(outside.join("preserve")).expect("outside file"),
            b"foreign"
        );
        fs::remove_file(staging).expect("remove fixture symlink");
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn delete_managed_removal_cleans_legacy_hash_named_artifacts() {
        let root = test_root("remove-legacy-path");
        let config = config(&root);
        let raw_info = b"d5:filesld6:lengthi4e4:pathl8:file.bineee4:name5:named12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            config.durable_profile_root().expect("durable profile root"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-legacy-path", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);
        let connection = Connection::open(database).expect("open raw database");
        connection
            .execute(
                "UPDATE torrents
                 SET publication_name = NULL, managed_artifacts = 'legacy'
                 WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("clear publication name");
        drop(connection);

        let payload = root.join("payload");
        let output = payload.join(&torrent_id);
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&output).expect("create legacy output");
        fs::write(output.join("payload.bin"), b"payload").expect("write output");
        fs::create_dir_all(&staging).expect("create legacy staging");
        fs::write(staging.join("partial.bin"), b"partial").expect("write staging");
        fs::write(&part, b"parts").expect("write part file");

        let mut service = ApplicationService::open(config)
            .await
            .expect("open legacy service");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-legacy-with-data".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id,
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("remove legacy torrent");
        assert!(!output.exists());
        assert!(!staging.exists());
        assert!(!part.exists());
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn keep_data_removal_preserves_managed_path_artifacts() {
        let root = test_root("remove-keep");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let payload = root.join("payload");
        let output = payload.join(torrent_id);
        let staging = payload.join(format!(".{torrent_id}.rstorrent-staging"));
        let part = payload.join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&output).expect("create output");
        fs::create_dir_all(&staging).expect("create staging");
        fs::write(output.join("payload.bin"), b"payload").expect("write output");
        fs::write(staging.join("partial.bin"), b"partial").expect("write staging");
        fs::write(&part, b"parts").expect("write part file");

        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        service
            .dispatch(add_request("add-remove-keep", torrent_id))
            .await
            .expect("add");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-keep".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.to_owned(),
                    data: RemovalDataPolicy::Keep,
                },
            })
            .await
            .expect("remove while keeping data");
        assert_eq!(
            fs::read(output.join("payload.bin")).expect("output"),
            b"payload"
        );
        assert_eq!(
            fs::read(staging.join("partial.bin")).expect("staging"),
            b"partial"
        );
        assert_eq!(fs::read(&part).expect("part"), b"parts");

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn failed_path_cleanup_stays_visible_and_retries_with_a_new_generation() {
        let root = test_root("remove-failure");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            config.durable_profile_root().expect("durable profile root"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-failure", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Staging)
            .expect("record staging ownership");
        drop(store);
        let part = root
            .join("payload")
            .join(format!(".{torrent_id}.rstorrent-parts"));
        fs::create_dir_all(&part).expect("create invalid part directory");

        let mut service = ApplicationService::open(config)
            .await
            .expect("open service");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-fails".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("accept failed cleanup intent");
        let failed = service
            .store_mut()
            .expect("store")
            .snapshot()
            .expect("failed snapshot")
            .torrents
            .remove(0);
        assert_eq!(failed.removal_state, Some(RemovalState::Failed));
        assert!(failed.error.is_some());

        fs::remove_dir(&part).expect("repair invalid part path");
        fs::write(&part, b"part").expect("create valid part file");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-retry".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("retry cleanup");
        assert!(!part.exists());
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn startup_resumes_a_durable_pending_path_removal() {
        let root = test_root("remove-restart");
        let config = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            config.durable_profile_root().expect("durable profile root"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-restart", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        store
            .mark_storage_prepared(&torrent_id, StorageState::Published)
            .expect("record published storage ownership");
        store
            .handle_durable(&RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-before-restart".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .expect("persist pending removal");
        drop(store);
        let output = root.join("payload").join("test");
        fs::create_dir_all(output.parent().expect("output parent")).expect("create payload root");
        fs::write(&output, b"payload").expect("write interrupted output");

        let mut service = ApplicationService::open(config)
            .await
            .expect("resume application");
        assert!(!output.exists());
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn platform_removal_requires_matching_generation_confirmation() {
        let root = test_root("remove-platform");
        let mut config = config(&root);
        config.storage_roots = vec![ConfiguredStorageRoot::platform("downloads")];
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let mut store = SessionStore::open(
            config.durable_profile_root().expect("durable profile root"),
            &config.profile_id,
            &config.storage_roots,
        )
        .expect("open setup store");
        store
            .handle_durable(&add_request("add-remove-platform", &torrent_id))
            .expect("add torrent");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let mut service = ApplicationService::open(config.clone())
            .await
            .expect("open service");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-platform".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("request platform removal");
        let initial_plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("platform plan");
        assert_eq!(initial_plan.name, "test");
        assert_eq!(initial_plan.storage_root, "downloads");
        assert!(
            service
                .confirm_platform_removal(&torrent_id, "stale-operation")
                .await
                .is_err()
        );
        assert_eq!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents[0]
                .removal_state,
            Some(RemovalState::AwaitingPlatform)
        );
        service
            .fail_platform_removal(
                &torrent_id,
                &initial_plan.operation_id,
                "provider permission was revoked",
            )
            .await
            .expect("record platform failure");
        let failed = service
            .store_mut()
            .expect("store")
            .snapshot()
            .expect("failed snapshot")
            .torrents
            .remove(0);
        assert_eq!(failed.removal_state, Some(RemovalState::Failed));
        assert_eq!(
            failed.error.as_deref(),
            Some("provider permission was revoked")
        );
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "retry-platform-removal".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("retry platform removal");
        let plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("retry platform plan");
        assert_ne!(plan.operation_id, initial_plan.operation_id);
        service
            .shutdown()
            .await
            .expect("shutdown before confirmation");
        drop(service);

        let mut service = ApplicationService::open(config)
            .await
            .expect("reopen awaiting platform removal");
        let resumed_plan = service
            .platform_removal_plan(&torrent_id)
            .await
            .expect("resume platform plan");
        assert_eq!(resumed_plan, plan);
        service
            .confirm_platform_removal(&torrent_id, &plan.operation_id)
            .await
            .expect("confirm current operation");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        let metadata_pending = "000102030405060708090a0b0c0d0e0f10111213";
        service
            .dispatch(add_request(
                "add-platform-without-metadata",
                metadata_pending,
            ))
            .await
            .expect("add metadata-pending platform torrent");
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-platform-without-metadata".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: metadata_pending.to_owned(),
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("remove metadata-pending torrent");
        assert!(
            service
                .store_mut()
                .expect("store")
                .snapshot()
                .expect("snapshot")
                .torrents
                .is_empty()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn tracker_retry_waiting_is_observed_without_another_command() {
        let root = test_root("tracker-retry-waiting");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut service = ApplicationService::open(config(&root))
            .await
            .expect("open service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("summary");
        summary.next_update().await.expect("summary snapshot");
        let diagnostics = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: Some(DiagnosticFilter {
                    profile: DiagnosticProfile::Normal,
                    minimum_severity: DiagnosticSeverity::Info,
                    categories: Vec::new(),
                }),
                catalog_page: None,
            })
            .expect("diagnostics");
        diagnostics
            .next_update()
            .await
            .expect("diagnostic snapshot");

        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "blocked-add".to_owned(),
                expected_revision: None,
                command: Command::AddMagnet {
                    magnet: format!(
                        "magnet:?xt=urn:btih:{torrent_id}&tr=udp%3A%2F%2F192.0.2.1%3A6969%2Fannounce"
                    ),
                    storage_root: "downloads".to_owned(),
                    start_content: true,
                    skip_files: Vec::new(),
                },
            })
            .await
            .expect("add");

        let waiting = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let update = summary.next_update().await.expect("summary update");
                let is_waiting = match update.payload {
                    ViewUpdatePayload::Snapshot {
                        snapshot: ViewSnapshot::TorrentList { torrents, .. },
                    } => torrents.first().is_some_and(|torrent| {
                        torrent.progress.disposition == ProgressDisposition::Waiting
                            && torrent.progress.reason == ProgressReason::WaitingForDiscovery
                    }),
                    ViewUpdatePayload::Patch {
                        patch: ViewPatch::TorrentList { upsert, .. },
                    } => upsert.first().is_some_and(|torrent| {
                        torrent.progress.disposition == ProgressDisposition::Waiting
                            && torrent.progress.reason == ProgressReason::WaitingForDiscovery
                    }),
                    _ => false,
                };
                if is_waiting {
                    break;
                }
            }
        })
        .await;
        assert!(
            waiting.is_ok(),
            "tracker owner did not publish scheduled discovery waiting"
        );

        let diagnostic = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let update = diagnostics.next_update().await.expect("diagnostic update");
                if serde_json::to_string(&update)
                    .expect("serialize diagnostic")
                    .contains("tracker_retry_scheduled")
                {
                    break;
                }
            }
        })
        .await;
        assert!(
            diagnostic.is_ok(),
            "missing scheduled tracker retry diagnostic"
        );
        let resume = service
            .load_resume_conservative(torrent_id)
            .expect("resume state");
        assert!(resume.desired_running);
        assert_eq!(resume.state, TorrentState::AwaitingMetadata);

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn offline_policy_reports_network_blockage_without_torrent_error() {
        let root = test_root("network-offline");
        let torrent_id = "000102030405060708090a0b0c0d0e0f10111213";
        let mut application_config = config(&root);
        application_config.network.policy = NetworkPolicy::Offline;
        let mut service = ApplicationService::open(application_config)
            .await
            .expect("open offline service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("summary");
        summary.next_update().await.expect("initial summary");

        service
            .dispatch(add_request("offline-add", torrent_id))
            .await
            .expect("add while offline");

        let torrent = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let update = summary.next_update().await.expect("summary update");
                let candidate = match update.payload {
                    ViewUpdatePayload::Snapshot {
                        snapshot: ViewSnapshot::TorrentList { mut torrents, .. },
                    } => torrents.pop(),
                    ViewUpdatePayload::Patch {
                        patch: ViewPatch::TorrentList { mut upsert, .. },
                    } => upsert.pop(),
                    _ => None,
                };
                if let Some(torrent) = candidate
                    && torrent.progress.reason == ProgressReason::NetworkDisabled
                {
                    break torrent;
                }
            }
        })
        .await
        .expect("network-disabled progress");

        assert_eq!(torrent.state, TorrentState::AwaitingMetadata);
        assert!(torrent.error.is_none());
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove offline test root");
    }

    #[tokio::test]
    async fn startup_projects_verified_metadata_name() {
        let root = test_root("metadata-name");
        let configuration = config(&root);
        let raw_info = b"d5:filesld6:lengthi4e4:pathl8:file.bineee4:name5:named12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service");
        let summary = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Summary,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
                catalog_page: None,
            })
            .expect("summary");
        let update = summary.next_update().await.expect("summary snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot: ViewSnapshot::TorrentList { torrents, .. },
        } = update.payload
        else {
            panic!("expected torrent-list snapshot");
        };
        assert_eq!(torrents[0].display_name.as_deref(), Some("named"));

        let files = service
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::Torrent {
                    torrent_id: torrent_id.clone(),
                },
                projection: ViewProjection::Files,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: None,
                catalog_page: Some(crate::CatalogPageRequest::default()),
            })
            .expect("files");
        let update = files.next_update().await.expect("files snapshot");
        let ViewUpdatePayload::Snapshot {
            snapshot:
                ViewSnapshot::Files {
                    filesystem_content_base,
                    ..
                },
        } = update.payload
        else {
            panic!("expected files snapshot");
        };
        assert_eq!(
            filesystem_content_base.as_deref(),
            root.join("payload").join("named").to_str()
        );

        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove metadata-name test root");
    }

    #[tokio::test]
    async fn corrupt_metadata_enters_repair_before_storage() {
        let root = test_root("metadata-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        connection
            .execute(
                "UPDATE torrents SET raw_info = x'00' WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("corrupt raw metadata");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with corrupt metadata");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        assert_eq!(
            fs::read_dir(root.join("payload"))
                .expect("read empty payload root")
                .count(),
            0
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn legacy_metadata_without_publication_name_enters_repair() {
        let root = test_root("legacy-publication-name");
        let configuration = config(&root);
        let raw_info = b"d5:filesld6:lengthi4e4:pathl8:file.bineee4:name5:named12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        connection
            .execute(
                "UPDATE torrents SET publication_name = NULL WHERE info_hash = ?1",
                [info_hash.as_slice()],
            )
            .expect("clear publication name");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with legacy metadata");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        assert!(
            snapshot.torrents[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("publication name"))
        );
        assert_eq!(
            fs::read_dir(root.join("payload"))
                .expect("read empty payload root")
                .count(),
            0
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn malformed_have_state_is_cleared_conservatively() {
        let root = test_root("have-corruption");
        let configuration = config(&root);
        let raw_info =
            b"d6:lengthi4e4:name4:test12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            &[configured_root],
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        let database = store
            .database_path()
            .expect("durable database path")
            .to_owned();
        drop(store);

        let connection = Connection::open(database).expect("open raw database");
        let mut have: Vec<u8> = connection
            .query_row(
                "SELECT have_state FROM torrents WHERE info_hash = ?1",
                [info_hash.as_slice()],
                |row| row.get(0),
            )
            .expect("read have state");
        *have.last_mut().expect("bitfield byte") = 1;
        connection
            .execute(
                "UPDATE torrents SET have_state = ?2 WHERE info_hash = ?1",
                rusqlite::params![info_hash.as_slice(), have],
            )
            .expect("corrupt have padding");
        drop(connection);

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with malformed have state");
        let response = service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "snapshot".to_owned(),
                expected_revision: None,
                command: Command::Snapshot,
            })
            .await
            .expect("snapshot");
        let ResponseOutcome::Success { snapshot } = response.outcome else {
            panic!("snapshot should succeed");
        };
        assert_eq!(snapshot.torrents[0].verified_piece_count, 0);
        assert_ne!(snapshot.torrents[0].state, TorrentState::NeedsRepair);
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn incomplete_storage_artifacts_enter_repair_without_overwrite() {
        let root = test_root("storage-repair");
        let configuration = config(&root);
        let raw_info = b"d5:filesld6:lengthi4e4:pathl4:testeee4:name4:root12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let info_hash: [u8; 20] = Sha1::digest(raw_info).into();
        let torrent_id = super::encode_info_hash(info_hash);
        let configured_root = configuration.storage_roots[0].clone();
        let mut store = SessionStore::open(
            configuration
                .durable_profile_root()
                .expect("durable profile root"),
            &configuration.profile_id,
            std::slice::from_ref(&configured_root),
        )
        .expect("open store");
        store
            .handle_durable(&add_request("add", &torrent_id))
            .expect("add");
        store
            .record_metadata(&torrent_id, raw_info)
            .expect("record metadata");
        drop(store);

        let crate::StorageRootLocation::Path(payload_root) = &configured_root.location else {
            unreachable!("test root is path-backed")
        };
        let incomplete_output = payload_root.join("root");
        fs::create_dir_all(&incomplete_output).expect("create incomplete output");
        fs::write(incomplete_output.join("preserve"), b"user artifact")
            .expect("write preserved artifact");

        let mut service = ApplicationService::open(configuration)
            .await
            .expect("open service with incomplete storage");
        let mut state = None;
        let mut error = None;
        for sequence in 0..100 {
            tokio::task::yield_now().await;
            let response = service
                .dispatch(RequestEnvelope {
                    version: CONTROL_VERSION,
                    request_id: format!("snapshot-{sequence}"),
                    expected_revision: None,
                    command: Command::Snapshot,
                })
                .await
                .expect("snapshot");
            let ResponseOutcome::Success { snapshot } = response.outcome else {
                panic!("snapshot should succeed");
            };
            state = Some(snapshot.torrents[0].state);
            error = snapshot.torrents[0].error.clone();
            if state == Some(TorrentState::NeedsRepair) {
                break;
            }
        }
        assert_eq!(state, Some(TorrentState::NeedsRepair));
        assert!(
            error
                .as_deref()
                .is_some_and(|error| error.contains("output already exists"))
        );
        assert_eq!(
            fs::read(incomplete_output.join("preserve"))
                .expect("read preserved incomplete artifact"),
            b"user artifact"
        );
        service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-conflicted-torrent".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id,
                    data: RemovalDataPolicy::DeleteManaged,
                },
            })
            .await
            .expect("remove conflicted torrent");
        assert_eq!(
            fs::read(incomplete_output.join("preserve"))
                .expect("preserve unowned conflicting destination"),
            b"user artifact"
        );
        service.shutdown().await.expect("shutdown");
        drop(service);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn durable_complete_torrent_seeds_across_restart_and_fences_lifecycle() {
        let root = test_root("incoming-seed-lifecycle");
        let payload = b"abcdefg";
        let raw_info = single_file_info("seed.bin", payload, 4);
        let info_hash: [u8; 20] = Sha1::digest(&raw_info).into();
        let torrent_id = crate::control::encode_info_hash(info_hash);
        let configuration = config(&root);
        persist_client_settings(
            &configuration,
            ClientSettings {
                listener: ListenerPolicy::AutomaticLoopback,
                preferred_listen_port: 6_881,
                port_mapping: crate::PortMappingPolicy::Disabled,
                peer_connection_limit: 1,
                upload_slots: 1,
            },
        );
        fs::create_dir_all(root.join("payload")).expect("create payload root");
        fs::write(root.join("payload/seed.bin"), payload).expect("write published payload");
        {
            let mut store = SessionStore::open(
                configuration
                    .durable_profile_root()
                    .expect("durable profile"),
                &configuration.profile_id,
                &configuration.storage_roots,
            )
            .expect("open fixture store");
            store
                .handle_durable(&add_request("add-seed", &torrent_id))
                .expect("add seed catalog row");
            store
                .record_metadata(&torrent_id, &raw_info)
                .expect("record seed metadata");
            store
                .record_pieces(&torrent_id, &[0, 1])
                .expect("record verified pieces");
            store
                .mark_storage_prepared(&torrent_id, StorageState::Published)
                .expect("record published storage");
            store.mark_complete(&torrent_id).expect("complete seed");
        }

        let mut first = ApplicationService::open(configuration.clone())
            .await
            .expect("open first application lifetime");
        let first_runtime = client_settings_runtime(&first).await;
        assert_eq!(first_runtime.active.peer_connection_limit, 1);
        assert_eq!(first_runtime.active.upload_slots, 1);
        assert_eq!(first_runtime.effective_peer_connection_limit, 1);
        let first_incoming = first
            .incoming_peer_snapshot()
            .expect("configured listener is active");
        assert_eq!(first_incoming.peer_budget.configured_limit, 1);
        assert_eq!(first_incoming.peer_budget.effective_limit, 1);
        wait_for_seed_registrations(&first, 1).await;
        let (mut peer, mut decoder, mut pending) =
            connect_application_seed_with_extensions(&first, info_hash, DEFAULT_PEER_ID, true)
                .await;
        let peer_endpoint = peer.local_addr().expect("incoming peer endpoint");
        let listener_endpoint = peer.peer_addr().expect("listener endpoint");
        peer.write_all(
            &encode_message(&PeerMessage::Extended {
                id: 0,
                payload: encode_extension_handshake(None),
            })
            .expect("encode remote extension handshake"),
        )
        .await
        .expect("send remote extension handshake");
        peer.write_all(&encode_message(&PeerMessage::Interested).expect("encode interested"))
            .await
            .expect("send interest");
        assert_eq!(
            read_peer_message(&mut peer, &mut decoder, &mut pending).await,
            PeerMessage::Unchoke
        );
        peer.write_all(
            &encode_message(&PeerMessage::Request(
                rstorrent_protocol::peer_wire::BlockRequest {
                    index: 1,
                    begin: 0,
                    length: 3,
                },
            ))
            .expect("encode payload request"),
        )
        .await
        .expect("send payload request");
        assert_eq!(
            read_peer_message(&mut peer, &mut decoder, &mut pending).await,
            PeerMessage::Piece {
                index: 1,
                begin: 0,
                block: b"efg".to_vec(),
            }
        );

        let incoming_peer = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(peer) = torrent_peer_views(&first, &torrent_id)
                    .await
                    .into_iter()
                    .find(|peer| {
                        peer.remote_interested == Some(true)
                            && peer.supports_ut_metadata == Some(true)
                            && peer.payload_uploaded_bytes.as_deref() == Some("3")
                    })
                {
                    break peer;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming upload reaches the Peers projection");
        assert_eq!(incoming_peer.direction, PeerDirection::Incoming);
        assert_eq!(incoming_peer.transport, PeerTransportKind::Tcp);
        assert_eq!(incoming_peer.lifecycle, PeerLifecycle::Connected);
        assert_eq!(incoming_peer.role, PeerRole::Content);
        assert_eq!(incoming_peer.remote_endpoint, peer_endpoint.to_string());
        assert_eq!(
            incoming_peer.local_endpoint.as_deref(),
            Some(listener_endpoint.to_string().as_str())
        );
        assert_eq!(incoming_peer.sources, [PeerSourceView::Incoming]);
        assert_eq!(incoming_peer.supports_extensions, Some(true));
        assert_eq!(incoming_peer.remote_interested, Some(true));
        assert_eq!(incoming_peer.local_choking, Some(false));
        assert!(incoming_peer.peer_record_id.is_some());
        assert!(incoming_peer.peer_id.is_some());
        assert!(incoming_peer.client_name.is_some());
        assert!(incoming_peer.payload_upload_rate_bytes.is_some());
        assert_eq!(
            incoming_peer.peer_flags,
            [
                PeerFlagView::Incoming,
                PeerFlagView::UploadAllowed,
                PeerFlagView::ExtensionProtocol,
                PeerFlagView::MetadataExtension,
                PeerFlagView::OptimisticUnchoke,
            ]
        );
        assert_eq!(
            incoming_peer.capabilities.local_endpoint,
            crate::CapabilityStatus::Available
        );
        assert_eq!(
            incoming_peer.capabilities.ut_metadata,
            crate::CapabilityStatus::Available
        );
        assert_eq!(
            incoming_peer.capabilities.interest_directions,
            crate::CapabilityStatus::Available
        );
        assert_eq!(
            incoming_peer.capabilities.local_choke,
            crate::CapabilityStatus::Available
        );
        assert_eq!(
            incoming_peer.capabilities.upload,
            crate::CapabilityStatus::Available
        );

        let connected_swarm_peer = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (state, peers) = torrent_swarm_view(&first, &torrent_id).await;
                assert_eq!(state, SwarmCatalogState::Active);
                if let Some(peer) = peers
                    .into_iter()
                    .find(|peer| peer.endpoint == peer_endpoint.to_string())
                {
                    break peer;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("incoming peer reaches the Swarm projection");
        assert_eq!(connected_swarm_peer.sources, [PeerSourceView::Incoming]);
        assert_eq!(connected_swarm_peer.state, SwarmPeerState::Connected);
        assert!(!connected_swarm_peer.connectable);

        drop(peer);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if torrent_peer_views(&first, &torrent_id).await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("closed incoming peer leaves the Peers projection");
        let closed_swarm_peer = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (_, peers) = torrent_swarm_view(&first, &torrent_id).await;
                if let Some(peer) = peers.into_iter().find(|peer| {
                    peer.endpoint == peer_endpoint.to_string()
                        && peer.state == SwarmPeerState::NotConnectable
                }) {
                    break peer;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("closed incoming peer remains as non-connectable swarm history");
        assert_eq!(closed_swarm_peer.sources, [PeerSourceView::Incoming]);
        assert!(!closed_swarm_peer.connectable);

        let zero_slots = ClientSettings {
            listener: ListenerPolicy::AutomaticLoopback,
            preferred_listen_port: 6_881,
            port_mapping: crate::PortMappingPolicy::Disabled,
            peer_connection_limit: 1,
            upload_slots: 0,
        };
        first
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "configure-zero-upload-slots".to_owned(),
                expected_revision: None,
                command: Command::SetClientSettings {
                    settings: zero_slots.clone(),
                },
            })
            .await
            .expect("persist zero upload slots");
        let configured = client_settings_runtime(&first).await;
        assert_eq!(configured.configured, zero_slots);
        assert_eq!(configured.active.upload_slots, 1);
        assert!(configured.restart_required);
        first.shutdown().await.expect("shutdown first lifetime");
        drop(first);

        let mut second = ApplicationService::open(configuration)
            .await
            .expect("open restarted application");
        let second_runtime = client_settings_runtime(&second).await;
        assert_eq!(second_runtime.active, zero_slots);
        assert!(!second_runtime.restart_required);
        wait_for_seed_registrations(&second, 1).await;
        let runtime_generation = second
            .torrent_runtimes
            .get(&torrent_id)
            .expect("complete torrent owns one runtime")
            .generation();
        let (mut choked_peer, mut choked_decoder, mut choked_pending) =
            connect_application_seed(&second, info_hash, *b"-RS-CHOKED--00000000").await;
        choked_peer
            .write_all(&encode_message(&PeerMessage::Interested).expect("encode interested"))
            .await
            .expect("send zero-slot interest");
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                read_peer_message(&mut choked_peer, &mut choked_decoder, &mut choked_pending,),
            )
            .await
            .is_err(),
            "zero upload slots must not unchoke an interested peer"
        );
        let choked_snapshot = second
            .incoming_peer_snapshot()
            .expect("incoming service remains active");
        assert_eq!(choked_snapshot.upload_scheduler.interested, 1);
        assert_eq!(choked_snapshot.upload_scheduler.regular, 0);
        assert_eq!(choked_snapshot.upload_scheduler.optimistic, 0);
        drop(choked_peer);
        let (mut archived_peer, _, _) =
            connect_application_seed(&second, info_hash, *b"-RS-ARCHIVE-00000000").await;
        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "archive-seed".to_owned(),
                expected_revision: None,
                command: Command::Archive {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("archive seed");
        assert_eq!(
            archived_peer
                .read(&mut [0; 1])
                .await
                .expect("observe archive close"),
            0
        );
        wait_for_seed_registrations(&second, 0).await;
        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "restore-seed".to_owned(),
                expected_revision: None,
                command: Command::RestoreArchive {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("restore seed");
        wait_for_seed_registrations(&second, 1).await;
        assert_eq!(
            second
                .torrent_runtimes
                .get(&torrent_id)
                .expect("restored seed retains its runtime")
                .generation(),
            runtime_generation
        );

        let (mut recheck_peer, _, _) =
            connect_application_seed(&second, info_hash, *b"-RS-RECHECK-00000000").await;
        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "recheck-seed".to_owned(),
                expected_revision: None,
                command: Command::ForceRecheck {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("force recheck seed");
        assert_eq!(
            recheck_peer
                .read(&mut [0; 1])
                .await
                .expect("observe recheck close"),
            0
        );
        wait_for_seed_registrations(&second, 1).await;
        assert_eq!(
            second
                .torrent_runtimes
                .get(&torrent_id)
                .expect("rechecked seed retains its runtime")
                .generation(),
            runtime_generation
        );

        let (mut paused_peer, _, _) =
            connect_application_seed(&second, info_hash, *b"-RS-PAUSED--00000000").await;
        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "pause-seed".to_owned(),
                expected_revision: None,
                command: Command::Pause {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("pause seed");
        assert_eq!(
            paused_peer
                .read(&mut [0; 1])
                .await
                .expect("observe pause close"),
            0
        );
        wait_for_seed_registrations(&second, 0).await;
        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "resume-seed".to_owned(),
                expected_revision: None,
                command: Command::Resume {
                    torrent_id: torrent_id.clone(),
                },
            })
            .await
            .expect("resume seed");
        wait_for_seed_registrations(&second, 1).await;
        assert_eq!(
            second
                .torrent_runtimes
                .get(&torrent_id)
                .expect("resumed seed retains its runtime")
                .generation(),
            runtime_generation
        );

        second
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "remove-seed".to_owned(),
                expected_revision: None,
                command: Command::RemoveTorrent {
                    torrent_id: torrent_id.clone(),
                    data: RemovalDataPolicy::Keep,
                },
            })
            .await
            .expect("remove seed");
        wait_for_seed_registrations(&second, 0).await;
        assert!(!second.torrent_runtimes.contains_key(&torrent_id));
        let terminal = second
            .incoming_peer_snapshot()
            .expect("incoming service remains enabled");
        assert_eq!(terminal.pending, 0);
        assert_eq!(terminal.established, 0);
        assert_eq!(terminal.reads, 0);
        second.shutdown().await.expect("shutdown second lifetime");
        drop(second);
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[tokio::test]
    async fn application_incoming_bootstrap_is_disabled_or_exactly_fixed() {
        let disabled_root = test_root("incoming-disabled");
        let mut disabled = ApplicationService::open(config(&disabled_root))
            .await
            .expect("open disabled incoming service");
        assert!(disabled.incoming_peer_snapshot().is_none());
        assert_eq!(
            client_settings_runtime(&disabled).await.listener_status,
            ListenerStatus::Disabled
        );
        disabled
            .shutdown()
            .await
            .expect("shutdown disabled service");
        drop(disabled);
        fs::remove_dir_all(disabled_root).expect("remove disabled root");

        let ineligible_root = test_root("incoming-loopback-upnp-ineligible");
        let ineligible_config = config(&ineligible_root);
        persist_client_settings(
            &ineligible_config,
            ClientSettings {
                listener: ListenerPolicy::AutomaticLoopback,
                port_mapping: crate::PortMappingPolicy::Upnp,
                ..ClientSettings::default()
            },
        );
        let mut ineligible = ApplicationService::open(ineligible_config)
            .await
            .expect("open ineligible loopback mapping policy");
        assert_eq!(
            client_settings_runtime(&ineligible)
                .await
                .port_mapping_status,
            crate::PortMappingStatus::Ineligible
        );
        assert_eq!(
            ineligible
                .reachability
                .as_ref()
                .expect("coordinator exists")
                .owner_counts(),
            crate::reachability::ReachabilityOwnerCounts::default()
        );
        ineligible
            .shutdown()
            .await
            .expect("shutdown ineligible app");
        drop(ineligible);
        fs::remove_dir_all(ineligible_root).expect("remove ineligible root");

        let fixed_root = test_root("incoming-fixed-conflict");
        let blocker = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind incoming port blocker");
        let port = blocker.local_addr().expect("blocker address").port();
        let mut configuration = config(&fixed_root);
        persist_client_settings(
            &configuration,
            ClientSettings {
                listener: ListenerPolicy::FixedLoopback { port },
                ..ClientSettings::default()
            },
        );
        let mut conflicted = ApplicationService::open(configuration.clone())
            .await
            .expect("fixed bind conflict keeps application available");
        assert!(conflicted.incoming_peer_snapshot().is_none());
        let runtime = client_settings_runtime(&conflicted).await;
        assert_eq!(
            runtime.active.listener,
            ListenerPolicy::FixedLoopback { port }
        );
        assert!(!runtime.restart_required);
        assert!(matches!(
            runtime.listener_status,
            ListenerStatus::BindFailed {
                reason: ListenerBindFailureReason::AddressInUse,
                ref detail,
            } if !detail.is_empty() && detail.len() <= 512
        ));
        assert!(matches!(
            runtime.session_udp_status,
            crate::SessionUdpStatus::Bound {
                port: active_udp_port,
                coordinated_with_tcp: false,
                ..
            } if active_udp_port != 0
        ));
        let diagnostics = conflicted
            .subscribe(SubscriptionSpec {
                selector: ViewSelector::TorrentList,
                projection: ViewProjection::Diagnostics,
                delivery: DeliveryPolicy {
                    min_interval_millis: 0,
                    max_queue_bytes: 256 * 1024,
                },
                diagnostics: Some(DiagnosticFilter::default()),
                catalog_page: None,
            })
            .expect("subscribe to bind diagnostics")
            .next_update()
            .await
            .expect("bind diagnostic snapshot");
        let diagnostics = serde_json::to_string(&diagnostics).expect("encode bind diagnostics");
        assert!(diagnostics.contains("incoming_listener_bind_failed"));
        assert!(diagnostics.contains("address_in_use"));

        let repaired = ClientSettings {
            listener: ListenerPolicy::AutomaticLoopback,
            preferred_listen_port: 6_881,
            port_mapping: crate::PortMappingPolicy::Disabled,
            peer_connection_limit: 321,
            upload_slots: 0,
        };
        let response = conflicted
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "repair-fixed-listener".to_owned(),
                expected_revision: None,
                command: Command::SetClientSettings {
                    settings: repaired.clone(),
                },
            })
            .await
            .expect("repair listener settings through command path");
        assert!(matches!(response.outcome, ResponseOutcome::Success { .. }));
        let configured = client_settings_runtime(&conflicted).await;
        assert_eq!(configured.configured, repaired);
        assert_eq!(
            configured.active.listener,
            ListenerPolicy::FixedLoopback { port }
        );
        assert!(configured.restart_required);
        assert!(matches!(
            configured.listener_status,
            ListenerStatus::BindFailed { .. }
        ));
        conflicted
            .shutdown()
            .await
            .expect("shutdown conflicted app");
        drop(blocker);
        drop(conflicted);

        configuration.peer_budget_max_open_files_for_testing = Some(25);
        let mut repaired_service = ApplicationService::open(configuration.clone())
            .await
            .expect("reopen repaired listener");
        let incoming = repaired_service
            .incoming_peer_snapshot()
            .expect("automatic listener starts after repair");
        assert_eq!(
            incoming.listen_address.ip(),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert_ne!(incoming.listen_address.port(), 0);
        assert_eq!(incoming.peer_budget.configured_limit, 321);
        assert_eq!(incoming.peer_budget.effective_limit, 5);
        assert_eq!(incoming.peer_budget.incoming_slack, 10);
        let runtime = client_settings_runtime(&repaired_service).await;
        assert_eq!(runtime.configured, repaired);
        assert_eq!(runtime.active, repaired);
        assert!(!runtime.restart_required);
        assert_eq!(runtime.effective_peer_connection_limit, 5);
        assert_eq!(
            runtime.listener_status,
            ListenerStatus::Listening {
                address: "127.0.0.1".to_owned(),
                port: incoming.listen_address.port(),
            }
        );
        let fixed = ClientSettings {
            listener: ListenerPolicy::FixedLoopback { port },
            ..repaired.clone()
        };
        repaired_service
            .dispatch(RequestEnvelope {
                version: CONTROL_VERSION,
                request_id: "select-released-fixed-listener".to_owned(),
                expected_revision: None,
                command: Command::SetClientSettings {
                    settings: fixed.clone(),
                },
            })
            .await
            .expect("select released fixed port");
        let pending_fixed = client_settings_runtime(&repaired_service).await;
        assert_eq!(pending_fixed.configured, fixed);
        assert_eq!(pending_fixed.active, repaired);
        assert!(pending_fixed.restart_required);
        repaired_service
            .shutdown()
            .await
            .expect("shutdown repaired service");
        drop(repaired_service);

        let mut fixed_service = ApplicationService::open(configuration)
            .await
            .expect("retry exact fixed listener on next generation");
        let incoming = fixed_service
            .incoming_peer_snapshot()
            .expect("fixed listener starts after conflict is gone");
        assert_eq!(incoming.listen_address.port(), port);
        let runtime = client_settings_runtime(&fixed_service).await;
        assert_eq!(runtime.configured, fixed);
        assert_eq!(runtime.active, fixed);
        assert!(!runtime.restart_required);
        assert_eq!(
            runtime.listener_status,
            ListenerStatus::Listening {
                address: "127.0.0.1".to_owned(),
                port,
            }
        );
        fixed_service
            .shutdown()
            .await
            .expect("shutdown fixed service");
        drop(fixed_service);
        fs::remove_dir_all(fixed_root).expect("remove fixed-conflict root");
    }
}

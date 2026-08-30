#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

mod advertised_endpoint;
mod application;
mod application_connection;
mod auto_manager;
mod control;
mod dht_views;
mod diagnostics;
mod download_queue;
mod durable_state;
mod file_views;
pub mod foreground_download;
mod have;
mod incoming_seeding;
mod media;
mod media_catalog_views;
mod profile_reset;
mod reachability;
mod session_network;
mod session_utp;
mod settings;
mod speed;
mod store;
mod store_schema;
mod torrent_runtime;
mod tracker_views;
mod views;

pub use application::{
    ApplicationConfig, ApplicationError, ApplicationPersistence, ApplicationService,
    PathRootStartupPolicy, PlatformFilePlan, PlatformRemovalPath, PlatformRemovalPlan,
    application_error_response,
};
pub use application_connection::{
    AcknowledgedViewStream, AcknowledgedViewStreamError, ApplicationCall, ApplicationCallError,
    ApplicationCallResult,
};
pub use control::{
    AddTorrentBytesRequest, AddTorrentDisposition, AddTorrentResult, CONTROL_VERSION, Command,
    CommandResult, ErrorCode, ErrorResponse, FileIndexRange, FilePriority, FileSelectionIntent,
    MAX_ROOT_ID_LENGTH, MAX_ROOT_LABEL_LENGTH, MagnetExportResult, MagnetExportSource,
    RemovalDataPolicy, RemovalState, RequestEnvelope, ResponseEnvelope, ResponseOutcome,
    ServiceSnapshot, StorageState, TorrentProtocolIdentities, TorrentSnapshot, TorrentState,
    validate_add_torrent_bytes_request,
};
pub use diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticField, DiagnosticFilter, DiagnosticProfile,
    DiagnosticRetention, DiagnosticSeverity, DiagnosticSubject, DiagnosticValue,
};
pub use file_views::{FileCatalogState, FileSelectionView, FileView};
pub use have::{HaveError, HaveState};
pub use media::{
    MAX_MEDIA_CAPABILITIES, MAX_MEDIA_READ_JOBS, MAX_MEDIA_REQUESTS,
    MAX_MEDIA_REQUESTS_PER_CAPABILITY, MEDIA_CAPABILITY_ABSOLUTE_TIMEOUT,
    MEDIA_CAPABILITY_IDLE_TIMEOUT, MEDIA_CAPABILITY_LENGTH, MEDIA_STREAMING_NO_PROGRESS_TIMEOUT,
    MediaCapabilityLease, MediaFileAvailability, MediaOriginError, MediaRangeError, MediaReadError,
    MediaResolveError, MediaResourceSnapshot, MediaUrlOutcome, MediaUrlResponse,
};
pub use media_catalog_views::{MediaCatalogState, MediaItemView, MediaRoleView};
pub use profile_reset::ProfileResetReport;
pub use reachability::Ipv6PinholeDiagnosticResult;
pub use rstorrent_engine::{
    DownloadResourceLimits, IncomingPeerServiceSnapshot, IncomingTcpBootstrap, NetworkConfig,
    NetworkPolicy, PeerTransportPolicy,
};
pub use settings::{
    ActiveDownloadsClampReason, AdvertisedPeerEndpointScope, AdvertisedPeerEndpointStatus,
    AdvertisedPeerEndpointUnavailableReason, BandwidthDirectionRuntimeView, BandwidthRuntimeView,
    ClientSettings, ClientSettingsApplicationState, ClientSettingsDegradedReason,
    ClientSettingsError, ClientSettingsPatch, ClientSettingsRuntimeView, DEFAULT_ACTIVE_DOWNLOADS,
    EffectiveListenerSettings, EncryptionPolicy, HttpsServerAuthenticationPolicy,
    Ipv6PinholeFailureStage, Ipv6PinholeStatus, ListenerBindFailureReason, ListenerPolicy,
    ListenerStatus, MAX_ACTIVE_DOWNLOADS, MIN_ACTIVE_DOWNLOADS, PortMappingFailureStage,
    PortMappingMechanism, PortMappingPolicy, PortMappingStatus, SessionUdpStatus,
    StorageRootAvailability, StorageRootSnapshot, StorageSettingsSnapshot, TorrentSettingsPatch,
    TorrentTransferLimits, TransferRateLimit, TransportAddressFamily, TransportFamilyRuntimeView,
};
pub use speed::{
    SessionCurrentRatesView, SpeedCurrentRate, SpeedHistoryAppend, SpeedHistoryAppendError,
    SpeedHistoryView, SpeedMetric, SpeedMetricAvailability, SpeedPersistenceState, SpeedRange,
    SpeedSeriesAppend, SpeedSeriesView,
};
pub use store::{
    ConfiguredStorageRoot, MAX_STORAGE_ROOT_LOCATOR_LENGTH, MAX_STORAGE_ROOTS, RemovalRecord,
    ResumeRecord, SessionStore, StorageRootLocation, StoreError, StoredStorageRoot, StoredTracker,
    StoredTrackerSource, StoredTrackerTransport,
};
pub use tracker_views::{
    TrackerAnnounceEventView, TrackerCatalogState, TrackerConnectionFamilyView,
    TrackerNextActionView, TrackerSecurityView, TrackerSourceView, TrackerStatusView,
    TrackerTransportView, TrackerView,
};
pub use views::{
    API_VERSION, ActivePiece, ActivePieceFieldUpdate, ActivePieceStageView, ActivePieceUpdate,
    ApiBackendIdentity, ApiEncoding, ApiHello, ApiLimits, ApiVersion, CapabilityStatus,
    CatalogPageRequest, CatalogPageView, CheckingPhaseView, CheckingProgressView, DeliveryMode,
    DeliveryPolicy, DhtAddressFamilyView, DhtBucketView, DhtFamilyInspectionView,
    DhtInspectionView, DhtLifecycleView, DhtLookupView, DhtNetworkPolicyView,
    DiskCheckpointStageView, DiskPieceStageView, DiskPieceView, DiskPipelineView, DiskPressureView,
    FileFieldUpdate, FileRowUpdate, IndexRange, IntegrityPreparationPhaseView,
    IntegrityPreparationView, MetadataAcquisitionPhaseView, MetadataAcquisitionView,
    OpenViewSetOptions, OpenViewSetRequest, OpenViewSetResponse, PeerDirection,
    PeerDisconnectReason, PeerFieldCapabilities, PeerFieldUpdate, PeerFlagView, PeerLifecycle,
    PeerMseMethodView, PeerRequestPhase, PeerRole, PeerRowUpdate, PeerSourceView,
    PeerTransportKind, PeerView, ProgressAction, ProgressAssessment, ProgressDisposition,
    ProgressInputs, ProgressPhase, ProgressReason, ResetReason, RowUpdateError, SubscriptionError,
    SubscriptionSpec, SubscriptionStats, SwarmCatalogState, SwarmCountsView, SwarmPeerState,
    SwarmPeerView, TorrentEtaView, TorrentFieldUpdate, TorrentOperationalState,
    TorrentPreparationView, TorrentRowUpdate, TorrentView, TorrentViewChange, UpdateBatch,
    UpdateViewSetRequest, VIEW_CONTRACT_VERSION, ViewDeliveryPolicy, ViewHub, ViewPatch,
    ViewProjection, ViewSelector, ViewSet, ViewSetError, ViewSetOwner, ViewSetStats, ViewSetUpdate,
    ViewSnapshot, ViewSpec, ViewSubscription, ViewUpdate, ViewUpdatePayload, assess_progress,
};

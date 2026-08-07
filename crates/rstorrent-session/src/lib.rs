#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

mod advertised_endpoint;
mod application;
mod application_connection;
mod control;
mod dht_views;
mod diagnostics;
mod durable_state;
mod file_views;
mod have;
mod incoming_seeding;
mod reachability;
mod session_network;
mod settings;
mod speed;
mod store;
mod torrent_runtime;
mod tracker_views;
mod views;

pub use application::{
    ApplicationConfig, ApplicationError, ApplicationPersistence, ApplicationService,
    PlatformRemovalPlan, application_error_response,
};
pub use application_connection::{
    AcknowledgedViewStream, AcknowledgedViewStreamError, ApplicationCall, ApplicationCallError,
    ApplicationCallResult,
};
pub use control::{
    AddTorrentBytesRequest, AddTorrentDisposition, AddTorrentResult, CONTROL_VERSION, Command,
    CommandResult, ErrorCode, ErrorResponse, FileIndexRange, FilePriority, FileSelectionIntent,
    MagnetExportResult, MagnetExportSource, RemovalDataPolicy, RemovalState, RequestEnvelope,
    ResponseEnvelope, ResponseOutcome, ServiceSnapshot, StorageState, TorrentSnapshot,
    TorrentState, validate_add_torrent_bytes_request,
};
pub use diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticField, DiagnosticFilter, DiagnosticProfile,
    DiagnosticRetention, DiagnosticSeverity, DiagnosticSubject, DiagnosticValue,
};
pub use file_views::{FileCatalogState, FileSelectionView, FileView};
pub use have::{HaveError, HaveState};
pub use rstorrent_engine::{
    DownloadResourceLimits, IncomingPeerServiceSnapshot, IncomingTcpBootstrap, NetworkConfig,
    NetworkPolicy,
};
pub use settings::{
    AdvertisedPeerEndpointScope, AdvertisedPeerEndpointStatus,
    AdvertisedPeerEndpointUnavailableReason, ClientSettings, ClientSettingsApplicationState,
    ClientSettingsDegradedReason, ClientSettingsError, ClientSettingsRuntimeView,
    EffectiveListenerSettings, HttpsServerAuthenticationPolicy, ListenerBindFailureReason,
    ListenerPolicy, ListenerStatus, PortMappingFailureStage, PortMappingMechanism,
    PortMappingPolicy, PortMappingStatus, SessionUdpStatus, StorageRootAvailability,
    StorageRootSnapshot, StorageSettingsSnapshot,
};
pub use speed::{
    SpeedCurrentRate, SpeedHistoryView, SpeedMetric, SpeedMetricAvailability,
    SpeedPersistenceState, SpeedRange, SpeedSeriesView,
};
pub use store::{
    ConfiguredStorageRoot, PreparedFileRecord, RemovalRecord, ResumeRecord, SessionStore,
    StorageRootLocation, StoreError, StoredStorageRoot, StoredTracker, StoredTrackerSource,
    StoredTrackerTransport,
};
pub use tracker_views::{
    TrackerAnnounceEventView, TrackerCatalogState, TrackerConnectionFamilyView,
    TrackerNextActionView, TrackerSecurityView, TrackerSourceView, TrackerStatusView,
    TrackerTransportView, TrackerView,
};
pub use views::{
    API_VERSION, ActivePiece, ActivePieceStageView, ApiEncoding, ApiHello, ApiLimits, ApiVersion,
    CapabilityStatus, CatalogPageRequest, CatalogPageView, CheckingPhaseView, CheckingProgressView,
    DeliveryMode, DeliveryPolicy, DhtBucketView, DhtInspectionView, DhtLifecycleView,
    DhtLookupView, DhtNetworkPolicyView, DiskCheckpointStageView, DiskPieceStageView,
    DiskPieceView, DiskPipelineView, DiskPressureView, IndexRange, OpenViewSetOptions,
    OpenViewSetRequest, OpenViewSetResponse, PeerDirection, PeerDisconnectReason,
    PeerFieldCapabilities, PeerFlagView, PeerLifecycle, PeerRequestPhase, PeerRole, PeerSourceView,
    PeerTransportKind, PeerView, ProgressAction, ProgressAssessment, ProgressDisposition,
    ProgressInputs, ProgressPhase, ProgressReason, ResetReason, SubscriptionError,
    SubscriptionSpec, SubscriptionStats, SwarmCatalogState, SwarmCountsView, SwarmPeerState,
    SwarmPeerView, TorrentEtaView, TorrentView, UpdateBatch, UpdateViewSetRequest,
    VIEW_CONTRACT_VERSION, ViewDeliveryPolicy, ViewHub, ViewPatch, ViewProjection, ViewSelector,
    ViewSet, ViewSetError, ViewSetOwner, ViewSetStats, ViewSetUpdate, ViewSnapshot, ViewSpec,
    ViewSubscription, ViewUpdate, ViewUpdatePayload, assess_progress,
};

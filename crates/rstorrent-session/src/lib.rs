#![forbid(unsafe_code)]

//! Durable application control and torrent-session ownership.

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

mod application;
mod control;
mod diagnostics;
mod file_views;
mod have;
mod store;
mod tracker_views;
mod view_sets;
mod views;

pub use application::{
    ApplicationConfig, ApplicationError, ApplicationService, PlatformRemovalPlan,
    application_error_response,
};
pub use control::{
    CONTROL_VERSION, Command, ErrorCode, ErrorResponse, RemovalDataPolicy, RemovalState,
    RequestEnvelope, ResponseEnvelope, ResponseOutcome, ServiceSnapshot, StorageState,
    TorrentSnapshot, TorrentState,
};
pub use diagnostics::{
    DiagnosticCategory, DiagnosticEvent, DiagnosticField, DiagnosticFilter, DiagnosticProfile,
    DiagnosticRetention, DiagnosticSeverity, DiagnosticSubject, DiagnosticValue,
};
pub use file_views::{FileCatalogState, FileSelectionView, FileView};
pub use have::{HaveError, HaveState};
pub use rstorrent_engine::{DownloadResourceLimits, NetworkConfig, NetworkPolicy};
pub use store::{
    ConfiguredStorageRoot, PreparedFileRecord, RemovalRecord, ResumeRecord, SessionStore,
    StorageRootLocation, StoreError,
};
pub use tracker_views::{
    TrackerAnnounceEventView, TrackerCatalogState, TrackerNextActionView, TrackerSourceView,
    TrackerStatusView, TrackerTransportView, TrackerView,
};
pub use view_sets::{
    API_VERSION, ApiEncoding, ApiHello, ApiLimits, ApiVersion, DeliveryMode, OpenViewSetOptions,
    OpenViewSetRequest, OpenViewSetResponse, UpdateBatch, UpdateViewSetRequest, ViewDeliveryPolicy,
    ViewSet, ViewSetError, ViewSetOwner, ViewSetStats, ViewSetUpdate, ViewSpec,
};
pub use views::{
    ActivePiece, ActivePieceStageView, CapabilityStatus, DeliveryPolicy, DiskPieceStageView,
    DiskPieceView, DiskPipelineView, DiskPressureView, IndexRange, PeerDirection,
    PeerDisconnectReason, PeerFieldCapabilities, PeerFlagView, PeerLifecycle, PeerRequestPhase,
    PeerRole, PeerSourceView, PeerTransportKind, PeerView, ProgressAction, ProgressAssessment,
    ProgressDisposition, ProgressInputs, ProgressPhase, ProgressReason, ResetReason,
    SubscriptionError, SubscriptionSpec, SubscriptionStats, TorrentView, VIEW_CONTRACT_VERSION,
    ViewHub, ViewPatch, ViewProjection, ViewSelector, ViewSnapshot, ViewSubscription, ViewUpdate,
    ViewUpdatePayload, assess_progress,
};

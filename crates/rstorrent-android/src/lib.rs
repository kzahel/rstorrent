//! Coarse Android control plane for an in-process RSTorrent engine.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, SocketAddr};
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rstorrent_engine::{
    DescriptorFile, DescriptorFileRole, DescriptorStorage, DescriptorStoragePlan, DownloadConfig,
    DownloadControl, DownloadError, DownloadProgress, DownloadReport, DownloadResourceLimits,
    NetworkConfig, NetworkPolicy, PlatformStorageBroker, PlatformStorageFailure,
    PlatformStorageFailureKind, PlatformStorageOperation, StorageFileAccess, StorageFileRole,
    StorageObjectKind, StorageObservation, download_verified_piece_to_descriptors_with_control,
    download_verified_piece_with_control, plan_descriptor_storage, platform_storage_channel,
};
use rstorrent_protocol::metainfo::{BEP9_METAINFO_LIMITS, Metainfo};
use rstorrent_session::{
    AddTorrentBytesRequest, ApplicationConfig, ApplicationService, ConfiguredStorageRoot,
    PlatformRemovalPlan, RequestEnvelope, ResponseEnvelope, SubscriptionSpec, ViewSubscription,
    ViewUpdate,
};
use sha1::{Digest, Sha1};
use tokio::sync::Mutex as AsyncMutex;

const INTERFACE_VERSION: &str = "rstorrent-android/0.3.0;uniffi/0.31.0";
const MIN_PAYLOAD_BYTES: u64 = 16 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TIMEOUT_SECONDS: u64 = 5 * 60;
const MAX_JOIN_MILLIS: u64 = 5 * 60 * 1_000;
const MAX_FILE_SELECTIONS: usize = 1_024;
const MAX_STORAGE_WRITE_DELAY_MILLIS: u64 = 5_000;
const DESCRIPTOR_HASH_BUFFER: usize = 16 * 1024;
const MAX_ANDROID_PATH_BYTES: usize = 4 * 1024;

uniffi::setup_scaffolding!();

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rstorrent_bootstrap_PlatformTrustBootstrap_initializeNative<
    'local,
>(
    mut unowned_env: jni::EnvUnowned<'local>,
    _class: jni::objects::JClass<'local>,
    context: jni::objects::JObject<'local>,
) {
    unowned_env
        .with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AndroidApplicationConfig {
    pub profile_root: String,
    pub profile_id: String,
    pub storage_root: String,
    pub platform_storage: bool,
    pub network_policy: AndroidNetworkPolicy,
    pub peer_connect_timeout_seconds: u64,
    pub peer_io_timeout_seconds: u64,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum AndroidNetworkPolicy {
    Offline,
    LoopbackOnly,
    Online,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AndroidMseDhWorkSnapshot {
    pub waiting: u64,
    pub active: u64,
    pub high_water: u64,
    pub tracked: u64,
    pub closed: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AndroidDownloadResourceSnapshot {
    pub registered_generations: u64,
    pub registered_generations_high_water: u64,
    pub outstanding_request_bytes: u64,
    pub outstanding_request_high_water: u64,
    pub buffered_payload_bytes: u64,
    pub buffered_payload_high_water: u64,
    pub active_piece_bytes: u64,
    pub active_piece_bytes_high_water: u64,
    pub active_pieces: u64,
    pub active_pieces_high_water: u64,
    pub active_storage_writes: u64,
    pub active_storage_writes_high_water: u64,
    pub active_storage_hashes: u64,
    pub active_storage_hashes_high_water: u64,
}

#[derive(Debug, uniffi::Error)]
pub enum AndroidClientError {
    Failure { detail: String },
}

impl std::fmt::Display for AndroidClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failure { detail } => formatter.write_str(detail),
        }
    }
}

impl std::error::Error for AndroidClientError {}

impl AndroidClientError {
    fn message(message: impl Into<String>) -> Self {
        Self::Failure {
            detail: message.into(),
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct AndroidApplicationClient {
    service: Arc<AsyncMutex<Option<ApplicationService>>>,
    platform_storage: Arc<PlatformStorageBroker>,
}

#[derive(Debug, uniffi::Object)]
pub struct AndroidViewSubscription {
    subscription: ViewSubscription,
}

#[uniffi::export(async_runtime = "tokio")]
impl AndroidApplicationClient {
    #[uniffi::constructor]
    pub async fn open(config: AndroidApplicationConfig) -> Result<Arc<Self>, AndroidClientError> {
        let (platform_client, platform_storage) = platform_storage_channel();
        let platform_enabled = config.platform_storage;
        let mut application_config = validate_application_config(config)?;
        if platform_enabled {
            application_config.platform_storage_client = Some(platform_client);
        }
        let service = ApplicationService::open(application_config)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))?;
        let service = Arc::new(AsyncMutex::new(Some(service)));
        ApplicationService::ensure_optional_maintenance_owner(&service).await;
        Ok(Arc::new(Self {
            service,
            platform_storage,
        }))
    }

    pub async fn dispatch(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .dispatch(request)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn add_torrent_bytes(
        &self,
        request: AddTorrentBytesRequest,
        source: Vec<u8>,
    ) -> Result<ResponseEnvelope, AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .add_torrent_bytes(request, source)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn subscribe(
        &self,
        spec: SubscriptionSpec,
    ) -> Result<Arc<AndroidViewSubscription>, AndroidClientError> {
        let subscription = self
            .service
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .subscribe(spec)
            .map_err(|error| AndroidClientError::message(error.to_string()))?;
        Ok(Arc::new(AndroidViewSubscription { subscription }))
    }

    pub async fn mse_dh_work_snapshot(
        &self,
    ) -> Result<AndroidMseDhWorkSnapshot, AndroidClientError> {
        let snapshot = self
            .service
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .mse_dh_work_snapshot();
        Ok(AndroidMseDhWorkSnapshot {
            waiting: snapshot.waiting as u64,
            active: snapshot.active as u64,
            high_water: snapshot.high_water as u64,
            tracked: snapshot.tracked as u64,
            closed: snapshot.closed,
        })
    }

    pub async fn download_resource_snapshot(
        &self,
    ) -> Result<AndroidDownloadResourceSnapshot, AndroidClientError> {
        let snapshot = self
            .service
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .session_download_resource_snapshot();
        Ok(AndroidDownloadResourceSnapshot {
            registered_generations: snapshot.registered_generations as u64,
            registered_generations_high_water: snapshot.registered_generations_high_water as u64,
            outstanding_request_bytes: snapshot.outstanding_request_bytes as u64,
            outstanding_request_high_water: snapshot.outstanding_request_high_water as u64,
            buffered_payload_bytes: snapshot.buffered_payload_bytes as u64,
            buffered_payload_high_water: snapshot.buffered_payload_high_water as u64,
            active_piece_bytes: snapshot.active_piece_bytes as u64,
            active_piece_bytes_high_water: snapshot.active_piece_bytes_high_water as u64,
            active_pieces: snapshot.active_pieces as u64,
            active_pieces_high_water: snapshot.active_pieces_high_water as u64,
            active_storage_writes: snapshot.active_storage_writes as u64,
            active_storage_writes_high_water: snapshot.active_storage_writes_high_water as u64,
            active_storage_hashes: snapshot.active_storage_hashes as u64,
            active_storage_hashes_high_water: snapshot.active_storage_hashes_high_water as u64,
        })
    }

    pub async fn probe_saf_storage_roots(&self) -> Result<bool, AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .probe_platform_storage_roots()
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn prepared_saf_files(
        &self,
        torrent_id: String,
    ) -> Result<Vec<PreparedFile>, AndroidClientError> {
        let files = self
            .service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .prepared_files(&torrent_id)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))?;
        files
            .into_iter()
            .map(|file| {
                Ok(PreparedFile {
                    file_index: u32::try_from(file.file_index)
                        .map_err(|_| AndroidClientError::message("file index exceeds u32"))?,
                    length: file.length,
                    sha1_hex: hex(&file.sha1),
                })
            })
            .collect()
    }

    pub async fn prepare_dynamic_saf_publication(
        &self,
        torrent_id: String,
    ) -> Result<String, AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .prepare_platform_publication(&torrent_id)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn confirm_dynamic_saf_publication(
        &self,
        torrent_id: String,
    ) -> Result<(), AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .confirm_platform_publication(&torrent_id)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn mark_saf_unavailable(
        &self,
        torrent_id: String,
        message: String,
    ) -> Result<(), AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .mark_storage_unavailable(&torrent_id, &message)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn prepare_saf_tree_replacement(&self) -> Result<Option<String>, AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .prepare_platform_storage_replacement("downloads")
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn saf_removal_plan(
        &self,
        torrent_id: String,
    ) -> Result<SafRemovalPlan, AndroidClientError> {
        let plan = self
            .service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .platform_removal_plan(&torrent_id)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))?;
        Ok(map_saf_removal_plan(plan))
    }

    pub async fn confirm_saf_removal(
        &self,
        torrent_id: String,
        operation_id: String,
    ) -> Result<(), AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .confirm_platform_removal(&torrent_id, &operation_id)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn fail_saf_removal(
        &self,
        torrent_id: String,
        operation_id: String,
        message: String,
    ) -> Result<(), AndroidClientError> {
        self.service
            .lock()
            .await
            .as_mut()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .fail_platform_removal(&torrent_id, &operation_id, &message)
            .await
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }

    pub async fn next_saf_storage_request(&self) -> Option<SafStorageRequest> {
        self.platform_storage.next_request().await.map(|request| {
            let (role, file_index) = match request.role {
                StorageFileRole::Namespace => (SafDynamicFileRole::Namespace, 0),
                StorageFileRole::Payload(file_index) => (
                    SafDynamicFileRole::Payload,
                    u32::try_from(file_index).unwrap_or(u32::MAX),
                ),
                StorageFileRole::Part => (SafDynamicFileRole::Part, 0),
            };
            SafStorageRequest {
                request_id: request.request_id,
                root_id: request.root_id,
                storage_id: request.storage_id,
                namespace_generation: request.namespace_generation,
                role,
                file_index,
                path: request.path,
                operation: match request.operation {
                    PlatformStorageOperation::Open => SafStorageOperation::Open,
                    PlatformStorageOperation::Observe => SafStorageOperation::Observe,
                    PlatformStorageOperation::Delete => SafStorageOperation::Delete,
                },
                access: match request.access {
                    StorageFileAccess::ReadExisting => SafStorageAccess::ReadExisting,
                    StorageFileAccess::ReadWriteExisting => SafStorageAccess::ReadWriteExisting,
                    StorageFileAccess::ReadWriteCreate => SafStorageAccess::ReadWriteCreate,
                },
                timeout_millis: request.timeout_millis,
            }
        })
    }

    pub async fn saf_storage_pool_snapshot(
        &self,
    ) -> Result<SafStoragePoolSnapshot, AndroidClientError> {
        let snapshot = self
            .service
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| AndroidClientError::message("application client is shut down"))?
            .storage_file_pool_snapshot();
        Ok(SafStoragePoolSnapshot {
            limit: snapshot.limit as u64,
            current_owned: snapshot.current_owned as u64,
            owned_high_water: snapshot.owned_high_water as u64,
            cached_entries: snapshot.cached_entries as u64,
            hits: snapshot.hits,
            misses: snapshot.misses,
            evictions: snapshot.evictions,
            singleflight_waits: snapshot.singleflight_waits,
            mode_upgrades: snapshot.mode_upgrades,
            open_failures: snapshot.open_failures,
            resource_retries: snapshot.resource_retries,
            platform_pending: snapshot.platform_pending as u64,
            platform_pending_high_water: snapshot.platform_pending_high_water as u64,
        })
    }

    pub fn complete_saf_storage_request(
        &self,
        request_id: u64,
        fd: i32,
        access: SafStorageAccess,
    ) -> Result<bool, AndroidClientError> {
        if !self.platform_storage.is_pending(request_id) {
            return Ok(false);
        }
        let mut file = match duplicate_descriptor(fd) {
            Ok(file) => file,
            Err(message) => {
                self.platform_storage.complete_error(
                    request_id,
                    PlatformStorageFailure::new(PlatformStorageFailureKind::Internal, message),
                );
                return Ok(false);
            }
        };
        if let Err(message) = validate_descriptor_access(fd, access) {
            self.platform_storage.complete_error(
                request_id,
                PlatformStorageFailure::new(PlatformStorageFailureKind::ProviderRefused, message),
            );
            return Ok(false);
        }
        if let Err(error) = file.stream_position() {
            self.platform_storage.complete_error(
                request_id,
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::NonSeekable,
                    error.to_string(),
                ),
            );
            return Ok(false);
        }
        Ok(self.platform_storage.complete_file(request_id, file))
    }

    pub fn fail_saf_storage_request(
        &self,
        request_id: u64,
        kind: SafStorageFailureKind,
        message: String,
    ) -> bool {
        let kind = match kind {
            SafStorageFailureKind::Missing => PlatformStorageFailureKind::Missing,
            SafStorageFailureKind::GrantUnavailable => PlatformStorageFailureKind::GrantUnavailable,
            SafStorageFailureKind::PermissionDenied => PlatformStorageFailureKind::PermissionDenied,
            SafStorageFailureKind::WrongKind => PlatformStorageFailureKind::WrongKind,
            SafStorageFailureKind::NameCollision => PlatformStorageFailureKind::NameCollision,
            SafStorageFailureKind::StaleGeneration => PlatformStorageFailureKind::StaleGeneration,
            SafStorageFailureKind::ProviderRefused => PlatformStorageFailureKind::ProviderRefused,
            SafStorageFailureKind::NonSeekable => PlatformStorageFailureKind::NonSeekable,
            SafStorageFailureKind::Cancelled => PlatformStorageFailureKind::Cancelled,
            SafStorageFailureKind::DeadlineExceeded => PlatformStorageFailureKind::DeadlineExceeded,
            SafStorageFailureKind::Internal => PlatformStorageFailureKind::Internal,
        };
        self.platform_storage
            .complete_error(request_id, PlatformStorageFailure::new(kind, message))
    }

    pub fn complete_saf_storage_delete(&self, request_id: u64) -> bool {
        self.platform_storage.complete_deleted(request_id)
    }

    pub fn complete_saf_storage_observation(
        &self,
        request_id: u64,
        observation: SafStorageObservation,
    ) -> Result<bool, AndroidClientError> {
        let observation = if observation.exists {
            let kind = observation.kind.ok_or_else(|| {
                AndroidClientError::message("present SAF observation has no kind")
            })?;
            StorageObservation::present(
                match kind {
                    SafStorageObjectKind::File => StorageObjectKind::File,
                    SafStorageObjectKind::Directory => StorageObjectKind::Directory,
                    SafStorageObjectKind::Other => StorageObjectKind::Other,
                },
                observation.length,
                observation.opaque_token,
            )
            .map_err(|error| AndroidClientError::message(error.to_string()))?
        } else {
            if observation.kind.is_some()
                || observation.length.is_some()
                || observation.opaque_token.is_some()
            {
                return Err(AndroidClientError::message(
                    "missing SAF observation contains present-only fields",
                ));
            }
            StorageObservation::missing()
        };
        Ok(self
            .platform_storage
            .complete_observation(request_id, observation))
    }

    pub async fn shutdown(&self) -> Result<(), AndroidClientError> {
        self.platform_storage.cancel_all();
        let service = self.service.lock().await.take();
        if let Some(mut service) = service {
            service
                .shutdown()
                .await
                .map_err(|error| AndroidClientError::message(error.to_string()))?;
        }
        Ok(())
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl AndroidViewSubscription {
    pub fn stream_id(&self) -> String {
        self.subscription.stream_id()
    }

    pub async fn next_update(&self) -> Option<ViewUpdate> {
        self.subscription.next_update().await
    }

    pub fn resync(&self) -> Result<(), AndroidClientError> {
        self.subscription
            .resync()
            .map_err(|error| AndroidClientError::message(error.to_string()))
    }
}

impl Drop for AndroidViewSubscription {
    fn drop(&mut self) {
        self.subscription.close();
    }
}

fn validate_application_config(
    config: AndroidApplicationConfig,
) -> Result<ApplicationConfig, AndroidClientError> {
    fn path(value: String, label: &str) -> Result<PathBuf, AndroidClientError> {
        if value.is_empty() || value.len() > MAX_ANDROID_PATH_BYTES || value.as_bytes().contains(&0)
        {
            return Err(AndroidClientError::message(format!(
                "{label} must be 1..={MAX_ANDROID_PATH_BYTES} bytes without NUL"
            )));
        }
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(AndroidClientError::message(format!(
                "{label} must be absolute"
            )));
        }
        Ok(path)
    }
    if config.peer_connect_timeout_seconds == 0
        || config.peer_connect_timeout_seconds > MAX_TIMEOUT_SECONDS
    {
        return Err(AndroidClientError::message(format!(
            "peer connect timeout must be 1..={MAX_TIMEOUT_SECONDS} seconds"
        )));
    }
    if config.peer_io_timeout_seconds == 0 || config.peer_io_timeout_seconds > MAX_TIMEOUT_SECONDS {
        return Err(AndroidClientError::message(format!(
            "peer I/O timeout must be 1..={MAX_TIMEOUT_SECONDS} seconds"
        )));
    }
    let storage_root = if config.platform_storage {
        ConfiguredStorageRoot::platform("downloads")
    } else {
        ConfiguredStorageRoot::path("downloads", path(config.storage_root, "storage root")?)
    };
    let network_policy = match config.network_policy {
        AndroidNetworkPolicy::Offline => NetworkPolicy::Offline,
        AndroidNetworkPolicy::LoopbackOnly => NetworkPolicy::LoopbackOnly,
        AndroidNetworkPolicy::Online => NetworkPolicy::Online,
    };
    let mut application = ApplicationConfig::new(
        path(config.profile_root, "profile root")?,
        config.profile_id,
        vec![storage_root],
        NetworkConfig::new(
            network_policy,
            Duration::from_secs(config.peer_connect_timeout_seconds),
            Duration::from_secs(config.peer_io_timeout_seconds),
        ),
    );
    if network_policy == NetworkPolicy::Online {
        application = application.with_fresh_profile_defaults();
    }
    application.download_resource_limits = DownloadResourceLimits::ANDROID;
    application.active_download_cap = Some(2);
    Ok(application)
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct EngineConfig {
    pub metainfo_path: String,
    pub output_path: String,
    pub peer_port: u16,
    pub timeout_seconds: u64,
    pub max_buffered_payload_bytes: u64,
    pub storage_write_delay_millis: u64,
    pub skip_files: Vec<u32>,
    pub materialize_files: Vec<u32>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafDescriptor {
    pub file_index: u32,
    pub fd: i32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafStorage {
    pub wanted_files: Vec<SafDescriptor>,
    pub part_fd: i32,
    pub reopened_part_fd: i32,
    pub materialization_files: Vec<SafDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafDynamicFileRole {
    Namespace,
    Payload,
    Part,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafStorageAccess {
    ReadExisting,
    ReadWriteExisting,
    ReadWriteCreate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafStorageOperation {
    Open,
    Observe,
    Delete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafStorageObjectKind {
    File,
    Directory,
    Other,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafStorageObservation {
    pub exists: bool,
    pub kind: Option<SafStorageObjectKind>,
    pub length: Option<u64>,
    pub opaque_token: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafStorageFailureKind {
    Missing,
    GrantUnavailable,
    PermissionDenied,
    WrongKind,
    NameCollision,
    StaleGeneration,
    ProviderRefused,
    NonSeekable,
    Cancelled,
    DeadlineExceeded,
    Internal,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafStorageRequest {
    pub request_id: u64,
    pub root_id: String,
    pub storage_id: String,
    pub namespace_generation: u64,
    pub role: SafDynamicFileRole,
    pub file_index: u32,
    pub path: Vec<String>,
    pub operation: SafStorageOperation,
    pub access: SafStorageAccess,
    pub timeout_millis: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafStoragePoolSnapshot {
    pub limit: u64,
    pub current_owned: u64,
    pub owned_high_water: u64,
    pub cached_entries: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub singleflight_waits: u64,
    pub mode_upgrades: u64,
    pub open_failures: u64,
    pub resource_retries: u64,
    pub platform_pending: u64,
    pub platform_pending_high_water: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SafFileRole {
    Wanted,
    Skipped,
    Padding,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafPlanFile {
    pub file_index: u32,
    pub path: Vec<String>,
    pub length: u64,
    pub role: SafFileRole,
    pub materialize: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafStoragePlan {
    pub valid: bool,
    pub message: Option<String>,
    pub info_hash_hex: String,
    pub name: String,
    pub files: Vec<SafPlanFile>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SafRemovalPlan {
    pub operation_id: String,
    pub torrent_id: String,
    pub storage_root: String,
    pub name: String,
}

fn map_saf_removal_plan(plan: PlatformRemovalPlan) -> SafRemovalPlan {
    SafRemovalPlan {
        operation_id: plan.operation_id,
        torrent_id: plan.torrent_id,
        storage_root: plan.storage_root,
        name: plan.name,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum StartDisposition {
    Started,
    Busy,
    NeedsJoin,
    Rejected,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StartResult {
    pub disposition: StartDisposition,
    pub generation: u64,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SessionState {
    Idle,
    Running,
    Cancelling,
    Prepared,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum TerminalOutcome {
    Prepared,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FailureKind {
    Peer,
    Timeout,
    Configuration,
    Protocol,
    PreexistingArtifact,
    Storage,
    Cleanup,
    Runtime,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct EngineReport {
    pub info_hash_hex: String,
    pub final_piece_hash_hex: String,
    pub bytes_written: u64,
    pub block_count: u64,
    pub payload_limit: u64,
    pub payload_high_water: u64,
    pub outstanding_request_limit: u64,
    pub outstanding_request_high_water: u64,
    pub active_piece_limit: u64,
    pub verification_buffer: u64,
    pub piece_count: u64,
    pub verified_piece_count: u64,
    pub skipped_piece_count: u64,
    pub selected_file_bytes: u64,
    pub skipped_file_bytes: u64,
    pub padding_bytes: u64,
    pub selected_written_bytes: u64,
    pub part_written_bytes: u64,
    pub materialized_bytes: u64,
    pub part_slots_before_materialization: u64,
    pub part_slots_after_materialization: u64,
    pub part_reopened: bool,
    pub part_path: Option<String>,
    pub prepared_files: Vec<PreparedFile>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PreparedFile {
    pub file_index: u32,
    pub length: u64,
    pub sha1_hex: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DescriptorInspection {
    pub success: bool,
    pub message: Option<String>,
    pub length: u64,
    pub allocated_bytes: u64,
    pub sha1_hex: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TerminalResult {
    pub outcome: TerminalOutcome,
    pub failure_kind: Option<FailureKind>,
    pub failure_message: Option<String>,
    pub report: Option<EngineReport>,
    pub elapsed_millis: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SessionSnapshot {
    pub interface_version: String,
    pub state: SessionState,
    pub generation: u64,
    pub task_alive: bool,
    pub cancellation_requested: bool,
    pub buffered_payload_bytes: u64,
    pub payload_high_water: u64,
    pub outstanding_request_bytes: u64,
    pub outstanding_request_high_water: u64,
    pub requested_bytes: u64,
    pub received_bytes: u64,
    pub stored_bytes: u64,
    pub terminal: Option<TerminalResult>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct JoinResult {
    pub joined: bool,
    pub terminal: Option<TerminalResult>,
}

#[derive(Debug, uniffi::Object)]
pub struct EngineSession {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    inner: Mutex<Inner>,
    changed: Condvar,
}

#[derive(Debug)]
struct Inner {
    state: SessionState,
    generation: u64,
    started: Option<Instant>,
    control: Option<DownloadControl>,
    progress: DownloadProgress,
    cancellation_requested: bool,
    worker: Option<JoinHandle<()>>,
    terminal: Option<TerminalResult>,
}

impl EngineSession {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn join_until(&self, timeout: Duration) -> JoinResult {
        let deadline = Instant::now() + timeout;
        let mut inner = self.lock();
        loop {
            if inner.terminal.is_some() {
                let terminal = inner.terminal.clone();
                let worker = inner.worker.take();
                drop(inner);
                if let Some(worker) = worker
                    && worker.join().is_err()
                {
                    let failure = runtime_failure("engine worker panicked");
                    let mut inner = self.lock();
                    inner.state = SessionState::Failed;
                    inner.terminal = Some(failure.clone());
                    return JoinResult {
                        joined: true,
                        terminal: Some(failure),
                    };
                }
                return JoinResult {
                    joined: true,
                    terminal,
                };
            }
            if inner.worker.is_none() {
                return JoinResult {
                    joined: true,
                    terminal: None,
                };
            }

            let now = Instant::now();
            if now >= deadline {
                return JoinResult {
                    joined: false,
                    terminal: None,
                };
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, wait) = self
                .shared
                .changed
                .wait_timeout(inner, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner = next;
            if wait.timed_out() && inner.terminal.is_none() {
                return JoinResult {
                    joined: false,
                    terminal: None,
                };
            }
        }
    }

    fn start_owned(
        &self,
        config: DownloadConfig,
        storage_write_delay: Duration,
        descriptors: Option<DescriptorStorage>,
    ) -> StartResult {
        let mut inner = self.lock();
        if matches!(
            inner.state,
            SessionState::Running | SessionState::Cancelling
        ) {
            return StartResult {
                disposition: StartDisposition::Busy,
                generation: inner.generation,
                message: Some("an engine task is already active".to_owned()),
            };
        }
        if inner.worker.is_some() {
            return StartResult {
                disposition: StartDisposition::NeedsJoin,
                generation: inner.generation,
                message: Some("the previous engine task must be joined".to_owned()),
            };
        }

        inner.generation += 1;
        let generation = inner.generation;
        let started = Instant::now();
        let control = DownloadControl::new();
        control.set_storage_write_delay(storage_write_delay);
        inner.state = SessionState::Running;
        inner.started = Some(started);
        inner.control = Some(control.clone());
        inner.progress = DownloadProgress::default();
        inner.cancellation_requested = false;
        inner.terminal = None;

        let shared = Arc::clone(&self.shared);
        let worker = std::thread::Builder::new()
            .name(format!("rstorrent-engine-{generation}"))
            .spawn(move || run_worker(shared, config, descriptors, control, started));
        match worker {
            Ok(worker) => {
                inner.worker = Some(worker);
                StartResult {
                    disposition: StartDisposition::Started,
                    generation,
                    message: None,
                }
            }
            Err(error) => {
                inner.state = SessionState::Idle;
                inner.started = None;
                inner.control = None;
                StartResult {
                    disposition: StartDisposition::Rejected,
                    generation,
                    message: Some(format!("failed to start engine worker: {error}")),
                }
            }
        }
    }
}

#[uniffi::export]
impl EngineSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    state: SessionState::Idle,
                    generation: 0,
                    started: None,
                    control: None,
                    progress: DownloadProgress::default(),
                    cancellation_requested: false,
                    worker: None,
                    terminal: None,
                }),
                changed: Condvar::new(),
            }),
        })
    }

    pub fn start(&self, config: EngineConfig) -> StartResult {
        let (config, storage_write_delay) = match validate_config(config) {
            Ok(config) => config,
            Err(message) => {
                let inner = self.lock();
                return StartResult {
                    disposition: StartDisposition::Rejected,
                    generation: inner.generation,
                    message: Some(message),
                };
            }
        };
        self.start_owned(config, storage_write_delay, None)
    }

    pub fn start_saf(&self, config: EngineConfig, storage: SafStorage) -> StartResult {
        let (config, storage_write_delay) = match validate_config(config) {
            Ok(config) => config,
            Err(message) => {
                let inner = self.lock();
                return StartResult {
                    disposition: StartDisposition::Rejected,
                    generation: inner.generation,
                    message: Some(message),
                };
            }
        };
        let descriptors = match duplicate_saf_storage(storage) {
            Ok(descriptors) => descriptors,
            Err(message) => {
                let inner = self.lock();
                return StartResult {
                    disposition: StartDisposition::Rejected,
                    generation: inner.generation,
                    message: Some(message),
                };
            }
        };
        self.start_owned(config, storage_write_delay, Some(descriptors))
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let inner = self.lock();
        let progress = inner
            .control
            .as_ref()
            .map(DownloadControl::snapshot)
            .unwrap_or(inner.progress);
        SessionSnapshot {
            interface_version: INTERFACE_VERSION.to_owned(),
            state: inner.state,
            generation: inner.generation,
            task_alive: inner
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished()),
            cancellation_requested: inner.cancellation_requested,
            buffered_payload_bytes: progress.buffered_payload_bytes as u64,
            payload_high_water: progress.payload_high_water as u64,
            outstanding_request_bytes: progress.outstanding_request_bytes as u64,
            outstanding_request_high_water: progress.outstanding_request_high_water as u64,
            requested_bytes: progress.requested_bytes as u64,
            received_bytes: progress.received_bytes as u64,
            stored_bytes: progress.stored_bytes as u64,
            terminal: inner.terminal.clone(),
        }
    }

    pub fn wait_for_terminal(&self, timeout_millis: u64) -> JoinResult {
        self.join_until(join_timeout(timeout_millis))
    }

    pub fn cancel_and_join(&self, timeout_millis: u64) -> JoinResult {
        let control = {
            let mut inner = self.lock();
            if inner.state == SessionState::Running {
                inner.state = SessionState::Cancelling;
            }
            if inner.control.is_some() {
                inner.cancellation_requested = true;
            }
            inner.control.clone()
        };
        if let Some(control) = control {
            control.cancel();
        }
        self.join_until(join_timeout(timeout_millis))
    }
}

impl Drop for EngineSession {
    fn drop(&mut self) {
        if let Some(control) = self.lock().control.clone() {
            control.cancel();
        }
    }
}

#[uniffi::export]
pub fn interface_version() -> String {
    INTERFACE_VERSION.to_owned()
}

#[uniffi::export]
pub fn saf_storage_plan(
    metainfo_bytes: Vec<u8>,
    skip_files: Vec<u32>,
    materialize_files: Vec<u32>,
) -> SafStoragePlan {
    let result = (|| {
        if metainfo_bytes.len() > BEP9_METAINFO_LIMITS.max_outer_bytes {
            return Err(format!(
                "metainfo exceeds input limit {}",
                BEP9_METAINFO_LIMITS.max_outer_bytes
            ));
        }
        if skip_files.len() > MAX_FILE_SELECTIONS || materialize_files.len() > MAX_FILE_SELECTIONS {
            return Err(format!(
                "file selection lists may contain at most {MAX_FILE_SELECTIONS} entries"
            ));
        }
        let metainfo = Metainfo::from_bytes_with_limits(&metainfo_bytes, BEP9_METAINFO_LIMITS)
            .map_err(|error| error.to_string())?;
        let skip_files: Vec<usize> = skip_files.into_iter().map(|index| index as usize).collect();
        let materialize_files: Vec<usize> = materialize_files
            .into_iter()
            .map(|index| index as usize)
            .collect();
        let plan = plan_descriptor_storage(&metainfo, &skip_files, &materialize_files)
            .map_err(|error| error.to_string())?;
        map_saf_storage_plan(plan).map_err(|error| error.to_string())
    })();
    result.unwrap_or_else(|message| SafStoragePlan {
        valid: false,
        message: Some(message),
        info_hash_hex: String::new(),
        name: String::new(),
        files: Vec::new(),
    })
}

fn map_saf_storage_plan(plan: DescriptorStoragePlan) -> Result<SafStoragePlan, AndroidClientError> {
    let files = plan
        .files
        .into_iter()
        .map(|file| {
            Ok(SafPlanFile {
                file_index: u32::try_from(file.file_index)
                    .map_err(|_| AndroidClientError::message("file index exceeds u32"))?,
                path: file.path,
                length: file.length,
                role: match file.role {
                    DescriptorFileRole::Wanted => SafFileRole::Wanted,
                    DescriptorFileRole::Skipped => SafFileRole::Skipped,
                    DescriptorFileRole::Padding => SafFileRole::Padding,
                },
                materialize: file.materialize,
            })
        })
        .collect::<Result<Vec<_>, AndroidClientError>>()?;
    Ok(SafStoragePlan {
        valid: true,
        message: None,
        info_hash_hex: hex(&plan.info_hash),
        name: plan.name,
        files,
    })
}

#[uniffi::export]
pub fn inspect_borrowed_descriptor(
    fd: i32,
    expected_length: u64,
    expected_sha1_hex: String,
) -> DescriptorInspection {
    match inspect_descriptor(fd, expected_length, &expected_sha1_hex) {
        Ok((length, allocated_bytes, sha1_hex)) => DescriptorInspection {
            success: true,
            message: None,
            length,
            allocated_bytes,
            sha1_hex,
        },
        Err(message) => DescriptorInspection {
            success: false,
            message: Some(message),
            length: 0,
            allocated_bytes: 0,
            sha1_hex: String::new(),
        },
    }
}

fn duplicate_saf_storage(storage: SafStorage) -> Result<DescriptorStorage, String> {
    if storage.wanted_files.len() > MAX_FILE_SELECTIONS
        || storage.materialization_files.len() > MAX_FILE_SELECTIONS
    {
        return Err(format!(
            "SAF descriptor lists may contain at most {MAX_FILE_SELECTIONS} entries"
        ));
    }
    fn files(descriptors: Vec<SafDescriptor>) -> Result<Vec<DescriptorFile>, String> {
        descriptors
            .into_iter()
            .map(|descriptor| {
                Ok(DescriptorFile {
                    file_index: descriptor.file_index as usize,
                    file: duplicate_descriptor(descriptor.fd)?,
                })
            })
            .collect()
    }
    Ok(DescriptorStorage {
        wanted_files: files(storage.wanted_files)?,
        part_file: duplicate_descriptor(storage.part_fd)?,
        reopened_part_file: duplicate_descriptor(storage.reopened_part_fd)?,
        materialization_files: files(storage.materialization_files)?,
    })
}

#[cfg(unix)]
fn duplicate_descriptor(fd: i32) -> Result<File, String> {
    if fd < 0 {
        return Err(format!("descriptor {fd} is invalid"));
    }
    // SAF owns the borrowed descriptor. F_DUPFD_CLOEXEC creates an independent
    // Rust-owned descriptor without transferring or closing the caller's one.
    let owned_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if owned_fd < 0 {
        return Err(format!(
            "duplicate descriptor {fd}: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: fcntl returned a new descriptor owned by this call.
    Ok(unsafe { File::from_raw_fd(owned_fd) })
}

#[cfg(unix)]
fn validate_descriptor_access(fd: i32, access: SafStorageAccess) -> Result<(), String> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(format!(
            "inspect descriptor access: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mode = flags & libc::O_ACCMODE;
    let compatible = match access {
        SafStorageAccess::ReadExisting => mode != libc::O_WRONLY,
        SafStorageAccess::ReadWriteExisting | SafStorageAccess::ReadWriteCreate => {
            mode == libc::O_RDWR
        }
    };
    compatible
        .then_some(())
        .ok_or_else(|| "provider descriptor mode is incompatible with the request".to_owned())
}

#[cfg(not(unix))]
fn duplicate_descriptor(_fd: i32) -> Result<File, String> {
    Err("descriptor storage requires a Unix platform".to_owned())
}

#[cfg(not(unix))]
fn validate_descriptor_access(_fd: i32, _access: SafStorageAccess) -> Result<(), String> {
    Err("SAF descriptors require a Unix target".to_owned())
}

fn inspect_descriptor(
    fd: i32,
    expected_length: u64,
    expected_sha1_hex: &str,
) -> Result<(u64, u64, String), String> {
    if expected_sha1_hex.len() != 40
        || !expected_sha1_hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("expected SHA-1 must contain exactly 40 hexadecimal characters".to_owned());
    }
    let mut file = duplicate_descriptor(fd)?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("inspect descriptor metadata: {error}"))?;
    let length = metadata.len();
    if length != expected_length {
        return Err(format!(
            "descriptor length {length} does not match expected {expected_length}"
        ));
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek descriptor for verification: {error}"))?;
    let mut remaining = length;
    let mut buffer = [0_u8; DESCRIPTOR_HASH_BUFFER];
    let mut hasher = Sha1::new();
    while remaining != 0 {
        let read_length = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "descriptor length exceeds address space".to_owned())?;
        file.read_exact(&mut buffer[..read_length])
            .map_err(|error| format!("read descriptor for verification: {error}"))?;
        hasher.update(&buffer[..read_length]);
        remaining -= read_length as u64;
    }
    let actual = hex(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected_sha1_hex) {
        return Err(format!(
            "descriptor SHA-1 {actual} does not match expected {}",
            expected_sha1_hex.to_ascii_lowercase()
        ));
    }
    Ok((length, allocated_bytes(&metadata), actual))
}

#[cfg(unix)]
fn allocated_bytes(metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.blocks().saturating_mul(512)
}

#[cfg(not(unix))]
fn allocated_bytes(_metadata: &std::fs::Metadata) -> u64 {
    0
}

fn validate_config(config: EngineConfig) -> Result<(DownloadConfig, Duration), String> {
    if config.metainfo_path.is_empty() || config.output_path.is_empty() {
        return Err("metainfo and output paths must be nonempty".to_owned());
    }
    if config.peer_port == 0 {
        return Err("peer port must be nonzero".to_owned());
    }
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&config.timeout_seconds) {
        return Err(format!(
            "timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds"
        ));
    }
    if !(MIN_PAYLOAD_BYTES..=MAX_PAYLOAD_BYTES).contains(&config.max_buffered_payload_bytes) {
        return Err(format!(
            "payload allowance must be between {MIN_PAYLOAD_BYTES} and \
             {MAX_PAYLOAD_BYTES} bytes"
        ));
    }
    if config.skip_files.len() > MAX_FILE_SELECTIONS
        || config.materialize_files.len() > MAX_FILE_SELECTIONS
    {
        return Err(format!(
            "file selection lists may contain at most {MAX_FILE_SELECTIONS} entries"
        ));
    }
    if config.storage_write_delay_millis > MAX_STORAGE_WRITE_DELAY_MILLIS {
        return Err(format!(
            "storage write delay may be at most \
             {MAX_STORAGE_WRITE_DELAY_MILLIS} milliseconds"
        ));
    }

    Ok((
        DownloadConfig {
            metainfo_path: PathBuf::from(config.metainfo_path),
            peer: SocketAddr::from((Ipv4Addr::LOCALHOST, config.peer_port)),
            output_path: PathBuf::from(config.output_path),
            network: NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                Duration::from_secs(config.timeout_seconds),
                Duration::from_secs(config.timeout_seconds),
            ),
            resource_limits: DownloadResourceLimits::new(
                config.max_buffered_payload_bytes as usize,
                config.max_buffered_payload_bytes as usize,
                config.max_buffered_payload_bytes as usize,
            ),
            skip_files: config
                .skip_files
                .into_iter()
                .map(|index| index as usize)
                .collect(),
            materialize_files: config
                .materialize_files
                .into_iter()
                .map(|index| index as usize)
                .collect(),
        },
        Duration::from_millis(config.storage_write_delay_millis),
    ))
}

fn run_worker(
    shared: Arc<Shared>,
    config: DownloadConfig,
    descriptors: Option<DescriptorStorage>,
    control: DownloadControl,
    started: Instant,
) {
    let descriptor_backed = descriptors.is_some();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| WorkerFailure::Runtime(format!("failed to build Tokio runtime: {error}")))
        .and_then(|runtime| {
            match descriptors {
                Some(descriptors) => {
                    runtime.block_on(download_verified_piece_to_descriptors_with_control(
                        config,
                        descriptors,
                        control.clone(),
                    ))
                }
                None => runtime.block_on(download_verified_piece_with_control(
                    config,
                    control.clone(),
                )),
            }
            .map_err(WorkerFailure::Engine)
        });

    let terminal = match result {
        Ok(report) => success_result(report, started.elapsed(), descriptor_backed),
        Err(WorkerFailure::Engine(DownloadError::Cancelled)) => TerminalResult {
            outcome: TerminalOutcome::Cancelled,
            failure_kind: None,
            failure_message: None,
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
        Err(WorkerFailure::Engine(error)) => TerminalResult {
            outcome: TerminalOutcome::Failed,
            failure_kind: Some(classify_failure(&error)),
            failure_message: Some(error.to_string()),
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
        Err(WorkerFailure::Runtime(message)) => TerminalResult {
            outcome: TerminalOutcome::Failed,
            failure_kind: Some(FailureKind::Runtime),
            failure_message: Some(message),
            report: None,
            elapsed_millis: millis(started.elapsed()),
        },
    };
    let mut inner = shared
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    inner.state = match terminal.outcome {
        TerminalOutcome::Prepared => SessionState::Prepared,
        TerminalOutcome::Succeeded => SessionState::Succeeded,
        TerminalOutcome::Failed => SessionState::Failed,
        TerminalOutcome::Cancelled => SessionState::Cancelled,
    };
    inner.progress = control.snapshot();
    inner.control = None;
    inner.terminal = Some(terminal);
    drop(inner);
    shared.changed.notify_all();
}

#[derive(Debug)]
enum WorkerFailure {
    Engine(DownloadError),
    Runtime(String),
}

fn classify_failure(error: &DownloadError) -> FailureKind {
    if error.is_existing_artifact() {
        return FailureKind::PreexistingArtifact;
    }
    match error {
        DownloadError::PeerClosed
        | DownloadError::Handshake(_)
        | DownloadError::Frame(_)
        | DownloadError::PeerRegistry(_)
        | DownloadError::PeerTask(_)
        | DownloadError::UdpTracker(_)
        | DownloadError::NoUsablePeer
        | DownloadError::NoUsableTrackerAddress
        | DownloadError::UdpTrackerResponseTooLarge { .. } => FailureKind::Peer,
        DownloadError::Io { operation, .. }
            if operation.contains("peer") || operation.contains("UDP tracker") =>
        {
            FailureKind::Peer
        }
        DownloadError::PeerTimedOut { .. }
        | DownloadError::NetworkTimedOut { .. }
        | DownloadError::UdpTrackerTimedOut { .. }
        | DownloadError::Dht(rstorrent_engine::dht::DhtError::LookupTimedOut) => {
            FailureKind::Timeout
        }
        DownloadError::NetworkDisabled
        | DownloadError::NetworkPolicyDenied { .. }
        | DownloadError::InvalidNetworkTimeout { .. }
        | DownloadError::InvalidResourceLimit(_)
        | DownloadError::Dht(rstorrent_engine::dht::DhtError::NetworkDisabled)
        | DownloadError::MetainfoTooLarge { .. }
        | DownloadError::Magnet(_)
        | DownloadError::Metainfo(_)
        | DownloadError::Layout(_) => FailureKind::Configuration,
        DownloadError::Metadata(_)
        | DownloadError::Pex(_)
        | DownloadError::ExtensionProtocolUnsupported
        | DownloadError::MetadataExtensionDisabled
        | DownloadError::InvalidPremetadataState(_)
        | DownloadError::Piece(_) => FailureKind::Protocol,
        DownloadError::SelectiveStorage(_) | DownloadError::Io { .. } => FailureKind::Storage,
        DownloadError::CleanupAfterFailure { .. } | DownloadError::PeerCleanup { .. } => {
            FailureKind::Cleanup
        }
        DownloadError::Entropy(_)
        | DownloadError::Dht(_)
        | DownloadError::Checkpoint(_)
        | DownloadError::PeerRuntime(_)
        | DownloadError::Swarm(_)
        | DownloadError::StorageTask(_)
        | DownloadError::TrackerTask(_)
        | DownloadError::Cancelled => FailureKind::Runtime,
    }
}

fn success_result(
    report: DownloadReport,
    elapsed: Duration,
    descriptor_backed: bool,
) -> TerminalResult {
    TerminalResult {
        outcome: if descriptor_backed {
            TerminalOutcome::Prepared
        } else {
            TerminalOutcome::Succeeded
        },
        failure_kind: None,
        failure_message: None,
        report: Some(EngineReport {
            info_hash_hex: hex(&report.info_hash),
            final_piece_hash_hex: hex(&report.piece_hash),
            bytes_written: report.bytes_written as u64,
            block_count: report.block_count as u64,
            payload_limit: report.payload_limit as u64,
            payload_high_water: report.payload_high_water as u64,
            outstanding_request_limit: report.outstanding_request_limit as u64,
            outstanding_request_high_water: report.outstanding_request_high_water as u64,
            active_piece_limit: report.active_piece_limit as u64,
            verification_buffer: report.verification_buffer as u64,
            piece_count: report.piece_count as u64,
            verified_piece_count: report.verified_piece_count as u64,
            skipped_piece_count: report.skipped_piece_count as u64,
            selected_file_bytes: report.selected_file_bytes,
            skipped_file_bytes: report.skipped_file_bytes,
            padding_bytes: report.padding_bytes,
            selected_written_bytes: report.selected_written_bytes as u64,
            part_written_bytes: report.part_written_bytes as u64,
            materialized_bytes: report.materialized_bytes,
            part_slots_before_materialization: report.part_slots_before_materialization as u64,
            part_slots_after_materialization: report.part_slots_after_materialization as u64,
            part_reopened: report.part_reopened,
            part_path: report.part_path.map(|path| path.display().to_string()),
            prepared_files: report
                .prepared_files
                .into_iter()
                .map(|file| PreparedFile {
                    file_index: file.file_index as u32,
                    length: file.length,
                    sha1_hex: hex(&file.sha1),
                })
                .collect(),
        }),
        elapsed_millis: millis(elapsed),
    }
}

fn runtime_failure(message: &str) -> TerminalResult {
    TerminalResult {
        outcome: TerminalOutcome::Failed,
        failure_kind: Some(FailureKind::Runtime),
        failure_message: Some(message.to_owned()),
        report: None,
        elapsed_millis: 0,
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn join_timeout(timeout_millis: u64) -> Duration {
    Duration::from_millis(timeout_millis.min(MAX_JOIN_MILLIS))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;

    use super::*;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-android-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn config(metainfo_path: String, output_path: String, peer_port: u16) -> EngineConfig {
        EngineConfig {
            metainfo_path,
            output_path,
            peer_port,
            timeout_seconds: 30,
            max_buffered_payload_bytes: 32 * 1024,
            storage_write_delay_millis: 0,
            skip_files: Vec::new(),
            materialize_files: Vec::new(),
        }
    }

    #[test]
    fn product_application_policy_maps_online_explicitly() {
        let root = test_path("application-network");
        let config = validate_application_config(AndroidApplicationConfig {
            profile_root: root.join("profile").display().to_string(),
            profile_id: "test".to_owned(),
            storage_root: String::new(),
            platform_storage: true,
            network_policy: AndroidNetworkPolicy::Online,
            peer_connect_timeout_seconds: 15,
            peer_io_timeout_seconds: 60,
        })
        .expect("valid Android application config");

        assert_eq!(config.network.policy, NetworkPolicy::Online);
        assert_eq!(
            config.initial_client_settings,
            rstorrent_session::ClientSettings::fresh_profile_default()
        );
        assert_eq!(config.network.peer_connect_timeout, Duration::from_secs(15));
        assert_eq!(config.network.peer_io_timeout, Duration::from_secs(60));
        assert_eq!(
            config.download_resource_limits,
            DownloadResourceLimits::ANDROID
        );
        assert_eq!(config.active_download_cap, Some(2));
    }

    fn two_file_metainfo() -> Vec<u8> {
        let mut metainfo = b"d4:infod5:filesld6:lengthi1e4:pathl1:aee\
d6:lengthi32768e4:pathl1:beee4:name7:fixture12:piece lengthi32768e\
6:pieces40:"
            .to_vec();
        metainfo.extend_from_slice(&[1; 40]);
        metainfo.extend_from_slice(b"ee");
        metainfo
    }

    #[test]
    fn plans_exact_saf_roles_and_materialization() {
        let plan = saf_storage_plan(two_file_metainfo(), vec![1], vec![1]);
        assert!(plan.valid, "{:?}", plan.message);
        assert_eq!(plan.name, "fixture");
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.files[0].role, SafFileRole::Wanted);
        assert!(!plan.files[0].materialize);
        assert_eq!(plan.files[1].role, SafFileRole::Skipped);
        assert!(plan.files[1].materialize);

        let duplicate = saf_storage_plan(two_file_metainfo(), vec![1], vec![1, 1]);
        assert!(!duplicate.valid);
        assert!(
            duplicate
                .message
                .expect("invalid plan message")
                .contains("duplicated")
        );
    }

    #[cfg(unix)]
    #[test]
    fn duplicates_borrowed_descriptors_and_hashes_with_a_fixed_buffer() {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rstorrent-android-descriptor-test-{}-{sequence}",
            std::process::id()
        ));
        let mut caller = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create descriptor fixture");
        caller
            .write_all(b"descriptor bytes")
            .expect("write fixture");
        caller.sync_all().expect("sync fixture");
        let mut owned = duplicate_descriptor(caller.as_raw_fd()).expect("duplicate descriptor");
        drop(caller);
        owned
            .seek(SeekFrom::Start(0))
            .expect("seek owned descriptor");
        let mut bytes = Vec::new();
        owned
            .read_to_end(&mut bytes)
            .expect("read owned descriptor");
        assert_eq!(bytes, b"descriptor bytes");
        let expected = hex(&Sha1::digest(&bytes));
        let inspection =
            inspect_borrowed_descriptor(owned.as_raw_fd(), bytes.len() as u64, expected.clone());
        assert!(inspection.success, "{:?}", inspection.message);
        assert_eq!(inspection.sha1_hex, expected);
        drop(owned);
        fs::remove_file(path).expect("remove descriptor fixture");
    }

    #[test]
    fn rejects_invalid_saf_descriptors_without_starting() {
        let session = EngineSession::new();
        let result = session.start_saf(
            config("/does/not/matter".to_owned(), "saf".to_owned(), 1),
            SafStorage {
                wanted_files: Vec::new(),
                part_fd: -1,
                reopened_part_fd: -1,
                materialization_files: Vec::new(),
            },
        );
        assert_eq!(result.disposition, StartDisposition::Rejected);
        assert_eq!(session.snapshot().state, SessionState::Idle);
        assert!(!session.snapshot().task_alive);
    }

    #[test]
    fn rejects_unbounded_configuration_before_starting() {
        let session = EngineSession::new();
        let mut invalid = config(
            "/does/not/matter".to_owned(),
            "/does/not/matter".to_owned(),
            1,
        );
        invalid.max_buffered_payload_bytes = MAX_PAYLOAD_BYTES + 1;
        let result = session.start(invalid);
        assert_eq!(result.disposition, StartDisposition::Rejected);
        assert_eq!(session.snapshot().state, SessionState::Idle);
        assert!(!session.snapshot().task_alive);
    }

    #[test]
    fn rejects_duplicate_start_and_joins_cancellation() {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rstorrent-android-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create test root");
        let metainfo_path = root.join("fixture.torrent");
        let output_path = root.join("output.bin");
        let mut metainfo =
            b"d4:infod6:lengthi1e4:name1:x12:piece lengthi16384e6:pieces20:".to_vec();
        metainfo.extend_from_slice(&[1; 20]);
        metainfo.extend_from_slice(b"ee");
        fs::write(&metainfo_path, metainfo).expect("write metainfo");

        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind controlled peer");
        let peer_port = listener.local_addr().expect("peer address").port();
        let (accepted_sender, accepted_receiver) = mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept engine");
            accepted_sender.send(()).expect("signal accept");
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).expect("read until cancel");
        });

        let config = config(
            metainfo_path.display().to_string(),
            output_path.display().to_string(),
            peer_port,
        );
        let session = EngineSession::new();
        let first = session.start(config.clone());
        assert_eq!(first.disposition, StartDisposition::Started);
        accepted_receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("engine connected");
        let second = session.start(config);
        assert_eq!(second.disposition, StartDisposition::Busy);

        let joined = session.cancel_and_join(5_000);
        assert!(joined.joined);
        assert_eq!(
            joined.terminal.expect("terminal result").outcome,
            TerminalOutcome::Cancelled
        );
        let snapshot = session.snapshot();
        assert_eq!(snapshot.state, SessionState::Cancelled);
        assert!(!snapshot.task_alive);
        assert_eq!(snapshot.buffered_payload_bytes, 0);
        assert_eq!(snapshot.requested_bytes, 0);
        assert_eq!(snapshot.received_bytes, 0);
        assert_eq!(snapshot.stored_bytes, 0);

        let repeated = session.cancel_and_join(1);
        assert!(repeated.joined);
        assert_eq!(
            repeated.terminal.expect("terminal result").outcome,
            TerminalOutcome::Cancelled
        );
        peer.join().expect("peer thread");
        assert!(!output_path.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }
}

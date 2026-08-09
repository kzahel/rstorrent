use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::time::timeout;

pub const DEFAULT_STORAGE_FILE_LIMIT: usize = 40;
pub const PLATFORM_STORAGE_REQUEST_CAPACITY: usize = 16;
pub const PLATFORM_STORAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_STORAGE_OBSERVATION_TOKEN_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct StorageFileKey {
    pub storage_id: String,
    pub namespace_generation: u64,
    pub role: StorageFileRole,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageFileRole {
    Namespace,
    Payload(usize),
    Part,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageFileAccess {
    ReadExisting,
    ReadWriteExisting,
    ReadWriteCreate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageObjectKind {
    File,
    Directory,
    Other,
}

/// Backend-neutral evidence about one exact logical storage artifact.
///
/// This value can disqualify a later trusting decision, but cannot establish
/// payload validity by itself. Unsupported tokens remain `None`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageObservation {
    pub exists: bool,
    pub kind: Option<StorageObjectKind>,
    pub length: Option<u64>,
    pub opaque_token: Option<String>,
}

impl StorageObservation {
    pub fn missing() -> Self {
        Self {
            exists: false,
            kind: None,
            length: None,
            opaque_token: None,
        }
    }

    pub fn present(
        kind: StorageObjectKind,
        length: Option<u64>,
        opaque_token: Option<String>,
    ) -> Result<Self, StorageFilePoolError> {
        let observation = Self {
            exists: true,
            kind: Some(kind),
            length,
            opaque_token,
        };
        observation.validate()?;
        Ok(observation)
    }

    pub fn validate(&self) -> Result<(), StorageFilePoolError> {
        if self.exists != self.kind.is_some() {
            return Err(StorageFilePoolError::InvalidObservation(
                "existence and object kind disagree",
            ));
        }
        if !self.exists && (self.length.is_some() || self.opaque_token.is_some()) {
            return Err(StorageFilePoolError::InvalidObservation(
                "missing artifact contains present-only fields",
            ));
        }
        if self
            .kind
            .is_some_and(|kind| kind != StorageObjectKind::File)
            && self.length.is_some()
        {
            return Err(StorageFilePoolError::InvalidObservation(
                "non-file artifact contains a file length",
            ));
        }
        if self
            .opaque_token
            .as_ref()
            .is_some_and(|token| token.len() > MAX_STORAGE_OBSERVATION_TOKEN_BYTES)
        {
            return Err(StorageFilePoolError::InvalidObservation(
                "opaque storage observation token exceeds 256 bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformStorageOperation {
    Open,
    Observe,
    Delete,
}

impl StorageFileAccess {
    fn writable(self) -> bool {
        !matches!(self, Self::ReadExisting)
    }

    fn creates(self) -> bool {
        matches!(self, Self::ReadWriteCreate)
    }

    fn satisfies(self, requested: Self) -> bool {
        self.writable() || !requested.writable()
    }
}

#[derive(Clone, Debug)]
pub enum StorageFileLocator {
    Path(PathBuf),
    Platform(PlatformStorageTarget),
}

#[derive(Clone, Debug)]
pub struct PlatformStorageTarget {
    pub root_id: String,
    pub storage_id: String,
    pub namespace_generation: u64,
    pub role: StorageFileRole,
    pub path: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StorageFileReference {
    pool: StorageFilePool,
    key: StorageFileKey,
    locator: StorageFileLocator,
}

impl StorageFileReference {
    pub fn new(pool: StorageFilePool, key: StorageFileKey, locator: StorageFileLocator) -> Self {
        Self { pool, key, locator }
    }

    pub fn key(&self) -> &StorageFileKey {
        &self.key
    }

    pub fn pool(&self) -> &StorageFilePool {
        &self.pool
    }

    pub async fn open(
        &self,
        access: StorageFileAccess,
    ) -> Result<Arc<StorageFileHandle>, StorageFilePoolError> {
        self.pool.open(self, access).await
    }

    pub async fn delete(&self) -> Result<(), StorageFilePoolError> {
        self.pool.invalidate_key(&self.key);
        match &self.locator {
            StorageFileLocator::Path(path) => match tokio::fs::remove_file(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(StorageFilePoolError::Io {
                    operation: "delete storage file",
                    source,
                }),
            },
            StorageFileLocator::Platform(target) => {
                self.pool
                    .inner
                    .platform
                    .as_ref()
                    .ok_or(StorageFilePoolError::PlatformUnavailable)?
                    .delete(target)
                    .await
            }
        }
    }

    pub async fn observe(&self) -> Result<StorageObservation, StorageFilePoolError> {
        self.pool.observe(self).await
    }
}

#[derive(Debug)]
pub struct StorageFileHandle {
    file: std::fs::File,
    _permit: OwnedSemaphorePermit,
}

impl StorageFileHandle {
    pub fn file(&self) -> &std::fs::File {
        &self.file
    }
}

#[derive(Clone, Debug)]
pub enum StorageFileLease {
    Pooled(Arc<StorageFileHandle>),
    Fixed(Arc<std::fs::File>),
}

impl StorageFileLease {
    pub fn fixed(file: std::fs::File) -> Self {
        Self::Fixed(Arc::new(file))
    }

    pub fn file(&self) -> &std::fs::File {
        match self {
            Self::Pooled(handle) => handle.file(),
            Self::Fixed(file) => file,
        }
    }
}

impl From<Arc<StorageFileHandle>> for StorageFileLease {
    fn from(handle: Arc<StorageFileHandle>) -> Self {
        Self::Pooled(handle)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct StorageFilePoolSnapshot {
    pub limit: usize,
    pub current_owned: usize,
    pub owned_high_water: usize,
    pub cached_entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub singleflight_waits: u64,
    pub mode_upgrades: u64,
    pub open_failures: u64,
    pub resource_retries: u64,
    pub platform_pending: usize,
    pub platform_pending_high_water: usize,
}

#[derive(Debug)]
pub enum StorageFilePoolError {
    Closed,
    Invalidated,
    InvalidPath,
    PlatformUnavailable,
    PlatformFailure(PlatformStorageFailure),
    InvalidObservation(&'static str),
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for StorageFilePoolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("storage file pool is closed"),
            Self::Invalidated => formatter.write_str("storage file acquisition was invalidated"),
            Self::InvalidPath => formatter.write_str("storage file path has no parent"),
            Self::PlatformUnavailable => {
                formatter.write_str("platform storage broker is unavailable")
            }
            Self::PlatformFailure(error) => write!(formatter, "platform storage: {error}"),
            Self::InvalidObservation(detail) => {
                write!(formatter, "invalid storage observation: {detail}")
            }
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl Error for StorageFilePoolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PlatformFailure(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl StorageFilePoolError {
    pub fn platform_failure_kind(&self) -> Option<PlatformStorageFailureKind> {
        match self {
            Self::PlatformUnavailable => Some(PlatformStorageFailureKind::Cancelled),
            Self::PlatformFailure(failure) => Some(failure.kind),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformStorageFailureKind {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformStorageFailure {
    pub kind: PlatformStorageFailureKind,
    pub detail: String,
}

impl PlatformStorageFailure {
    pub fn new(kind: PlatformStorageFailureKind, detail: impl Into<String>) -> Self {
        let mut detail = detail.into();
        detail.truncate(1024);
        Self { kind, detail }
    }
}

impl fmt::Display for PlatformStorageFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.detail)
    }
}

impl Error for PlatformStorageFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformStorageRequest {
    pub request_id: u64,
    pub root_id: String,
    pub storage_id: String,
    pub namespace_generation: u64,
    pub role: StorageFileRole,
    pub path: Vec<String>,
    pub operation: PlatformStorageOperation,
    pub access: StorageFileAccess,
    pub timeout_millis: u64,
}

#[derive(Debug)]
enum PlatformStorageResponse {
    File(std::fs::File),
    Observation(StorageObservation),
    Deleted,
}

struct PendingPlatformStorageRequest {
    request: PlatformStorageRequest,
    reply: oneshot::Sender<Result<PlatformStorageResponse, PlatformStorageFailure>>,
}

#[derive(Clone, Debug)]
pub struct PlatformStorageClient {
    sender: mpsc::Sender<PendingPlatformStorageRequest>,
    next_request_id: Arc<AtomicU64>,
    pending: Arc<AtomicUsize>,
    pending_high_water: Arc<AtomicUsize>,
    root_failures: Arc<Mutex<HashMap<String, PlatformStorageFailure>>>,
    health_wake: Arc<Mutex<Option<Arc<Notify>>>>,
}

impl PlatformStorageClient {
    async fn open(
        &self,
        target: &PlatformStorageTarget,
        access: StorageFileAccess,
    ) -> Result<std::fs::File, StorageFilePoolError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = PlatformStorageRequest {
            request_id,
            root_id: target.root_id.clone(),
            storage_id: target.storage_id.clone(),
            namespace_generation: target.namespace_generation,
            role: target.role,
            path: target.path.clone(),
            operation: PlatformStorageOperation::Open,
            access,
            timeout_millis: u64::try_from(PLATFORM_STORAGE_REQUEST_TIMEOUT.as_millis())
                .expect("platform timeout fits u64 milliseconds"),
        };
        let (reply, response) = oneshot::channel();
        self.sender
            .send(PendingPlatformStorageRequest { request, reply })
            .await
            .map_err(|_| StorageFilePoolError::PlatformUnavailable)?;
        let pending = self.pending.fetch_add(1, Ordering::AcqRel) + 1;
        update_high_water(&self.pending_high_water, pending);
        let result = timeout(PLATFORM_STORAGE_REQUEST_TIMEOUT, response).await;
        self.pending.fetch_sub(1, Ordering::AcqRel);
        match result {
            Ok(Ok(Ok(PlatformStorageResponse::File(file)))) => Ok(file),
            Ok(Ok(Ok(PlatformStorageResponse::Deleted))) => Err(
                StorageFilePoolError::PlatformFailure(PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned deletion for an open request",
                )),
            ),
            Ok(Ok(Ok(PlatformStorageResponse::Observation(_)))) => Err(
                StorageFilePoolError::PlatformFailure(PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned an observation for an open request",
                )),
            ),
            Ok(Ok(Err(error))) => {
                self.record_root_failure(&target.root_id, &error);
                Err(StorageFilePoolError::PlatformFailure(error))
            }
            Ok(Err(_)) => Err(StorageFilePoolError::PlatformUnavailable),
            Err(_) => Err(StorageFilePoolError::PlatformFailure(
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::DeadlineExceeded,
                    "platform storage request exceeded its deadline",
                ),
            )),
        }
    }

    async fn observe(
        &self,
        target: &PlatformStorageTarget,
    ) -> Result<StorageObservation, StorageFilePoolError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = PlatformStorageRequest {
            request_id,
            root_id: target.root_id.clone(),
            storage_id: target.storage_id.clone(),
            namespace_generation: target.namespace_generation,
            role: target.role,
            path: target.path.clone(),
            operation: PlatformStorageOperation::Observe,
            access: StorageFileAccess::ReadExisting,
            timeout_millis: u64::try_from(PLATFORM_STORAGE_REQUEST_TIMEOUT.as_millis())
                .expect("platform timeout fits u64 milliseconds"),
        };
        let response = self.request(request).await?;
        match response {
            PlatformStorageResponse::Observation(observation) => Ok(observation),
            PlatformStorageResponse::File(_) => Err(StorageFilePoolError::PlatformFailure(
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned a file for an observation request",
                ),
            )),
            PlatformStorageResponse::Deleted => Err(StorageFilePoolError::PlatformFailure(
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned deletion for an observation request",
                ),
            )),
        }
    }

    async fn delete(&self, target: &PlatformStorageTarget) -> Result<(), StorageFilePoolError> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let request = PlatformStorageRequest {
            request_id,
            root_id: target.root_id.clone(),
            storage_id: target.storage_id.clone(),
            namespace_generation: target.namespace_generation,
            role: target.role,
            path: target.path.clone(),
            operation: PlatformStorageOperation::Delete,
            access: StorageFileAccess::ReadWriteExisting,
            timeout_millis: u64::try_from(PLATFORM_STORAGE_REQUEST_TIMEOUT.as_millis())
                .expect("platform timeout fits u64 milliseconds"),
        };
        let (reply, response) = oneshot::channel();
        self.sender
            .send(PendingPlatformStorageRequest { request, reply })
            .await
            .map_err(|_| StorageFilePoolError::PlatformUnavailable)?;
        let pending = self.pending.fetch_add(1, Ordering::AcqRel) + 1;
        update_high_water(&self.pending_high_water, pending);
        let result = timeout(PLATFORM_STORAGE_REQUEST_TIMEOUT, response).await;
        self.pending.fetch_sub(1, Ordering::AcqRel);
        match result {
            Ok(Ok(Ok(PlatformStorageResponse::Deleted))) => Ok(()),
            Ok(Ok(Ok(PlatformStorageResponse::File(_)))) => Err(
                StorageFilePoolError::PlatformFailure(PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned a file for a deletion request",
                )),
            ),
            Ok(Ok(Ok(PlatformStorageResponse::Observation(_)))) => Err(
                StorageFilePoolError::PlatformFailure(PlatformStorageFailure::new(
                    PlatformStorageFailureKind::Internal,
                    "platform returned an observation for a deletion request",
                )),
            ),
            Ok(Ok(Err(error))) => {
                self.record_root_failure(&target.root_id, &error);
                Err(StorageFilePoolError::PlatformFailure(error))
            }
            Ok(Err(_)) => Err(StorageFilePoolError::PlatformUnavailable),
            Err(_) => Err(StorageFilePoolError::PlatformFailure(
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::DeadlineExceeded,
                    "platform storage deletion exceeded its deadline",
                ),
            )),
        }
    }

    async fn request(
        &self,
        request: PlatformStorageRequest,
    ) -> Result<PlatformStorageResponse, StorageFilePoolError> {
        let root_id = request.root_id.clone();
        let (reply, response) = oneshot::channel();
        self.sender
            .send(PendingPlatformStorageRequest { request, reply })
            .await
            .map_err(|_| StorageFilePoolError::PlatformUnavailable)?;
        let pending = self.pending.fetch_add(1, Ordering::AcqRel) + 1;
        update_high_water(&self.pending_high_water, pending);
        let result = timeout(PLATFORM_STORAGE_REQUEST_TIMEOUT, response).await;
        self.pending.fetch_sub(1, Ordering::AcqRel);
        match result {
            Ok(Ok(Ok(response))) => Ok(response),
            Ok(Ok(Err(error))) => {
                self.record_root_failure(&root_id, &error);
                Err(StorageFilePoolError::PlatformFailure(error))
            }
            Ok(Err(_)) => Err(StorageFilePoolError::PlatformUnavailable),
            Err(_) => Err(StorageFilePoolError::PlatformFailure(
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::DeadlineExceeded,
                    "platform storage request exceeded its deadline",
                ),
            )),
        }
    }

    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::Acquire)
    }

    pub fn pending_high_water(&self) -> usize {
        self.pending_high_water.load(Ordering::Acquire)
    }

    fn record_root_failure(&self, root_id: &str, failure: &PlatformStorageFailure) {
        if !matches!(
            failure.kind,
            PlatformStorageFailureKind::GrantUnavailable
                | PlatformStorageFailureKind::PermissionDenied
                | PlatformStorageFailureKind::ProviderRefused
                | PlatformStorageFailureKind::NonSeekable
        ) {
            return;
        }
        self.root_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(root_id.to_owned(), failure.clone());
        if let Some(wake) = self
            .health_wake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            wake.notify_one();
        }
    }

    fn take_root_failures(&self) -> Vec<(String, PlatformStorageFailure)> {
        std::mem::take(
            &mut *self
                .root_failures
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
        .into_iter()
        .collect()
    }

    fn set_health_wake(&self, wake: Arc<Notify>) {
        *self
            .health_wake
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(wake);
    }
}

#[derive(Debug)]
pub struct PlatformStorageBroker {
    receiver: AsyncMutex<mpsc::Receiver<PendingPlatformStorageRequest>>,
    pending: Mutex<
        HashMap<u64, oneshot::Sender<Result<PlatformStorageResponse, PlatformStorageFailure>>>,
    >,
}

impl PlatformStorageBroker {
    pub async fn next_request(&self) -> Option<PlatformStorageRequest> {
        loop {
            let pending = self.receiver.lock().await.recv().await?;
            if pending.reply.is_closed() {
                continue;
            }
            self.pending_guard()
                .insert(pending.request.request_id, pending.reply);
            return Some(pending.request);
        }
    }

    pub fn complete_file(&self, request_id: u64, file: std::fs::File) -> bool {
        self.pending_guard()
            .remove(&request_id)
            .is_some_and(|reply| reply.send(Ok(PlatformStorageResponse::File(file))).is_ok())
    }

    pub fn is_pending(&self, request_id: u64) -> bool {
        self.pending_guard().contains_key(&request_id)
    }

    pub fn complete_deleted(&self, request_id: u64) -> bool {
        self.pending_guard()
            .remove(&request_id)
            .is_some_and(|reply| reply.send(Ok(PlatformStorageResponse::Deleted)).is_ok())
    }

    pub fn complete_observation(&self, request_id: u64, observation: StorageObservation) -> bool {
        if let Err(error) = observation.validate() {
            return self.complete_error(
                request_id,
                PlatformStorageFailure::new(
                    PlatformStorageFailureKind::ProviderRefused,
                    error.to_string(),
                ),
            );
        }
        self.pending_guard()
            .remove(&request_id)
            .is_some_and(|reply| {
                reply
                    .send(Ok(PlatformStorageResponse::Observation(observation)))
                    .is_ok()
            })
    }

    pub fn complete_error(&self, request_id: u64, failure: PlatformStorageFailure) -> bool {
        self.pending_guard()
            .remove(&request_id)
            .is_some_and(|reply| reply.send(Err(failure)).is_ok())
    }

    pub fn cancel_all(&self) {
        let pending = std::mem::take(&mut *self.pending_guard());
        for (_, reply) in pending {
            let _ = reply.send(Err(PlatformStorageFailure::new(
                PlatformStorageFailureKind::Cancelled,
                "platform storage broker stopped",
            )));
        }
    }

    fn pending_guard(
        &self,
    ) -> MutexGuard<
        '_,
        HashMap<u64, oneshot::Sender<Result<PlatformStorageResponse, PlatformStorageFailure>>>,
    > {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn platform_storage_channel() -> (PlatformStorageClient, Arc<PlatformStorageBroker>) {
    let (sender, receiver) = mpsc::channel(PLATFORM_STORAGE_REQUEST_CAPACITY);
    (
        PlatformStorageClient {
            sender,
            next_request_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(AtomicUsize::new(0)),
            pending_high_water: Arc::new(AtomicUsize::new(0)),
            root_failures: Arc::new(Mutex::new(HashMap::new())),
            health_wake: Arc::new(Mutex::new(None)),
        },
        Arc::new(PlatformStorageBroker {
            receiver: AsyncMutex::new(receiver),
            pending: Mutex::new(HashMap::new()),
        }),
    )
}

#[derive(Debug)]
struct PoolEntry {
    access: StorageFileAccess,
    last_used: u64,
    handle: Arc<StorageFileHandle>,
}

#[derive(Debug, Default)]
struct PoolState {
    entries: HashMap<StorageFileKey, PoolEntry>,
    key_locks: HashMap<StorageFileKey, Weak<AsyncMutex<()>>>,
    storage_versions: HashMap<String, u64>,
    clock: u64,
    closed: bool,
}

#[derive(Debug)]
struct PoolMetrics {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    singleflight_waits: AtomicU64,
    mode_upgrades: AtomicU64,
    open_failures: AtomicU64,
    resource_retries: AtomicU64,
    owned_high_water: AtomicUsize,
}

impl Default for PoolMetrics {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            singleflight_waits: AtomicU64::new(0),
            mode_upgrades: AtomicU64::new(0),
            open_failures: AtomicU64::new(0),
            resource_retries: AtomicU64::new(0),
            owned_high_water: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug)]
struct StorageFilePoolInner {
    limit: usize,
    permits: Arc<Semaphore>,
    state: Mutex<PoolState>,
    platform: Option<PlatformStorageClient>,
    metrics: PoolMetrics,
}

#[derive(Clone, Debug)]
pub struct StorageFilePool {
    inner: Arc<StorageFilePoolInner>,
}

impl StorageFilePool {
    pub fn new(
        limit: usize,
        platform: Option<PlatformStorageClient>,
    ) -> Result<Self, &'static str> {
        if limit == 0 {
            return Err("storage file limit must be nonzero");
        }
        Ok(Self {
            inner: Arc::new(StorageFilePoolInner {
                limit,
                permits: Arc::new(Semaphore::new(limit)),
                state: Mutex::new(PoolState::default()),
                platform,
                metrics: PoolMetrics::default(),
            }),
        })
    }

    async fn open(
        &self,
        reference: &StorageFileReference,
        access: StorageFileAccess,
    ) -> Result<Arc<StorageFileHandle>, StorageFilePoolError> {
        if let Some(handle) = self.cached(&reference.key, access)? {
            self.inner.metrics.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(handle);
        }
        self.inner.metrics.misses.fetch_add(1, Ordering::Relaxed);
        let key_lock = self.key_lock(&reference.key)?;
        if key_lock.try_lock().is_err() {
            self.inner
                .metrics
                .singleflight_waits
                .fetch_add(1, Ordering::Relaxed);
        }
        let _key_guard = key_lock.lock().await;
        if let Some(handle) = self.cached(&reference.key, access)? {
            self.inner.metrics.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(handle);
        }

        let removed = {
            let mut state = self.state_guard();
            if state.closed {
                return Err(StorageFilePoolError::Closed);
            }
            match state.entries.get(&reference.key) {
                Some(entry) if !entry.access.satisfies(access) => {
                    self.inner
                        .metrics
                        .mode_upgrades
                        .fetch_add(1, Ordering::Relaxed);
                    state.entries.remove(&reference.key)
                }
                _ => None,
            }
        };
        drop(removed);

        let storage_version = self.storage_version(&reference.key.storage_id)?;

        let permit = self.acquire_permit().await?;
        let file = match self.acquire_file(&reference.locator, access).await {
            Ok(file) => file,
            Err(first) if is_descriptor_exhaustion(&first) => {
                self.inner
                    .metrics
                    .resource_retries
                    .fetch_add(1, Ordering::Relaxed);
                self.evict_one();
                self.acquire_file(&reference.locator, access).await?
            }
            Err(error) => {
                self.inner
                    .metrics
                    .open_failures
                    .fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        let handle = Arc::new(StorageFileHandle {
            file,
            _permit: permit,
        });
        self.observe_owned_high_water();
        let replaced = {
            let mut state = self.state_guard();
            if state.closed {
                return Err(StorageFilePoolError::Closed);
            }
            if state
                .storage_versions
                .get(&reference.key.storage_id)
                .copied()
                .unwrap_or_default()
                != storage_version
            {
                return Err(StorageFilePoolError::Invalidated);
            }
            state.clock = state.clock.wrapping_add(1);
            let last_used = state.clock;
            state.entries.insert(
                reference.key.clone(),
                PoolEntry {
                    access,
                    last_used,
                    handle: handle.clone(),
                },
            )
        };
        drop(replaced);
        Ok(handle)
    }

    async fn observe(
        &self,
        reference: &StorageFileReference,
    ) -> Result<StorageObservation, StorageFilePoolError> {
        let storage_version = self.storage_version(&reference.key.storage_id)?;
        let observation = match &reference.locator {
            StorageFileLocator::Path(path) => observe_path(path.clone()).await?,
            StorageFileLocator::Platform(target) => {
                self.inner
                    .platform
                    .as_ref()
                    .ok_or(StorageFilePoolError::PlatformUnavailable)?
                    .observe(target)
                    .await?
            }
        };
        let current_version = self.storage_version(&reference.key.storage_id)?;
        if current_version != storage_version {
            return Err(StorageFilePoolError::Invalidated);
        }
        Ok(observation)
    }

    pub fn snapshot(&self) -> StorageFilePoolSnapshot {
        let state = self.state_guard();
        let platform_pending = self
            .inner
            .platform
            .as_ref()
            .map_or(0, PlatformStorageClient::pending);
        let platform_pending_high_water = self
            .inner
            .platform
            .as_ref()
            .map_or(0, PlatformStorageClient::pending_high_water);
        StorageFilePoolSnapshot {
            limit: self.inner.limit,
            current_owned: self
                .inner
                .limit
                .saturating_sub(self.inner.permits.available_permits()),
            owned_high_water: self.inner.metrics.owned_high_water.load(Ordering::Acquire),
            cached_entries: state.entries.len(),
            hits: self.inner.metrics.hits.load(Ordering::Relaxed),
            misses: self.inner.metrics.misses.load(Ordering::Relaxed),
            evictions: self.inner.metrics.evictions.load(Ordering::Relaxed),
            singleflight_waits: self
                .inner
                .metrics
                .singleflight_waits
                .load(Ordering::Relaxed),
            mode_upgrades: self.inner.metrics.mode_upgrades.load(Ordering::Relaxed),
            open_failures: self.inner.metrics.open_failures.load(Ordering::Relaxed),
            resource_retries: self.inner.metrics.resource_retries.load(Ordering::Relaxed),
            platform_pending,
            platform_pending_high_water,
        }
    }

    pub fn take_platform_root_failures(&self) -> Vec<(String, PlatformStorageFailure)> {
        self.inner
            .platform
            .as_ref()
            .map_or_else(Vec::new, PlatformStorageClient::take_root_failures)
    }

    pub fn set_platform_health_wake(&self, wake: Arc<Notify>) {
        if let Some(platform) = &self.inner.platform {
            platform.set_health_wake(wake);
        }
    }

    pub fn invalidate_storage(&self, storage_id: &str) {
        let removed = {
            let mut state = self.state_guard();
            let version = state
                .storage_versions
                .entry(storage_id.to_owned())
                .or_default();
            *version = version.wrapping_add(1);
            let keys = state
                .entries
                .keys()
                .filter(|key| key.storage_id == storage_id)
                .cloned()
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| state.entries.remove(&key))
                .collect::<Vec<_>>()
        };
        drop(removed);
    }

    pub fn invalidate_key(&self, key: &StorageFileKey) {
        let removed = {
            let mut state = self.state_guard();
            let version = state
                .storage_versions
                .entry(key.storage_id.clone())
                .or_default();
            *version = version.wrapping_add(1);
            state.entries.remove(key)
        };
        drop(removed);
    }

    pub fn invalidate_all(&self) {
        let removed = {
            let mut state = self.state_guard();
            for version in state.storage_versions.values_mut() {
                *version = version.wrapping_add(1);
            }
            state
                .entries
                .drain()
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        };
        drop(removed);
    }

    fn storage_version(&self, storage_id: &str) -> Result<u64, StorageFilePoolError> {
        let mut state = self.state_guard();
        if state.closed {
            return Err(StorageFilePoolError::Closed);
        }
        Ok(*state
            .storage_versions
            .entry(storage_id.to_owned())
            .or_default())
    }

    pub async fn shutdown(&self) -> Result<(), StorageFilePoolError> {
        let removed = {
            let mut state = self.state_guard();
            state.closed = true;
            state.key_locks.clear();
            state
                .entries
                .drain()
                .map(|(_, entry)| entry)
                .collect::<Vec<_>>()
        };
        drop(removed);
        let permits = self
            .inner
            .permits
            .clone()
            .acquire_many_owned(
                u32::try_from(self.inner.limit).map_err(|_| StorageFilePoolError::Closed)?,
            )
            .await
            .map_err(|_| StorageFilePoolError::Closed)?;
        drop(permits);
        Ok(())
    }

    fn cached(
        &self,
        key: &StorageFileKey,
        access: StorageFileAccess,
    ) -> Result<Option<Arc<StorageFileHandle>>, StorageFilePoolError> {
        let mut state = self.state_guard();
        if state.closed {
            return Err(StorageFilePoolError::Closed);
        }
        state.clock = state.clock.wrapping_add(1);
        let last_used = state.clock;
        Ok(state.entries.get_mut(key).and_then(|entry| {
            entry.access.satisfies(access).then(|| {
                entry.last_used = last_used;
                entry.handle.clone()
            })
        }))
    }

    fn key_lock(&self, key: &StorageFileKey) -> Result<Arc<AsyncMutex<()>>, StorageFilePoolError> {
        let mut state = self.state_guard();
        if state.closed {
            return Err(StorageFilePoolError::Closed);
        }
        if let Some(lock) = state.key_locks.get(key).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(AsyncMutex::new(()));
        state.key_locks.insert(key.clone(), Arc::downgrade(&lock));
        Ok(lock)
    }

    async fn acquire_permit(&self) -> Result<OwnedSemaphorePermit, StorageFilePoolError> {
        loop {
            match self.inner.permits.clone().try_acquire_owned() {
                Ok(permit) => return Ok(permit),
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err(StorageFilePoolError::Closed);
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {
                    if !self.evict_one() {
                        return self
                            .inner
                            .permits
                            .clone()
                            .acquire_owned()
                            .await
                            .map_err(|_| StorageFilePoolError::Closed);
                    }
                }
            }
        }
    }

    fn evict_one(&self) -> bool {
        let removed = {
            let mut state = self.state_guard();
            let key = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            key.and_then(|key| state.entries.remove(&key))
        };
        if removed.is_some() {
            self.inner.metrics.evictions.fetch_add(1, Ordering::Relaxed);
        }
        let evicted = removed.is_some();
        drop(removed);
        evicted
    }

    async fn acquire_file(
        &self,
        locator: &StorageFileLocator,
        access: StorageFileAccess,
    ) -> Result<std::fs::File, StorageFilePoolError> {
        match locator {
            StorageFileLocator::Path(path) => open_path(path.clone(), access).await,
            StorageFileLocator::Platform(target) => {
                let platform = self
                    .inner
                    .platform
                    .as_ref()
                    .ok_or(StorageFilePoolError::PlatformUnavailable)?;
                platform.open(target, access).await
            }
        }
    }

    fn observe_owned_high_water(&self) {
        let current = self
            .inner
            .limit
            .saturating_sub(self.inner.permits.available_permits());
        update_high_water(&self.inner.metrics.owned_high_water, current);
    }

    fn state_guard(&self) -> MutexGuard<'_, PoolState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

async fn open_path(
    path: PathBuf,
    access: StorageFileAccess,
) -> Result<std::fs::File, StorageFilePoolError> {
    tokio::task::spawn_blocking(move || {
        if access.creates() {
            let parent = path.parent().ok_or(StorageFilePoolError::InvalidPath)?;
            std::fs::create_dir_all(parent).map_err(|source| StorageFilePoolError::Io {
                operation: "create storage file parent",
                source,
            })?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(access.writable());
        if access.creates() {
            options.create(true);
        }
        options
            .open(&path)
            .map_err(|source| StorageFilePoolError::Io {
                operation: "open storage file",
                source,
            })
    })
    .await
    .map_err(|source| StorageFilePoolError::Io {
        operation: "join storage file open",
        source: io::Error::other(source),
    })?
}

async fn observe_path(path: PathBuf) -> Result<StorageObservation, StorageFilePoolError> {
    tokio::task::spawn_blocking(move || {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(StorageObservation::missing());
            }
            Err(source) => {
                return Err(StorageFilePoolError::Io {
                    operation: "observe storage artifact",
                    source,
                });
            }
        };
        let kind = if metadata.file_type().is_symlink() {
            StorageObjectKind::Other
        } else if metadata.is_file() {
            StorageObjectKind::File
        } else if metadata.is_dir() {
            StorageObjectKind::Directory
        } else {
            StorageObjectKind::Other
        };
        let length = (kind == StorageObjectKind::File).then_some(metadata.len());
        let opaque_token = metadata.modified().ok().and_then(|modified| {
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|duration| {
                    format!(
                        "mtime-v1:{}:{}",
                        duration.as_secs(),
                        duration.subsec_nanos()
                    )
                })
        });
        StorageObservation::present(kind, length, opaque_token)
    })
    .await
    .map_err(|source| StorageFilePoolError::Io {
        operation: "join storage artifact observation",
        source: io::Error::other(source),
    })?
}

fn is_descriptor_exhaustion(error: &StorageFilePoolError) -> bool {
    let StorageFilePoolError::Io { source, .. } = error else {
        return false;
    };
    matches!(source.raw_os_error(), Some(23 | 24))
}

fn update_high_water(high_water: &AtomicUsize, value: usize) {
    let mut observed = high_water.load(Ordering::Relaxed);
    while value > observed {
        match high_water.compare_exchange_weak(
            observed,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => observed = next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PlatformStorageFailure, PlatformStorageFailureKind, PlatformStorageOperation,
        StorageFileAccess, StorageFileKey, StorageFileLocator, StorageFilePool,
        StorageFileReference, StorageFileRole, StorageObjectKind, StorageObservation,
        platform_storage_channel,
    };
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn temp_root(name: &str) -> PathBuf {
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).expect("random temp suffix");
        std::env::temp_dir().join(format!(
            "rstorrent-pool-{name}-{}",
            u64::from_be_bytes(random)
        ))
    }

    fn reference(
        pool: StorageFilePool,
        root: &std::path::Path,
        index: usize,
    ) -> StorageFileReference {
        StorageFileReference::new(
            pool,
            StorageFileKey {
                storage_id: "test".to_owned(),
                namespace_generation: 0,
                role: StorageFileRole::Payload(index),
            },
            StorageFileLocator::Path(root.join(format!("{index}.bin"))),
        )
    }

    #[tokio::test]
    async fn enforces_actual_handle_limit_across_in_flight_eviction() {
        let root = temp_root("limit");
        let pool = StorageFilePool::new(2, None).expect("pool");
        let first = reference(pool.clone(), &root, 0)
            .open(StorageFileAccess::ReadWriteCreate)
            .await
            .expect("first");
        let second = reference(pool.clone(), &root, 1)
            .open(StorageFileAccess::ReadWriteCreate)
            .await
            .expect("second");
        let third_reference = reference(pool.clone(), &root, 2);
        let third = tokio::spawn(async move {
            third_reference
                .open(StorageFileAccess::ReadWriteCreate)
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(pool.snapshot().current_owned, 2);
        assert!(!third.is_finished());
        drop(first);
        let third = third.await.expect("third task").expect("third handle");
        assert_eq!(pool.snapshot().current_owned, 2);
        assert!(pool.snapshot().evictions >= 2);
        drop(second);
        drop(third);
        pool.shutdown().await.expect("shutdown");
        assert_eq!(pool.snapshot().current_owned, 0);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn compatible_concurrent_opens_singleflight_and_hit() {
        let root = temp_root("singleflight");
        let pool = StorageFilePool::new(2, None).expect("pool");
        let reference = reference(pool.clone(), &root, 0);
        let (first, second) = tokio::join!(
            reference.open(StorageFileAccess::ReadWriteCreate),
            reference.open(StorageFileAccess::ReadWriteCreate),
        );
        let first = first.expect("first");
        let second = second.expect("second");
        assert!(Arc::ptr_eq(&first, &second));
        let snapshot = pool.snapshot();
        assert_eq!(snapshot.current_owned, 1);
        assert_eq!(snapshot.misses, 2);
        assert!(snapshot.hits >= 1);
        drop(first);
        drop(second);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn read_existing_never_creates_and_writable_open_upgrades() {
        let root = temp_root("mode");
        std::fs::create_dir_all(&root).expect("root");
        let pool = StorageFilePool::new(2, None).expect("pool");
        let reference = reference(pool.clone(), &root, 0);
        let missing = reference
            .open(StorageFileAccess::ReadExisting)
            .await
            .expect_err("missing read");
        assert!(missing.to_string().contains("open storage file"));
        assert!(!root.join("0.bin").exists());

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("0.bin"))
            .expect("seed file");
        file.write_all(b"hello").expect("seed bytes");
        drop(file);
        let read = reference
            .open(StorageFileAccess::ReadExisting)
            .await
            .expect("read handle");
        drop(read);
        let writable = reference
            .open(StorageFileAccess::ReadWriteExisting)
            .await
            .expect("writable handle");
        assert_eq!(pool.snapshot().mode_upgrades, 1);
        drop(writable);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn platform_completion_and_late_response_are_bounded() {
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(2, Some(client)).expect("pool");
        let target = super::PlatformStorageTarget {
            root_id: "root".to_owned(),
            storage_id: "torrent".to_owned(),
            namespace_generation: 7,
            role: StorageFileRole::Part,
            path: vec!["part".to_owned()],
        };
        let reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: "torrent".to_owned(),
                namespace_generation: 7,
                role: StorageFileRole::Part,
            },
            StorageFileLocator::Platform(target),
        );
        let open =
            tokio::spawn(async move { reference.open(StorageFileAccess::ReadWriteCreate).await });
        let request = broker.next_request().await.expect("request");
        assert_eq!(request.path, vec!["part"]);
        let root = temp_root("platform");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&root)
            .expect("platform file");
        assert!(broker.complete_file(request.request_id, file));
        let handle = open.await.expect("open task").expect("platform handle");
        assert!(!broker.complete_error(
            request.request_id,
            PlatformStorageFailure::new(PlatformStorageFailureKind::Internal, "late response",),
        ));
        drop(handle);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_file(root).expect("cleanup");
    }

    #[tokio::test]
    async fn invalidation_fences_an_in_flight_platform_completion() {
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(2, Some(client)).expect("pool");
        let reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: "torrent".to_owned(),
                namespace_generation: 0,
                role: StorageFileRole::Part,
            },
            StorageFileLocator::Platform(super::PlatformStorageTarget {
                root_id: "root".to_owned(),
                storage_id: "torrent".to_owned(),
                namespace_generation: 0,
                role: StorageFileRole::Part,
                path: vec!["part".to_owned()],
            }),
        );
        let open =
            tokio::spawn(async move { reference.open(StorageFileAccess::ReadWriteCreate).await });
        let request = broker.next_request().await.expect("request");
        pool.invalidate_storage("torrent");
        let path = temp_root("invalidation");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("platform file");
        assert!(broker.complete_file(request.request_id, file));
        assert!(matches!(
            open.await.expect("open task"),
            Err(super::StorageFilePoolError::Invalidated)
        ));
        assert_eq!(pool.snapshot().cached_entries, 0);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_file(path).expect("cleanup");
    }

    #[tokio::test]
    async fn broker_cancellation_rejects_open_without_materializing_or_leaking() {
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(2, Some(client)).expect("pool");
        let reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: "torrent".to_owned(),
                namespace_generation: 4,
                role: StorageFileRole::Payload(2),
            },
            StorageFileLocator::Platform(super::PlatformStorageTarget {
                root_id: "root".to_owned(),
                storage_id: "torrent".to_owned(),
                namespace_generation: 4,
                role: StorageFileRole::Payload(2),
                path: vec!["published".to_owned(), "payload.bin".to_owned()],
            }),
        );
        let open =
            tokio::spawn(async move { reference.open(StorageFileAccess::ReadExisting).await });
        let request = broker.next_request().await.expect("platform request");
        assert_eq!(request.access, StorageFileAccess::ReadExisting);
        assert_eq!(request.operation, PlatformStorageOperation::Open);
        assert_eq!(
            request.path,
            ["published".to_owned(), "payload.bin".to_owned()]
        );
        assert_eq!(pool.snapshot().platform_pending, 1);

        broker.cancel_all();
        let error = open.await.expect("open task").expect_err("cancelled open");
        assert_eq!(
            error.platform_failure_kind(),
            Some(PlatformStorageFailureKind::Cancelled)
        );
        assert_eq!(pool.snapshot().platform_pending, 0);
        assert_eq!(pool.snapshot().current_owned, 0);
        assert_eq!(pool.snapshot().cached_entries, 0);
        pool.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn path_observation_is_bounded_and_never_creates() {
        let root = temp_root("observe-path");
        std::fs::create_dir_all(&root).expect("root");
        let pool = StorageFilePool::new(2, None).expect("pool");
        let reference = reference(pool.clone(), &root, 0);
        assert_eq!(
            reference.observe().await.expect("missing observation"),
            StorageObservation::missing()
        );
        assert!(!root.join("0.bin").exists());

        std::fs::write(root.join("0.bin"), b"hello").expect("payload");
        let observed = reference.observe().await.expect("file observation");
        assert!(observed.exists);
        assert_eq!(observed.kind, Some(StorageObjectKind::File));
        assert_eq!(observed.length, Some(5));
        assert!(
            observed
                .opaque_token
                .as_ref()
                .is_none_or(|token| token.len() <= super::MAX_STORAGE_OBSERVATION_TOKEN_BYTES)
        );
        assert_eq!(pool.snapshot().current_owned, 0);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[tokio::test]
    async fn platform_observation_uses_typed_response_without_opening_a_handle() {
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(2, Some(client)).expect("pool");
        let reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: "torrent".to_owned(),
                namespace_generation: 7,
                role: StorageFileRole::Payload(1),
            },
            StorageFileLocator::Platform(super::PlatformStorageTarget {
                root_id: "root".to_owned(),
                storage_id: "torrent".to_owned(),
                namespace_generation: 7,
                role: StorageFileRole::Payload(1),
                path: vec!["published".to_owned(), "file.bin".to_owned()],
            }),
        );
        let observation = tokio::spawn(async move { reference.observe().await });
        let request = broker.next_request().await.expect("observation request");
        assert_eq!(request.operation, PlatformStorageOperation::Observe);
        assert_eq!(request.namespace_generation, 7);
        assert_eq!(request.path, ["published", "file.bin"]);
        let expected = StorageObservation::present(
            StorageObjectKind::File,
            Some(23),
            Some("provider-v1:opaque".to_owned()),
        )
        .expect("valid observation");
        assert!(broker.complete_observation(request.request_id, expected.clone()));
        assert_eq!(
            observation
                .await
                .expect("observation task")
                .expect("result"),
            expected
        );
        assert_eq!(pool.snapshot().current_owned, 0);
        pool.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn invalidation_rejects_a_late_platform_observation() {
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(2, Some(client)).expect("pool");
        let reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: "torrent".to_owned(),
                namespace_generation: 3,
                role: StorageFileRole::Namespace,
            },
            StorageFileLocator::Platform(super::PlatformStorageTarget {
                root_id: "root".to_owned(),
                storage_id: "torrent".to_owned(),
                namespace_generation: 3,
                role: StorageFileRole::Namespace,
                path: vec!["published".to_owned()],
            }),
        );
        let observation = tokio::spawn(async move { reference.observe().await });
        let request = broker.next_request().await.expect("observation request");
        pool.invalidate_storage("torrent");
        assert!(
            broker.complete_observation(
                request.request_id,
                StorageObservation::present(StorageObjectKind::Directory, None, None)
                    .expect("directory observation"),
            )
        );
        assert!(matches!(
            observation.await.expect("observation task"),
            Err(super::StorageFilePoolError::Invalidated)
        ));
        pool.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn bounds_ten_thousand_logical_files_across_hundred_torrents() {
        let root = temp_root("logical-scale");
        let pool = StorageFilePool::new(8, None).expect("pool");
        for logical_index in 0..10_000 {
            let file_index = logical_index % 100;
            let reference = StorageFileReference::new(
                pool.clone(),
                StorageFileKey {
                    storage_id: format!("torrent-{}", logical_index / 100),
                    namespace_generation: 0,
                    role: StorageFileRole::Payload(file_index),
                },
                StorageFileLocator::Path(root.join(format!("{file_index}.bin"))),
            );
            drop(
                reference
                    .open(StorageFileAccess::ReadWriteCreate)
                    .await
                    .expect("open logical file"),
            );
        }
        let snapshot = pool.snapshot();
        assert_eq!(snapshot.owned_high_water, 8);
        assert_eq!(snapshot.cached_entries, 8);
        assert!(snapshot.evictions >= 9_992);
        pool.shutdown().await.expect("shutdown");
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}

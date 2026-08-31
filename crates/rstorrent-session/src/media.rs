use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rstorrent_engine::{
    ActiveFileError, ActiveFileReader, DownloadControl, PlatformStorageSpec, StorageFilePool,
    StreamingDemandError, StreamingDemandLease, TorrentId, VerifiedFileError, VerifiedFileReader,
};
use rstorrent_protocol::content::TorrentContent;
use rstorrent_protocol::storage_layout::ContentLayout;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use url::{Host, Url};

pub const MEDIA_CAPABILITY_RANDOM_BYTES: usize = 32;
pub const MEDIA_CAPABILITY_LENGTH: usize = 43;
pub const MAX_MEDIA_CAPABILITIES: usize = 128;
pub const MAX_MEDIA_REQUESTS: usize = 16;
pub const MAX_MEDIA_REQUESTS_PER_CAPABILITY: usize = 4;
pub const MAX_MEDIA_READ_JOBS: usize = 8;
pub const MEDIA_CAPABILITY_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const MEDIA_CAPABILITY_ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
pub const MEDIA_STREAMING_NO_PROGRESS_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MediaResourceSnapshot {
    pub active_bodies: usize,
    pub body_high_water: usize,
    pub active_streaming_leases: usize,
    pub streaming_lease_high_water: usize,
    pub active_streaming_reads: usize,
    pub streaming_read_high_water: usize,
    pub demanded_bytes_read: u64,
    pub demanded_bytes_served: u64,
    pub streaming_stall_timeouts: usize,
    pub verified_handoffs: usize,
}

#[derive(Debug, Default)]
struct MediaMetrics {
    active_bodies: AtomicUsize,
    body_high_water: AtomicUsize,
    active_streaming_leases: AtomicUsize,
    streaming_lease_high_water: AtomicUsize,
    active_streaming_reads: AtomicUsize,
    streaming_read_high_water: AtomicUsize,
    demanded_bytes_read: AtomicU64,
    demanded_bytes_served: AtomicU64,
    streaming_stall_timeouts: AtomicUsize,
    verified_handoffs: AtomicUsize,
}

impl MediaMetrics {
    fn increment(active: &AtomicUsize, high_water: &AtomicUsize) {
        let current = active.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        high_water.fetch_max(current, Ordering::AcqRel);
    }

    fn snapshot(&self) -> MediaResourceSnapshot {
        MediaResourceSnapshot {
            active_bodies: self.active_bodies.load(Ordering::Acquire),
            body_high_water: self.body_high_water.load(Ordering::Acquire),
            active_streaming_leases: self.active_streaming_leases.load(Ordering::Acquire),
            streaming_lease_high_water: self.streaming_lease_high_water.load(Ordering::Acquire),
            active_streaming_reads: self.active_streaming_reads.load(Ordering::Acquire),
            streaming_read_high_water: self.streaming_read_high_water.load(Ordering::Acquire),
            demanded_bytes_read: self.demanded_bytes_read.load(Ordering::Acquire),
            demanded_bytes_served: self.demanded_bytes_served.load(Ordering::Acquire),
            streaming_stall_timeouts: self.streaming_stall_timeouts.load(Ordering::Acquire),
            verified_handoffs: self.verified_handoffs.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum MediaFileAvailability {
    Available,
    Streamable,
    MetadataUnavailable,
    InvalidFile,
    Padding,
    Incomplete,
    Checking,
    Unverified,
    StorageUnavailable,
    Removing,
    ServerUnavailable,
    ResourceLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaUrlOutcome {
    Created {
        url: String,
        idle_timeout_millis: String,
        absolute_timeout_millis: String,
    },
    Unavailable {
        reason: MediaFileAvailability,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MediaUrlResponse {
    #[schemars(regex(pattern = "^t1-[0-9a-f]{32}$"))]
    pub torrent_id: String,
    pub file_index: u32,
    pub outcome: MediaUrlOutcome,
}

impl MediaUrlResponse {
    pub(crate) fn unavailable(
        torrent_id: String,
        file_index: u32,
        reason: MediaFileAvailability,
    ) -> Self {
        Self {
            torrent_id,
            file_index,
            outcome: MediaUrlOutcome::Unavailable { reason },
        }
    }
}

#[derive(Debug)]
pub struct MediaCapabilityLease {
    reader: MediaCapabilityReader,
    cancellation: CancellationToken,
    last_used: Arc<Mutex<Instant>>,
    read_jobs: Arc<Semaphore>,
    streaming_demand: Option<StreamingDemandLease>,
    metrics: Arc<MediaMetrics>,
    body_counted: bool,
    streaming_counted: bool,
    streaming_origin: bool,
    _global_request: OwnedSemaphorePermit,
    _capability_request: OwnedSemaphorePermit,
    absolute_deadline: Instant,
}

#[derive(Clone, Debug)]
enum MediaCapabilityReader {
    Verified(VerifiedFileReader),
    Active {
        reader: ActiveFileReader,
        control: DownloadControl,
        verified: Option<VerifiedMediaSource>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum VerifiedMediaSource {
    Path {
        root: PathBuf,
        content: Arc<TorrentContent>,
        file_index: usize,
        pool: StorageFilePool,
        torrent_id: TorrentId,
        read_jobs: Arc<Semaphore>,
    },
    Platform {
        spec: PlatformStorageSpec,
        content: Arc<TorrentContent>,
        file_index: usize,
        read_jobs: Arc<Semaphore>,
    },
}

impl VerifiedMediaSource {
    pub(crate) fn path(
        root: PathBuf,
        content: TorrentContent,
        file_index: usize,
        pool: StorageFilePool,
        torrent_id: TorrentId,
        read_jobs: Arc<Semaphore>,
    ) -> Self {
        Self::Path {
            root,
            content: Arc::new(content),
            file_index,
            pool,
            torrent_id,
            read_jobs,
        }
    }

    pub(crate) fn platform(
        spec: PlatformStorageSpec,
        content: TorrentContent,
        file_index: usize,
        read_jobs: Arc<Semaphore>,
    ) -> Self {
        Self::Platform {
            spec,
            content: Arc::new(content),
            file_index,
            read_jobs,
        }
    }

    async fn open(&self) -> Result<VerifiedFileReader, VerifiedFileError> {
        let (content, file_index) = match self {
            Self::Path {
                content,
                file_index,
                ..
            }
            | Self::Platform {
                content,
                file_index,
                ..
            } => (content, *file_index),
        };
        let layout = ContentLayout::from_content(content);
        let mut verified = vec![false; content.piece_count()];
        for piece in layout
            .file_piece_range(file_index)
            .map_err(VerifiedFileError::Layout)?
            .into_iter()
            .flatten()
        {
            verified
                [usize::try_from(piece).map_err(|_| VerifiedFileError::ArithmeticOverflow)?] = true;
        }
        match self {
            Self::Path {
                root,
                content,
                file_index,
                pool,
                torrent_id,
                read_jobs,
            } => {
                VerifiedFileReader::open_verified_content_with_pool(
                    root,
                    content,
                    &verified,
                    *file_index,
                    pool.clone(),
                    *torrent_id,
                    read_jobs.clone(),
                )
                .await
            }
            Self::Platform {
                spec,
                content,
                file_index,
                read_jobs,
            } => {
                VerifiedFileReader::open_verified_content_with_platform(
                    spec,
                    content,
                    &verified,
                    *file_index,
                    read_jobs.clone(),
                )
                .await
            }
        }
    }
}

impl MediaCapabilityLease {
    pub fn file_name(&self) -> &str {
        self.reader.file_name()
    }

    pub fn length(&self) -> u64 {
        self.reader.length()
    }

    pub fn is_active(&self) -> bool {
        matches!(&self.reader, MediaCapabilityReader::Active { .. })
    }

    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    pub fn is_live(&self) -> bool {
        let now = Instant::now();
        let last_used = *self
            .last_used
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !self.cancellation.is_cancelled()
            && now.duration_since(last_used) < MEDIA_CAPABILITY_IDLE_TIMEOUT
            && now < self.absolute_deadline
    }

    pub fn touch(&self) {
        *self
            .last_used
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    pub async fn wait_for_range(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<(), MediaRangeError> {
        if self.handoff_to_verified().await? {
            return Ok(());
        }
        let MediaCapabilityReader::Active {
            reader, control, ..
        } = &self.reader
        else {
            return Ok(());
        };
        let reader = reader.clone();
        let control = control.clone();
        if !self.is_live() {
            return Err(MediaRangeError::Revoked);
        }
        let (current, ahead) = reader
            .demand_intervals(offset, length)
            .map_err(MediaRangeError::Active)?;
        match self.streaming_demand.as_ref() {
            Some(demand) => demand
                .update(current, ahead)
                .map_err(map_streaming_demand_error)?,
            None => {
                self.streaming_demand = Some(
                    control
                        .acquire_streaming_demand(current, ahead)
                        .map_err(map_streaming_demand_error)?,
                );
                if !self.streaming_counted {
                    MediaMetrics::increment(
                        &self.metrics.active_streaming_leases,
                        &self.metrics.streaming_lease_high_water,
                    );
                    self.streaming_counted = true;
                }
            }
        }
        let (demand_id, mut updates, mut progress_revision) = {
            let demand = self
                .streaming_demand
                .as_ref()
                .expect("active range installed a demand");
            (
                demand.id(),
                demand.subscribe(),
                demand.progress_revision().ok_or(MediaRangeError::Revoked)?,
            )
        };
        let mut deadline = Instant::now() + MEDIA_STREAMING_NO_PROGRESS_TIMEOUT;
        let active_cancellation = reader.cancellation();
        loop {
            if self.handoff_to_verified().await? {
                return Ok(());
            }
            if !self.is_live() || reader.cancellation().is_cancelled() {
                return Err(MediaRangeError::Revoked);
            }
            match reader.is_range_verified(offset, length) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(ActiveFileError::Unavailable | ActiveFileError::Closed) => {
                    return Err(MediaRangeError::Revoked);
                }
                Err(error) => return Err(MediaRangeError::Active(error)),
            }
            let sleep = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline));
            tokio::pin!(sleep);
            tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(MediaRangeError::Revoked),
                _ = active_cancellation.cancelled() => {
                    return if self.handoff_to_verified().await? {
                        Ok(())
                    } else {
                        Err(MediaRangeError::Revoked)
                    };
                },
                _ = &mut sleep => {
                    self.metrics.streaming_stall_timeouts.fetch_add(1, Ordering::AcqRel);
                    return Err(MediaRangeError::NoProgress);
                },
                changed = updates.changed() => {
                    changed.map_err(|_| MediaRangeError::Revoked)?;
                    let next = updates
                        .borrow_and_update()
                        .demands()
                        .iter()
                        .find(|candidate| candidate.id() == demand_id)
                        .map(|candidate| candidate.progress_revision())
                        .ok_or(MediaRangeError::Revoked)?;
                    if next != progress_revision {
                        progress_revision = next;
                        deadline = Instant::now() + MEDIA_STREAMING_NO_PROGRESS_TIMEOUT;
                    }
                }
            }
        }
    }

    async fn handoff_to_verified(&mut self) -> Result<bool, MediaRangeError> {
        let MediaCapabilityReader::Active {
            reader, verified, ..
        } = &self.reader
        else {
            return Ok(true);
        };
        let file_length = usize::try_from(reader.length()).map_err(|_| MediaRangeError::Revoked)?;
        if !reader
            .is_range_verified(0, file_length)
            .map_err(MediaRangeError::Active)?
        {
            return Ok(false);
        }
        let reader = reader.clone();
        let Some(source) = verified.clone() else {
            return Ok(false);
        };
        if !reader.is_generation_current() {
            return Err(MediaRangeError::Revoked);
        }
        let verified = source.open().await.map_err(|_| MediaRangeError::Revoked)?;
        if !reader.is_generation_current()
            || verified.length() != reader.length()
            || verified.file_name() != reader.file_name()
        {
            return Err(MediaRangeError::Revoked);
        }
        self.streaming_demand = None;
        if self.streaming_counted {
            self.metrics
                .active_streaming_leases
                .fetch_sub(1, Ordering::AcqRel);
            self.streaming_counted = false;
        }
        self.metrics
            .verified_handoffs
            .fetch_add(1, Ordering::AcqRel);
        self.reader = MediaCapabilityReader::Verified(verified);
        Ok(true)
    }

    pub async fn read_range(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, MediaReadError> {
        let active_reader = match &self.reader {
            MediaCapabilityReader::Verified(reader) => {
                let result = reader
                    .read_range(offset, length)
                    .await
                    .map_err(MediaReadError::Verified);
                return self.record_read_result(result);
            }
            MediaCapabilityReader::Active { reader, .. } => reader.clone(),
        };
        let active_result = {
            let _permit = self
                .read_jobs
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| MediaReadError::Closed)?;
            MediaMetrics::increment(
                &self.metrics.active_streaming_reads,
                &self.metrics.streaming_read_high_water,
            );
            let result = active_reader
                .read_range(offset, length)
                .await
                .map_err(MediaReadError::Active);
            self.metrics
                .active_streaming_reads
                .fetch_sub(1, Ordering::AcqRel);
            result
        };
        if !matches!(
            active_result,
            Err(MediaReadError::Active(ActiveFileError::Closed))
        ) {
            return self.record_read_result(active_result);
        }

        self.handoff_after_active_close().await?;
        let MediaCapabilityReader::Verified(reader) = &self.reader else {
            return Err(MediaReadError::Closed);
        };
        let result = reader
            .read_range(offset, length)
            .await
            .map_err(MediaReadError::Verified);
        self.record_read_result(result)
    }

    async fn handoff_after_active_close(&mut self) -> Result<(), MediaReadError> {
        match self.handoff_to_verified().await {
            Ok(true) => Ok(()),
            Err(MediaRangeError::Active(error)) => Err(MediaReadError::Active(error)),
            Ok(false)
            | Err(
                MediaRangeError::NoProgress | MediaRangeError::Revoked | MediaRangeError::Saturated,
            ) => Err(MediaReadError::Closed),
        }
    }

    fn record_read_result(
        &self,
        result: Result<Vec<u8>, MediaReadError>,
    ) -> Result<Vec<u8>, MediaReadError> {
        if self.streaming_origin
            && let Ok(bytes) = &result
        {
            self.metrics.demanded_bytes_read.fetch_add(
                u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                Ordering::AcqRel,
            );
        }
        result
    }

    pub fn touch_served(&self, length: usize) {
        self.touch();
        if self.streaming_origin {
            self.metrics
                .demanded_bytes_served
                .fetch_add(u64::try_from(length).unwrap_or(u64::MAX), Ordering::AcqRel);
        }
    }
}

impl Drop for MediaCapabilityLease {
    fn drop(&mut self) {
        if self.streaming_counted {
            self.metrics
                .active_streaming_leases
                .fetch_sub(1, Ordering::AcqRel);
        }
        if self.body_counted {
            self.metrics.active_bodies.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl MediaCapabilityReader {
    fn file_name(&self) -> &str {
        match self {
            Self::Verified(reader) => reader.file_name(),
            Self::Active { reader, .. } => reader.file_name(),
        }
    }

    fn length(&self) -> u64 {
        match self {
            Self::Verified(reader) => reader.length(),
            Self::Active { reader, .. } => reader.length(),
        }
    }
}

#[derive(Debug)]
pub enum MediaRangeError {
    NoProgress,
    Revoked,
    Saturated,
    Active(ActiveFileError),
}

#[derive(Debug)]
pub enum MediaReadError {
    Closed,
    Verified(VerifiedFileError),
    Active(ActiveFileError),
}

fn map_streaming_demand_error(error: StreamingDemandError) -> MediaRangeError {
    match error {
        StreamingDemandError::Capacity => MediaRangeError::Saturated,
        StreamingDemandError::InvalidInterval { .. }
        | StreamingDemandError::UnknownDemand(_)
        | StreamingDemandError::IdentifierExhausted => MediaRangeError::Revoked,
    }
}

impl fmt::Display for MediaReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("media read owner is closed"),
            Self::Verified(error) => write!(formatter, "verified media read failed: {error}"),
            Self::Active(error) => write!(formatter, "active media read failed: {error}"),
        }
    }
}

impl Error for MediaReadError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaResolveError {
    NotFound,
    Busy,
}

#[derive(Debug)]
pub enum MediaOriginError {
    Invalid,
    InsecureNonLoopback,
}

impl fmt::Display for MediaOriginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => formatter.write_str("media origin is invalid"),
            Self::InsecureNonLoopback => {
                formatter.write_str("plain HTTP media origin must be loopback")
            }
        }
    }
}

impl Error for MediaOriginError {}

#[derive(Debug)]
pub(crate) enum MediaRegistryError {
    ServerUnavailable,
    ResourceLimit,
    Random(String),
}

#[derive(Debug)]
struct MediaCapabilityEntry {
    torrent_id: String,
    file_index: u32,
    reader: MediaCapabilityReader,
    created: Instant,
    last_used: Arc<Mutex<Instant>>,
    cancellation: CancellationToken,
    requests: Arc<Semaphore>,
}

#[derive(Debug)]
pub(crate) struct MediaCapabilities {
    origin: Option<String>,
    entries: HashMap<String, MediaCapabilityEntry>,
    by_file: HashMap<(String, u32), String>,
    requests: Arc<Semaphore>,
    read_jobs: Arc<Semaphore>,
    metrics: Arc<MediaMetrics>,
}

impl MediaCapabilities {
    pub(crate) fn new() -> Self {
        Self {
            origin: None,
            entries: HashMap::new(),
            by_file: HashMap::new(),
            requests: Arc::new(Semaphore::new(MAX_MEDIA_REQUESTS)),
            read_jobs: Arc::new(Semaphore::new(MAX_MEDIA_READ_JOBS)),
            metrics: Arc::new(MediaMetrics::default()),
        }
    }

    pub(crate) fn read_jobs(&self) -> Arc<Semaphore> {
        self.read_jobs.clone()
    }

    pub(crate) fn resource_snapshot(&self) -> MediaResourceSnapshot {
        self.metrics.snapshot()
    }

    pub(crate) fn set_origin(&mut self, origin: &str) -> Result<(), MediaOriginError> {
        let origin = validated_origin(origin)?;
        self.replace_origin(origin);
        Ok(())
    }

    pub(crate) fn set_origin_for_local_http_host(
        &mut self,
        origin: &str,
        exact_host: &str,
    ) -> Result<(), MediaOriginError> {
        let origin = validated_local_http_origin(origin, exact_host)?;
        self.replace_origin(origin);
        Ok(())
    }

    pub(crate) fn set_origin_for_private_lan_http(
        &mut self,
        origin: &str,
        exact_socket: std::net::SocketAddr,
    ) -> Result<(), MediaOriginError> {
        let origin = validated_private_lan_http_origin(origin, exact_socket)?;
        self.replace_origin(origin);
        Ok(())
    }

    fn replace_origin(&mut self, origin: String) {
        if self.origin.as_deref() != Some(origin.as_str()) {
            self.revoke_all();
            self.origin = Some(origin);
        }
    }

    pub(crate) fn create(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: VerifiedFileReader,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        let origin = self
            .origin
            .clone()
            .ok_or(MediaRegistryError::ServerUnavailable)?;
        let token = self.upsert_reader(
            torrent_id,
            file_index,
            MediaCapabilityReader::Verified(reader),
        )?;
        Ok(created_outcome(&origin, &token))
    }

    pub(crate) fn create_internal(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: VerifiedFileReader,
    ) -> Result<String, MediaRegistryError> {
        self.upsert_reader(
            torrent_id,
            file_index,
            MediaCapabilityReader::Verified(reader),
        )
    }

    pub(crate) fn create_active(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: ActiveFileReader,
        control: DownloadControl,
        verified: Option<VerifiedMediaSource>,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        self.create_reader(
            torrent_id,
            file_index,
            MediaCapabilityReader::Active {
                reader,
                control,
                verified,
            },
        )
    }

    fn create_reader(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: MediaCapabilityReader,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        let origin = self
            .origin
            .clone()
            .ok_or(MediaRegistryError::ServerUnavailable)?;
        let token = self.upsert_reader(torrent_id, file_index, reader)?;
        Ok(created_outcome(&origin, &token))
    }

    fn upsert_reader(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: MediaCapabilityReader,
    ) -> Result<String, MediaRegistryError> {
        let now = Instant::now();
        self.purge_expired(now);
        let key = (torrent_id.clone(), file_index);
        if let Some(token) = self.by_file.get(&key).cloned()
            && let Some(entry) = self.entries.get_mut(&token)
        {
            entry.reader = reader;
            *entry
                .last_used
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = now;
            return Ok(token);
        }
        if self.entries.len() >= MAX_MEDIA_CAPABILITIES {
            return Err(MediaRegistryError::ResourceLimit);
        }
        let token = self.allocate_token()?;
        self.entries.insert(
            token.clone(),
            MediaCapabilityEntry {
                torrent_id: torrent_id.clone(),
                file_index,
                reader,
                created: now,
                last_used: Arc::new(Mutex::new(now)),
                cancellation: CancellationToken::new(),
                requests: Arc::new(Semaphore::new(MAX_MEDIA_REQUESTS_PER_CAPABILITY)),
            },
        );
        self.by_file.insert(key, token.clone());
        Ok(token)
    }

    pub(crate) fn resolve(
        &mut self,
        token: &str,
    ) -> Result<MediaCapabilityLease, MediaResolveError> {
        if !valid_capability(token) {
            return Err(MediaResolveError::NotFound);
        }
        let now = Instant::now();
        self.purge_expired(now);
        let entry = self
            .entries
            .get_mut(token)
            .ok_or(MediaResolveError::NotFound)?;
        let global = self
            .requests
            .clone()
            .try_acquire_owned()
            .map_err(map_admission_error)?;
        let capability = entry
            .requests
            .clone()
            .try_acquire_owned()
            .map_err(map_admission_error)?;
        *entry
            .last_used
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = now;
        let streaming_origin = matches!(entry.reader, MediaCapabilityReader::Active { .. });
        MediaMetrics::increment(&self.metrics.active_bodies, &self.metrics.body_high_water);
        Ok(MediaCapabilityLease {
            reader: entry.reader.clone(),
            cancellation: entry.cancellation.clone(),
            last_used: Arc::clone(&entry.last_used),
            read_jobs: Arc::clone(&self.read_jobs),
            streaming_demand: None,
            metrics: Arc::clone(&self.metrics),
            body_counted: true,
            streaming_counted: false,
            streaming_origin,
            _global_request: global,
            _capability_request: capability,
            absolute_deadline: entry.created + MEDIA_CAPABILITY_ABSOLUTE_TIMEOUT,
        })
    }

    pub(crate) fn revoke_torrent(&mut self, torrent_id: &str) {
        let tokens = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.torrent_id == torrent_id)
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in tokens {
            self.remove(&token);
        }
    }

    pub(crate) fn revoke_all(&mut self) {
        for (_, entry) in self.entries.drain() {
            entry.cancellation.cancel();
        }
        self.by_file.clear();
    }

    pub(crate) async fn drain_reads(&self) {
        if let Ok(permits) = self
            .read_jobs
            .clone()
            .acquire_many_owned(MAX_MEDIA_READ_JOBS as u32)
            .await
        {
            self.read_jobs.close();
            drop(permits);
        }
    }

    fn purge_expired(&mut self, now: Instant) {
        let expired = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                now.duration_since(
                    *entry
                        .last_used
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()),
                ) >= MEDIA_CAPABILITY_IDLE_TIMEOUT
                    || now.duration_since(entry.created) >= MEDIA_CAPABILITY_ABSOLUTE_TIMEOUT
            })
            .map(|(token, _)| token.clone())
            .collect::<Vec<_>>();
        for token in expired {
            self.remove(&token);
        }
    }

    fn remove(&mut self, token: &str) {
        if let Some(entry) = self.entries.remove(token) {
            entry.cancellation.cancel();
            self.by_file.remove(&(entry.torrent_id, entry.file_index));
        }
    }

    fn allocate_token(&self) -> Result<String, MediaRegistryError> {
        for _ in 0..4 {
            let mut random = [0_u8; MEDIA_CAPABILITY_RANDOM_BYTES];
            getrandom::fill(&mut random)
                .map_err(|error| MediaRegistryError::Random(error.to_string()))?;
            let token = URL_SAFE_NO_PAD.encode(random);
            debug_assert_eq!(token.len(), MEDIA_CAPABILITY_LENGTH);
            if !self.entries.contains_key(&token) {
                return Ok(token);
            }
        }
        Err(MediaRegistryError::ResourceLimit)
    }
}

impl Drop for MediaCapabilities {
    fn drop(&mut self) {
        self.revoke_all();
        self.read_jobs.close();
        self.requests.close();
    }
}

fn created_outcome(origin: &str, token: &str) -> MediaUrlOutcome {
    MediaUrlOutcome::Created {
        url: format!("{origin}/media/v1/{token}"),
        idle_timeout_millis: MEDIA_CAPABILITY_IDLE_TIMEOUT.as_millis().to_string(),
        absolute_timeout_millis: MEDIA_CAPABILITY_ABSOLUTE_TIMEOUT.as_millis().to_string(),
    }
}

fn valid_capability(token: &str) -> bool {
    token.len() == MEDIA_CAPABILITY_LENGTH
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn map_admission_error(error: TryAcquireError) -> MediaResolveError {
    match error {
        TryAcquireError::NoPermits => MediaResolveError::Busy,
        TryAcquireError::Closed => MediaResolveError::NotFound,
    }
}

fn validated_origin(origin: &str) -> Result<String, MediaOriginError> {
    let parsed = parsed_origin(origin)?;
    if parsed.scheme() == "http" && !is_loopback_host(parsed.host()) {
        return Err(MediaOriginError::InsecureNonLoopback);
    }
    Ok(origin.to_owned())
}

fn validated_local_http_origin(origin: &str, exact_host: &str) -> Result<String, MediaOriginError> {
    if exact_host.is_empty()
        || exact_host.len() > 253
        || !exact_host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(MediaOriginError::Invalid);
    }
    let parsed = parsed_origin(origin)?;
    if parsed.scheme() != "http"
        || parsed
            .host_str()
            .is_none_or(|host| !host.eq_ignore_ascii_case(exact_host))
    {
        return Err(MediaOriginError::InsecureNonLoopback);
    }
    Ok(origin.to_owned())
}

fn validated_private_lan_http_origin(
    origin: &str,
    exact_socket: std::net::SocketAddr,
) -> Result<String, MediaOriginError> {
    let std::net::SocketAddr::V4(socket) = exact_socket else {
        return Err(MediaOriginError::InsecureNonLoopback);
    };
    if socket.port() == 0 || socket.ip().is_loopback() || !socket.ip().is_private() {
        return Err(MediaOriginError::InsecureNonLoopback);
    }
    parsed_origin(origin)?;
    if origin != format!("http://{exact_socket}") {
        return Err(MediaOriginError::InsecureNonLoopback);
    }
    Ok(origin.to_owned())
}

fn parsed_origin(origin: &str) -> Result<Url, MediaOriginError> {
    if origin.is_empty() || origin.len() > 512 || origin.ends_with('/') {
        return Err(MediaOriginError::Invalid);
    }
    let parsed = Url::parse(origin).map_err(|_| MediaOriginError::Invalid)?;
    if parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
        || parsed.host().is_none()
        || parsed.port_or_known_default().is_none()
    {
        return Err(MediaOriginError::Invalid);
    }
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(MediaOriginError::Invalid);
    }
    Ok(parsed)
}

fn is_loopback_host(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MediaOriginError, valid_capability, validated_local_http_origin, validated_origin,
        validated_private_lan_http_origin,
    };

    #[test]
    fn accepts_only_bounded_secure_or_loopback_origins() {
        assert_eq!(
            validated_origin("http://127.0.0.1:43121").expect("loopback"),
            "http://127.0.0.1:43121"
        );
        assert_eq!(
            validated_origin("https://media.example.test").expect("HTTPS"),
            "https://media.example.test"
        );
        assert!(matches!(
            validated_origin("http://192.0.2.1:8080"),
            Err(MediaOriginError::InsecureNonLoopback)
        ));
        for invalid in [
            "http://127.0.0.1:8080/",
            "ftp://127.0.0.1:21",
            "https://user@example.test",
            "https://example.test/path",
            "https://example.test?query",
        ] {
            assert!(validated_origin(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn local_http_origin_requires_the_separately_exact_host() {
        assert_eq!(
            validated_local_http_origin("http://penguin.linux.test:3030", "penguin.linux.test")
                .expect("exact local host"),
            "http://penguin.linux.test:3030"
        );
        for (origin, host) in [
            ("http://penguin.linux.test.evil:3030", "penguin.linux.test"),
            ("https://penguin.linux.test:3030", "penguin.linux.test"),
            ("http://penguin.linux.test:3030/", "penguin.linux.test"),
            ("http://penguin.linux.test:3030", ""),
        ] {
            assert!(
                validated_local_http_origin(origin, host).is_err(),
                "accepted {origin} for {host}"
            );
        }
    }

    #[test]
    fn private_lan_http_origin_requires_one_exact_rfc1918_socket() {
        let socket = "192.168.1.20:3030".parse().expect("private socket");
        assert_eq!(
            validated_private_lan_http_origin("http://192.168.1.20:3030", socket)
                .expect("exact private LAN origin"),
            "http://192.168.1.20:3030"
        );
        for (origin, socket) in [
            ("http://192.168.1.21:3030", "192.168.1.20:3030"),
            ("https://192.168.1.20:3030", "192.168.1.20:3030"),
            ("http://127.0.0.1:3030", "127.0.0.1:3030"),
            ("http://8.8.8.8:3030", "8.8.8.8:3030"),
            ("http://[fd00::20]:3030", "[fd00::20]:3030"),
        ] {
            assert!(
                validated_private_lan_http_origin(origin, socket.parse().expect("socket")).is_err(),
                "accepted {origin} for {socket}"
            );
        }
    }

    #[test]
    fn validates_exact_capability_alphabet_and_length() {
        assert!(valid_capability(&"a".repeat(43)));
        assert!(valid_capability(&format!("{}-_", "a".repeat(41))));
        assert!(!valid_capability(&"a".repeat(42)));
        assert!(!valid_capability(&format!("{}=", "a".repeat(42))));
    }
}

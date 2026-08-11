use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rstorrent_engine::{
    ActiveFileError, ActiveFileReader, DownloadControl, PlatformStorageSpec, StorageFilePool,
    StreamingDemandError, StreamingDemandLease, VerifiedFileError, VerifiedFileReader,
};
use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::storage_layout::TorrentLayout;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "snake_case")]
pub enum MediaFileAvailability {
    Available,
    Streamable,
    MetadataUnavailable,
    InvalidFile,
    Padding,
    NotPublished,
    Checking,
    Unverified,
    StorageUnavailable,
    Removing,
    ServerUnavailable,
    ResourceLimit,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
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
pub struct MediaUrlResponse {
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
    _global_request: OwnedSemaphorePermit,
    _capability_request: OwnedSemaphorePermit,
    absolute_deadline: Instant,
}

#[derive(Clone, Debug)]
enum MediaCapabilityReader {
    Published(VerifiedFileReader),
    Active {
        reader: ActiveFileReader,
        control: DownloadControl,
        published: Option<PublishedMediaSource>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum PublishedMediaSource {
    Path {
        root: PathBuf,
        metainfo: Arc<Metainfo>,
        file_index: usize,
        pool: StorageFilePool,
        storage_id: String,
        read_jobs: Arc<Semaphore>,
    },
    Platform {
        spec: PlatformStorageSpec,
        metainfo: Arc<Metainfo>,
        file_index: usize,
        read_jobs: Arc<Semaphore>,
    },
}

impl PublishedMediaSource {
    pub(crate) fn path(
        root: PathBuf,
        metainfo: Metainfo,
        file_index: usize,
        pool: StorageFilePool,
        storage_id: String,
        read_jobs: Arc<Semaphore>,
    ) -> Self {
        Self::Path {
            root,
            metainfo: Arc::new(metainfo),
            file_index,
            pool,
            storage_id,
            read_jobs,
        }
    }

    pub(crate) fn platform(
        spec: PlatformStorageSpec,
        metainfo: Metainfo,
        file_index: usize,
        read_jobs: Arc<Semaphore>,
    ) -> Self {
        Self::Platform {
            spec,
            metainfo: Arc::new(metainfo),
            file_index,
            read_jobs,
        }
    }

    async fn open(&self) -> Result<VerifiedFileReader, VerifiedFileError> {
        let (metainfo, file_index) = match self {
            Self::Path {
                metainfo,
                file_index,
                ..
            }
            | Self::Platform {
                metainfo,
                file_index,
                ..
            } => (metainfo, *file_index),
        };
        let layout = TorrentLayout::from_metainfo(metainfo);
        let mut verified = vec![false; metainfo.piece_hashes.len()];
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
                metainfo,
                file_index,
                pool,
                storage_id,
                read_jobs,
            } => {
                VerifiedFileReader::open_published_with_pool(
                    root,
                    metainfo,
                    &verified,
                    *file_index,
                    pool.clone(),
                    storage_id,
                    read_jobs.clone(),
                )
                .await
            }
            Self::Platform {
                spec,
                metainfo,
                file_index,
                read_jobs,
            } => {
                VerifiedFileReader::open_published_with_platform(
                    spec,
                    metainfo,
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
        if self.handoff_if_published().await? {
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
        let mut publication = control.content_publication_updates();
        loop {
            if self.handoff_if_published().await? {
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
                    return if self.handoff_if_published().await? {
                        Ok(())
                    } else {
                        Err(MediaRangeError::Revoked)
                    };
                },
                _ = &mut sleep => return Err(MediaRangeError::NoProgress),
                changed = publication.changed() => {
                    changed.map_err(|_| MediaRangeError::Revoked)?;
                    if self.handoff_if_published().await? {
                        return Ok(());
                    }
                }
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

    async fn handoff_if_published(&mut self) -> Result<bool, MediaRangeError> {
        let MediaCapabilityReader::Active {
            reader,
            control,
            published,
        } = &self.reader
        else {
            return Ok(true);
        };
        if !control.content_is_published() {
            return Ok(false);
        }
        let reader = reader.clone();
        let source = published.clone().ok_or(MediaRangeError::Revoked)?;
        if !reader.is_generation_current() {
            return Err(MediaRangeError::Revoked);
        }
        let published = source.open().await.map_err(|_| MediaRangeError::Revoked)?;
        if !reader.is_generation_current()
            || published.length() != reader.length()
            || published.file_name() != reader.file_name()
        {
            return Err(MediaRangeError::Revoked);
        }
        self.streaming_demand = None;
        self.reader = MediaCapabilityReader::Published(published);
        Ok(true)
    }

    pub async fn read_range(&self, offset: u64, length: usize) -> Result<Vec<u8>, MediaReadError> {
        match &self.reader {
            MediaCapabilityReader::Published(reader) => reader
                .read_range(offset, length)
                .await
                .map_err(MediaReadError::Published),
            MediaCapabilityReader::Active { reader, .. } => {
                let _permit = self
                    .read_jobs
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| MediaReadError::Closed)?;
                reader
                    .read_range(offset, length)
                    .await
                    .map_err(MediaReadError::Active)
            }
        }
    }
}

impl MediaCapabilityReader {
    fn file_name(&self) -> &str {
        match self {
            Self::Published(reader) => reader.file_name(),
            Self::Active { reader, .. } => reader.file_name(),
        }
    }

    fn length(&self) -> u64 {
        match self {
            Self::Published(reader) => reader.length(),
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
    Published(VerifiedFileError),
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
            Self::Published(error) => write!(formatter, "published media read failed: {error}"),
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
}

impl MediaCapabilities {
    pub(crate) fn new() -> Self {
        Self {
            origin: None,
            entries: HashMap::new(),
            by_file: HashMap::new(),
            requests: Arc::new(Semaphore::new(MAX_MEDIA_REQUESTS)),
            read_jobs: Arc::new(Semaphore::new(MAX_MEDIA_READ_JOBS)),
        }
    }

    pub(crate) fn read_jobs(&self) -> Arc<Semaphore> {
        self.read_jobs.clone()
    }

    pub(crate) fn set_origin(&mut self, origin: &str) -> Result<(), MediaOriginError> {
        let origin = validated_origin(origin)?;
        if self.origin.as_deref() != Some(origin.as_str()) {
            self.revoke_all();
            self.origin = Some(origin);
        }
        Ok(())
    }

    pub(crate) fn create(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: VerifiedFileReader,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        self.create_reader(
            torrent_id,
            file_index,
            MediaCapabilityReader::Published(reader),
        )
    }

    pub(crate) fn create_active(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: ActiveFileReader,
        control: DownloadControl,
        published: Option<PublishedMediaSource>,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        self.create_reader(
            torrent_id,
            file_index,
            MediaCapabilityReader::Active {
                reader,
                control,
                published,
            },
        )
    }

    fn create_reader(
        &mut self,
        torrent_id: String,
        file_index: u32,
        reader: MediaCapabilityReader,
    ) -> Result<MediaUrlOutcome, MediaRegistryError> {
        let now = Instant::now();
        self.purge_expired(now);
        let origin = self
            .origin
            .clone()
            .ok_or(MediaRegistryError::ServerUnavailable)?;
        let key = (torrent_id.clone(), file_index);
        if let Some(token) = self.by_file.get(&key).cloned()
            && let Some(entry) = self.entries.get_mut(&token)
        {
            entry.reader = reader;
            *entry
                .last_used
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = now;
            return Ok(created_outcome(&origin, &token));
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
        Ok(created_outcome(&origin, &token))
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
        Ok(MediaCapabilityLease {
            reader: entry.reader.clone(),
            cancellation: entry.cancellation.clone(),
            last_used: Arc::clone(&entry.last_used),
            read_jobs: Arc::clone(&self.read_jobs),
            streaming_demand: None,
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
    if parsed.scheme() == "http" && !is_loopback_host(parsed.host()) {
        return Err(MediaOriginError::InsecureNonLoopback);
    }
    Ok(origin.to_owned())
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
    use super::{MediaOriginError, valid_capability, validated_origin};

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
    fn validates_exact_capability_alphabet_and_length() {
        assert!(valid_capability(&"a".repeat(43)));
        assert!(valid_capability(&format!("{}-_", "a".repeat(41))));
        assert!(!valid_capability(&"a".repeat(42)));
        assert!(!valid_capability(&format!("{}=", "a".repeat(42))));
    }
}

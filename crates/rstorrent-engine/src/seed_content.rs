//! Conservative read-only access to verified published logical content.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::peer_wire::BlockRequest;
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};

use crate::artifact_layout::{ArtifactLayoutError, PublicationShape, PublishedArtifactLayout};
use crate::positional_io::read_exact_at;
use crate::selective_storage::{
    PlatformStorageSpec, SelectiveStorageError, torrent_storage_paths_for_metainfo,
};
use crate::storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, PlatformStorageTarget, StorageFileAccess, StorageFileKey,
    StorageFileLocator, StorageFilePool, StorageFilePoolError, StorageFileReference,
    StorageFileRole, StorageObjectKind,
};

#[derive(Clone, Debug)]
struct SeedFile {
    label: String,
    expected_length: u64,
    reference: StorageFileReference,
    pieces: Vec<usize>,
}

#[derive(Debug, Default)]
struct ReadMetrics {
    current_reads: AtomicUsize,
    read_high_water: AtomicUsize,
    current_file_leases: AtomicUsize,
    file_lease_high_water: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SeedContentSnapshot {
    pub current_reads: usize,
    pub read_high_water: usize,
    pub current_file_leases: usize,
    pub file_lease_high_water: usize,
}

#[derive(Clone, Debug)]
pub struct SeedContent {
    info_hash: [u8; 20],
    private: bool,
    layout: TorrentLayout,
    files: Vec<Option<SeedFile>>,
    available: Arc<[AtomicBool]>,
    metrics: Arc<ReadMetrics>,
}

impl SeedContent {
    pub async fn open_published(
        storage_root: &Path,
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
    ) -> Result<Self, SeedContentError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default seed file pool is valid");
        Self::open_published_with_pool(
            storage_root,
            metainfo,
            verified,
            skipped,
            pool,
            &hex(metainfo.info_hash),
        )
        .await
    }

    pub async fn open_published_with_pool(
        storage_root: &Path,
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
        pool: StorageFilePool,
        storage_id: &str,
    ) -> Result<Self, SeedContentError> {
        let layout = TorrentLayout::from_metainfo(metainfo);
        let paths = torrent_storage_paths_for_metainfo(storage_root, metainfo)
            .map_err(SeedContentError::StoragePlan)?;
        let artifact = PublishedArtifactLayout::from_metainfo(metainfo)
            .map_err(SeedContentError::ArtifactLayout)?;
        let namespace_reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: storage_id.to_owned(),
                namespace_generation: 1,
                role: StorageFileRole::Namespace,
            },
            StorageFileLocator::Path(paths.output),
        );
        let references = artifact
            .files
            .iter()
            .map(|file| {
                let path = file
                    .qualified_components
                    .iter()
                    .fold(storage_root.to_path_buf(), |path, part| path.join(part));
                StorageFileReference::new(
                    pool.clone(),
                    StorageFileKey {
                        storage_id: storage_id.to_owned(),
                        namespace_generation: 1,
                        role: StorageFileRole::Payload(file.file_index),
                    },
                    StorageFileLocator::Path(path),
                )
            })
            .collect();
        Self::open_with_references(
            metainfo,
            verified,
            skipped,
            layout,
            artifact,
            namespace_reference,
            references,
        )
        .await
    }

    pub async fn open_published_with_platform(
        spec: &PlatformStorageSpec,
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
    ) -> Result<Self, SeedContentError> {
        if !spec.published
            || spec.publication_name != metainfo.name
            || spec.publication_shape != PublicationShape::from_metainfo(metainfo)
        {
            return Err(SeedContentError::InvalidPlatformNamespace);
        }
        let layout = TorrentLayout::from_metainfo(metainfo);
        let artifact = PublishedArtifactLayout::from_metainfo(metainfo)
            .map_err(SeedContentError::ArtifactLayout)?;
        let target = |role, path| PlatformStorageTarget {
            root_id: spec.root_id.clone(),
            storage_id: spec.storage_id.clone(),
            namespace_generation: spec.namespace_generation,
            role,
            path,
        };
        let reference = |role, path| {
            StorageFileReference::new(
                spec.pool.clone(),
                StorageFileKey {
                    storage_id: spec.storage_id.clone(),
                    namespace_generation: spec.namespace_generation,
                    role,
                },
                StorageFileLocator::Platform(target(role, path)),
            )
        };
        let namespace_reference =
            reference(StorageFileRole::Namespace, vec![artifact.namespace.clone()]);
        let references = artifact
            .files
            .iter()
            .map(|file| {
                reference(
                    StorageFileRole::Payload(file.file_index),
                    file.qualified_components.clone(),
                )
            })
            .collect();
        Self::open_with_references(
            metainfo,
            verified,
            skipped,
            layout,
            artifact,
            namespace_reference,
            references,
        )
        .await
    }

    async fn open_with_references(
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
        layout: TorrentLayout,
        artifact: PublishedArtifactLayout,
        namespace_reference: StorageFileReference,
        references: Vec<StorageFileReference>,
    ) -> Result<Self, SeedContentError> {
        if verified.len() != layout.piece_count() {
            return Err(SeedContentError::InvalidHaveLength {
                actual: verified.len(),
                expected: layout.piece_count(),
            });
        }
        let selection = FileSelection::new(&layout, skipped).map_err(SeedContentError::Layout)?;
        let namespace =
            namespace_reference
                .observe()
                .await
                .map_err(|source| SeedContentError::Storage {
                    artifact: "published namespace".to_owned(),
                    source,
                })?;
        let expected_namespace_kind = match artifact.shape {
            PublicationShape::File => StorageObjectKind::File,
            PublicationShape::Tree => StorageObjectKind::Directory,
        };
        if !namespace.exists || namespace.kind != Some(expected_namespace_kind) {
            return Err(SeedContentError::UnexpectedArtifact(
                "published namespace".to_owned(),
            ));
        }

        let mut files = Vec::with_capacity(layout.files().len());
        for ((file, logical), reference) in
            layout.files().iter().zip(&artifact.files).zip(references)
        {
            if file.padding || !selection.is_wanted(logical.file_index) {
                files.push(None);
                continue;
            }
            let label = format!("payload file {}", logical.file_index);
            let observation = reference.observe().await;
            let readable = observation.is_ok_and(|observation| {
                observation.exists
                    && observation.kind == Some(StorageObjectKind::File)
                    && observation.length == Some(file.length)
            });
            if !readable {
                files.push(None);
                continue;
            }
            let pieces = layout
                .file_piece_range(logical.file_index)
                .map_err(SeedContentError::Layout)?
                .into_iter()
                .flatten()
                .map(|piece| {
                    usize::try_from(piece).map_err(|_| SeedContentError::ArithmeticOverflow)
                })
                .collect::<Result<Vec<_>, _>>()?;
            files.push(Some(SeedFile {
                label,
                expected_length: file.length,
                reference,
                pieces,
            }));
        }

        let mut available = Vec::with_capacity(layout.piece_count());
        for (piece, verified) in verified.iter().copied().enumerate() {
            let readable = if verified {
                let piece_index =
                    u32::try_from(piece).map_err(|_| SeedContentError::ArithmeticOverflow)?;
                let length = layout
                    .piece_length_at(piece_index)
                    .map_err(SeedContentError::Layout)?;
                layout
                    .file_segments(piece_index, 0, length)
                    .map_err(SeedContentError::Layout)?
                    .iter()
                    .all(|segment| segment.padding || files[segment.file_index].is_some())
            } else {
                false
            };
            available.push(AtomicBool::new(readable));
        }

        Ok(Self {
            info_hash: metainfo.info_hash,
            private: metainfo.private,
            layout,
            files,
            available: available.into(),
            metrics: Arc::new(ReadMetrics::default()),
        })
    }

    pub fn availability(&self) -> Vec<bool> {
        self.available
            .iter()
            .map(|available| available.load(Ordering::Acquire))
            .collect()
    }

    pub fn info_hash(&self) -> [u8; 20] {
        self.info_hash
    }

    pub fn is_private(&self) -> bool {
        self.private
    }

    pub fn piece_lengths(&self) -> Result<Vec<u32>, SeedContentError> {
        (0..self.layout.piece_count())
            .map(|index| {
                let index =
                    u32::try_from(index).map_err(|_| SeedContentError::ArithmeticOverflow)?;
                self.layout
                    .piece_length_at(index)
                    .map_err(SeedContentError::Layout)
            })
            .collect()
    }

    pub fn snapshot(&self) -> SeedContentSnapshot {
        SeedContentSnapshot {
            current_reads: self.metrics.current_reads.load(Ordering::Acquire),
            read_high_water: self.metrics.read_high_water.load(Ordering::Acquire),
            current_file_leases: self.metrics.current_file_leases.load(Ordering::Acquire),
            file_lease_high_water: self.metrics.file_lease_high_water.load(Ordering::Acquire),
        }
    }

    pub async fn read_block(&self, request: BlockRequest) -> Result<Vec<u8>, SeedContentError> {
        let piece = usize::try_from(request.index)
            .map_err(|_| SeedContentError::InvalidRequest(request))?;
        if !self
            .available
            .get(piece)
            .is_some_and(|available| available.load(Ordering::Acquire))
            || request.length == 0
        {
            return Err(SeedContentError::InvalidRequest(request));
        }
        let segments = self
            .layout
            .file_segments(request.index, request.begin, request.length)
            .map_err(SeedContentError::Layout)?;
        let metrics = self.metrics.clone();
        let _read = CounterGuard::new(&metrics.current_reads, &metrics.read_high_water);
        let mut block = vec![0; request.length as usize];
        for segment in segments {
            if segment.padding {
                continue;
            }
            let file = self
                .files
                .get(segment.file_index)
                .and_then(Option::as_ref)
                .ok_or(SeedContentError::UnavailablePiece(request.index))?;
            let reference = file.reference.clone();
            let label = file.label.clone();
            let expected_length = file.expected_length;
            let observation = match reference.observe().await {
                Ok(observation) => observation,
                Err(source) => {
                    self.invalidate_file(segment.file_index);
                    return Err(SeedContentError::Storage {
                        artifact: label,
                        source,
                    });
                }
            };
            if !observation.exists
                || observation.kind != Some(StorageObjectKind::File)
                || observation.length != Some(expected_length)
            {
                self.invalidate_file(segment.file_index);
                return Err(SeedContentError::UnexpectedArtifact(label));
            }
            let handle = match reference.open(StorageFileAccess::ReadExisting).await {
                Ok(handle) => handle,
                Err(source) => {
                    self.invalidate_file(segment.file_index);
                    return Err(SeedContentError::Storage {
                        artifact: label,
                        source,
                    });
                }
            };
            let _lease =
                CounterGuard::new(&metrics.current_file_leases, &metrics.file_lease_high_water);
            let segment_block = tokio::task::spawn_blocking(move || {
                let opened = handle
                    .file()
                    .metadata()
                    .map_err(|source| SeedContentError::Io {
                        operation: "inspect open seed payload",
                        artifact: label.clone(),
                        source,
                    })?;
                if !opened.is_file() || opened.len() != expected_length {
                    return Err(SeedContentError::UnexpectedArtifact(label.clone()));
                }
                let mut bytes = vec![0; segment.length];
                read_exact_at(handle.file(), &mut bytes, segment.file_offset).map_err(
                    |source| SeedContentError::Io {
                        operation: "read seed payload",
                        artifact: label,
                        source,
                    },
                )?;
                Ok(bytes)
            })
            .await
            .map_err(|error| SeedContentError::TaskJoin(error.to_string()));
            let segment_block = match segment_block {
                Ok(Ok(block)) => block,
                Ok(Err(error)) => {
                    self.invalidate_file(segment.file_index);
                    return Err(error);
                }
                Err(error) => {
                    self.invalidate_file(segment.file_index);
                    return Err(error);
                }
            };
            let end = segment
                .block_offset
                .checked_add(segment.length)
                .ok_or(SeedContentError::ArithmeticOverflow)?;
            block[segment.block_offset..end].copy_from_slice(&segment_block);
        }
        Ok(block)
    }

    fn invalidate_file(&self, file_index: usize) {
        if let Some(Some(file)) = self.files.get(file_index) {
            for piece in &file.pieces {
                if let Some(available) = self.available.get(*piece) {
                    available.store(false, Ordering::Release);
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum SeedContentError {
    InvalidHaveLength {
        actual: usize,
        expected: usize,
    },
    InvalidRequest(BlockRequest),
    UnavailablePiece(u32),
    UnexpectedArtifact(String),
    InvalidPlatformNamespace,
    ArithmeticOverflow,
    Layout(LayoutError),
    ArtifactLayout(ArtifactLayoutError),
    StoragePlan(SelectiveStorageError),
    Storage {
        artifact: String,
        source: StorageFilePoolError,
    },
    Io {
        operation: &'static str,
        artifact: String,
        source: io::Error,
    },
    TaskJoin(String),
}

impl fmt::Display for SeedContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHaveLength { actual, expected } => {
                write!(
                    formatter,
                    "have length {actual} does not match piece count {expected}"
                )
            }
            Self::InvalidRequest(request) => write!(formatter, "invalid seed request {request:?}"),
            Self::UnavailablePiece(piece) => write!(formatter, "piece {piece} is not readable"),
            Self::UnexpectedArtifact(artifact) => {
                write!(
                    formatter,
                    "seed payload has an unexpected type or length: {artifact}",
                )
            }
            Self::InvalidPlatformNamespace => {
                formatter.write_str("seed platform namespace does not match verified metadata")
            }
            Self::ArithmeticOverflow => formatter.write_str("seed content arithmetic overflow"),
            Self::Layout(error) => write!(formatter, "seed layout: {error}"),
            Self::ArtifactLayout(error) => write!(formatter, "seed artifact layout: {error}"),
            Self::StoragePlan(error) => write!(formatter, "seed storage plan: {error}"),
            Self::Storage { artifact, source } => {
                write!(formatter, "access seed {artifact}: {source}")
            }
            Self::Io {
                operation,
                artifact,
                source,
            } => {
                write!(formatter, "{operation} {artifact}: {source}")
            }
            Self::TaskJoin(error) => write!(formatter, "seed read task: {error}"),
        }
    }
}

impl Error for SeedContentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::ArtifactLayout(error) => Some(error),
            Self::StoragePlan(error) => Some(error),
            Self::Storage { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

struct CounterGuard<'a> {
    current: &'a AtomicUsize,
}

impl<'a> CounterGuard<'a> {
    fn new(current: &'a AtomicUsize, high_water: &AtomicUsize) -> Self {
        let value = current.fetch_add(1, Ordering::AcqRel) + 1;
        high_water.fetch_max(value, Ordering::AcqRel);
        Self { current }
    }
}

impl Drop for CounterGuard<'_> {
    fn drop(&mut self) {
        self.current.fetch_sub(1, Ordering::AcqRel);
    }
}

fn hex(bytes: [u8; 20]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(40);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::peer_wire::BlockRequest;

    use crate::artifact_layout::PublicationShape;
    use crate::selective_storage::PlatformStorageSpec;
    use crate::storage_file_pool::{
        PlatformStorageFailure, PlatformStorageFailureKind, PlatformStorageOperation,
        StorageFileAccess, StorageObjectKind, StorageObservation, platform_storage_channel,
    };

    use super::{SeedContent, SeedContentError, StorageFilePool};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn root(label: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-seed-content-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn single() -> Metainfo {
        Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]; 2],
            piece_length: 4,
            total_length: 7,
            name: "single.bin".to_owned(),
            private: false,
            mode: MetainfoMode::SingleFile,
            files: vec![MetainfoFile {
                path: vec!["single.bin".to_owned()],
                length: 7,
                offset: 0,
                padding: false,
            }],
        }
    }

    fn multi() -> Metainfo {
        Metainfo {
            info_hash: [3; 20],
            piece_hashes: vec![[4; 20]; 3],
            piece_length: 4,
            total_length: 10,
            name: "tree".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![
                MetainfoFile {
                    path: vec!["a".to_owned()],
                    length: 3,
                    offset: 0,
                    padding: false,
                },
                MetainfoFile {
                    path: vec!["b".to_owned()],
                    length: 3,
                    offset: 3,
                    padding: false,
                },
                MetainfoFile {
                    path: vec![".pad".to_owned(), "2".to_owned()],
                    length: 2,
                    offset: 6,
                    padding: true,
                },
                MetainfoFile {
                    path: vec!["c".to_owned()],
                    length: 2,
                    offset: 8,
                    padding: false,
                },
            ],
        }
    }

    #[tokio::test]
    async fn reads_single_file_and_short_final_piece() {
        let root = root("single");
        tokio::fs::create_dir_all(&root).await.expect("create root");
        tokio::fs::write(root.join("single.bin"), b"abcdefg")
            .await
            .expect("write payload");
        let content = SeedContent::open_published(&root, &single(), &[true, true], &[])
            .await
            .expect("open seed content");
        assert_eq!(content.availability(), [true, true]);
        assert_eq!(content.piece_lengths().expect("piece lengths"), [4, 3]);
        assert_eq!(
            content
                .read_block(BlockRequest {
                    index: 1,
                    begin: 0,
                    length: 3,
                })
                .await
                .expect("read final piece"),
            b"efg"
        );
        let snapshot = content.snapshot();
        assert_eq!(snapshot.read_high_water, 1);
        assert_eq!(snapshot.file_lease_high_water, 1);
        assert_eq!(snapshot.current_reads, 0);
        assert_eq!(snapshot.current_file_leases, 0);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn reads_cross_file_and_padding_ranges() {
        let root = root("multi");
        tokio::fs::create_dir_all(root.join("tree"))
            .await
            .expect("create tree");
        tokio::fs::write(root.join("tree/a"), b"abc")
            .await
            .expect("write a");
        tokio::fs::write(root.join("tree/b"), b"def")
            .await
            .expect("write b");
        tokio::fs::write(root.join("tree/c"), b"gh")
            .await
            .expect("write c");
        let pool = StorageFilePool::new(1, None).expect("create one-handle pool");
        let content = SeedContent::open_published_with_pool(
            &root,
            &multi(),
            &[true, true, true],
            &[],
            pool.clone(),
            "multi-seed",
        )
        .await
        .expect("open seed content");
        assert_eq!(content.availability(), [true, true, true]);
        assert_eq!(
            content
                .read_block(BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 4,
                })
                .await
                .expect("cross-file read"),
            b"abcd"
        );
        assert_eq!(
            content
                .read_block(BlockRequest {
                    index: 1,
                    begin: 0,
                    length: 4,
                })
                .await
                .expect("padding read"),
            [b'e', b'f', 0, 0]
        );
        let pool = pool.snapshot();
        assert_eq!(pool.limit, 1);
        assert_eq!(pool.current_owned, 1);
        assert_eq!(pool.owned_high_water, 1);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn masks_skipped_missing_and_truncated_piece_sources() {
        let root = root("masked");
        tokio::fs::create_dir_all(root.join("tree"))
            .await
            .expect("create tree");
        tokio::fs::write(root.join("tree/a"), b"abc")
            .await
            .expect("write a");
        tokio::fs::write(root.join("tree/b"), b"de")
            .await
            .expect("write truncated b");
        tokio::fs::write(root.join("tree/c"), b"gh")
            .await
            .expect("write c");
        let truncated = SeedContent::open_published(&root, &multi(), &[true, true, true], &[])
            .await
            .expect("open with masked source");
        assert_eq!(truncated.availability(), [false, false, true]);
        let skipped = SeedContent::open_published(&root, &multi(), &[true, true, true], &[0])
            .await
            .expect("open skipped source");
        assert_eq!(skipped.availability(), [false, false, true]);
        assert!(matches!(
            skipped
                .read_block(BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 4,
                })
                .await,
            Err(SeedContentError::InvalidRequest(_))
        ));
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn failed_read_observation_retracts_all_affected_piece_availability() {
        let root = root("retract");
        tokio::fs::create_dir_all(&root).await.expect("create root");
        tokio::fs::write(root.join("single.bin"), b"abcdefg")
            .await
            .expect("write payload");
        let content = SeedContent::open_published(&root, &single(), &[true, true], &[])
            .await
            .expect("open seed content");
        tokio::fs::write(root.join("single.bin"), b"short")
            .await
            .expect("truncate payload");
        assert!(
            content
                .read_block(BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 4,
                })
                .await
                .is_err()
        );
        assert_eq!(content.availability(), [false, false]);
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn platform_content_uses_observation_and_shared_pool_for_upload_reads() {
        let root = root("platform");
        tokio::fs::create_dir_all(root.join("tree"))
            .await
            .expect("create tree");
        tokio::fs::write(root.join("tree/a"), b"abc")
            .await
            .expect("write a");
        tokio::fs::write(root.join("tree/b"), b"def")
            .await
            .expect("write b");
        tokio::fs::write(root.join("tree/c"), b"gh")
            .await
            .expect("write c");
        let (client, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(1, Some(client)).expect("pool");
        let provider_root = root.clone();
        let provider = tokio::spawn(async move {
            while let Some(request) = broker.next_request().await {
                let path = request
                    .path
                    .iter()
                    .fold(provider_root.clone(), |path, part| path.join(part));
                match request.operation {
                    PlatformStorageOperation::Observe => {
                        let observation = match std::fs::symlink_metadata(&path) {
                            Ok(metadata) => StorageObservation::present(
                                if metadata.is_file() {
                                    StorageObjectKind::File
                                } else if metadata.is_dir() {
                                    StorageObjectKind::Directory
                                } else {
                                    StorageObjectKind::Other
                                },
                                metadata.is_file().then_some(metadata.len()),
                                None,
                            )
                            .expect("provider observation"),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                StorageObservation::missing()
                            }
                            Err(error) => {
                                broker.complete_error(
                                    request.request_id,
                                    PlatformStorageFailure::new(
                                        PlatformStorageFailureKind::ProviderRefused,
                                        error.to_string(),
                                    ),
                                );
                                continue;
                            }
                        };
                        broker.complete_observation(request.request_id, observation);
                    }
                    PlatformStorageOperation::Open => {
                        assert_eq!(request.access, StorageFileAccess::ReadExisting);
                        let file = OpenOptions::new().read(true).open(path).expect("open file");
                        broker.complete_file(request.request_id, file);
                    }
                    PlatformStorageOperation::Delete => panic!("unexpected deletion"),
                }
            }
        });
        let content = SeedContent::open_published_with_platform(
            &PlatformStorageSpec {
                pool: pool.clone(),
                root_id: "root".to_owned(),
                storage_id: "platform-seed".to_owned(),
                publication_name: "tree".to_owned(),
                publication_shape: PublicationShape::Tree,
                namespace_generation: 9,
                managed: true,
                published: true,
            },
            &multi(),
            &[true, true, true],
            &[],
        )
        .await
        .expect("open platform content");
        assert_eq!(content.availability(), [true, true, true]);
        assert_eq!(
            content
                .read_block(BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 4,
                })
                .await
                .expect("platform cross-file read"),
            b"abcd"
        );
        let snapshot = pool.snapshot();
        assert_eq!(snapshot.limit, 1);
        assert_eq!(snapshot.owned_high_water, 1);
        assert_eq!(snapshot.platform_pending, 0);
        drop(content);
        pool.shutdown().await.expect("shutdown");
        provider.abort();
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }
}

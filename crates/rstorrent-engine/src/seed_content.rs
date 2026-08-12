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
use crate::identity::TorrentId;
use crate::positional_io::read_exact_at;
use crate::selective_storage::{
    PlatformStorageSpec, SelectiveStorageError, torrent_storage_paths_for_metainfo,
};
use crate::storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, PlatformStorageTarget, StorageFileAccess, StorageFileKey,
    StorageFileLocator, StorageFilePool, StorageFilePoolError, StorageFileReference,
    StorageFileRole, StorageObjectKind,
};

/// Maximum payload prepared by one verified logical-file read.
pub const MAX_VERIFIED_FILE_READ_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerifiedFileSnapshot {
    pub current_reads: usize,
    pub read_high_water: usize,
    pub current_file_leases: usize,
    pub file_lease_high_water: usize,
}

/// Immutable authority for bounded reads from one verified published file.
#[derive(Clone, Debug)]
pub struct VerifiedFileReader {
    file_index: usize,
    file_name: String,
    expected_length: u64,
    reference: StorageFileReference,
    read_jobs: Arc<tokio::sync::Semaphore>,
    metrics: Arc<ReadMetrics>,
}

impl VerifiedFileReader {
    pub async fn open_published_with_pool(
        storage_root: &Path,
        metainfo: &Metainfo,
        verified: &[bool],
        file_index: usize,
        pool: StorageFilePool,
        torrent_id: TorrentId,
        read_jobs: Arc<tokio::sync::Semaphore>,
    ) -> Result<Self, VerifiedFileError> {
        let layout = TorrentLayout::from_metainfo(metainfo);
        let paths = torrent_storage_paths_for_metainfo(storage_root, metainfo, torrent_id)
            .map_err(VerifiedFileError::StoragePlan)?;
        let storage_id = torrent_id.to_string();
        let artifact = PublishedArtifactLayout::from_metainfo(metainfo)
            .map_err(VerifiedFileError::ArtifactLayout)?;
        let logical = artifact
            .files
            .get(file_index)
            .ok_or(VerifiedFileError::InvalidFileIndex(file_index))?;
        let namespace_reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: storage_id.clone(),
                namespace_generation: 1,
                role: StorageFileRole::Namespace,
            },
            StorageFileLocator::Path(paths.output),
        );
        let path = logical
            .qualified_components
            .iter()
            .fold(storage_root.to_path_buf(), |path, part| path.join(part));
        let reference = StorageFileReference::new(
            pool,
            StorageFileKey {
                storage_id,
                namespace_generation: 1,
                role: StorageFileRole::Payload(file_index),
            },
            StorageFileLocator::Path(path),
        );
        Self::open_with_reference(
            metainfo,
            verified,
            file_index,
            layout,
            artifact,
            namespace_reference,
            reference,
            read_jobs,
        )
        .await
    }

    pub async fn open_published_with_platform(
        spec: &PlatformStorageSpec,
        metainfo: &Metainfo,
        verified: &[bool],
        file_index: usize,
        read_jobs: Arc<tokio::sync::Semaphore>,
    ) -> Result<Self, VerifiedFileError> {
        if !spec.published
            || spec.publication_name != metainfo.name
            || spec.publication_shape != PublicationShape::from_metainfo(metainfo)
        {
            return Err(VerifiedFileError::InvalidPlatformNamespace);
        }
        let layout = TorrentLayout::from_metainfo(metainfo);
        let artifact = PublishedArtifactLayout::from_metainfo(metainfo)
            .map_err(VerifiedFileError::ArtifactLayout)?;
        let logical = artifact
            .files
            .get(file_index)
            .ok_or(VerifiedFileError::InvalidFileIndex(file_index))?;
        let reference = |role, path| {
            let target = PlatformStorageTarget {
                root_id: spec.root_id.clone(),
                storage_id: spec.storage_id.clone(),
                namespace_generation: spec.namespace_generation,
                role,
                path,
            };
            StorageFileReference::new(
                spec.pool.clone(),
                StorageFileKey {
                    storage_id: spec.storage_id.clone(),
                    namespace_generation: spec.namespace_generation,
                    role,
                },
                StorageFileLocator::Platform(target),
            )
        };
        let namespace_reference =
            reference(StorageFileRole::Namespace, vec![artifact.namespace.clone()]);
        let file_reference = reference(
            StorageFileRole::Payload(file_index),
            logical.qualified_components.clone(),
        );
        Self::open_with_reference(
            metainfo,
            verified,
            file_index,
            layout,
            artifact,
            namespace_reference,
            file_reference,
            read_jobs,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn open_with_reference(
        metainfo: &Metainfo,
        verified: &[bool],
        file_index: usize,
        layout: TorrentLayout,
        artifact: PublishedArtifactLayout,
        namespace_reference: StorageFileReference,
        reference: StorageFileReference,
        read_jobs: Arc<tokio::sync::Semaphore>,
    ) -> Result<Self, VerifiedFileError> {
        if read_jobs.is_closed() {
            return Err(VerifiedFileError::ReadOwnerClosed);
        }
        if verified.len() != layout.piece_count() {
            return Err(VerifiedFileError::InvalidHaveLength {
                actual: verified.len(),
                expected: layout.piece_count(),
            });
        }
        let file = layout
            .files()
            .get(file_index)
            .ok_or(VerifiedFileError::InvalidFileIndex(file_index))?;
        if file.padding {
            return Err(VerifiedFileError::PaddingFile(file_index));
        }
        let all_verified = layout
            .file_piece_range(file_index)
            .map_err(VerifiedFileError::Layout)?
            .into_iter()
            .flatten()
            .all(|piece| {
                usize::try_from(piece)
                    .ok()
                    .and_then(|piece| verified.get(piece))
                    .copied()
                    .unwrap_or(false)
            });
        if !all_verified {
            return Err(VerifiedFileError::UnverifiedFile(file_index));
        }
        let namespace =
            namespace_reference
                .observe()
                .await
                .map_err(|source| VerifiedFileError::Storage {
                    artifact: "published namespace".to_owned(),
                    source,
                })?;
        let expected_namespace_kind = match artifact.shape {
            PublicationShape::File => StorageObjectKind::File,
            PublicationShape::Tree => StorageObjectKind::Directory,
        };
        if !namespace.exists || namespace.kind != Some(expected_namespace_kind) {
            return Err(VerifiedFileError::UnexpectedArtifact(
                "published namespace".to_owned(),
            ));
        }
        let label = format!("payload file {file_index}");
        observe_exact_file(&reference, file.length, &label).await?;
        let file_name = metainfo.files[file_index]
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| metainfo.name.clone());
        Ok(Self {
            file_index,
            file_name,
            expected_length: file.length,
            reference,
            read_jobs,
            metrics: Arc::new(ReadMetrics::default()),
        })
    }

    pub fn file_index(&self) -> usize {
        self.file_index
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn length(&self) -> u64 {
        self.expected_length
    }

    pub fn snapshot(&self) -> VerifiedFileSnapshot {
        VerifiedFileSnapshot {
            current_reads: self.metrics.current_reads.load(Ordering::Acquire),
            read_high_water: self.metrics.read_high_water.load(Ordering::Acquire),
            current_file_leases: self.metrics.current_file_leases.load(Ordering::Acquire),
            file_lease_high_water: self.metrics.file_lease_high_water.load(Ordering::Acquire),
        }
    }

    pub async fn read_range(
        &self,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, VerifiedFileError> {
        if length > MAX_VERIFIED_FILE_READ_BYTES {
            return Err(VerifiedFileError::ReadTooLarge {
                actual: length,
                maximum: MAX_VERIFIED_FILE_READ_BYTES,
            });
        }
        let length_u64 =
            u64::try_from(length).map_err(|_| VerifiedFileError::ArithmeticOverflow)?;
        let end = offset
            .checked_add(length_u64)
            .ok_or(VerifiedFileError::ArithmeticOverflow)?;
        if offset > self.expected_length || end > self.expected_length {
            return Err(VerifiedFileError::InvalidRange {
                offset,
                length,
                file_length: self.expected_length,
            });
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        let permit = self
            .read_jobs
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| VerifiedFileError::ReadOwnerClosed)?;
        let label = format!("payload file {}", self.file_index);
        observe_exact_file(&self.reference, self.expected_length, &label).await?;
        let handle = self
            .reference
            .open(StorageFileAccess::ReadExisting)
            .await
            .map_err(|source| VerifiedFileError::Storage {
                artifact: label.clone(),
                source,
            })?;
        let expected_length = self.expected_length;
        let metrics = self.metrics.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let _read = OwnedCounterGuard::read(metrics.clone());
            let _lease = OwnedCounterGuard::lease(metrics);
            let opened = handle
                .file()
                .metadata()
                .map_err(|source| VerifiedFileError::Io {
                    operation: "inspect open verified payload",
                    artifact: label.clone(),
                    source,
                })?;
            if !opened.is_file() || opened.len() != expected_length {
                return Err(VerifiedFileError::UnexpectedArtifact(label));
            }
            let mut bytes = vec![0; length];
            read_exact_at(handle.file(), &mut bytes, offset).map_err(|source| {
                VerifiedFileError::Io {
                    operation: "read verified payload",
                    artifact: label,
                    source,
                }
            })?;
            Ok(bytes)
        })
        .await
        .map_err(|error| VerifiedFileError::TaskJoin(error.to_string()))?
    }
}

async fn observe_exact_file(
    reference: &StorageFileReference,
    expected_length: u64,
    label: &str,
) -> Result<(), VerifiedFileError> {
    let observation = reference
        .observe()
        .await
        .map_err(|source| VerifiedFileError::Storage {
            artifact: label.to_owned(),
            source,
        })?;
    if !observation.exists
        || observation.kind != Some(StorageObjectKind::File)
        || observation.length != Some(expected_length)
    {
        return Err(VerifiedFileError::UnexpectedArtifact(label.to_owned()));
    }
    Ok(())
}

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
        torrent_id: TorrentId,
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
    ) -> Result<Self, SeedContentError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default seed file pool is valid");
        Self::open_published_with_pool(storage_root, torrent_id, metainfo, verified, skipped, pool)
            .await
    }

    pub async fn open_published_with_pool(
        storage_root: &Path,
        torrent_id: TorrentId,
        metainfo: &Metainfo,
        verified: &[bool],
        skipped: &[usize],
        pool: StorageFilePool,
    ) -> Result<Self, SeedContentError> {
        let layout = TorrentLayout::from_metainfo(metainfo);
        let paths = torrent_storage_paths_for_metainfo(storage_root, metainfo, torrent_id)
            .map_err(SeedContentError::StoragePlan)?;
        let storage_id = torrent_id.to_string();
        let artifact = PublishedArtifactLayout::from_metainfo(metainfo)
            .map_err(SeedContentError::ArtifactLayout)?;
        let namespace_reference = StorageFileReference::new(
            pool.clone(),
            StorageFileKey {
                storage_id: storage_id.clone(),
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
                        storage_id: storage_id.clone(),
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

#[derive(Debug)]
pub enum VerifiedFileError {
    InvalidHaveLength {
        actual: usize,
        expected: usize,
    },
    InvalidFileIndex(usize),
    PaddingFile(usize),
    UnverifiedFile(usize),
    InvalidRange {
        offset: u64,
        length: usize,
        file_length: u64,
    },
    ReadTooLarge {
        actual: usize,
        maximum: usize,
    },
    UnexpectedArtifact(String),
    InvalidPlatformNamespace,
    ReadOwnerClosed,
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

impl fmt::Display for VerifiedFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHaveLength { actual, expected } => write!(
                formatter,
                "have length {actual} does not match piece count {expected}"
            ),
            Self::InvalidFileIndex(index) => {
                write!(formatter, "verified file index {index} is invalid")
            }
            Self::PaddingFile(index) => {
                write!(formatter, "torrent file {index} is padding")
            }
            Self::UnverifiedFile(index) => {
                write!(formatter, "torrent file {index} is not fully verified")
            }
            Self::InvalidRange {
                offset,
                length,
                file_length,
            } => write!(
                formatter,
                "verified file range {offset}+{length} exceeds length {file_length}"
            ),
            Self::ReadTooLarge { actual, maximum } => write!(
                formatter,
                "verified file read length {actual} exceeds bound {maximum}"
            ),
            Self::UnexpectedArtifact(artifact) => write!(
                formatter,
                "verified payload has an unexpected type or length: {artifact}"
            ),
            Self::InvalidPlatformNamespace => {
                formatter.write_str("platform namespace does not match verified published metadata")
            }
            Self::ReadOwnerClosed => formatter.write_str("verified file read owner is closed"),
            Self::ArithmeticOverflow => formatter.write_str("verified file arithmetic overflow"),
            Self::Layout(error) => write!(formatter, "verified file layout: {error}"),
            Self::ArtifactLayout(error) => {
                write!(formatter, "verified file artifact layout: {error}")
            }
            Self::StoragePlan(error) => write!(formatter, "verified file storage plan: {error}"),
            Self::Storage { artifact, source } => {
                write!(formatter, "access verified {artifact}: {source}")
            }
            Self::Io {
                operation,
                artifact,
                source,
            } => write!(formatter, "{operation} {artifact}: {source}"),
            Self::TaskJoin(error) => write!(formatter, "verified file read task: {error}"),
        }
    }
}

impl Error for VerifiedFileError {
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

enum OwnedCounterKind {
    Read,
    Lease,
}

struct OwnedCounterGuard {
    metrics: Arc<ReadMetrics>,
    kind: OwnedCounterKind,
}

impl OwnedCounterGuard {
    fn read(metrics: Arc<ReadMetrics>) -> Self {
        let value = metrics.current_reads.fetch_add(1, Ordering::AcqRel) + 1;
        metrics.read_high_water.fetch_max(value, Ordering::AcqRel);
        Self {
            metrics,
            kind: OwnedCounterKind::Read,
        }
    }

    fn lease(metrics: Arc<ReadMetrics>) -> Self {
        let value = metrics.current_file_leases.fetch_add(1, Ordering::AcqRel) + 1;
        metrics
            .file_lease_high_water
            .fetch_max(value, Ordering::AcqRel);
        Self {
            metrics,
            kind: OwnedCounterKind::Lease,
        }
    }
}

impl Drop for OwnedCounterGuard {
    fn drop(&mut self) {
        match self.kind {
            OwnedCounterKind::Read => {
                self.metrics.current_reads.fetch_sub(1, Ordering::AcqRel);
            }
            OwnedCounterKind::Lease => {
                self.metrics
                    .current_file_leases
                    .fetch_sub(1, Ordering::AcqRel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::peer_wire::BlockRequest;

    use crate::artifact_layout::PublicationShape;
    use crate::identity::TorrentId;
    use crate::selective_storage::PlatformStorageSpec;
    use crate::storage_file_pool::{
        PlatformStorageFailure, PlatformStorageFailureKind, PlatformStorageOperation,
        StorageFileAccess, StorageObjectKind, StorageObservation, platform_storage_channel,
    };

    use super::{
        SeedContent, SeedContentError, StorageFilePool, VerifiedFileError, VerifiedFileReader,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_torrent_id() -> TorrentId {
        TorrentId::new([0x61; 16]).expect("nonzero test owner")
    }

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
        let content =
            SeedContent::open_published(&root, test_torrent_id(), &single(), &[true, true], &[])
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
            test_torrent_id(),
            &multi(),
            &[true, true, true],
            &[],
            pool.clone(),
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
        let truncated = SeedContent::open_published(
            &root,
            test_torrent_id(),
            &multi(),
            &[true, true, true],
            &[],
        )
        .await
        .expect("open with masked source");
        assert_eq!(truncated.availability(), [false, false, true]);
        let skipped = SeedContent::open_published(
            &root,
            test_torrent_id(),
            &multi(),
            &[true, true, true],
            &[0],
        )
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
        let content =
            SeedContent::open_published(&root, test_torrent_id(), &single(), &[true, true], &[])
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
    async fn verified_file_reader_confines_partial_torrent_reads_to_one_file() {
        let root = root("verified-file");
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
        let pool = StorageFilePool::new(1, None).expect("pool");
        let read_jobs = Arc::new(tokio::sync::Semaphore::new(1));
        let reader = VerifiedFileReader::open_published_with_pool(
            &root,
            &multi(),
            &[true, false, false],
            0,
            pool.clone(),
            test_torrent_id(),
            read_jobs.clone(),
        )
        .await
        .expect("first file is verified through its intersecting piece");
        assert_eq!(reader.file_index(), 0);
        assert_eq!(reader.file_name(), "a");
        assert_eq!(reader.length(), 3);
        assert_eq!(reader.read_range(1, 2).await.expect("bounded read"), b"bc");
        assert!(matches!(
            VerifiedFileReader::open_published_with_pool(
                &root,
                &multi(),
                &[true, false, false],
                1,
                pool.clone(),
                test_torrent_id(),
                read_jobs.clone(),
            )
            .await,
            Err(VerifiedFileError::UnverifiedFile(1))
        ));
        assert!(matches!(
            VerifiedFileReader::open_published_with_pool(
                &root,
                &multi(),
                &[true, true, true],
                2,
                pool,
                test_torrent_id(),
                read_jobs,
            )
            .await,
            Err(VerifiedFileError::PaddingFile(2))
        ));
        tokio::fs::write(root.join("tree/a"), b"changed")
            .await
            .expect("replace file length");
        assert!(matches!(
            reader.read_range(0, 1).await,
            Err(VerifiedFileError::UnexpectedArtifact(_))
        ));
        let snapshot = reader.snapshot();
        assert_eq!(snapshot.read_high_water, 1);
        assert_eq!(snapshot.file_lease_high_water, 1);
        assert_eq!(snapshot.current_reads, 0);
        assert_eq!(snapshot.current_file_leases, 0);
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
        let reader = VerifiedFileReader::open_published_with_platform(
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
            &[true, true, false],
            1,
            Arc::new(tokio::sync::Semaphore::new(1)),
        )
        .await
        .expect("open verified platform file");
        assert_eq!(
            reader.read_range(1, 2).await.expect("platform range"),
            b"ef"
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

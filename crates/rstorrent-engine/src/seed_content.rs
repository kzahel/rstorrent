//! Conservative read-only access to verified published path content.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::peer_wire::BlockRequest;
use rstorrent_protocol::storage_layout::{FileSelection, LayoutError, TorrentLayout};

use crate::positional_io::read_exact_at;
use crate::selective_storage::{
    PublicationShape, SelectiveStorageError, torrent_storage_paths_for_metainfo,
};
use crate::storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, StorageFileAccess, StorageFileKey, StorageFileLocator,
    StorageFilePool, StorageFilePoolError, StorageFileReference, StorageFileRole,
};

#[derive(Clone, Debug)]
struct SeedFile {
    path: PathBuf,
    expected_length: u64,
    reference: StorageFileReference,
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
    available: Vec<bool>,
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
        if verified.len() != layout.piece_count() {
            return Err(SeedContentError::InvalidHaveLength {
                actual: verified.len(),
                expected: layout.piece_count(),
            });
        }
        let selection = FileSelection::new(&layout, skipped).map_err(SeedContentError::Layout)?;
        let paths = torrent_storage_paths_for_metainfo(storage_root, metainfo)
            .map_err(SeedContentError::StoragePlan)?;
        let artifact = inspect_path(&paths.output, "inspect published artifact").await?;
        let valid_artifact = match paths.publication_shape {
            PublicationShape::File => artifact.is_file(),
            PublicationShape::Tree => artifact.is_dir(),
        };
        if !valid_artifact || artifact.file_type().is_symlink() {
            return Err(SeedContentError::UnexpectedFileType(paths.output));
        }

        let mut files = Vec::with_capacity(layout.files().len());
        for (index, file) in layout.files().iter().enumerate() {
            if file.padding || !selection.is_wanted(index) {
                files.push(None);
                continue;
            }
            let path = published_payload_path(
                paths.publication_shape,
                &paths.output,
                &file.path,
                index,
                layout.files().len(),
            )?;
            match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata)
                    if metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.len() == file.length =>
                {
                    files.push(Some(SeedFile {
                        reference: StorageFileReference::new(
                            pool.clone(),
                            StorageFileKey {
                                storage_id: storage_id.to_owned(),
                                namespace_generation: 1,
                                role: StorageFileRole::Payload(index),
                            },
                            StorageFileLocator::Path(path.clone()),
                        ),
                        path,
                        expected_length: file.length,
                    }));
                }
                Ok(_) | Err(_) => files.push(None),
            }
        }

        let mut available = Vec::with_capacity(layout.piece_count());
        for (piece, verified) in verified.iter().copied().enumerate() {
            if !verified {
                available.push(false);
                continue;
            }
            let piece = u32::try_from(piece).map_err(|_| SeedContentError::ArithmeticOverflow)?;
            let length = layout
                .piece_length_at(piece)
                .map_err(SeedContentError::Layout)?;
            let readable = layout
                .file_segments(piece, 0, length)
                .map_err(SeedContentError::Layout)?
                .iter()
                .all(|segment| segment.padding || files[segment.file_index].is_some());
            available.push(readable);
        }

        Ok(Self {
            info_hash: metainfo.info_hash,
            private: metainfo.private,
            layout,
            files,
            available,
            metrics: Arc::new(ReadMetrics::default()),
        })
    }

    pub fn availability(&self) -> &[bool] {
        &self.available
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
        if !self.available.get(piece).copied().unwrap_or(false) || request.length == 0 {
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
            let (reference, path, expected_length) = self
                .files
                .get(segment.file_index)
                .and_then(Option::as_ref)
                .map(|file| {
                    (
                        file.reference.clone(),
                        file.path.clone(),
                        file.expected_length,
                    )
                })
                .ok_or(SeedContentError::UnavailablePiece(request.index))?;
            let before = tokio::fs::symlink_metadata(&path).await.map_err(|source| {
                SeedContentError::Io {
                    operation: "inspect seed payload before read",
                    path: path.clone(),
                    source,
                }
            })?;
            if !before.is_file()
                || before.file_type().is_symlink()
                || before.len() != expected_length
            {
                return Err(SeedContentError::UnexpectedFileType(path));
            }
            let handle = reference
                .open(StorageFileAccess::ReadExisting)
                .await
                .map_err(|source| SeedContentError::FilePool {
                    path: path.clone(),
                    source,
                })?;
            let _lease =
                CounterGuard::new(&metrics.current_file_leases, &metrics.file_lease_high_water);
            let segment_block = tokio::task::spawn_blocking(move || {
                let opened = handle
                    .file()
                    .metadata()
                    .map_err(|source| SeedContentError::Io {
                        operation: "inspect open seed payload",
                        path: path.clone(),
                        source,
                    })?;
                if !opened.is_file() || opened.len() != expected_length {
                    return Err(SeedContentError::UnexpectedFileType(path.clone()));
                }
                let mut bytes = vec![0; segment.length];
                read_exact_at(handle.file(), &mut bytes, segment.file_offset).map_err(
                    |source| SeedContentError::Io {
                        operation: "read seed payload",
                        path,
                        source,
                    },
                )?;
                Ok(bytes)
            })
            .await
            .map_err(|error| SeedContentError::TaskJoin(error.to_string()))??;
            let end = segment
                .block_offset
                .checked_add(segment.length)
                .ok_or(SeedContentError::ArithmeticOverflow)?;
            block[segment.block_offset..end].copy_from_slice(&segment_block);
        }
        Ok(block)
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
    UnexpectedFileType(PathBuf),
    ArithmeticOverflow,
    Layout(LayoutError),
    StoragePlan(SelectiveStorageError),
    FilePool {
        path: PathBuf,
        source: StorageFilePoolError,
    },
    Io {
        operation: &'static str,
        path: PathBuf,
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
            Self::UnexpectedFileType(path) => {
                write!(
                    formatter,
                    "seed payload has an unexpected type or length: {}",
                    path.display()
                )
            }
            Self::ArithmeticOverflow => formatter.write_str("seed content arithmetic overflow"),
            Self::Layout(error) => write!(formatter, "seed layout: {error}"),
            Self::StoragePlan(error) => write!(formatter, "seed storage plan: {error}"),
            Self::FilePool { path, source } => {
                write!(formatter, "open seed payload {}: {source}", path.display())
            }
            Self::Io {
                operation,
                path,
                source,
            } => {
                write!(formatter, "{operation} {}: {source}", path.display())
            }
            Self::TaskJoin(error) => write!(formatter, "seed read task: {error}"),
        }
    }
}

impl Error for SeedContentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::StoragePlan(error) => Some(error),
            Self::FilePool { source, .. } => Some(source),
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

async fn inspect_path(
    path: &Path,
    operation: &'static str,
) -> Result<std::fs::Metadata, SeedContentError> {
    tokio::fs::symlink_metadata(path)
        .await
        .map_err(|source| SeedContentError::Io {
            operation,
            path: path.to_path_buf(),
            source,
        })
}

fn hex(bytes: [u8; 20]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(40);
    for byte in bytes {
        write!(&mut encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    encoded
}

fn published_payload_path(
    shape: PublicationShape,
    root: &Path,
    components: &[String],
    file_index: usize,
    file_count: usize,
) -> Result<PathBuf, SeedContentError> {
    match shape {
        PublicationShape::File if file_index == 0 && file_count == 1 => Ok(root.to_path_buf()),
        PublicationShape::File => Err(SeedContentError::ArithmeticOverflow),
        PublicationShape::Tree => Ok(components
            .iter()
            .fold(root.to_path_buf(), |path, part| path.join(part))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::peer_wire::BlockRequest;

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
}

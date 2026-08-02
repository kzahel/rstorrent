use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use rstorrent_protocol::metainfo::Metainfo;
use rstorrent_protocol::storage_layout::{
    FileSelection, LayoutError, LayoutSegment, SegmentTarget, TorrentLayout,
};
use sha1::{Digest, Sha1};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::checkpoint::DurabilityTarget;
use crate::part_file::{PartFile, PartFileError, PartFileIdentity};
use crate::storage::VERIFICATION_CHUNK_LENGTH;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectiveWriteStats {
    pub wanted_bytes: usize,
    pub skipped_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterializationReport {
    pub file_index: usize,
    pub bytes: u64,
    pub slots_before: usize,
    pub slots_after: usize,
}

#[derive(Debug)]
pub struct DescriptorFile {
    pub file_index: usize,
    pub file: std::fs::File,
}

#[derive(Debug)]
pub struct DescriptorStorage {
    pub wanted_files: Vec<DescriptorFile>,
    pub part_file: std::fs::File,
    pub reopened_part_file: std::fs::File,
    pub materialization_files: Vec<DescriptorFile>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorFileRole {
    Wanted,
    Skipped,
    Padding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorStoragePlanFile {
    pub file_index: usize,
    pub path: Vec<String>,
    pub length: u64,
    pub role: DescriptorFileRole,
    pub materialize: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorStoragePlan {
    pub info_hash: [u8; 20],
    pub name: String,
    pub files: Vec<DescriptorStoragePlanFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFileHash {
    pub file_index: usize,
    pub length: u64,
    pub sha1: [u8; 20],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumedStorage {
    Created,
    Staging,
    Published,
}

#[derive(Debug)]
pub enum SelectiveStorageError {
    InvalidOutputPath,
    ExistingOutput(PathBuf),
    ExistingStaging(PathBuf),
    ExistingPartFile(PathBuf),
    ExistingMaterialization(PathBuf),
    IncompleteResumeArtifacts,
    UnexpectedFileType {
        path: PathBuf,
    },
    UnexpectedFileLength {
        file_index: usize,
        expected: u64,
        actual: u64,
    },
    Layout(LayoutError),
    PartFile(PartFileError),
    MissingWantedFile {
        file_index: usize,
    },
    InvalidDescriptorManifest {
        role: &'static str,
        file_index: usize,
        reason: &'static str,
    },
    NonemptyDescriptor {
        role: &'static str,
        file_index: usize,
        length: u64,
    },
    PaddingInPeerBlock,
    InvalidVerifiedPiece {
        piece_index: usize,
    },
    IncompleteSelection {
        piece_index: usize,
    },
    PreparedHashMismatch {
        file_index: usize,
    },
    NotPublished,
    InvalidStorageOperation(&'static str),
    AlreadyWanted {
        file_index: usize,
    },
    IncompleteMaterialization {
        file_index: usize,
        piece_index: usize,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for SelectiveStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOutputPath => write!(formatter, "output path has no file name"),
            Self::ExistingOutput(path) => {
                write!(formatter, "output already exists: {}", path.display())
            }
            Self::ExistingStaging(path) => {
                write!(
                    formatter,
                    "staging output already exists: {}",
                    path.display()
                )
            }
            Self::ExistingPartFile(path) => {
                write!(formatter, "part file already exists: {}", path.display())
            }
            Self::ExistingMaterialization(path) => write!(
                formatter,
                "materialization path already exists: {}",
                path.display()
            ),
            Self::IncompleteResumeArtifacts => write!(
                formatter,
                "resumable storage must contain one selected tree and its part file"
            ),
            Self::UnexpectedFileType { path } => {
                write!(
                    formatter,
                    "resumable storage is not a regular file: {}",
                    path.display()
                )
            }
            Self::UnexpectedFileLength {
                file_index,
                expected,
                actual,
            } => write!(
                formatter,
                "resumable file {file_index} has length {actual}, expected {expected}"
            ),
            Self::Layout(error) => write!(formatter, "storage layout: {error}"),
            Self::PartFile(error) => write!(formatter, "part file: {error}"),
            Self::MissingWantedFile { file_index } => {
                write!(formatter, "wanted file {file_index} is not open")
            }
            Self::InvalidDescriptorManifest {
                role,
                file_index,
                reason,
            } => write!(
                formatter,
                "{role} descriptor for file {file_index} is invalid: {reason}"
            ),
            Self::NonemptyDescriptor {
                role,
                file_index,
                length,
            } => write!(
                formatter,
                "{role} descriptor for file {file_index} is not empty: {length} bytes"
            ),
            Self::PaddingInPeerBlock => {
                write!(formatter, "peer block unexpectedly includes padding")
            }
            Self::InvalidVerifiedPiece { piece_index } => {
                write!(formatter, "verified piece index {piece_index} is invalid")
            }
            Self::IncompleteSelection { piece_index } => {
                write!(formatter, "required piece {piece_index} is not verified")
            }
            Self::PreparedHashMismatch { file_index } => {
                write!(
                    formatter,
                    "published file {file_index} hash differs from preparation"
                )
            }
            Self::NotPublished => {
                write!(formatter, "selected tree is not published")
            }
            Self::InvalidStorageOperation(operation) => {
                write!(formatter, "{operation} is invalid for this storage backing")
            }
            Self::AlreadyWanted { file_index } => {
                write!(formatter, "file {file_index} is already wanted")
            }
            Self::IncompleteMaterialization {
                file_index,
                piece_index,
            } => write!(
                formatter,
                "file {file_index} cannot be materialized without verified piece {piece_index}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl Error for SelectiveStorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::PartFile(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<LayoutError> for SelectiveStorageError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<PartFileError> for SelectiveStorageError {
    fn from(error: PartFileError) -> Self {
        Self::PartFile(error)
    }
}

#[derive(Debug)]
enum StorageBacking {
    Paths {
        output_root: PathBuf,
        staging_root: PathBuf,
        part_path: PathBuf,
    },
    Descriptors {
        reopened_part_file: Option<std::fs::File>,
        materialization_files: Vec<Option<std::fs::File>>,
    },
}

#[derive(Debug)]
pub struct SelectiveStorage {
    backing: StorageBacking,
    identity: PartFileIdentity,
    layout: TorrentLayout,
    selection: FileSelection,
    files: Vec<Option<File>>,
    part_file: Option<PartFile>,
    verified: Vec<bool>,
    published: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SelectiveHashIoStats {
    wanted_file_seeks: usize,
    wanted_file_reads: usize,
    part_file_reads: usize,
    wanted_file_duplicates: usize,
    blocking_jobs: usize,
}

#[derive(Debug)]
struct BlockingHashPlan {
    files: Vec<std::fs::File>,
    spans: Vec<BlockingHashSpan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockingHashSpan {
    WantedFile {
        file_slot: usize,
        file_offset: u64,
        length: usize,
    },
    Padding {
        length: usize,
    },
}

type BlockingHashResult = Result<([u8; 20], SelectiveHashIoStats), SelectiveStorageError>;

impl BlockingHashPlan {
    fn hash(mut self) -> BlockingHashResult {
        let mut hasher = Sha1::new();
        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        let mut stats = SelectiveHashIoStats {
            wanted_file_duplicates: self.files.len(),
            blocking_jobs: 1,
            ..SelectiveHashIoStats::default()
        };
        for span in self.spans {
            let mut consumed = 0_usize;
            match span {
                BlockingHashSpan::WantedFile {
                    file_slot,
                    file_offset,
                    length: span_length,
                } => {
                    let file = self.files.get_mut(file_slot).ok_or(
                        SelectiveStorageError::InvalidStorageOperation(
                            "blocking hash file slot is absent",
                        ),
                    )?;
                    while consumed < span_length {
                        let length = (span_length - consumed).min(buffer.len());
                        let offset = file_offset
                            .checked_add(u64::try_from(consumed).map_err(|_| {
                                SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                            })?)
                            .ok_or(SelectiveStorageError::Layout(
                                LayoutError::ArithmeticOverflow,
                            ))?;
                        read_exact_at(file, &mut buffer[..length], offset).map_err(|source| {
                            SelectiveStorageError::Io {
                                operation:
                                    "read selected staging range in blocking verification job",
                                source,
                            }
                        })?;
                        stats.wanted_file_reads = stats.wanted_file_reads.saturating_add(1);
                        hasher.update(&buffer[..length]);
                        consumed += length;
                    }
                }
                BlockingHashSpan::Padding {
                    length: span_length,
                } => {
                    buffer.fill(0);
                    while consumed < span_length {
                        let length = (span_length - consumed).min(buffer.len());
                        hasher.update(&buffer[..length]);
                        consumed += length;
                    }
                }
            }
        }
        Ok((hasher.finalize().into(), stats))
    }
}

async fn await_blocking_hash(
    task: tokio::task::JoinHandle<BlockingHashResult>,
) -> BlockingHashResult {
    task.await.map_err(|source| SelectiveStorageError::Io {
        operation: "join selected piece blocking verification job",
        source: io::Error::other(source),
    })?
}

fn read_exact_at(
    file: &mut std::fs::File,
    mut bytes: &mut [u8],
    mut offset: u64,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match positional_read(file, bytes, offset) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "selected staging file ended during piece verification",
                ));
            }
            Ok(read) => {
                offset = offset
                    .checked_add(read as u64)
                    .ok_or_else(|| io::Error::other("positional read offset overflow"))?;
                bytes = &mut bytes[read..];
            }
            Err(source) if source.kind() == io::ErrorKind::Interrupted => {}
            Err(source) => return Err(source),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn positional_read(file: &mut std::fs::File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;

    file.read_at(bytes, offset)
}

#[cfg(windows)]
fn positional_read(file: &mut std::fs::File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;

    file.seek_read(bytes, offset)
}

#[cfg(not(any(unix, windows)))]
fn positional_read(file: &mut std::fs::File, bytes: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::io::{Read, Seek, SeekFrom};

    file.seek(SeekFrom::Start(offset))?;
    file.read(bytes)
}

impl SelectiveStorage {
    pub async fn create(
        output_root: PathBuf,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
    ) -> Result<Self, SelectiveStorageError> {
        let staging_root = selective_staging_path(&output_root)?;
        let part_path = selective_part_path(&output_root)?;
        if path_exists(&output_root, "inspect selected output").await? {
            return Err(SelectiveStorageError::ExistingOutput(output_root));
        }
        if path_exists(&staging_root, "inspect selected staging").await? {
            return Err(SelectiveStorageError::ExistingStaging(staging_root));
        }
        if path_exists(&part_path, "inspect selected part file").await? {
            return Err(SelectiveStorageError::ExistingPartFile(part_path));
        }

        tokio::fs::create_dir(&staging_root)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "create selected staging root",
                source,
            })?;

        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding || !selection.is_wanted(file_index) {
                files.push(None);
                continue;
            }
            let path = joined_path(&staging_root, &metainfo_file.path);
            let parent = path
                .parent()
                .ok_or(SelectiveStorageError::InvalidOutputPath)?;
            tokio::fs::create_dir_all(parent).await.map_err(|source| {
                SelectiveStorageError::Io {
                    operation: "create selected file parent",
                    source,
                }
            })?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "create selected staging file",
                    source,
                })?;
            file.set_len(metainfo_file.length).await.map_err(|source| {
                SelectiveStorageError::Io {
                    operation: "size selected staging file",
                    source,
                }
            })?;
            files.push(Some(file));
        }

        let identity = PartFileIdentity {
            info_hash: metainfo.info_hash,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        };
        let part_file = PartFile::create(part_path.clone(), identity).await?;
        let piece_count = layout.piece_count();

        Ok(Self {
            backing: StorageBacking::Paths {
                output_root,
                staging_root,
                part_path,
            },
            identity,
            layout,
            selection,
            files,
            part_file: Some(part_file),
            verified: vec![false; piece_count],
            published: false,
        })
    }

    pub async fn create_with_descriptors(
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        materialize_files: &[usize],
        descriptors: DescriptorStorage,
    ) -> Result<Self, SelectiveStorageError> {
        let mut wanted_files =
            collect_descriptors(layout.files().len(), "wanted", descriptors.wanted_files)?;
        let mut materialization_files = collect_descriptors(
            layout.files().len(),
            "materialization",
            descriptors.materialization_files,
        )?;

        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            let expected = !metainfo_file.padding && selection.is_wanted(file_index);
            let provided = wanted_files[file_index].take();
            match (expected, provided) {
                (true, Some(file)) => {
                    files.push(Some(
                        initialize_descriptor_file(
                            file,
                            "wanted",
                            file_index,
                            metainfo_file.length,
                        )
                        .await?,
                    ));
                }
                (true, None) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index,
                        reason: "required descriptor is missing",
                    });
                }
                (false, Some(_)) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index,
                        reason: "descriptor is not expected by the selection",
                    });
                }
                (false, None) => files.push(None),
            }
        }

        let mut materialization_expected = vec![false; layout.files().len()];
        for &file_index in materialize_files {
            let expected = materialization_expected.get_mut(file_index).ok_or(
                SelectiveStorageError::InvalidDescriptorManifest {
                    role: "materialization",
                    file_index,
                    reason: "file index is out of range",
                },
            )?;
            if *expected {
                return Err(SelectiveStorageError::InvalidDescriptorManifest {
                    role: "materialization",
                    file_index,
                    reason: "file index is duplicated",
                });
            }
            *expected = true;
        }
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            let provided = materialization_files[file_index].take();
            match (materialization_expected[file_index], provided) {
                (true, Some(file)) => {
                    materialization_files[file_index] =
                        Some(validate_empty_descriptor(file, "materialization", file_index).await?);
                }
                (true, None) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "materialization",
                        file_index,
                        reason: "required descriptor is missing",
                    });
                }
                (false, Some(_)) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "materialization",
                        file_index,
                        reason: "descriptor is not expected",
                    });
                }
                (false, None) => {}
            }
            if materialization_expected[file_index]
                && (metainfo_file.padding || selection.is_wanted(file_index))
            {
                return Err(SelectiveStorageError::InvalidDescriptorManifest {
                    role: "materialization",
                    file_index,
                    reason: "file is padding or already wanted",
                });
            }
        }

        let identity = PartFileIdentity {
            info_hash: metainfo.info_hash,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        };
        let part_file = PartFile::create_preopened(descriptors.part_file, identity).await?;
        let validation_reopen = descriptors
            .reopened_part_file
            .try_clone()
            .map_err(|source| SelectiveStorageError::Io {
                operation: "duplicate descriptor for part-file identity validation",
                source,
            })?;
        drop(PartFile::open_preopened(validation_reopen, identity).await?);
        let piece_count = layout.piece_count();

        Ok(Self {
            backing: StorageBacking::Descriptors {
                reopened_part_file: Some(descriptors.reopened_part_file),
                materialization_files,
            },
            identity,
            layout,
            selection,
            files,
            part_file: Some(part_file),
            verified: vec![false; piece_count],
            published: false,
        })
    }

    pub async fn resume_with_descriptors(
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        descriptors: DescriptorStorage,
        verified: Vec<bool>,
    ) -> Result<Self, SelectiveStorageError> {
        if verified.len() != layout.piece_count() {
            return Err(SelectiveStorageError::InvalidVerifiedPiece {
                piece_index: verified.len(),
            });
        }
        let mut wanted_files =
            collect_descriptors(layout.files().len(), "wanted", descriptors.wanted_files)?;
        if !descriptors.materialization_files.is_empty() {
            return Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "materialization",
                file_index: descriptors.materialization_files[0].file_index,
                reason: "materialization is not supported while resuming",
            });
        }

        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            let expected = !metainfo_file.padding && selection.is_wanted(file_index);
            let provided = wanted_files[file_index].take();
            match (expected, provided) {
                (true, Some(file)) => {
                    files.push(Some(
                        validate_descriptor_length(file, file_index, metainfo_file.length).await?,
                    ));
                }
                (true, None) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index,
                        reason: "required descriptor is missing",
                    });
                }
                (false, Some(_)) => {
                    return Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index,
                        reason: "descriptor is not expected by the selection",
                    });
                }
                (false, None) => files.push(None),
            }
        }

        let identity = PartFileIdentity {
            info_hash: metainfo.info_hash,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        };
        let part_file = PartFile::open_preopened(descriptors.part_file, identity).await?;
        let validation_reopen = descriptors
            .reopened_part_file
            .try_clone()
            .map_err(|source| SelectiveStorageError::Io {
                operation: "duplicate descriptor for resumed part-file identity validation",
                source,
            })?;
        drop(PartFile::open_preopened(validation_reopen, identity).await?);

        Ok(Self {
            backing: StorageBacking::Descriptors {
                reopened_part_file: Some(descriptors.reopened_part_file),
                materialization_files: (0..layout.files().len()).map(|_| None).collect(),
            },
            identity,
            layout,
            selection,
            files,
            part_file: Some(part_file),
            verified,
            published: false,
        })
    }

    pub async fn resume(
        output_root: PathBuf,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        if verified.len() != layout.piece_count() {
            return Err(SelectiveStorageError::InvalidVerifiedPiece {
                piece_index: verified.len(),
            });
        }
        let staging_root = selective_staging_path(&output_root)?;
        let part_path = selective_part_path(&output_root)?;
        let output_exists = path_exists(&output_root, "inspect resumable selected output").await?;
        let staging_exists =
            path_exists(&staging_root, "inspect resumable selected staging").await?;
        let part_exists = path_exists(&part_path, "inspect resumable part file").await?;

        if !output_exists && !staging_exists && !part_exists {
            let storage = Self::create(output_root, metainfo, layout, selection).await?;
            return Ok((storage, ResumedStorage::Created));
        }
        if output_exists == staging_exists || !part_exists {
            return Err(SelectiveStorageError::IncompleteResumeArtifacts);
        }

        let (tree_root, resumed, published) = if output_exists {
            (&output_root, ResumedStorage::Published, true)
        } else {
            (&staging_root, ResumedStorage::Staging, false)
        };
        let tree_metadata = tokio::fs::symlink_metadata(tree_root)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "inspect resumable selected tree",
                source,
            })?;
        if !tree_metadata.is_dir() || tree_metadata.file_type().is_symlink() {
            return Err(SelectiveStorageError::UnexpectedFileType {
                path: tree_root.clone(),
            });
        }

        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding || !selection.is_wanted(file_index) {
                files.push(None);
                continue;
            }
            let path = joined_path(tree_root, &metainfo_file.path);
            let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|source| {
                SelectiveStorageError::Io {
                    operation: "inspect resumable selected file",
                    source,
                }
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(SelectiveStorageError::UnexpectedFileType { path });
            }
            if metadata.len() != metainfo_file.length {
                return Err(SelectiveStorageError::UnexpectedFileLength {
                    file_index,
                    expected: metainfo_file.length,
                    actual: metadata.len(),
                });
            }
            files.push(Some(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .await
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "open resumable selected file",
                        source,
                    })?,
            ));
        }

        let identity = PartFileIdentity {
            info_hash: metainfo.info_hash,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        };
        let part_file = PartFile::open(part_path.clone(), identity).await?;
        Ok((
            Self {
                backing: StorageBacking::Paths {
                    output_root,
                    staging_root,
                    part_path,
                },
                identity,
                layout,
                selection,
                files,
                part_file: Some(part_file),
                verified,
                published,
            },
            resumed,
        ))
    }

    pub fn selected_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| !file.padding && self.selection.is_wanted(*index))
            .map(|(_, file)| file.length)
            .sum()
    }

    pub fn skipped_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| !file.padding && !self.selection.is_wanted(*index))
            .map(|(_, file)| file.length)
            .sum()
    }

    pub fn padding_bytes(&self) -> u64 {
        self.layout
            .files()
            .iter()
            .filter(|file| file.padding)
            .map(|file| file.length)
            .sum()
    }

    pub fn part_path(&self) -> Option<&Path> {
        match &self.backing {
            StorageBacking::Paths { part_path, .. } => Some(part_path),
            StorageBacking::Descriptors { .. } => None,
        }
    }

    pub fn part_slots(&self) -> usize {
        self.part_file
            .as_ref()
            .map_or(0, PartFile::mapped_piece_count)
    }

    pub fn is_published(&self) -> bool {
        self.published
    }

    pub async fn write_block(
        &mut self,
        piece_index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        let (segments, stats) = self.plan_write(piece_index, begin, bytes.len())?;
        for segment in segments {
            let segment_bytes = &bytes[segment.block_offset..segment.block_offset + segment.length];
            match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let file = self.files[file_index]
                        .as_mut()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    file.seek(SeekFrom::Start(file_offset))
                        .await
                        .map_err(|source| SelectiveStorageError::Io {
                            operation: "seek selected staging file for write",
                            source,
                        })?;
                    file.write_all(segment_bytes).await.map_err(|source| {
                        SelectiveStorageError::Io {
                            operation: "write selected staging range",
                            source,
                        }
                    })?;
                }
                SegmentTarget::SkippedFile { .. } => {
                    self.part_file_mut()?
                        .write_piece_range(
                            usize::try_from(piece_index).map_err(|_| {
                                SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                            })?,
                            segment.piece_offset,
                            segment_bytes,
                        )
                        .await?;
                }
                SegmentTarget::Padding => {
                    unreachable!("padding was rejected while planning the write");
                }
            }
        }
        Ok(stats)
    }

    pub(crate) fn write_stats(
        &self,
        piece_index: u32,
        begin: u32,
        length: usize,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        self.plan_write(piece_index, begin, length)
            .map(|(_, stats)| stats)
    }

    fn plan_write(
        &self,
        piece_index: u32,
        begin: u32,
        length: usize,
    ) -> Result<(Vec<LayoutSegment>, SelectiveWriteStats), SelectiveStorageError> {
        let length = u32::try_from(length)
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        let segments = self
            .layout
            .segments(piece_index, begin, length, &self.selection)?;
        let mut stats = SelectiveWriteStats::default();
        for segment in &segments {
            match segment.target {
                SegmentTarget::WantedFile { .. } => {
                    stats.wanted_bytes = stats.wanted_bytes.saturating_add(segment.length);
                }
                SegmentTarget::SkippedFile { .. } => {
                    stats.skipped_bytes = stats.skipped_bytes.saturating_add(segment.length);
                }
                SegmentTarget::Padding => {
                    return Err(SelectiveStorageError::PaddingInPeerBlock);
                }
            }
        }
        Ok((segments, stats))
    }

    pub async fn hash_piece(
        &mut self,
        piece_index: u32,
    ) -> Result<[u8; 20], SelectiveStorageError> {
        self.hash_piece_with_stats(piece_index)
            .await
            .map(|(hash, _stats)| hash)
    }

    async fn prepare_blocking_hash_plan(
        &self,
        segments: &[LayoutSegment],
    ) -> Result<Option<BlockingHashPlan>, SelectiveStorageError> {
        if segments
            .iter()
            .any(|segment| matches!(segment.target, SegmentTarget::SkippedFile { .. }))
        {
            return Ok(None);
        }

        let mut file_indices = Vec::new();
        let mut files = Vec::new();
        let mut spans = Vec::with_capacity(segments.len());
        for segment in segments {
            match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let file_slot = if let Some(slot) = file_indices
                        .iter()
                        .position(|existing| *existing == file_index)
                    {
                        slot
                    } else {
                        let file = self.files[file_index]
                            .as_ref()
                            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                            .try_clone()
                            .await
                            .map_err(|source| SelectiveStorageError::Io {
                                operation:
                                    "duplicate selected staging file for blocking verification",
                                source,
                            })?
                            .into_std()
                            .await;
                        let slot = files.len();
                        file_indices.push(file_index);
                        files.push(file);
                        slot
                    };
                    spans.push(BlockingHashSpan::WantedFile {
                        file_slot,
                        file_offset,
                        length: segment.length,
                    });
                }
                SegmentTarget::Padding => spans.push(BlockingHashSpan::Padding {
                    length: segment.length,
                }),
                SegmentTarget::SkippedFile { .. } => {
                    return Err(SelectiveStorageError::InvalidStorageOperation(
                        "skipped segment entered all-wanted blocking hash plan",
                    ));
                }
            }
        }
        Ok(Some(BlockingHashPlan { files, spans }))
    }

    async fn hash_piece_with_stats(
        &mut self,
        piece_index: u32,
    ) -> Result<([u8; 20], SelectiveHashIoStats), SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let segments = self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?;
        if let Some(plan) = self.prepare_blocking_hash_plan(&segments).await? {
            return await_blocking_hash(tokio::task::spawn_blocking(move || plan.hash())).await;
        }

        let mut hasher = Sha1::new();
        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        let mut stats = SelectiveHashIoStats::default();
        for segment in segments {
            let mut consumed = 0_usize;
            match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let file = self.files[file_index]
                        .as_mut()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    file.seek(SeekFrom::Start(file_offset))
                        .await
                        .map_err(|source| SelectiveStorageError::Io {
                            operation: "seek selected staging file for verification",
                            source,
                        })?;
                    stats.wanted_file_seeks = stats.wanted_file_seeks.saturating_add(1);
                    while consumed < segment.length {
                        let length = (segment.length - consumed).min(buffer.len());
                        file.read_exact(&mut buffer[..length])
                            .await
                            .map_err(|source| SelectiveStorageError::Io {
                                operation: "read selected staging range for verification",
                                source,
                            })?;
                        stats.wanted_file_reads = stats.wanted_file_reads.saturating_add(1);
                        hasher.update(&buffer[..length]);
                        consumed += length;
                    }
                }
                SegmentTarget::SkippedFile { .. } => {
                    let piece = usize::try_from(piece_index).map_err(|_| {
                        SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                    })?;
                    while consumed < segment.length {
                        let length = (segment.length - consumed).min(buffer.len());
                        let consumed_u32 = u32::try_from(consumed).map_err(|_| {
                            SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                        })?;
                        let begin = segment.piece_offset.checked_add(consumed_u32).ok_or(
                            SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow),
                        )?;
                        self.part_file_mut()?
                            .read_piece_range(piece, begin, &mut buffer[..length])
                            .await?;
                        stats.part_file_reads = stats.part_file_reads.saturating_add(1);
                        hasher.update(&buffer[..length]);
                        consumed += length;
                    }
                }
                SegmentTarget::Padding => {
                    buffer.fill(0);
                    while consumed < segment.length {
                        let length = (segment.length - consumed).min(buffer.len());
                        hasher.update(&buffer[..length]);
                        consumed += length;
                    }
                }
            }
        }
        Ok((hasher.finalize().into(), stats))
    }

    pub fn record_verified(&mut self, piece_index: usize) -> Result<(), SelectiveStorageError> {
        self.set_verified(piece_index, true)
    }

    pub(crate) fn durability_targets(
        &self,
        piece_index: u32,
    ) -> Result<Vec<DurabilityTarget>, SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let segments = self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?;
        let mut targets = Vec::new();
        for segment in segments {
            let target = match segment.target {
                SegmentTarget::WantedFile { file_index, .. } => {
                    DurabilityTarget::WantedFile(file_index)
                }
                SegmentTarget::SkippedFile { .. } => DurabilityTarget::PartFile,
                SegmentTarget::Padding => continue,
            };
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    pub(crate) async fn checkpoint_handles(
        &self,
    ) -> Result<BTreeMap<DurabilityTarget, std::fs::File>, SelectiveStorageError> {
        let mut handles = BTreeMap::new();
        for (file_index, file) in self.files.iter().enumerate() {
            let Some(file) = file else {
                continue;
            };
            let handle = file
                .try_clone()
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "duplicate selected file for durability checkpoint",
                    source,
                })?
                .into_std()
                .await;
            handles.insert(DurabilityTarget::WantedFile(file_index), handle);
        }
        let part_file = self
            .part_file
            .as_ref()
            .ok_or(SelectiveStorageError::NotPublished)?;
        handles.insert(
            DurabilityTarget::PartFile,
            part_file.duplicate_for_checkpoint().await?,
        );
        Ok(handles)
    }

    pub fn set_verified(
        &mut self,
        piece_index: usize,
        verified: bool,
    ) -> Result<(), SelectiveStorageError> {
        let piece = self
            .verified
            .get_mut(piece_index)
            .ok_or(SelectiveStorageError::InvalidVerifiedPiece { piece_index })?;
        *piece = verified;
        Ok(())
    }

    pub async fn sync_piece(&mut self, piece_index: u32) -> Result<(), SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let segments = self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?;
        let mut wanted_files = Vec::new();
        let mut sync_part = false;
        for segment in segments {
            match segment.target {
                SegmentTarget::WantedFile { file_index, .. } => {
                    if !wanted_files.contains(&file_index) {
                        wanted_files.push(file_index);
                    }
                }
                SegmentTarget::SkippedFile { .. } => sync_part = true,
                SegmentTarget::Padding => {}
            }
        }
        for file_index in wanted_files {
            self.files[file_index]
                .as_ref()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                .sync_data()
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "flush verified selected piece",
                    source,
                })?;
        }
        if sync_part {
            self.part_file_mut()?.sync_payload().await?;
        }
        Ok(())
    }

    pub async fn publish(&mut self) -> Result<(), SelectiveStorageError> {
        self.sync_verified().await?;
        let (output_root, staging_root) = match &self.backing {
            StorageBacking::Paths {
                output_root,
                staging_root,
                ..
            } => (output_root.clone(), staging_root.clone()),
            StorageBacking::Descriptors { .. } => {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "path publication",
                ));
            }
        };

        if path_exists(&output_root, "inspect selected output before publish").await? {
            return Err(SelectiveStorageError::ExistingOutput(output_root));
        }
        tokio::fs::rename(&staging_root, &output_root)
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "publish selected tree",
                source,
            })?;
        self.published = true;
        Ok(())
    }

    pub async fn prepare_descriptors(&mut self) -> Result<(), SelectiveStorageError> {
        if !matches!(self.backing, StorageBacking::Descriptors { .. }) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "descriptor preparation",
            ));
        }
        self.sync_verified().await?;
        self.published = true;
        Ok(())
    }

    pub async fn finish_published(&mut self) -> Result<(), SelectiveStorageError> {
        if !self.published {
            return Err(SelectiveStorageError::NotPublished);
        }
        self.sync_verified().await
    }

    async fn sync_verified(&mut self) -> Result<(), SelectiveStorageError> {
        for piece_index in 0..self.layout.piece_count() {
            let piece_index_u32 = u32::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self
                .layout
                .request_ranges(piece_index_u32, &self.selection)?
                .is_empty()
                && !self.verified[piece_index]
            {
                return Err(SelectiveStorageError::IncompleteSelection { piece_index });
            }
        }

        for file in self.files.iter().flatten() {
            file.sync_data()
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "flush selected staging file",
                    source,
                })?;
        }
        self.part_file_mut()?.sync_payload().await?;
        if matches!(self.backing, StorageBacking::Paths { .. }) {
            self.files.iter_mut().for_each(|file| {
                file.take();
            });
        }
        Ok(())
    }

    pub async fn reopen_part_file(&mut self) -> Result<(), SelectiveStorageError> {
        self.part_file.take();
        self.part_file = Some(match &mut self.backing {
            StorageBacking::Paths { part_path, .. } => {
                PartFile::open(part_path.clone(), self.identity).await?
            }
            StorageBacking::Descriptors {
                reopened_part_file, ..
            } => {
                let file = reopened_part_file.take().ok_or(
                    SelectiveStorageError::InvalidStorageOperation(
                        "descriptor part file was already reopened",
                    ),
                )?;
                PartFile::open_preopened(file, self.identity).await?
            }
        });
        Ok(())
    }

    pub async fn materialize_file(
        &mut self,
        file_index: usize,
    ) -> Result<MaterializationReport, SelectiveStorageError> {
        if !self.published {
            return Err(SelectiveStorageError::NotPublished);
        }
        let metainfo_file = self
            .layout
            .files()
            .get(file_index)
            .ok_or(LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: self.layout.files().len(),
            })?
            .clone();
        if self.selection.is_wanted(file_index) {
            return Err(SelectiveStorageError::AlreadyWanted { file_index });
        }
        for piece_index in self.layout.file_pieces(file_index)? {
            let piece_index_usize = usize::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self.verified[piece_index_usize] {
                return Err(SelectiveStorageError::IncompleteMaterialization {
                    file_index,
                    piece_index: piece_index_usize,
                });
            }
        }

        let (mut output, paths) = match &mut self.backing {
            StorageBacking::Paths { output_root, .. } => {
                let destination = joined_path(output_root, &metainfo_file.path);
                let temporary = materialization_path(&destination)?;
                if path_exists(&destination, "inspect materialized output").await? {
                    return Err(SelectiveStorageError::ExistingMaterialization(destination));
                }
                if path_exists(&temporary, "inspect materialization staging").await? {
                    return Err(SelectiveStorageError::ExistingMaterialization(temporary));
                }
                let parent = destination
                    .parent()
                    .ok_or(SelectiveStorageError::InvalidOutputPath)?;
                tokio::fs::create_dir_all(parent).await.map_err(|source| {
                    SelectiveStorageError::Io {
                        operation: "create materialized file parent",
                        source,
                    }
                })?;
                let output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .await
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "create materialization staging file",
                        source,
                    })?;
                (output, Some((temporary, destination)))
            }
            StorageBacking::Descriptors {
                materialization_files,
                ..
            } => {
                let file = materialization_files
                    .get_mut(file_index)
                    .and_then(Option::take)
                    .ok_or(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "materialization",
                        file_index,
                        reason: "descriptor was already consumed",
                    })?;
                (File::from_std(file), None)
            }
        };
        let write_result = async {
            output
                .set_len(metainfo_file.length)
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "size materialization staging file",
                    source,
                })?;

            let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
            let mut file_offset = 0_u64;
            while file_offset < metainfo_file.length {
                let torrent_offset = metainfo_file.offset.checked_add(file_offset).ok_or(
                    SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow),
                )?;
                let piece_index = torrent_offset / u64::from(self.layout.piece_length());
                let piece_offset_u64 = torrent_offset % u64::from(self.layout.piece_length());
                let piece_offset = u32::try_from(piece_offset_u64)
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
                let piece_remaining = u64::from(self.layout.piece_length()) - piece_offset_u64;
                let length = usize::try_from(
                    (metainfo_file.length - file_offset)
                        .min(piece_remaining)
                        .min(buffer.len() as u64),
                )
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
                self.part_file_mut()?
                    .read_piece_range(
                        usize::try_from(piece_index).map_err(|_| {
                            SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                        })?,
                        piece_offset,
                        &mut buffer[..length],
                    )
                    .await?;
                output
                    .write_all(&buffer[..length])
                    .await
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "write materialization staging file",
                        source,
                    })?;
                file_offset += length as u64;
            }
            output
                .sync_data()
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "flush materialization staging file",
                    source,
                })
        }
        .await;
        if let Err(error) = write_result {
            drop(output);
            if let Some((temporary, _)) = &paths {
                let _ = remove_file_if_present(temporary).await;
            }
            return Err(error);
        }
        match &paths {
            Some((temporary, destination)) => {
                drop(output);
                if let Err(source) = tokio::fs::rename(temporary, destination).await {
                    let _ = remove_file_if_present(temporary).await;
                    return Err(SelectiveStorageError::Io {
                        operation: "publish materialized file",
                        source,
                    });
                }
            }
            None => {
                self.files[file_index] = Some(output);
            }
        }

        let slots_before = self.part_slots();
        self.selection.set_wanted(&self.layout, file_index, true)?;
        for piece_index in self.layout.file_pieces(file_index)? {
            if !self
                .layout
                .piece_has_skipped_file(piece_index, &self.selection)?
            {
                self.part_file_mut()?
                    .release_piece(usize::try_from(piece_index).map_err(|_| {
                        SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                    })?)
                    .await?;
            }
        }
        Ok(MaterializationReport {
            file_index,
            bytes: metainfo_file.length,
            slots_before,
            slots_after: self.part_slots(),
        })
    }

    pub async fn finalize_descriptor_hashes(
        &mut self,
    ) -> Result<Vec<PreparedFileHash>, SelectiveStorageError> {
        if !matches!(self.backing, StorageBacking::Descriptors { .. }) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "descriptor hash finalization",
            ));
        }
        if !self.published {
            return Err(SelectiveStorageError::NotPublished);
        }
        let mut hashes = Vec::new();
        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.padding || !self.selection.is_wanted(file_index) {
                continue;
            }
            let file = self.files[file_index]
                .as_mut()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
            file.seek(SeekFrom::Start(0))
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "seek prepared descriptor for hashing",
                    source,
                })?;
            let mut remaining = metainfo_file.length;
            let mut hasher = Sha1::new();
            while remaining != 0 {
                let length = usize::try_from(remaining.min(buffer.len() as u64))
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
                file.read_exact(&mut buffer[..length])
                    .await
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "read prepared descriptor for hashing",
                        source,
                    })?;
                hasher.update(&buffer[..length]);
                remaining -= length as u64;
            }
            hashes.push(PreparedFileHash {
                file_index,
                length: metainfo_file.length,
                sha1: hasher.finalize().into(),
            });
        }
        self.files.iter_mut().for_each(|file| {
            file.take();
        });
        Ok(hashes)
    }

    fn part_file_mut(&mut self) -> Result<&mut PartFile, SelectiveStorageError> {
        self.part_file
            .as_mut()
            .ok_or(SelectiveStorageError::NotPublished)
    }
}

pub fn plan_descriptor_storage(
    metainfo: &Metainfo,
    skip_files: &[usize],
    materialize_files: &[usize],
) -> Result<DescriptorStoragePlan, SelectiveStorageError> {
    let layout = TorrentLayout::from_metainfo(metainfo);
    let selection = FileSelection::new(&layout, skip_files)?;
    let mut materialize = vec![false; layout.files().len()];
    for &file_index in materialize_files {
        let selected = materialize.get_mut(file_index).ok_or(
            SelectiveStorageError::InvalidDescriptorManifest {
                role: "materialization",
                file_index,
                reason: "file index is out of range",
            },
        )?;
        if *selected {
            return Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "materialization",
                file_index,
                reason: "file index is duplicated",
            });
        }
        let file = &layout.files()[file_index];
        if file.padding || selection.is_wanted(file_index) {
            return Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "materialization",
                file_index,
                reason: "file is padding or already wanted",
            });
        }
        *selected = true;
    }
    let files = layout
        .files()
        .iter()
        .enumerate()
        .map(|(file_index, file)| DescriptorStoragePlanFile {
            file_index,
            path: file.path.clone(),
            length: file.length,
            role: if file.padding {
                DescriptorFileRole::Padding
            } else if selection.is_wanted(file_index) {
                DescriptorFileRole::Wanted
            } else {
                DescriptorFileRole::Skipped
            },
            materialize: materialize[file_index],
        })
        .collect();
    Ok(DescriptorStoragePlan {
        info_hash: metainfo.info_hash,
        name: metainfo.name.clone(),
        files,
    })
}

pub async fn verify_prepared_descriptors(
    mut descriptors: Vec<DescriptorFile>,
    expected: &[PreparedFileHash],
) -> Result<(), SelectiveStorageError> {
    descriptors.sort_by_key(|file| file.file_index);
    if descriptors
        .windows(2)
        .any(|pair| pair[0].file_index == pair[1].file_index)
    {
        let duplicate = descriptors
            .windows(2)
            .find(|pair| pair[0].file_index == pair[1].file_index)
            .expect("duplicate descriptor pair exists")[0]
            .file_index;
        return Err(SelectiveStorageError::InvalidDescriptorManifest {
            role: "published",
            file_index: duplicate,
            reason: "file index is duplicated",
        });
    }
    let mut expected = expected.to_vec();
    expected.sort_by_key(|file| file.file_index);
    if descriptors.len() != expected.len() {
        return Err(SelectiveStorageError::InvalidDescriptorManifest {
            role: "published",
            file_index: 0,
            reason: "descriptor count does not match the prepared manifest",
        });
    }

    let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
    for (descriptor, prepared) in descriptors.into_iter().zip(expected) {
        if descriptor.file_index != prepared.file_index {
            return Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "published",
                file_index: descriptor.file_index,
                reason: "file index does not match the prepared manifest",
            });
        }
        let mut file = File::from_std(descriptor.file);
        let actual = file
            .metadata()
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "inspect published descriptor",
                source,
            })?
            .len();
        if actual != prepared.length {
            return Err(SelectiveStorageError::UnexpectedFileLength {
                file_index: prepared.file_index,
                expected: prepared.length,
                actual,
            });
        }
        file.seek(SeekFrom::Start(0))
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "seek published descriptor",
                source,
            })?;
        let mut remaining = prepared.length;
        let mut hasher = Sha1::new();
        while remaining != 0 {
            let length = usize::try_from(remaining.min(buffer.len() as u64))
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            file.read_exact(&mut buffer[..length])
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "read published descriptor",
                    source,
                })?;
            hasher.update(&buffer[..length]);
            remaining -= length as u64;
        }
        let actual_hash: [u8; 20] = hasher.finalize().into();
        if actual_hash != prepared.sha1 {
            return Err(SelectiveStorageError::PreparedHashMismatch {
                file_index: prepared.file_index,
            });
        }
    }
    Ok(())
}

fn collect_descriptors(
    file_count: usize,
    role: &'static str,
    descriptors: Vec<DescriptorFile>,
) -> Result<Vec<Option<std::fs::File>>, SelectiveStorageError> {
    let mut files: Vec<Option<std::fs::File>> = (0..file_count).map(|_| None).collect();
    for descriptor in descriptors {
        let file = files.get_mut(descriptor.file_index).ok_or(
            SelectiveStorageError::InvalidDescriptorManifest {
                role,
                file_index: descriptor.file_index,
                reason: "file index is out of range",
            },
        )?;
        if file.is_some() {
            return Err(SelectiveStorageError::InvalidDescriptorManifest {
                role,
                file_index: descriptor.file_index,
                reason: "file index is duplicated",
            });
        }
        *file = Some(descriptor.file);
    }
    Ok(files)
}

async fn initialize_descriptor_file(
    file: std::fs::File,
    role: &'static str,
    file_index: usize,
    length: u64,
) -> Result<File, SelectiveStorageError> {
    let file = File::from_std(file);
    let existing_length = file
        .metadata()
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "inspect descriptor staging file",
            source,
        })?
        .len();
    if existing_length != 0 {
        return Err(SelectiveStorageError::NonemptyDescriptor {
            role,
            file_index,
            length: existing_length,
        });
    }
    file.set_len(length)
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "size descriptor staging file",
            source,
        })?;
    Ok(file)
}

async fn validate_descriptor_length(
    file: std::fs::File,
    file_index: usize,
    expected: u64,
) -> Result<File, SelectiveStorageError> {
    let file = File::from_std(file);
    let actual = file
        .metadata()
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "inspect resumable descriptor file",
            source,
        })?
        .len();
    if actual != expected {
        return Err(SelectiveStorageError::UnexpectedFileLength {
            file_index,
            expected,
            actual,
        });
    }
    Ok(file)
}

async fn validate_empty_descriptor(
    file: std::fs::File,
    role: &'static str,
    file_index: usize,
) -> Result<std::fs::File, SelectiveStorageError> {
    let file = File::from_std(file);
    let length = file
        .metadata()
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "inspect descriptor staging file",
            source,
        })?
        .len();
    if length != 0 {
        return Err(SelectiveStorageError::NonemptyDescriptor {
            role,
            file_index,
            length,
        });
    }
    Ok(file.into_std().await)
}

pub fn selective_staging_path(output_root: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(output_root, ".rstorrent-staging")
}

pub fn selective_part_path(output_root: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(output_root, ".rstorrent-parts")
}

fn materialization_path(destination: &Path) -> Result<PathBuf, SelectiveStorageError> {
    sibling_with_suffix(destination, ".rstorrent-materializing")
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, SelectiveStorageError> {
    let file_name = path
        .file_name()
        .ok_or(SelectiveStorageError::InvalidOutputPath)?;
    let mut sibling = OsString::from(".");
    sibling.push(file_name);
    sibling.push(suffix);
    Ok(path.with_file_name(sibling))
}

fn joined_path(root: &Path, components: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    path.extend(components);
    path
}

async fn path_exists(path: &Path, operation: &'static str) -> Result<bool, SelectiveStorageError> {
    tokio::fs::try_exists(path)
        .await
        .map_err(|source| SelectiveStorageError::Io { operation, source })
}

async fn remove_file_if_present(path: &Path) -> Result<(), io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn remove_selective_staging_if_present(output_root: &Path) -> Result<(), io::Error> {
    let staging = selective_staging_path(output_root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid output root"))?;
    match tokio::fs::remove_dir_all(staging).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn remove_selective_part_if_present(output_root: &Path) -> Result<(), io::Error> {
    let part = selective_part_path(output_root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid output root"))?;
    remove_file_if_present(&part).await
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::metainfo::{Metainfo, MetainfoFile, MetainfoMode};
    use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
    use sha1::{Digest, Sha1};

    use super::{
        BlockingHashResult, DescriptorFile, DescriptorStorage, PreparedFileHash, ResumedStorage,
        SelectiveStorage, SelectiveStorageError, VERIFICATION_CHUNK_LENGTH, await_blocking_hash,
        collect_descriptors, materialization_path, remove_selective_part_if_present,
        remove_selective_staging_if_present, selective_part_path, selective_staging_path,
        verify_prepared_descriptors,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-selective-storage-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    fn fixture() -> Metainfo {
        let lengths = [20_000, 50_000, 7_000, 18_000, 0, 3_304, 35_000];
        let paths = [
            &["wanted", "start.bin"][..],
            &["skip", "large.bin"][..],
            &["later.bin"][..],
            &["wanted", "end.bin"][..],
            &["wanted", "empty.bin"][..],
            &[".pad", "3304"][..],
            &["tail.bin"][..],
        ];
        let mut offset = 0_u64;
        let files = lengths
            .into_iter()
            .enumerate()
            .map(|(index, length)| {
                let file = MetainfoFile {
                    path: paths[index].iter().map(|part| (*part).to_owned()).collect(),
                    length,
                    offset,
                    padding: index == 5,
                };
                offset += length;
                file
            })
            .collect();
        Metainfo {
            info_hash: [1; 20],
            piece_hashes: vec![[2; 20]; 5],
            piece_length: 32_768,
            total_length: 133_304,
            name: "fixture".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files,
        }
    }

    fn torrent_bytes(metainfo: &Metainfo) -> Vec<u8> {
        let mut bytes: Vec<u8> = (0..metainfo.total_length)
            .map(|offset| (offset % 251) as u8)
            .collect();
        for file in metainfo.files.iter().filter(|file| file.padding) {
            bytes[file.offset as usize..(file.offset + file.length) as usize].fill(0);
        }
        bytes
    }

    async fn clean(output: &Path) {
        let _ = tokio::fs::remove_dir_all(output).await;
        let _ = remove_selective_staging_if_present(output).await;
        let _ = remove_selective_part_if_present(output).await;
    }

    #[tokio::test]
    async fn contiguous_wanted_piece_hash_runs_in_one_bounded_job() {
        let output = test_path("contiguous-hash");
        clean(&output).await;
        let piece_length = 256 * 1024_u32;
        let bytes = (0..piece_length as usize)
            .map(|offset| ((offset * 41 + offset / 23) & 0xff) as u8)
            .collect::<Vec<_>>();
        let expected: [u8; 20] = Sha1::digest(&bytes).into();
        let metainfo = Metainfo {
            info_hash: [7; 20],
            piece_hashes: vec![expected],
            piece_length,
            total_length: u64::from(piece_length),
            name: "contiguous".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![MetainfoFile {
                path: vec!["payload.bin".to_owned()],
                length: u64::from(piece_length),
                offset: 0,
                padding: false,
            }],
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await
                .expect("create storage");
        for request in layout.request_ranges(0, &selection).expect("requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write block");
        }
        let (actual, stats) = storage
            .hash_piece_with_stats(0)
            .await
            .expect("hash contiguous piece");
        assert_eq!(actual, expected);
        assert_eq!(stats.wanted_file_seeks, 0);
        assert_eq!(
            stats.wanted_file_reads,
            bytes.len().div_ceil(VERIFICATION_CHUNK_LENGTH)
        );
        assert_eq!(stats.part_file_reads, 0);
        assert_eq!(stats.wanted_file_duplicates, 1);
        assert_eq!(stats.blocking_jobs, 1);
        clean(&output).await;
    }

    #[tokio::test]
    async fn blocking_hash_preserves_cross_file_and_padding_order() {
        let output = test_path("blocking-cross-file-padding");
        clean(&output).await;
        let piece_length = 256 * 1024_u32;
        let first_length = 100_000_u64;
        let padding_length = 4_000_u64;
        let second_length = u64::from(piece_length) - first_length - padding_length;
        let mut bytes = (0..piece_length as usize)
            .map(|offset| ((offset * 17 + offset / 31) & 0xff) as u8)
            .collect::<Vec<_>>();
        bytes[first_length as usize..(first_length + padding_length) as usize].fill(0);
        let expected: [u8; 20] = Sha1::digest(&bytes).into();
        let metainfo = Metainfo {
            info_hash: [8; 20],
            piece_hashes: vec![expected],
            piece_length,
            total_length: u64::from(piece_length),
            name: "cross-file-padding".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![
                MetainfoFile {
                    path: vec!["first.bin".to_owned()],
                    length: first_length,
                    offset: 0,
                    padding: false,
                },
                MetainfoFile {
                    path: vec![".pad".to_owned(), "4000".to_owned()],
                    length: padding_length,
                    offset: first_length,
                    padding: true,
                },
                MetainfoFile {
                    path: vec!["second.bin".to_owned()],
                    length: second_length,
                    offset: first_length + padding_length,
                    padding: false,
                },
            ],
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await
                .expect("create storage");
        for request in layout.request_ranges(0, &selection).expect("requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write block");
        }
        let (actual, stats) = storage
            .hash_piece_with_stats(0)
            .await
            .expect("hash cross-file piece");
        assert_eq!(actual, expected);
        assert_eq!(stats.wanted_file_seeks, 0);
        assert_eq!(stats.wanted_file_duplicates, 2);
        assert_eq!(stats.blocking_jobs, 1);
        assert_eq!(
            stats.wanted_file_reads,
            (first_length as usize).div_ceil(VERIFICATION_CHUNK_LENGTH)
                + (second_length as usize).div_ceil(VERIFICATION_CHUNK_LENGTH)
        );
        assert_eq!(stats.part_file_reads, 0);
        clean(&output).await;
    }

    #[tokio::test]
    async fn blocking_hash_reports_a_truncated_staging_file() {
        let output = test_path("blocking-truncated");
        clean(&output).await;
        let metainfo = Metainfo {
            info_hash: [9; 20],
            piece_hashes: vec![[0; 20]],
            piece_length: 32_768,
            total_length: 32_768,
            name: "truncated".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![MetainfoFile {
                path: vec!["payload.bin".to_owned()],
                length: 32_768,
                offset: 0,
                padding: false,
            }],
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await
                .expect("create storage");
        for request in layout.request_ranges(0, &selection).expect("requests") {
            storage
                .write_block(0, request.begin, vec![3; request.length as usize])
                .await
                .expect("write block");
        }
        storage.files[0]
            .as_ref()
            .expect("wanted file")
            .set_len(16_384)
            .await
            .expect("truncate staging file");
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::Io {
                operation: "read selected staging range in blocking verification job",
                source,
            }) if source.kind() == std::io::ErrorKind::UnexpectedEof
        ));
        clean(&output).await;
    }

    #[tokio::test]
    async fn blocking_hash_task_panic_is_a_typed_join_failure() {
        let task = tokio::task::spawn_blocking(|| -> BlockingHashResult {
            panic!("controlled blocking hash panic")
        });
        assert!(matches!(
            await_blocking_hash(task).await,
            Err(SelectiveStorageError::Io {
                operation: "join selected piece blocking verification job",
                source,
            }) if source.kind() == std::io::ErrorKind::Other
        ));
    }

    fn new_descriptor(path: &Path) -> std::fs::File {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create descriptor file")
    }

    fn descriptor_manifest(
        root: &Path,
        wanted_indices: &[usize],
        materialization_indices: &[usize],
    ) -> DescriptorStorage {
        let part_path = root.join("part");
        let part_file = new_descriptor(&part_path);
        let reopened_part_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part_path)
            .expect("open manifest part descriptor independently");
        DescriptorStorage {
            wanted_files: wanted_indices
                .iter()
                .map(|&file_index| {
                    let path = root.join(format!("wanted-{file_index}"));
                    DescriptorFile {
                        file_index,
                        file: new_descriptor(&path),
                    }
                })
                .collect(),
            part_file,
            reopened_part_file,
            materialization_files: materialization_indices
                .iter()
                .map(|&file_index| {
                    let path = root.join(format!("materialization-{file_index}"));
                    DescriptorFile {
                        file_index,
                        file: new_descriptor(&path),
                    }
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn coalesced_write_preserves_wanted_and_part_accounting() {
        let output = test_path("coalesced-write-accounting");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage = SelectiveStorage::create(output.clone(), &metainfo, layout, selection)
            .await
            .expect("create selected storage");

        let planned = storage
            .write_stats(0, 0, metainfo.piece_length as usize)
            .expect("plan coalesced write");
        assert_eq!(planned.wanted_bytes, 20_000);
        assert_eq!(planned.skipped_bytes, 12_768);
        let actual = storage
            .write_block(0, 0, bytes[..metainfo.piece_length as usize].to_vec())
            .await
            .expect("write coalesced piece range");
        assert_eq!(actual, planned);
        assert_eq!(
            storage.hash_piece(0).await.expect("hash coalesced piece"),
            <[u8; 20]>::from(Sha1::digest(&bytes[..metainfo.piece_length as usize]))
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn stages_hashes_publishes_reopens_and_materializes() {
        let output = test_path("fixture");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection)
                .await
                .expect("create selected storage");

        assert_eq!(storage.selected_bytes(), 73_000);
        assert_eq!(storage.skipped_bytes(), 57_000);
        assert_eq!(storage.padding_bytes(), 3_304);
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        let staging = selective_staging_path(&output).expect("staging path");
        assert!(
            tokio::fs::try_exists(staging.join("wanted/start.bin"))
                .await
                .expect("wanted staging")
        );
        assert!(
            !tokio::fs::try_exists(staging.join("skip/large.bin"))
                .await
                .expect("skipped staging")
        );
        assert!(
            !tokio::fs::try_exists(staging.join(".pad/3304"))
                .await
                .expect("padding staging")
        );
        assert!(matches!(
            storage.publish().await,
            Err(SelectiveStorageError::IncompleteSelection { piece_index: 0 })
        ));
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::PartFile(
                crate::part_file::PartFileError::MissingSlot { piece_index: 0 }
            ))
        ));

        let mut wanted_written = 0;
        let mut skipped_written = 0;
        for piece_index in [0_u32, 2, 3, 4] {
            for request in layout
                .request_ranges(piece_index, &storage.selection)
                .expect("requests")
            {
                let torrent_offset =
                    piece_index as usize * layout.piece_length() as usize + request.begin as usize;
                let block =
                    bytes[torrent_offset..torrent_offset + request.length as usize].to_vec();
                let stats = storage
                    .write_block(piece_index, request.begin, block)
                    .await
                    .expect("write mapped block");
                wanted_written += stats.wanted_bytes;
                skipped_written += stats.skipped_bytes;
            }
            let piece_start = piece_index as usize * layout.piece_length() as usize;
            let piece_length = layout.piece_length_at(piece_index).expect("piece length") as usize;
            let expected: [u8; 20] =
                Sha1::digest(&bytes[piece_start..piece_start + piece_length]).into();
            let (actual, stats) = storage
                .hash_piece_with_stats(piece_index)
                .await
                .expect("mixed hash");
            assert_eq!(actual, expected);
            if piece_index == 0 {
                assert_eq!(stats.blocking_jobs, 0);
                assert_eq!(stats.wanted_file_duplicates, 0);
                assert!(stats.wanted_file_seeks > 0);
                assert!(stats.part_file_reads > 0);
            }
            storage
                .record_verified(piece_index as usize)
                .expect("record verified");
        }
        assert_eq!(wanted_written, 73_000);
        assert_eq!(skipped_written, 24_232);
        assert_eq!(storage.part_slots(), 2);

        storage.publish().await.expect("publish selected tree");
        assert!(
            tokio::fs::try_exists(output.join("wanted/empty.bin"))
                .await
                .expect("empty output")
        );
        assert!(
            !tokio::fs::try_exists(output.join("skip/large.bin"))
                .await
                .expect("skipped output")
        );
        assert!(
            !tokio::fs::try_exists(output.join(".pad/3304"))
                .await
                .expect("padding output")
        );
        assert!(
            !tokio::fs::try_exists(&staging)
                .await
                .expect("staging removed")
        );

        storage.reopen_part_file().await.expect("reopen part file");
        let report = storage
            .materialize_file(2)
            .await
            .expect("materialize later file");
        assert_eq!(report.bytes, 7_000);
        assert_eq!(report.slots_before, 2);
        assert_eq!(report.slots_after, 2);
        assert_eq!(
            tokio::fs::read(output.join("later.bin"))
                .await
                .expect("materialized bytes"),
            bytes[70_000..77_000]
        );
        assert!(
            tokio::fs::try_exists(selective_part_path(&output).expect("part path"))
                .await
                .expect("part exists")
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn resumes_staging_and_published_trees_with_exact_geometry() {
        let output = test_path("resume");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage =
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await
                .expect("create resumable storage");
        for request in layout
            .request_ranges(0, &selection)
            .expect("first-piece requests")
        {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write first piece");
        }
        let expected_hash = storage.hash_piece(0).await.expect("hash first piece");
        storage.sync_piece(0).await.expect("sync first piece");
        storage.record_verified(0).expect("record first piece");
        drop(storage);

        let mut verified = vec![false; layout.piece_count()];
        verified[0] = true;
        let (mut storage, resumed) = SelectiveStorage::resume(
            output.clone(),
            &metainfo,
            layout.clone(),
            selection.clone(),
            verified.clone(),
        )
        .await
        .expect("resume staging");
        assert_eq!(resumed, ResumedStorage::Staging);
        assert_eq!(
            storage.hash_piece(0).await.expect("recheck first piece"),
            expected_hash
        );
        for piece_index in [2_usize, 3, 4] {
            storage
                .record_verified(piece_index)
                .expect("complete selection for publication");
        }
        storage.publish().await.expect("publish resumed tree");
        drop(storage);

        let (storage, resumed) = SelectiveStorage::resume(
            output.clone(),
            &metainfo,
            layout.clone(),
            selection,
            verified,
        )
        .await
        .expect("resume published");
        assert_eq!(resumed, ResumedStorage::Published);
        assert!(storage.is_published());
        drop(storage);

        let wanted = output.join("wanted/start.bin");
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&wanted)
            .await
            .expect("open wanted file");
        file.set_len(1).await.expect("truncate wanted file");
        drop(file);
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        assert!(matches!(
            SelectiveStorage::resume(output.clone(), &metainfo, layout, selection, vec![false; 5])
                .await,
            Err(SelectiveStorageError::UnexpectedFileLength {
                file_index: 0,
                expected: 20_000,
                actual: 1
            })
        ));
        clean(&output).await;
    }

    #[tokio::test]
    async fn descriptor_storage_reuses_mapping_reopen_and_materialization() {
        let root = test_path("descriptors");
        tokio::fs::create_dir(&root)
            .await
            .expect("create descriptor root");
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let wanted_paths: Vec<_> = [0_usize, 3, 4, 6]
            .into_iter()
            .map(|file_index| {
                let path = root.join(format!("wanted-{file_index}"));
                (file_index, path.clone(), new_descriptor(&path))
            })
            .collect();
        let materialized_path = root.join("materialized-2");
        let part_path = root.join("part");
        let part_file = new_descriptor(&part_path);
        let reopened_part_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&part_path)
            .expect("open independent part descriptor");
        let descriptors = DescriptorStorage {
            wanted_files: wanted_paths
                .into_iter()
                .map(|(file_index, _, file)| DescriptorFile { file_index, file })
                .collect(),
            part_file,
            reopened_part_file,
            materialization_files: vec![DescriptorFile {
                file_index: 2,
                file: new_descriptor(&materialized_path),
            }],
        };
        let mut storage = SelectiveStorage::create_with_descriptors(
            &metainfo,
            layout.clone(),
            selection,
            &[2],
            descriptors,
        )
        .await
        .expect("create descriptor storage");
        assert_eq!(storage.part_path(), None);

        for piece_index in [0_u32, 2, 3, 4] {
            for request in layout
                .request_ranges(piece_index, &storage.selection)
                .expect("descriptor requests")
            {
                let torrent_offset =
                    piece_index as usize * layout.piece_length() as usize + request.begin as usize;
                storage
                    .write_block(
                        piece_index,
                        request.begin,
                        bytes[torrent_offset..torrent_offset + request.length as usize].to_vec(),
                    )
                    .await
                    .expect("write descriptor block");
            }
            let piece_start = piece_index as usize * layout.piece_length() as usize;
            let piece_length = layout.piece_length_at(piece_index).expect("piece length") as usize;
            let expected: [u8; 20] =
                Sha1::digest(&bytes[piece_start..piece_start + piece_length]).into();
            assert_eq!(
                storage
                    .hash_piece(piece_index)
                    .await
                    .expect("descriptor mixed hash"),
                expected
            );
            storage
                .record_verified(piece_index as usize)
                .expect("record descriptor verification");
        }

        storage
            .prepare_descriptors()
            .await
            .expect("sync descriptor storage");
        storage
            .reopen_part_file()
            .await
            .expect("reopen descriptor part file");
        let report = storage
            .materialize_file(2)
            .await
            .expect("materialize to descriptor");
        assert_eq!(report.bytes, 7_000);
        assert_eq!(report.slots_before, 2);
        assert_eq!(report.slots_after, 2);
        let hashes = storage
            .finalize_descriptor_hashes()
            .await
            .expect("hash prepared descriptors");
        assert_eq!(
            hashes
                .iter()
                .map(|hash| hash.file_index)
                .collect::<Vec<_>>(),
            vec![0, 2, 3, 4, 6]
        );
        for hash in hashes {
            let file = &metainfo.files[hash.file_index];
            let expected: [u8; 20] =
                Sha1::digest(&bytes[file.offset as usize..(file.offset + file.length) as usize])
                    .into();
            assert_eq!(hash.length, file.length);
            assert_eq!(hash.sha1, expected);
        }
        drop(storage);

        for file_index in [0_usize, 3, 4, 6] {
            let path = root.join(format!("wanted-{file_index}"));
            let file = &metainfo.files[file_index];
            assert_eq!(
                std::fs::read(path).expect("read wanted descriptor output"),
                bytes[file.offset as usize..(file.offset + file.length) as usize]
            );
        }
        assert_eq!(
            std::fs::read(&materialized_path).expect("read materialized descriptor"),
            bytes[70_000..77_000]
        );
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove descriptor root");
    }

    #[tokio::test]
    async fn resumes_descriptors_and_verifies_fresh_publication_handles() {
        let root = test_path("descriptor-resume");
        tokio::fs::create_dir(&root)
            .await
            .expect("create descriptor root");
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let descriptors = descriptor_manifest(&root, &[0, 3, 4, 6], &[]);
        let mut storage = SelectiveStorage::create_with_descriptors(
            &metainfo,
            layout.clone(),
            selection.clone(),
            &[],
            descriptors,
        )
        .await
        .expect("create descriptor storage");
        for request in layout
            .request_ranges(0, &selection)
            .expect("piece requests")
        {
            let offset = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[offset..offset + request.length as usize].to_vec(),
                )
                .await
                .expect("write descriptor block");
        }
        storage.record_verified(0).expect("record first piece");
        storage.sync_piece(0).await.expect("sync first piece");
        drop(storage);

        let reopen = |path: &Path| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .expect("reopen descriptor")
        };
        let part_path = root.join("part");
        let descriptors = DescriptorStorage {
            wanted_files: [0_usize, 3, 4, 6]
                .into_iter()
                .map(|file_index| DescriptorFile {
                    file_index,
                    file: reopen(&root.join(format!("wanted-{file_index}"))),
                })
                .collect(),
            part_file: reopen(&part_path),
            reopened_part_file: reopen(&part_path),
            materialization_files: Vec::new(),
        };
        let mut verified = vec![false; layout.piece_count()];
        verified[0] = true;
        let mut resumed = SelectiveStorage::resume_with_descriptors(
            &metainfo,
            layout,
            selection,
            descriptors,
            verified,
        )
        .await
        .expect("resume descriptor storage");
        let expected_piece: [u8; 20] = Sha1::digest(&bytes[..32_768]).into();
        assert_eq!(
            resumed.hash_piece(0).await.expect("hash resumed piece"),
            expected_piece
        );
        drop(resumed);

        let file = &metainfo.files[0];
        let prepared = PreparedFileHash {
            file_index: 0,
            length: file.length,
            sha1: Sha1::digest(&bytes[..file.length as usize]).into(),
        };
        verify_prepared_descriptors(
            vec![DescriptorFile {
                file_index: 0,
                file: reopen(&root.join("wanted-0")),
            }],
            std::slice::from_ref(&prepared),
        )
        .await
        .expect("verify fresh published descriptor");
        std::fs::write(root.join("wanted-0"), vec![0_u8; file.length as usize])
            .expect("corrupt published descriptor");
        assert!(matches!(
            verify_prepared_descriptors(
                vec![DescriptorFile {
                    file_index: 0,
                    file: reopen(&root.join("wanted-0")),
                }],
                &[prepared],
            )
            .await,
            Err(SelectiveStorageError::PreparedHashMismatch { file_index: 0 })
        ));
        tokio::fs::remove_dir_all(root)
            .await
            .expect("remove descriptor root");
    }

    #[test]
    fn descriptor_manifest_rejects_duplicate_and_out_of_range_indices() {
        let first_path = test_path("manifest-first");
        let second_path = test_path("manifest-second");
        let duplicate = collect_descriptors(
            2,
            "wanted",
            vec![
                DescriptorFile {
                    file_index: 1,
                    file: new_descriptor(&first_path),
                },
                DescriptorFile {
                    file_index: 1,
                    file: new_descriptor(&second_path),
                },
            ],
        );
        assert!(matches!(
            duplicate,
            Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "wanted",
                file_index: 1,
                reason: "file index is duplicated",
            })
        ));
        std::fs::remove_file(first_path).expect("remove first manifest file");
        std::fs::remove_file(second_path).expect("remove second manifest file");

        let range_path = test_path("manifest-range");
        let out_of_range = collect_descriptors(
            2,
            "materialization",
            vec![DescriptorFile {
                file_index: 2,
                file: new_descriptor(&range_path),
            }],
        );
        assert!(matches!(
            out_of_range,
            Err(SelectiveStorageError::InvalidDescriptorManifest {
                role: "materialization",
                file_index: 2,
                reason: "file index is out of range",
            })
        ));
        std::fs::remove_file(range_path).expect("remove range manifest file");
    }

    #[tokio::test]
    async fn descriptor_manifest_rejects_missing_unexpected_and_nonempty_files() {
        let metainfo = fixture();
        let cases = [
            ("missing-wanted", vec![3, 4, 6], vec![2], false),
            ("unexpected-wanted", vec![0, 1, 3, 4, 6], vec![2], false),
            ("missing-materialization", vec![0, 3, 4, 6], vec![], false),
            (
                "unexpected-materialization",
                vec![0, 3, 4, 6],
                vec![1, 2],
                false,
            ),
            ("nonempty-wanted", vec![0, 3, 4, 6], vec![2], true),
        ];
        for (name, wanted, materializations, make_nonempty) in cases {
            let root = test_path(name);
            tokio::fs::create_dir(&root)
                .await
                .expect("create manifest case root");
            let descriptors = descriptor_manifest(&root, &wanted, &materializations);
            if make_nonempty {
                std::fs::write(root.join("wanted-0"), b"preserve")
                    .expect("make wanted descriptor nonempty");
            }
            let layout = TorrentLayout::from_metainfo(&metainfo);
            let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
            let result = SelectiveStorage::create_with_descriptors(
                &metainfo,
                layout,
                selection,
                &[2],
                descriptors,
            )
            .await;
            match name {
                "missing-wanted" => assert!(matches!(
                    result,
                    Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index: 0,
                        reason: "required descriptor is missing",
                    })
                )),
                "unexpected-wanted" => assert!(matches!(
                    result,
                    Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "wanted",
                        file_index: 1,
                        reason: "descriptor is not expected by the selection",
                    })
                )),
                "missing-materialization" => assert!(matches!(
                    result,
                    Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "materialization",
                        file_index: 2,
                        reason: "required descriptor is missing",
                    })
                )),
                "unexpected-materialization" => assert!(matches!(
                    result,
                    Err(SelectiveStorageError::InvalidDescriptorManifest {
                        role: "materialization",
                        file_index: 1,
                        reason: "descriptor is not expected",
                    })
                )),
                "nonempty-wanted" => {
                    assert!(matches!(
                        result,
                        Err(SelectiveStorageError::NonemptyDescriptor {
                            role: "wanted",
                            file_index: 0,
                            length: 8,
                        })
                    ));
                    assert_eq!(
                        std::fs::read(root.join("wanted-0"))
                            .expect("read preserved nonempty descriptor"),
                        b"preserve"
                    );
                }
                _ => unreachable!("all manifest cases are enumerated"),
            }
            tokio::fs::remove_dir_all(root)
                .await
                .expect("remove manifest case root");
        }
    }

    #[tokio::test]
    async fn releases_a_slot_after_the_last_skipped_file_is_materialized() {
        let output = test_path("release");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[2]).expect("selection");
        let mut storage = SelectiveStorage::create(output.clone(), &metainfo, layout, selection)
            .await
            .expect("create selected storage");

        storage
            .write_block(2, 0, vec![0; 16_384])
            .await
            .expect("store skipped file range");
        assert_eq!(storage.part_slots(), 1);
        for piece in 0..5 {
            storage.record_verified(piece).expect("record verification");
        }
        storage.publish().await.expect("publish selected tree");
        storage.reopen_part_file().await.expect("reopen part file");
        let report = storage
            .materialize_file(2)
            .await
            .expect("materialize final skipped file");
        assert_eq!(report.slots_before, 1);
        assert_eq!(report.slots_after, 0);

        storage
            .reopen_part_file()
            .await
            .expect("reopen released map");
        assert_eq!(storage.part_slots(), 0);
        clean(&output).await;
    }

    #[tokio::test]
    async fn rejects_incomplete_materialization_without_final_or_partial_file() {
        let output = test_path("incomplete");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let mut storage = SelectiveStorage::create(output.clone(), &metainfo, layout, selection)
            .await
            .expect("create selected storage");
        for piece in [0, 2, 3, 4] {
            storage.record_verified(piece).expect("record verification");
        }
        storage.publish().await.expect("publish selected tree");
        assert!(matches!(
            storage.materialize_file(1).await,
            Err(SelectiveStorageError::IncompleteMaterialization {
                file_index: 1,
                piece_index: 1,
            })
        ));
        let final_path = output.join("skip/large.bin");
        assert!(
            !tokio::fs::try_exists(&final_path)
                .await
                .expect("final state")
        );
        assert!(
            !tokio::fs::try_exists(materialization_path(&final_path).expect("temporary path"))
                .await
                .expect("temporary state")
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn refuses_and_preserves_every_preexisting_artifact() {
        let output = test_path("existing");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");

        tokio::fs::create_dir(&output)
            .await
            .expect("create existing output");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await,
            Err(SelectiveStorageError::ExistingOutput(_))
        ));
        assert!(tokio::fs::try_exists(&output).await.expect("output state"));
        tokio::fs::remove_dir(&output)
            .await
            .expect("remove existing output");

        let staging = selective_staging_path(&output).expect("staging path");
        tokio::fs::create_dir(&staging)
            .await
            .expect("create existing staging");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout.clone(), selection.clone())
                .await,
            Err(SelectiveStorageError::ExistingStaging(_))
        ));
        assert!(
            tokio::fs::try_exists(&staging)
                .await
                .expect("staging state")
        );
        tokio::fs::remove_dir(&staging)
            .await
            .expect("remove existing staging");

        let part = selective_part_path(&output).expect("part path");
        tokio::fs::write(&part, b"owned elsewhere")
            .await
            .expect("create existing part");
        assert!(matches!(
            SelectiveStorage::create(output.clone(), &metainfo, layout, selection).await,
            Err(SelectiveStorageError::ExistingPartFile(_))
        ));
        assert_eq!(
            tokio::fs::read(&part).await.expect("preserved part"),
            b"owned elsewhere"
        );
        clean(&output).await;
    }
}

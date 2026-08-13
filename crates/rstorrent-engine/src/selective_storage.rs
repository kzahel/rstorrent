use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use rstorrent_protocol::content::{ExpectedPieceIntegrity, TorrentContent};
use rstorrent_protocol::merkle::{MerkleAccumulator, MerkleError, Sha256Hash, hash_block};
use rstorrent_protocol::metainfo::{Metainfo, MetainfoFormat};
use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH};
use rstorrent_protocol::storage_layout::{
    ContentLayout, FileSelection, LayoutError, LayoutSegment, SegmentTarget, TorrentLayout,
};
use sha1::{Digest, Sha1};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

use crate::artifact_layout::PublicationShape;
use crate::checkpoint::DurabilityTarget;
use crate::identity::{ContentFingerprint, TorrentId};
use crate::namespace_transition::{
    NamespaceAction, NamespaceState, NamespaceTransitionError, NamespaceTransitionInput,
    NamespaceTransitionOutcome, decide_namespace_transition,
};
use crate::part_file::{
    PartFile, PartFileCheckpointReference, PartFileError, PartFileIdentity, PartFileSpan,
};
use crate::positional_io::{read_exact_at, write_all_at};
use crate::resume_validation::{ResumeStorageEvidence, ResumeValidationRejectReason};
use crate::storage_file_pool::{
    DEFAULT_STORAGE_FILE_LIMIT, PlatformStorageFailure, PlatformStorageFailureKind,
    PlatformStorageTarget, StorageFileAccess, StorageFileKey, StorageFileLease, StorageFileLocator,
    StorageFilePool, StorageFilePoolError, StorageFileReference, StorageFileRole,
};

pub const VERIFICATION_CHUNK_LENGTH: usize = 16 * 1024;
pub const MAX_UPLOAD_READ_SEGMENTS: usize = MAX_REQUEST_BLOCK_LENGTH as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FastResumeValidation {
    pub evidence: ResumeStorageEvidence,
    pub committed_pieces: usize,
    pub relevant_files: usize,
    pub artifact_observations: usize,
    pub part_header_bytes: u64,
    pub payload_bytes_read: u64,
    pub hash_jobs: usize,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionReconcileReport {
    pub route_epoch: u64,
    pub promoted_files: Vec<usize>,
    pub demoted_files: Vec<usize>,
    pub invalidated_pieces: Vec<usize>,
}

#[derive(Debug)]
pub struct SelectiveUploadReadPlan {
    request: BlockRequest,
    route_epoch: u64,
    spans: Vec<SelectiveUploadReadSpan>,
}

pub const MAX_ACTIVE_FILE_READ_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct SelectiveFileReadPlan {
    file_index: usize,
    offset: u64,
    length: usize,
    route_epoch: u64,
    source: RetainedFileSource,
}

impl SelectiveFileReadPlan {
    pub const fn file_index(&self) -> usize {
        self.file_index
    }

    pub const fn offset(&self) -> u64 {
        self.offset
    }

    pub const fn length(&self) -> usize {
        self.length
    }

    pub const fn route_epoch(&self) -> u64 {
        self.route_epoch
    }

    pub async fn execute(self) -> Result<Vec<u8>, SelectiveStorageError> {
        let source = self.source.acquire(StorageFileAccess::ReadExisting).await?;
        tokio::task::spawn_blocking(move || {
            let mut bytes = vec![0_u8; self.length];
            read_exact_at(source.file(), &mut bytes, self.offset)?;
            Ok::<_, io::Error>(bytes)
        })
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join active file read",
            source: io::Error::other(source),
        })?
        .map_err(|source| SelectiveStorageError::Io {
            operation: "read active file range",
            source,
        })
    }
}

#[derive(Clone, Debug)]
enum SelectiveUploadReadSpan {
    File {
        source: RetainedFileSource,
        file_offset: u64,
        block_offset: usize,
        length: usize,
    },
    Part {
        source: PartFileCheckpointReference,
        file_offset: u64,
        block_offset: usize,
        length: usize,
    },
    Padding {
        block_offset: usize,
        length: usize,
    },
}

impl SelectiveUploadReadPlan {
    pub const fn request(&self) -> BlockRequest {
        self.request
    }

    pub const fn route_epoch(&self) -> u64 {
        self.route_epoch
    }

    pub fn segment_count(&self) -> usize {
        self.spans.len()
    }

    pub async fn execute(self) -> Result<Vec<u8>, SelectiveStorageError> {
        let mut block = vec![0_u8; self.request.length as usize];
        for span in self.spans {
            let (source, file_offset, block_offset, length) =
                match span {
                    SelectiveUploadReadSpan::File {
                        source,
                        file_offset,
                        block_offset,
                        length,
                    } => (
                        source.acquire(StorageFileAccess::ReadExisting).await?,
                        file_offset,
                        block_offset,
                        length,
                    ),
                    SelectiveUploadReadSpan::Part {
                        source,
                        file_offset,
                        block_offset,
                        length,
                    } => (
                        source
                            .acquire(StorageFileAccess::ReadExisting)
                            .await
                            .map_err(SelectiveStorageError::PartFile)?,
                        file_offset,
                        block_offset,
                        length,
                    ),
                    SelectiveUploadReadSpan::Padding {
                        block_offset,
                        length,
                    } => {
                        let end = block_offset.checked_add(length).ok_or(
                            SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow),
                        )?;
                        block
                            .get_mut(block_offset..end)
                            .ok_or(SelectiveStorageError::Layout(
                                LayoutError::ArithmeticOverflow,
                            ))?
                            .fill(0);
                        continue;
                    }
                };
            let bytes = tokio::task::spawn_blocking(move || {
                let mut bytes = vec![0_u8; length];
                read_exact_at(source.file(), &mut bytes, file_offset)?;
                Ok::<_, io::Error>(bytes)
            })
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "join active upload read",
                source: io::Error::other(source),
            })?
            .map_err(|source| SelectiveStorageError::Io {
                operation: "read active upload range",
                source,
            })?;
            let end =
                block_offset
                    .checked_add(bytes.len())
                    .ok_or(SelectiveStorageError::Layout(
                        LayoutError::ArithmeticOverflow,
                    ))?;
            block
                .get_mut(block_offset..end)
                .ok_or(SelectiveStorageError::Layout(
                    LayoutError::ArithmeticOverflow,
                ))?
                .copy_from_slice(&bytes);
        }
        Ok(block)
    }
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

#[cfg_attr(not(feature = "descriptor-storage-diagnostics"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorFileRole {
    Wanted,
    Skipped,
    Padding,
}

#[cfg_attr(not(feature = "descriptor-storage-diagnostics"), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorStoragePlanFile {
    pub file_index: usize,
    pub path: Vec<String>,
    pub length: u64,
    pub role: DescriptorFileRole,
    pub materialize: bool,
}

#[cfg_attr(not(feature = "descriptor-storage-diagnostics"), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorStoragePlan {
    pub info_hash: [u8; 20],
    pub name: String,
    pub files: Vec<DescriptorStoragePlanFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TorrentStoragePaths {
    /// Whether the recognizable/staging payload artifact is one file or a
    /// directory tree. Logical piece/file routing remains identical.
    pub publication_shape: PublicationShape,
    /// Recognizable final file or directory beneath the selected storage root.
    pub output: PathBuf,
    /// Hidden, opaque-owner-keyed file or directory used before publication.
    pub staging: PathBuf,
    /// Hidden, opaque-owner-keyed selective part file.
    pub part: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TorrentArtifactIdentity {
    pub torrent_id: TorrentId,
    pub content_fingerprint: ContentFingerprint,
}

impl TorrentArtifactIdentity {
    fn part_file(self, layout: &ContentLayout) -> PartFileIdentity {
        PartFileIdentity {
            torrent_id: self.torrent_id,
            content_fingerprint: self.content_fingerprint,
            piece_count: layout.piece_count(),
            piece_length: layout.piece_length(),
            total_length: layout.total_length(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFileHash {
    pub file_index: usize,
    pub length: u64,
    pub sha1: [u8; 20],
}

#[derive(Clone, Debug)]
pub struct PlatformStorageSpec {
    pub pool: StorageFilePool,
    pub root_id: String,
    pub storage_id: String,
    pub publication_name: String,
    pub publication_shape: PublicationShape,
    pub namespace_generation: u64,
    pub managed: bool,
    pub published: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumedStorage {
    Created,
    Staging,
    Published,
}

pub type ResumeArtifactState = NamespaceState;

#[derive(Debug)]
pub enum SelectiveStorageError {
    InvalidOutputPath,
    InvalidPublicationName,
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
    Merkle(MerkleError),
    PartFile(PartFileError),
    MissingWantedFile {
        file_index: usize,
    },
    StaleWriteRoute {
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
    NamespaceTransition(NamespaceTransitionError),
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
            Self::InvalidPublicationName => {
                write!(formatter, "publication name is not one safe path component")
            }
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
                "resumable storage must contain exactly one selected tree"
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
            Self::Merkle(error) => write!(formatter, "storage Merkle verification: {error}"),
            Self::PartFile(error) => write!(formatter, "part file: {error}"),
            Self::MissingWantedFile { file_index } => {
                write!(formatter, "wanted file {file_index} is not open")
            }
            Self::StaleWriteRoute { file_index } => {
                write!(formatter, "wanted file {file_index} write route is stale")
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
            Self::NamespaceTransition(error) => write!(formatter, "namespace transition: {error}"),
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
            Self::Merkle(error) => Some(error),
            Self::PartFile(error) => Some(error),
            Self::NamespaceTransition(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl SelectiveStorageError {
    pub fn platform_failure_kind(&self) -> Option<PlatformStorageFailureKind> {
        match self {
            Self::PartFile(error) => error.platform_failure_kind(),
            Self::Io { source, .. } => source
                .get_ref()
                .and_then(|error| error.downcast_ref::<StorageFilePoolError>())
                .and_then(StorageFilePoolError::platform_failure_kind),
            _ => None,
        }
    }

    pub(crate) fn is_missing_or_short_source(&self) -> bool {
        match self {
            Self::MissingWantedFile { .. } => true,
            Self::UnexpectedFileLength {
                expected, actual, ..
            } => actual < expected,
            Self::PartFile(
                PartFileError::MissingSlot { .. } | PartFileError::TruncatedPayload { .. },
            ) => true,
            _ if self.platform_failure_kind() == Some(PlatformStorageFailureKind::Missing) => true,
            _ => error_chain_contains_absent_source(self),
        }
    }
}

fn error_chain_contains_absent_source(error: &(dyn Error + 'static)) -> bool {
    let mut current = error.source();
    while let Some(source) = current {
        if source.downcast_ref::<io::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::UnexpectedEof
            )
        }) || source
            .downcast_ref::<PlatformStorageFailure>()
            .is_some_and(|failure| failure.kind == PlatformStorageFailureKind::Missing)
        {
            return true;
        }
        current = source.source();
    }
    false
}

impl From<LayoutError> for SelectiveStorageError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

impl From<MerkleError> for SelectiveStorageError {
    fn from(error: MerkleError) -> Self {
        Self::Merkle(error)
    }
}

impl From<PartFileError> for SelectiveStorageError {
    fn from(error: PartFileError) -> Self {
        Self::PartFile(error)
    }
}

impl From<NamespaceTransitionError> for SelectiveStorageError {
    fn from(error: NamespaceTransitionError) -> Self {
        Self::NamespaceTransition(error)
    }
}

#[derive(Debug)]
enum StorageBacking {
    Paths {
        output_root: PathBuf,
        staging_root: PathBuf,
        part_path: PathBuf,
        part_reference: StorageFileReference,
        storage_id: String,
    },
    Platform {
        spec: PlatformStorageSpec,
        part_reference: StorageFileReference,
    },
    Descriptors {
        reopened_part_file: Option<std::fs::File>,
        materialization_files: Vec<Option<std::fs::File>>,
    },
}

#[derive(Debug)]
pub struct SelectiveStorage {
    content: Option<Arc<TorrentContent>>,
    backing: StorageBacking,
    identity: PartFileIdentity,
    publication_shape: PublicationShape,
    layout: ContentLayout,
    selection: FileSelection,
    files: Vec<Option<RetainedFile>>,
    skipped_sources: Vec<Option<RetainedFileSource>>,
    part_file: Option<PartFile>,
    part_checkpoint_handle: Option<Arc<OnceLock<CheckpointFileReference>>>,
    pending_promotions: Vec<usize>,
    route_epoch: u64,
    verified: Vec<bool>,
    namespace_state: NamespaceState,
    namespace_generation: u64,
}

pub(crate) type CheckpointHandles =
    BTreeMap<DurabilityTarget, Arc<OnceLock<CheckpointFileReference>>>;

#[derive(Clone, Debug)]
pub(crate) enum CheckpointFileReference {
    Dynamic(StorageFileReference),
    Fixed(StorageFileLease),
}

impl CheckpointFileReference {
    pub(crate) async fn acquire(&self) -> Result<StorageFileLease, SelectiveStorageError> {
        match self {
            Self::Dynamic(reference) => reference
                .open(StorageFileAccess::ReadWriteExisting)
                .await
                .map(StorageFileLease::from)
                .map_err(|error| SelectiveStorageError::Io {
                    operation: "acquire durability checkpoint file",
                    source: io::Error::other(error),
                }),
            Self::Fixed(file) => Ok(file.clone()),
        }
    }
}

#[derive(Clone, Debug)]
struct RetainedFile {
    source: RetainedFileSource,
    routing_generation: u64,
}

#[derive(Clone, Debug)]
enum RetainedFileSource {
    Dynamic {
        reference: StorageFileReference,
        file_index: usize,
        expected_length: u64,
    },
    Fixed(StorageFileLease),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetainedSourceObservation {
    Exact,
    Missing,
    WrongLength,
    WrongKind,
}

impl RetainedFileSource {
    async fn observe_exact(
        &self,
        expected_length: u64,
    ) -> Result<RetainedSourceObservation, SelectiveStorageError> {
        match self {
            Self::Dynamic {
                reference,
                expected_length: retained_expected,
                ..
            } => {
                debug_assert_eq!(*retained_expected, expected_length);
                let observation =
                    reference
                        .observe()
                        .await
                        .map_err(|error| SelectiveStorageError::Io {
                            operation: "observe resume payload source",
                            source: io::Error::other(error),
                        })?;
                if !observation.exists {
                    return Ok(RetainedSourceObservation::Missing);
                }
                if observation.kind != Some(crate::storage_file_pool::StorageObjectKind::File) {
                    return Ok(RetainedSourceObservation::WrongKind);
                }
                Ok(if observation.length == Some(expected_length) {
                    RetainedSourceObservation::Exact
                } else {
                    RetainedSourceObservation::WrongLength
                })
            }
            Self::Fixed(file) => {
                let actual = file
                    .file()
                    .metadata()
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "inspect resume payload descriptor",
                        source,
                    })?
                    .len();
                Ok(if actual == expected_length {
                    RetainedSourceObservation::Exact
                } else {
                    RetainedSourceObservation::WrongLength
                })
            }
        }
    }

    async fn acquire(
        &self,
        access: StorageFileAccess,
    ) -> Result<StorageFileLease, SelectiveStorageError> {
        match self {
            Self::Dynamic {
                reference,
                file_index,
                expected_length,
            } => {
                let file = reference.open(access).await.map(StorageFileLease::from);
                let file = match file {
                    Ok(file) => file,
                    Err(StorageFilePoolError::Io { source, .. })
                        if source.kind() == io::ErrorKind::NotFound =>
                    {
                        return Err(SelectiveStorageError::MissingWantedFile {
                            file_index: *file_index,
                        });
                    }
                    Err(StorageFilePoolError::PlatformFailure(failure))
                        if failure.kind == PlatformStorageFailureKind::Missing =>
                    {
                        return Err(SelectiveStorageError::MissingWantedFile {
                            file_index: *file_index,
                        });
                    }
                    Err(error) => {
                        return Err(SelectiveStorageError::Io {
                            operation: "acquire selected file",
                            source: io::Error::other(error),
                        });
                    }
                };
                let actual = file
                    .file()
                    .metadata()
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "inspect acquired selected file",
                        source,
                    })?
                    .len();
                if actual == *expected_length
                    || (!matches!(access, StorageFileAccess::ReadWriteCreate)
                        && actual > *expected_length)
                {
                    return Ok(file);
                }
                if matches!(access, StorageFileAccess::ReadWriteCreate) {
                    file.file().set_len(*expected_length).map_err(|source| {
                        SelectiveStorageError::Io {
                            operation: "size acquired selected file",
                            source,
                        }
                    })?;
                    return Ok(file);
                }
                Err(SelectiveStorageError::UnexpectedFileLength {
                    file_index: *file_index,
                    expected: *expected_length,
                    actual,
                })
            }
            Self::Fixed(file) => Ok(file.clone()),
        }
    }

    fn checkpoint_reference(&self) -> CheckpointFileReference {
        match self {
            Self::Dynamic { reference, .. } => CheckpointFileReference::Dynamic(reference.clone()),
            Self::Fixed(file) => CheckpointFileReference::Fixed(file.clone()),
        }
    }

    async fn is_available(&self) -> Result<bool, SelectiveStorageError> {
        match self {
            Self::Dynamic {
                reference,
                expected_length,
                ..
            } => {
                let file = match reference.open(StorageFileAccess::ReadExisting).await {
                    Ok(file) => file,
                    Err(StorageFilePoolError::Io { source, .. })
                        if source.kind() == io::ErrorKind::NotFound =>
                    {
                        return Ok(false);
                    }
                    Err(StorageFilePoolError::PlatformFailure(failure))
                        if failure.kind == PlatformStorageFailureKind::Missing =>
                    {
                        return Ok(false);
                    }
                    Err(error) => {
                        return Err(SelectiveStorageError::Io {
                            operation: "probe selected file source",
                            source: io::Error::other(error),
                        });
                    }
                };
                Ok(file
                    .file()
                    .metadata()
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "inspect selected file source",
                        source,
                    })?
                    .len()
                    >= *expected_length)
            }
            Self::Fixed(_) => Ok(true),
        }
    }
}

impl RetainedFile {
    async fn new(control: File, operation: &'static str) -> Result<Self, SelectiveStorageError> {
        let file = control.into_std().await;
        let _ = operation;
        Ok(Self {
            source: RetainedFileSource::Fixed(StorageFileLease::fixed(file)),
            routing_generation: 0,
        })
    }

    fn dynamic(reference: StorageFileReference, file_index: usize, expected_length: u64) -> Self {
        Self {
            source: RetainedFileSource::Dynamic {
                reference,
                file_index,
                expected_length,
            },
            routing_generation: 0,
        }
    }

    async fn acquire(
        &self,
        access: StorageFileAccess,
    ) -> Result<StorageFileLease, SelectiveStorageError> {
        self.source.acquire(access).await
    }

    fn checkpoint_reference(&self) -> CheckpointFileReference {
        self.source.checkpoint_reference()
    }
}

#[derive(Clone, Debug)]
struct SelectiveWritePlan {
    payload: Arc<Vec<u8>>,
    spans: Vec<SelectiveWriteSpan>,
    stats: SelectiveWriteStats,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SelectiveWriteSpan {
    destination: SelectiveWriteDestination,
    block_offset: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectiveWriteDestination {
    WantedFile {
        file_index: usize,
        file_offset: u64,
        routing_generation: u64,
    },
    PartFile(PartFileSpan),
}

#[derive(Debug)]
struct ExecutableWriteSpan {
    file: ExecutableFileSource,
    file_offset: u64,
    block_offset: usize,
    length: usize,
}

#[derive(Clone, Debug)]
enum ExecutableFileSource {
    Wanted(RetainedFileSource),
    Part(PartFileCheckpointReference),
}

impl ExecutableFileSource {
    async fn acquire_write(&self) -> Result<StorageFileLease, SelectiveStorageError> {
        match self {
            Self::Wanted(source) => source.acquire(StorageFileAccess::ReadWriteCreate).await,
            Self::Part(source) => source
                .acquire(StorageFileAccess::ReadWriteExisting)
                .await
                .map_err(SelectiveStorageError::PartFile),
        }
    }
}

#[derive(Debug)]
pub(crate) struct SelectiveWriteJob {
    payload: Arc<Vec<u8>>,
    spans: Vec<ExecutableWriteSpan>,
    stats: SelectiveWriteStats,
}

impl SelectiveWriteJob {
    pub(crate) async fn execute(self) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        let payload = self.payload;
        for span in self.spans {
            let file = span.file.acquire_write().await?;
            let payload = payload.clone();
            tokio::task::spawn_blocking(move || {
                write_all_at(
                    file.file(),
                    &payload[span.block_offset..span.block_offset + span.length],
                    span.file_offset,
                )
            })
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "join positional selected write",
                source: io::Error::other(source),
            })?
            .map_err(|source| SelectiveStorageError::Io {
                operation: "write positional selected range",
                source,
            })?;
        }
        Ok(self.stats)
    }
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
pub(crate) struct SelectiveHashPlan {
    spans: Vec<BlockingHashSpan>,
    algorithm: PieceHashAlgorithm,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PieceHashAlgorithm {
    Sha1,
    V2Merkle { target_height: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComputedPieceHash {
    Sha1([u8; 20]),
    Sha256 {
        root: Sha256Hash,
        retained_hash_high_water: usize,
    },
}

#[derive(Clone, Debug)]
enum BlockingHashSpan {
    WantedFile {
        file: RetainedFileSource,
        file_offset: u64,
        length: usize,
    },
    PartFile {
        file: PartFileCheckpointReference,
        span: PartFileSpan,
    },
    Padding {
        length: usize,
    },
}

type BlockingHashResult = Result<([u8; 20], SelectiveHashIoStats), SelectiveStorageError>;

impl SelectiveHashPlan {
    async fn hash(self) -> BlockingHashResult {
        if self.algorithm != PieceHashAlgorithm::Sha1 {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "SHA-1 execution of a v2 hash plan",
            ));
        }
        let mut hasher = Sha1::new();
        let mut stats = SelectiveHashIoStats {
            blocking_jobs: 1,
            ..SelectiveHashIoStats::default()
        };
        let zeroes = [0_u8; VERIFICATION_CHUNK_LENGTH];
        for span in self.spans {
            match span {
                BlockingHashSpan::WantedFile {
                    file,
                    file_offset,
                    length,
                } => {
                    let file = file.acquire(StorageFileAccess::ReadExisting).await?;
                    let (next_hasher, reads) = spawn_blocking_hash_span(
                        hasher,
                        file,
                        file_offset,
                        length,
                        "read selected staging range in blocking verification job",
                    )
                    .await?;
                    hasher = next_hasher;
                    stats.wanted_file_reads = stats.wanted_file_reads.saturating_add(reads);
                }
                BlockingHashSpan::PartFile { file, span } => {
                    let file = file
                        .acquire(StorageFileAccess::ReadExisting)
                        .await
                        .map_err(SelectiveStorageError::PartFile)?;
                    let (next_hasher, reads) = spawn_blocking_hash_span(
                        hasher,
                        file,
                        span.file_offset,
                        span.length,
                        "read part-file range in blocking verification job",
                    )
                    .await?;
                    hasher = next_hasher;
                    stats.part_file_reads = stats.part_file_reads.saturating_add(reads);
                }
                BlockingHashSpan::Padding { length } => {
                    let mut consumed = 0_usize;
                    while consumed < length {
                        let length = (length - consumed).min(zeroes.len());
                        hasher.update(&zeroes[..length]);
                        consumed += length;
                    }
                }
            }
        }
        Ok((hasher.finalize().into(), stats))
    }

    async fn hash_content(self) -> Result<ComputedPieceHash, SelectiveStorageError> {
        let PieceHashAlgorithm::V2Merkle { target_height } = self.algorithm else {
            return self
                .hash()
                .await
                .map(|(hash, _)| ComputedPieceHash::Sha1(hash));
        };
        let mut accumulator = MerkleAccumulator::new(0)?;
        let mut high_water = 0;
        for span in self.spans {
            let (source, offset, length) = match span {
                BlockingHashSpan::WantedFile {
                    file,
                    file_offset,
                    length,
                } => (
                    file.acquire(StorageFileAccess::ReadExisting).await?,
                    file_offset,
                    length,
                ),
                BlockingHashSpan::PartFile { .. } | BlockingHashSpan::Padding { .. } => {
                    return Err(SelectiveStorageError::InvalidStorageOperation(
                        "v2 Merkle plan contains a part or padding span",
                    ));
                }
            };
            accumulator = tokio::task::spawn_blocking(move || {
                hash_merkle_file_span(accumulator, source, offset, length)
            })
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "join v2 piece blocking verification job",
                source: io::Error::other(source),
            })??;
            high_water = high_water.max(accumulator.retained_hash_high_water());
        }
        let root = accumulator.finish_padded_to(target_height)?;
        Ok(ComputedPieceHash::Sha256 {
            root,
            retained_hash_high_water: high_water,
        })
    }
}

fn hash_merkle_file_span(
    mut accumulator: MerkleAccumulator,
    file: StorageFileLease,
    file_offset: u64,
    span_length: usize,
) -> Result<MerkleAccumulator, SelectiveStorageError> {
    let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
    let mut consumed = 0_usize;
    while consumed < span_length {
        let length = (span_length - consumed).min(buffer.len());
        let offset = file_offset
            .checked_add(
                u64::try_from(consumed)
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?,
            )
            .ok_or(SelectiveStorageError::Layout(
                LayoutError::ArithmeticOverflow,
            ))?;
        read_exact_at(file.file(), &mut buffer[..length], offset).map_err(|source| {
            SelectiveStorageError::Io {
                operation: "read selected v2 range in blocking verification job",
                source,
            }
        })?;
        accumulator.push(hash_block(&buffer[..length])?)?;
        consumed += length;
    }
    Ok(accumulator)
}

async fn spawn_blocking_hash_span(
    hasher: Sha1,
    file: StorageFileLease,
    file_offset: u64,
    span_length: usize,
    operation: &'static str,
) -> Result<(Sha1, usize), SelectiveStorageError> {
    tokio::task::spawn_blocking(move || {
        hash_file_span(hasher, file, file_offset, span_length, operation)
    })
    .await
    .map_err(|source| SelectiveStorageError::Io {
        operation: "join selected piece blocking verification job",
        source: io::Error::other(source),
    })?
}

fn hash_file_span(
    mut hasher: Sha1,
    file: StorageFileLease,
    file_offset: u64,
    span_length: usize,
    operation: &'static str,
) -> Result<(Sha1, usize), SelectiveStorageError> {
    let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
    let mut consumed = 0_usize;
    let mut reads = 0_usize;
    while consumed < span_length {
        let length = (span_length - consumed).min(VERIFICATION_CHUNK_LENGTH);
        let offset = file_offset
            .checked_add(
                u64::try_from(consumed)
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?,
            )
            .ok_or(SelectiveStorageError::Layout(
                LayoutError::ArithmeticOverflow,
            ))?;
        read_exact_at(file.file(), &mut buffer[..length], offset)
            .map_err(|source| SelectiveStorageError::Io { operation, source })?;
        hasher.update(&buffer[..length]);
        consumed += length;
        reads = reads.saturating_add(1);
    }
    Ok((hasher, reads))
}

impl SelectiveHashPlan {
    pub(crate) async fn execute_content(self) -> Result<ComputedPieceHash, SelectiveStorageError> {
        self.hash_content().await
    }
}

#[cfg(test)]
async fn await_blocking_hash(
    task: tokio::task::JoinHandle<BlockingHashResult>,
) -> BlockingHashResult {
    task.await.map_err(|source| SelectiveStorageError::Io {
        operation: "join selected piece blocking verification job",
        source: io::Error::other(source),
    })?
}

async fn sync_file(
    file: StorageFileLease,
    operation: &'static str,
) -> Result<(), SelectiveStorageError> {
    tokio::task::spawn_blocking(move || file.file().sync_data())
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join selected file sync",
            source: io::Error::other(source),
        })?
        .map_err(|source| SelectiveStorageError::Io { operation, source })
}

async fn validate_publication_artifact(
    root: &Path,
    shape: PublicationShape,
) -> Result<(), SelectiveStorageError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || inspect_publication_artifact(&root, shape, true).map(drop))
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join publication artifact validation",
            source: io::Error::other(source),
        })?
}

fn inspect_publication_artifact(
    root: &Path,
    shape: PublicationShape,
    allow_missing: bool,
) -> Result<Vec<PathBuf>, SelectiveStorageError> {
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(source) if allow_missing && source.kind() == io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(source) => {
            return Err(SelectiveStorageError::Io {
                operation: "inspect publication artifact",
                source,
            });
        }
    };
    let matches_shape = match shape {
        PublicationShape::File => metadata.is_file(),
        PublicationShape::Tree => metadata.is_dir(),
    };
    if metadata.file_type().is_symlink() || !matches_shape {
        return Err(SelectiveStorageError::UnexpectedFileType {
            path: root.to_path_buf(),
        });
    }
    if shape == PublicationShape::File {
        return Ok(Vec::new());
    }
    let mut directories = Vec::new();
    inspect_publication_directory(root, &mut directories)?;
    Ok(directories)
}

fn inspect_publication_directory(
    directory: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<(), SelectiveStorageError> {
    let entries = std::fs::read_dir(directory).map_err(|source| SelectiveStorageError::Io {
        operation: "read publication staging directory",
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| SelectiveStorageError::Io {
            operation: "read publication staging entry",
            source,
        })?;
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|source| SelectiveStorageError::Io {
                operation: "inspect publication staging entry",
                source,
            })?;
        if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
            return Err(SelectiveStorageError::UnexpectedFileType { path });
        }
        if metadata.is_dir() {
            inspect_publication_directory(&path, directories)?;
        }
    }
    directories.push(directory.to_path_buf());
    Ok(())
}

async fn sync_publication_directories(root: &Path) -> Result<(), SelectiveStorageError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        for directory in inspect_publication_artifact(&root, PublicationShape::Tree, false)? {
            sync_directory_blocking(&directory).map_err(|source| SelectiveStorageError::Io {
                operation: "flush publication staging directory",
                source,
            })?;
        }
        Ok(())
    })
    .await
    .map_err(|source| SelectiveStorageError::Io {
        operation: "join publication directory flush",
        source: io::Error::other(source),
    })?
}

async fn sync_publication_directory(directory: PathBuf) -> Result<(), SelectiveStorageError> {
    tokio::task::spawn_blocking(move || sync_directory_blocking(&directory))
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join publication parent flush",
            source: io::Error::other(source),
        })?
        .map_err(|source| SelectiveStorageError::Io {
            operation: "flush publication parent directory",
            source,
        })
}

#[cfg(unix)]
fn sync_directory_blocking(directory: &Path) -> Result<(), io::Error> {
    std::fs::File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory_blocking(_directory: &Path) -> Result<(), io::Error> {
    // Directory handles are not portably syncable through std. The file
    // payloads are synced before publication and rename remains atomic on the
    // supported local filesystems, so use the strongest portable boundary.
    Ok(())
}

async fn rename_noreplace(
    source: PathBuf,
    destination: PathBuf,
) -> Result<(), SelectiveStorageError> {
    let collision = destination.clone();
    tokio::task::spawn_blocking(move || rename_noreplace_blocking(&source, &destination))
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join torrent payload publication",
            source: io::Error::other(source),
        })?
        .map_err(|source| {
            if source.kind() == io::ErrorKind::AlreadyExists {
                SelectiveStorageError::ExistingOutput(collision)
            } else {
                SelectiveStorageError::Io {
                    operation: "publish torrent payload artifact without replacement",
                    source,
                }
            }
        })
}

#[cfg(unix)]
fn rename_noreplace_blocking(source: &Path, destination: &Path) -> Result<(), io::Error> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)
}

#[cfg(not(unix))]
fn rename_noreplace_blocking(source: &Path, destination: &Path) -> Result<(), io::Error> {
    std::fs::rename(source, destination)
}

impl SelectiveStorage {
    pub async fn create_content(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        content: Arc<TorrentContent>,
        skipped: &[usize],
    ) -> Result<Self, SelectiveStorageError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default storage file limit is nonzero");
        Self::create_content_with_pool(output_root, artifact_identity, content, skipped, pool).await
    }

    pub(crate) async fn create_content_with_pool(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        content: Arc<TorrentContent>,
        skipped: &[usize],
        pool: StorageFilePool,
    ) -> Result<Self, SelectiveStorageError> {
        let layout = ContentLayout::from_content(&content);
        let selection = FileSelection::new_content(&layout, skipped)?;
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_content(&content),
        )?;
        let mut storage =
            Self::create_with_paths_and_pool(paths, artifact_identity, layout, selection, pool)
                .await?;
        storage.content = Some(content);
        Ok(storage)
    }

    pub async fn resume_content(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        content: Arc<TorrentContent>,
        skipped: &[usize],
        verified: Vec<bool>,
        expected_artifacts: Option<ResumeArtifactState>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default storage file limit is nonzero");
        let layout = ContentLayout::from_content(&content);
        let selection = FileSelection::new_content(&layout, skipped)?;
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_content(&content),
        )?;
        let (mut storage, resumed) = Self::resume_with_paths_and_pool_expected(
            paths,
            artifact_identity,
            layout,
            selection,
            verified,
            pool,
            expected_artifacts,
        )
        .await?;
        storage.content = Some(content);
        Ok((storage, resumed))
    }

    pub(crate) async fn resume_content_with_pool(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        content: Arc<TorrentContent>,
        skipped: &[usize],
        verified: Vec<bool>,
        pool: StorageFilePool,
        expected_artifacts: Option<ResumeArtifactState>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let layout = ContentLayout::from_content(&content);
        let selection = FileSelection::new_content(&layout, skipped)?;
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_content(&content),
        )?;
        let (mut storage, resumed) = Self::resume_with_paths_and_pool_expected(
            paths,
            artifact_identity,
            layout,
            selection,
            verified,
            pool,
            expected_artifacts,
        )
        .await?;
        storage.content = Some(content);
        Ok((storage, resumed))
    }

    pub async fn create(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
    ) -> Result<Self, SelectiveStorageError> {
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_metainfo(metainfo),
        )?;
        Self::create_with_paths(paths, artifact_identity, layout, selection).await
    }

    #[cfg(test)]
    pub(crate) async fn create_with_pool(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        pool: StorageFilePool,
    ) -> Result<Self, SelectiveStorageError> {
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_metainfo(metainfo),
        )?;
        Self::create_with_paths_and_pool(paths, artifact_identity, layout, selection, pool).await
    }

    pub(crate) async fn create_with_paths(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: TorrentLayout,
        selection: FileSelection,
    ) -> Result<Self, SelectiveStorageError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default storage file limit is nonzero");
        Self::create_with_paths_and_pool(paths, artifact_identity, layout, selection, pool).await
    }

    pub(crate) async fn create_with_paths_and_pool(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: impl Into<ContentLayout>,
        selection: FileSelection,
        pool: StorageFilePool,
    ) -> Result<Self, SelectiveStorageError> {
        let layout = layout.into();
        let TorrentStoragePaths {
            publication_shape,
            output: output_root,
            staging: staging_root,
            part: part_path,
        } = paths;
        if path_exists(&output_root, "inspect selected output").await? {
            return Err(SelectiveStorageError::ExistingOutput(output_root));
        }
        if path_exists(&staging_root, "inspect selected staging").await? {
            return Err(SelectiveStorageError::ExistingStaging(staging_root));
        }
        if path_exists(&part_path, "inspect selected part file").await? {
            return Err(SelectiveStorageError::ExistingPartFile(part_path));
        }

        let storage_id = storage_instance_id(artifact_identity.torrent_id);
        let mut files = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding || !selection.is_wanted(file_index) {
                files.push(None);
                continue;
            }
            let path = payload_path(
                publication_shape,
                &staging_root,
                &metainfo_file.path,
                file_index,
                layout.files().len(),
            )?;
            files.push(Some(RetainedFile::dynamic(
                path_storage_reference(
                    &pool,
                    &storage_id,
                    0,
                    StorageFileRole::Payload(file_index),
                    path,
                ),
                file_index,
                metainfo_file.length,
            )));
        }

        let part_reference = path_storage_reference(
            &pool,
            &storage_id,
            0,
            StorageFileRole::Part,
            part_path.clone(),
        );

        let identity = artifact_identity.part_file(&layout);
        let piece_count = layout.piece_count();
        let skipped_sources = vec![None; layout.files().len()];

        Ok(Self {
            content: None,
            backing: StorageBacking::Paths {
                output_root,
                staging_root,
                part_path,
                part_reference,
                storage_id,
            },
            identity,
            publication_shape,
            layout,
            selection,
            files,
            skipped_sources,
            part_file: None,
            part_checkpoint_handle: None,
            pending_promotions: Vec::new(),
            route_epoch: 0,
            verified: vec![false; piece_count],
            namespace_state: NamespaceState::Staging,
            namespace_generation: NamespaceState::Staging.initial_generation(),
        })
    }

    pub async fn create_with_platform(
        spec: PlatformStorageSpec,
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let layout = ContentLayout::from(layout);
        Self::create_with_platform_layout(
            spec,
            artifact_identity,
            layout,
            selection,
            verified,
            PublicationShape::from_metainfo(metainfo),
        )
        .await
    }

    pub async fn create_content_with_platform(
        spec: PlatformStorageSpec,
        artifact_identity: TorrentArtifactIdentity,
        content: Arc<TorrentContent>,
        skipped: &[usize],
        verified: Vec<bool>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let layout = ContentLayout::from_content(&content);
        let selection = FileSelection::new_content(&layout, skipped)?;
        let shape = PublicationShape::from_content(&content);
        let (mut storage, resumed) = Self::create_with_platform_layout(
            spec,
            artifact_identity,
            layout,
            selection,
            verified,
            shape,
        )
        .await?;
        storage.content = Some(content);
        Ok((storage, resumed))
    }

    async fn create_with_platform_layout(
        spec: PlatformStorageSpec,
        artifact_identity: TorrentArtifactIdentity,
        layout: ContentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
        publication_shape: PublicationShape,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        validate_publication_name(&spec.publication_name)?;
        if spec.publication_shape != publication_shape {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "platform publication shape",
            ));
        }
        if spec.storage_id != storage_instance_id(artifact_identity.torrent_id) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "platform storage identity",
            ));
        }
        if verified.len() != layout.piece_count() {
            return Err(SelectiveStorageError::InvalidVerifiedPiece {
                piece_index: verified.len(),
            });
        }
        if spec.managed {
            spec.pool.invalidate_storage(&spec.storage_id);
        }
        let resuming = spec.managed;
        let mut files = Vec::with_capacity(layout.files().len());
        let mut skipped_sources = Vec::with_capacity(layout.files().len());
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding {
                files.push(None);
                skipped_sources.push(None);
                continue;
            }
            let source = RetainedFileSource::Dynamic {
                reference: platform_storage_reference(
                    &spec,
                    StorageFileRole::Payload(file_index),
                    metainfo_file.path.clone(),
                ),
                file_index,
                expected_length: metainfo_file.length,
            };
            if selection.is_wanted(file_index) {
                files.push(Some(RetainedFile {
                    source,
                    routing_generation: 0,
                }));
                skipped_sources.push(None);
            } else if resuming {
                files.push(None);
                skipped_sources.push(Some(source));
            } else {
                files.push(None);
                skipped_sources.push(None);
            }
        }
        let part_reference = platform_storage_reference(&spec, StorageFileRole::Part, Vec::new());
        let identity = artifact_identity.part_file(&layout);
        let piece_count = layout.piece_count();
        let resumed = if spec.managed {
            if spec.published {
                ResumedStorage::Published
            } else {
                ResumedStorage::Staging
            }
        } else {
            ResumedStorage::Created
        };
        Ok((
            Self {
                content: None,
                backing: StorageBacking::Platform {
                    spec: spec.clone(),
                    part_reference,
                },
                identity,
                publication_shape: spec.publication_shape,
                layout,
                selection,
                files,
                skipped_sources,
                part_file: None,
                part_checkpoint_handle: None,
                pending_promotions: Vec::new(),
                route_epoch: 0,
                verified: if resumed == ResumedStorage::Created {
                    vec![false; piece_count]
                } else {
                    verified
                },
                namespace_state: if spec.published {
                    NamespaceState::Published
                } else {
                    NamespaceState::Staging
                },
                namespace_generation: spec.namespace_generation,
            },
            resumed,
        ))
    }

    pub async fn create_with_descriptors(
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        materialize_files: &[usize],
        descriptors: DescriptorStorage,
    ) -> Result<Self, SelectiveStorageError> {
        let layout = ContentLayout::from(layout);
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
                        RetainedFile::new(
                            initialize_descriptor_file(
                                file,
                                "wanted",
                                file_index,
                                metainfo_file.length,
                            )
                            .await?,
                            "retain selected descriptor",
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

        let identity = artifact_identity.part_file(&layout);
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
        let skipped_sources = vec![None; layout.files().len()];

        Ok(Self {
            content: None,
            backing: StorageBacking::Descriptors {
                reopened_part_file: Some(descriptors.reopened_part_file),
                materialization_files,
            },
            identity,
            publication_shape: PublicationShape::from_metainfo(metainfo),
            layout,
            selection,
            files,
            skipped_sources,
            part_file: Some(part_file),
            part_checkpoint_handle: None,
            pending_promotions: Vec::new(),
            route_epoch: 0,
            verified: vec![false; piece_count],
            namespace_state: NamespaceState::Staging,
            namespace_generation: NamespaceState::Staging.initial_generation(),
        })
    }

    pub async fn resume_with_descriptors(
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        descriptors: DescriptorStorage,
        verified: Vec<bool>,
    ) -> Result<Self, SelectiveStorageError> {
        let layout = ContentLayout::from(layout);
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
                        RetainedFile::new(
                            validate_descriptor_length(file, file_index, metainfo_file.length)
                                .await?,
                            "retain resumed selected descriptor",
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

        let identity = artifact_identity.part_file(&layout);
        let part_file = PartFile::open_preopened(descriptors.part_file, identity).await?;
        let validation_reopen = descriptors
            .reopened_part_file
            .try_clone()
            .map_err(|source| SelectiveStorageError::Io {
                operation: "duplicate descriptor for resumed part-file identity validation",
                source,
            })?;
        drop(PartFile::open_preopened(validation_reopen, identity).await?);

        let skipped_sources = vec![None; layout.files().len()];
        Ok(Self {
            content: None,
            backing: StorageBacking::Descriptors {
                reopened_part_file: Some(descriptors.reopened_part_file),
                materialization_files: (0..layout.files().len()).map(|_| None).collect(),
            },
            identity,
            publication_shape: PublicationShape::from_metainfo(metainfo),
            layout,
            selection,
            files,
            skipped_sources,
            part_file: Some(part_file),
            part_checkpoint_handle: None,
            pending_promotions: Vec::new(),
            route_epoch: 0,
            verified,
            namespace_state: NamespaceState::Staging,
            namespace_generation: NamespaceState::Staging.initial_generation(),
        })
    }

    pub async fn resume(
        output_root: PathBuf,
        artifact_identity: TorrentArtifactIdentity,
        metainfo: &Metainfo,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let paths = torrent_storage_paths_for_output_with_shape(
            output_root,
            artifact_identity.torrent_id,
            PublicationShape::from_metainfo(metainfo),
        )?;
        Self::resume_with_paths(paths, artifact_identity, layout, selection, verified).await
    }

    pub(crate) async fn resume_with_paths(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default storage file limit is nonzero");
        Self::resume_with_paths_and_pool(
            paths,
            artifact_identity,
            layout,
            selection,
            verified,
            pool,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn resume_with_paths_expected(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
        expected_artifacts: ResumeArtifactState,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let pool = StorageFilePool::new(DEFAULT_STORAGE_FILE_LIMIT, None)
            .expect("default storage file limit is nonzero");
        Self::resume_with_paths_and_pool_expected(
            paths,
            artifact_identity,
            layout,
            selection,
            verified,
            pool,
            Some(expected_artifacts),
        )
        .await
    }

    pub(crate) async fn resume_with_paths_and_pool(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: TorrentLayout,
        selection: FileSelection,
        verified: Vec<bool>,
        pool: StorageFilePool,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        Self::resume_with_paths_and_pool_expected(
            paths,
            artifact_identity,
            layout,
            selection,
            verified,
            pool,
            None,
        )
        .await
    }

    pub(crate) async fn resume_with_paths_and_pool_expected(
        paths: TorrentStoragePaths,
        artifact_identity: TorrentArtifactIdentity,
        layout: impl Into<ContentLayout>,
        selection: FileSelection,
        verified: Vec<bool>,
        pool: StorageFilePool,
        expected_artifacts: Option<ResumeArtifactState>,
    ) -> Result<(Self, ResumedStorage), SelectiveStorageError> {
        let layout = layout.into();
        if verified.len() != layout.piece_count() {
            return Err(SelectiveStorageError::InvalidVerifiedPiece {
                piece_index: verified.len(),
            });
        }
        let TorrentStoragePaths {
            publication_shape,
            output: output_root,
            staging: staging_root,
            part: part_path,
        } = paths;
        let storage_id = storage_instance_id(artifact_identity.torrent_id);
        // Recheck must observe the current namespace instead of an open handle
        // retained by the preceding download/check generation.
        pool.invalidate_storage(&storage_id);
        let output_exists = path_exists(&output_root, "inspect resumable selected output").await?;
        let staging_exists =
            path_exists(&staging_root, "inspect resumable selected staging").await?;
        let part_exists = path_exists(&part_path, "inspect resumable part file").await?;
        if layout.format() == MetainfoFormat::V2 && part_exists {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "v2 content cannot resume a part artifact",
            ));
        }

        match expected_artifacts {
            Some(ResumeArtifactState::None) => {
                if output_exists {
                    return Err(SelectiveStorageError::ExistingOutput(output_root));
                }
                if staging_exists {
                    return Err(SelectiveStorageError::ExistingStaging(staging_root));
                }
                if part_exists {
                    return Err(SelectiveStorageError::ExistingPartFile(part_path));
                }
            }
            Some(ResumeArtifactState::Staging) if output_exists => {
                return Err(SelectiveStorageError::IncompleteResumeArtifacts);
            }
            Some(ResumeArtifactState::Publishing) if output_exists && staging_exists => {
                return Err(SelectiveStorageError::IncompleteResumeArtifacts);
            }
            Some(ResumeArtifactState::Published) if staging_exists => {
                return Err(SelectiveStorageError::IncompleteResumeArtifacts);
            }
            Some(
                ResumeArtifactState::Staging
                | ResumeArtifactState::Publishing
                | ResumeArtifactState::Published,
            )
            | None => {}
        }

        if !output_exists && !staging_exists && !part_exists {
            let storage = Self::create_with_paths_and_pool(
                TorrentStoragePaths {
                    publication_shape,
                    output: output_root,
                    staging: staging_root,
                    part: part_path,
                },
                artifact_identity,
                layout,
                selection,
                pool,
            )
            .await?;
            return Ok((storage, ResumedStorage::Created));
        }
        if output_exists && staging_exists {
            return Err(SelectiveStorageError::IncompleteResumeArtifacts);
        }

        let (artifact_root, resumed, published) = if output_exists {
            (&output_root, ResumedStorage::Published, true)
        } else {
            (&staging_root, ResumedStorage::Staging, false)
        };
        let namespace_generation = u64::from(published);
        if output_exists || staging_exists {
            let artifact_metadata =
                tokio::fs::symlink_metadata(artifact_root)
                    .await
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "inspect resumable payload artifact",
                        source,
                    })?;
            let expected_type = match publication_shape {
                PublicationShape::File => artifact_metadata.is_file(),
                PublicationShape::Tree => artifact_metadata.is_dir(),
            };
            if !expected_type || artifact_metadata.file_type().is_symlink() {
                return Err(SelectiveStorageError::UnexpectedFileType {
                    path: artifact_root.clone(),
                });
            }
            validate_publication_artifact(artifact_root, publication_shape).await?;
        }

        let mut files = Vec::with_capacity(layout.files().len());
        let mut skipped_sources = Vec::with_capacity(layout.files().len());
        let mut pending_promotions = Vec::new();
        for (file_index, metainfo_file) in layout.files().iter().enumerate() {
            if metainfo_file.padding {
                files.push(None);
                skipped_sources.push(None);
                continue;
            }
            let path = payload_path(
                publication_shape,
                artifact_root,
                &metainfo_file.path,
                file_index,
                layout.files().len(),
            )?;
            match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) => {
                    if !metadata.is_file() || metadata.file_type().is_symlink() {
                        return Err(SelectiveStorageError::UnexpectedFileType { path });
                    }
                    let source = RetainedFileSource::Dynamic {
                        reference: path_storage_reference(
                            &pool,
                            &storage_id,
                            namespace_generation,
                            StorageFileRole::Payload(file_index),
                            path,
                        ),
                        file_index,
                        expected_length: metainfo_file.length,
                    };
                    if selection.is_wanted(file_index) {
                        files.push(Some(RetainedFile {
                            source,
                            routing_generation: 0,
                        }));
                        skipped_sources.push(None);
                    } else {
                        files.push(None);
                        skipped_sources.push(Some(source));
                    }
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    if !selection.is_wanted(file_index) {
                        files.push(None);
                        skipped_sources.push(None);
                        continue;
                    }
                    files.push(Some(RetainedFile::dynamic(
                        path_storage_reference(
                            &pool,
                            &storage_id,
                            namespace_generation,
                            StorageFileRole::Payload(file_index),
                            path,
                        ),
                        file_index,
                        metainfo_file.length,
                    )));
                    skipped_sources.push(None);
                    pending_promotions.push(file_index);
                }
                Err(source) => {
                    return Err(SelectiveStorageError::Io {
                        operation: "inspect resumable selected file",
                        source,
                    });
                }
            }
        }

        let identity = artifact_identity.part_file(&layout);
        let part_reference = path_storage_reference(
            &pool,
            &storage_id,
            namespace_generation,
            StorageFileRole::Part,
            part_path.clone(),
        );
        let part_file = if part_exists {
            Some(
                PartFile::open_with_reference(
                    part_reference.clone(),
                    Some(part_path.clone()),
                    identity,
                )
                .await?,
            )
        } else {
            None
        };
        let storage = Self {
            content: None,
            backing: StorageBacking::Paths {
                output_root,
                staging_root,
                part_path,
                part_reference,
                storage_id,
            },
            identity,
            publication_shape,
            layout,
            selection,
            files,
            skipped_sources,
            part_file,
            part_checkpoint_handle: None,
            pending_promotions,
            route_epoch: 0,
            verified,
            namespace_state: if published {
                NamespaceState::Published
            } else {
                NamespaceState::Staging
            },
            namespace_generation,
        };
        Ok((storage, resumed))
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

    /// Validate committed resume pieces from artifact structure alone.
    ///
    /// This method never reads payload bytes and never schedules hash work.
    pub(crate) async fn validate_fast_resume(
        &mut self,
        resumed: ResumedStorage,
    ) -> Result<FastResumeValidation, SelectiveStorageError> {
        let committed_pieces = self.verified.iter().filter(|verified| **verified).count();
        let mut validation = FastResumeValidation {
            evidence: ResumeStorageEvidence::Matches,
            committed_pieces,
            relevant_files: 0,
            artifact_observations: 0,
            part_header_bytes: self.part_file.as_ref().map_or(0, PartFile::header_length),
            payload_bytes_read: 0,
            hash_jobs: 0,
        };
        if resumed == ResumedStorage::Created && committed_pieces != 0 {
            validation.evidence = ResumeStorageEvidence::ContentMismatch(
                ResumeValidationRejectReason::CreatedStorageWithCommittedPieces,
            );
            return Ok(validation);
        }
        if committed_pieces == 0 {
            return Ok(validation);
        }

        // Prefix counts let each file ask whether its piece interval contains
        // a committed bit without a file-by-piece nested scan.
        let mut committed_prefix = Vec::with_capacity(self.verified.len() + 1);
        committed_prefix.push(0_u32);
        for verified in &self.verified {
            let next = committed_prefix
                .last()
                .copied()
                .unwrap_or_default()
                .checked_add(u32::from(*verified))
                .ok_or(SelectiveStorageError::InvalidStorageOperation(
                    "resume committed-piece count overflow",
                ))?;
            committed_prefix.push(next);
        }
        let mut part_required = vec![false; self.verified.len()];

        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.padding || metainfo_file.length == 0 {
                continue;
            }
            let range = self
                .layout
                .file_piece_range(file_index)?
                .expect("nonempty files have a piece range");
            let first = usize::try_from(*range.start())
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            let last = usize::try_from(*range.end())
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if committed_prefix[last + 1] == committed_prefix[first] {
                continue;
            }
            validation.relevant_files += 1;

            let (source, missing_may_use_part) = if self.selection.is_wanted(file_index) {
                (
                    self.files[file_index].as_ref().map(|file| &file.source),
                    self.pending_promotions.contains(&file_index),
                )
            } else {
                (self.skipped_sources[file_index].as_ref(), true)
            };
            let observation = match source {
                Some(source) => {
                    validation.artifact_observations += 1;
                    source.observe_exact(metainfo_file.length).await?
                }
                None => RetainedSourceObservation::Missing,
            };
            match observation {
                RetainedSourceObservation::Exact => continue,
                RetainedSourceObservation::WrongKind => {
                    validation.evidence = ResumeStorageEvidence::NeedsRepair;
                    return Ok(validation);
                }
                RetainedSourceObservation::WrongLength => {
                    validation.evidence = ResumeStorageEvidence::ContentMismatch(
                        ResumeValidationRejectReason::UnexpectedPayloadLength,
                    );
                    return Ok(validation);
                }
                RetainedSourceObservation::Missing if !missing_may_use_part => {
                    validation.evidence = ResumeStorageEvidence::ContentMismatch(
                        ResumeValidationRejectReason::MissingPayloadFile,
                    );
                    return Ok(validation);
                }
                RetainedSourceObservation::Missing => {
                    for piece_index in range {
                        let piece_index = usize::try_from(piece_index).map_err(|_| {
                            SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                        })?;
                        if self.verified[piece_index] {
                            part_required[piece_index] = true;
                        }
                    }
                }
            }
        }

        if !part_required.iter().any(|required| *required) {
            return Ok(validation);
        }
        if self.part_file.is_none()
            && let StorageBacking::Platform { part_reference, .. } = &self.backing
        {
            self.part_file =
                PartFile::open_optional_with_reference(part_reference.clone(), None, self.identity)
                    .await?;
            validation.part_header_bytes =
                self.part_file.as_ref().map_or(0, PartFile::header_length);
        }
        let Some(part_file) = self.part_file.as_ref() else {
            validation.evidence = ResumeStorageEvidence::ContentMismatch(
                ResumeValidationRejectReason::MissingPartFile,
            );
            return Ok(validation);
        };
        validation.artifact_observations += 1;
        let Some(part_length) = part_file.observed_file_length().await? else {
            validation.evidence = ResumeStorageEvidence::ContentMismatch(
                ResumeValidationRejectReason::MissingPartFile,
            );
            return Ok(validation);
        };
        for (piece_index, required) in part_required.into_iter().enumerate() {
            if !required {
                continue;
            }
            if !part_file.has_piece(piece_index)? {
                validation.evidence = ResumeStorageEvidence::ContentMismatch(
                    ResumeValidationRejectReason::MissingPartSlot,
                );
                return Ok(validation);
            }
            if !part_file.has_complete_piece_at_length(piece_index, part_length)? {
                validation.evidence = ResumeStorageEvidence::ContentMismatch(
                    ResumeValidationRejectReason::TruncatedPartSlot,
                );
                return Ok(validation);
            }
        }
        Ok(validation)
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
            StorageBacking::Platform { .. } | StorageBacking::Descriptors { .. } => None,
        }
    }

    pub fn part_slots(&self) -> usize {
        self.part_file
            .as_ref()
            .map_or(0, PartFile::mapped_piece_count)
    }

    pub fn has_part_file(&self) -> bool {
        self.part_file.is_some()
    }

    pub fn is_published(&self) -> bool {
        self.namespace_state == NamespaceState::Published
    }

    pub const fn namespace_state(&self) -> NamespaceState {
        self.namespace_state
    }

    pub const fn namespace_generation(&self) -> u64 {
        self.namespace_generation
    }

    fn decide_namespace_transition(
        &self,
        action: NamespaceAction,
        observed: bool,
    ) -> Result<NamespaceTransitionOutcome, SelectiveStorageError> {
        let outcome = decide_namespace_transition(NamespaceTransitionInput {
            state: self.namespace_state,
            current_generation: self.namespace_generation,
            expected_generation: self.namespace_generation,
            action,
        })?;
        if outcome.observation_required && !observed {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "unobserved namespace transition",
            ));
        }
        Ok(outcome)
    }

    fn commit_namespace_transition(&mut self, outcome: NamespaceTransitionOutcome) {
        if outcome.revoke_access {
            match &self.backing {
                StorageBacking::Paths {
                    part_reference,
                    storage_id,
                    ..
                } => part_reference.pool().invalidate_storage(storage_id),
                StorageBacking::Platform { spec, .. } => {
                    spec.pool.invalidate_storage(&spec.storage_id);
                }
                StorageBacking::Descriptors { .. } => {}
            }
        }
        self.namespace_state = outcome.state;
        self.namespace_generation = outcome.generation;
    }

    pub fn verified_pieces(&self) -> &[bool] {
        &self.verified
    }

    pub const fn route_epoch(&self) -> u64 {
        self.route_epoch
    }

    pub fn prepare_upload_read(
        &self,
        request: BlockRequest,
        expected_route_epoch: u64,
    ) -> Result<SelectiveUploadReadPlan, SelectiveStorageError> {
        if expected_route_epoch != self.route_epoch {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "stale active upload route epoch",
            ));
        }
        if request.length == 0 || request.length > MAX_REQUEST_BLOCK_LENGTH {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "invalid active upload request length",
            ));
        }
        let piece_index = usize::try_from(request.index).map_err(|_| {
            SelectiveStorageError::InvalidVerifiedPiece {
                piece_index: usize::MAX,
            }
        })?;
        if !self.verified.get(piece_index).copied().unwrap_or(false) {
            return Err(SelectiveStorageError::InvalidVerifiedPiece { piece_index });
        }
        let segments = self.layout.segments(
            request.index,
            request.begin,
            request.length,
            &self.selection,
        )?;
        if segments.len() > MAX_UPLOAD_READ_SEGMENTS {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "active upload read segment limit",
            ));
        }
        let mut spans = Vec::with_capacity(segments.len());
        for segment in segments {
            let span = match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let part_span = if self.pending_promotions.contains(&file_index) {
                        match self.part_file.as_ref() {
                            Some(part_file) if part_file.has_piece(piece_index)? => {
                                let span = part_file.plan_read_piece_range(
                                    piece_index,
                                    segment.piece_offset,
                                    segment.length,
                                )?;
                                Some((part_file.checkpoint_reference(), span))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some((source, part_span)) = part_span {
                        SelectiveUploadReadSpan::Part {
                            source,
                            file_offset: part_span.file_offset,
                            block_offset: segment.block_offset,
                            length: segment.length,
                        }
                    } else {
                        let file = self.files[file_index]
                            .as_ref()
                            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                        SelectiveUploadReadSpan::File {
                            source: file.source.clone(),
                            file_offset,
                            block_offset: segment.block_offset,
                            length: segment.length,
                        }
                    }
                }
                SegmentTarget::SkippedFile {
                    file_index,
                    file_offset,
                } => {
                    let part_span = match self.part_file.as_ref() {
                        Some(part_file) if part_file.has_piece(piece_index)? => {
                            let span = part_file.plan_read_piece_range(
                                piece_index,
                                segment.piece_offset,
                                segment.length,
                            )?;
                            Some((part_file.checkpoint_reference(), span))
                        }
                        _ => None,
                    };
                    if let Some((source, part_span)) = part_span {
                        SelectiveUploadReadSpan::Part {
                            source,
                            file_offset: part_span.file_offset,
                            block_offset: segment.block_offset,
                            length: segment.length,
                        }
                    } else {
                        let source = self.skipped_sources[file_index]
                            .as_ref()
                            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                        SelectiveUploadReadSpan::File {
                            source: source.clone(),
                            file_offset,
                            block_offset: segment.block_offset,
                            length: segment.length,
                        }
                    }
                }
                SegmentTarget::Padding => SelectiveUploadReadSpan::Padding {
                    block_offset: segment.block_offset,
                    length: segment.length,
                },
            };
            spans.push(span);
        }
        Ok(SelectiveUploadReadPlan {
            request,
            route_epoch: self.route_epoch,
            spans,
        })
    }

    pub fn prepare_file_read(
        &self,
        file_index: usize,
        offset: u64,
        length: usize,
        expected_route_epoch: u64,
    ) -> Result<SelectiveFileReadPlan, SelectiveStorageError> {
        if expected_route_epoch != self.route_epoch {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "stale active file route epoch",
            ));
        }
        if length == 0 || length > MAX_ACTIVE_FILE_READ_BYTES {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "invalid active file read length",
            ));
        }
        let file = self
            .layout
            .files()
            .get(file_index)
            .ok_or(SelectiveStorageError::Layout(
                LayoutError::InvalidFileIndex {
                    index: file_index,
                    file_count: self.layout.files().len(),
                },
            ))?;
        if file.padding || !self.selection.is_wanted(file_index) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "active file is not selected payload",
            ));
        }
        if self.pending_promotions.contains(&file_index) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "active file promotion is not reconciled",
            ));
        }
        let length_u64 = u64::try_from(length)
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        let end = offset
            .checked_add(length_u64)
            .ok_or(SelectiveStorageError::Layout(
                LayoutError::ArithmeticOverflow,
            ))?;
        if end > file.length {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "active file read exceeds file length",
            ));
        }
        let torrent_start =
            file.offset
                .checked_add(offset)
                .ok_or(SelectiveStorageError::Layout(
                    LayoutError::ArithmeticOverflow,
                ))?;
        let torrent_end =
            torrent_start
                .checked_add(length_u64 - 1)
                .ok_or(SelectiveStorageError::Layout(
                    LayoutError::ArithmeticOverflow,
                ))?;
        let piece_length = u64::from(self.layout.piece_length());
        let first_piece = torrent_start / piece_length;
        let last_piece = torrent_end / piece_length;
        for piece in first_piece..=last_piece {
            let piece = usize::try_from(piece)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self.verified.get(piece).copied().unwrap_or(false) {
                return Err(SelectiveStorageError::InvalidVerifiedPiece { piece_index: piece });
            }
        }
        let source = self.files[file_index]
            .as_ref()
            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
            .source
            .clone();
        Ok(SelectiveFileReadPlan {
            file_index,
            offset,
            length,
            route_epoch: self.route_epoch,
            source,
        })
    }

    pub async fn write_block(
        &mut self,
        piece_index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        let plan = self.plan_write(piece_index, begin, bytes).await?;
        self.execute_write_plan(plan).await
    }

    async fn plan_write(
        &mut self,
        piece_index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<SelectiveWritePlan, SelectiveStorageError> {
        let (segments, stats) = self.plan_layout_write(piece_index, begin, bytes.len())?;
        let piece_index_usize = usize::try_from(piece_index)
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        let payload = Arc::new(bytes);
        let mut spans = Vec::with_capacity(segments.len());
        for segment in segments {
            let destination = match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let file = self.files[file_index]
                        .as_ref()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    SelectiveWriteDestination::WantedFile {
                        file_index,
                        file_offset,
                        routing_generation: file.routing_generation,
                    }
                }
                SegmentTarget::SkippedFile { .. } if self.layout.format() == MetainfoFormat::V2 => {
                    return Err(SelectiveStorageError::InvalidStorageOperation(
                        "write skipped v2 piece",
                    ));
                }
                SegmentTarget::SkippedFile {
                    file_index,
                    file_offset,
                } => match self.files[file_index].as_ref() {
                    Some(file) => SelectiveWriteDestination::WantedFile {
                        file_index,
                        file_offset,
                        routing_generation: file.routing_generation,
                    },
                    None => SelectiveWriteDestination::PartFile(
                        self.ensure_part_file()
                            .await?
                            .plan_write_piece_range(
                                piece_index_usize,
                                segment.piece_offset,
                                segment.length,
                            )
                            .await?,
                    ),
                },
                SegmentTarget::Padding => {
                    unreachable!("padding was rejected while planning the write");
                }
            };
            spans.push(SelectiveWriteSpan {
                destination,
                block_offset: segment.block_offset,
                length: segment.length,
            });
        }
        Ok(SelectiveWritePlan {
            payload,
            spans,
            stats,
        })
    }

    pub(crate) async fn prepare_write(
        &mut self,
        piece_index: u32,
        begin: u32,
        bytes: Vec<u8>,
    ) -> Result<SelectiveWriteJob, SelectiveStorageError> {
        let plan = self.plan_write(piece_index, begin, bytes).await?;
        self.prepare_write_job(plan)
    }

    async fn execute_write_plan(
        &self,
        plan: SelectiveWritePlan,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        self.prepare_write_job(plan)?.execute().await
    }

    fn prepare_write_job(
        &self,
        plan: SelectiveWritePlan,
    ) -> Result<SelectiveWriteJob, SelectiveStorageError> {
        let mut executable = Vec::with_capacity(plan.spans.len());
        for span in &plan.spans {
            let end = span
                .block_offset
                .checked_add(span.length)
                .filter(|end| *end <= plan.payload.len())
                .ok_or(SelectiveStorageError::Layout(
                    LayoutError::ArithmeticOverflow,
                ))?;
            debug_assert!(end <= plan.payload.len());
            let (file, file_offset) = match span.destination {
                SelectiveWriteDestination::WantedFile {
                    file_index,
                    file_offset,
                    routing_generation,
                } => {
                    let file = self.files[file_index]
                        .as_ref()
                        .ok_or(SelectiveStorageError::StaleWriteRoute { file_index })?;
                    if file.routing_generation != routing_generation {
                        return Err(SelectiveStorageError::StaleWriteRoute { file_index });
                    }
                    (
                        ExecutableFileSource::Wanted(file.source.clone()),
                        file_offset,
                    )
                }
                SelectiveWriteDestination::PartFile(part_span) => {
                    let part_file = self
                        .part_file
                        .as_ref()
                        .ok_or(SelectiveStorageError::NotPublished)?;
                    part_file.validate_span(part_span)?;
                    (
                        ExecutableFileSource::Part(part_file.checkpoint_reference()),
                        part_span.file_offset,
                    )
                }
            };
            executable.push(ExecutableWriteSpan {
                file,
                file_offset,
                block_offset: span.block_offset,
                length: span.length,
            });
        }
        Ok(SelectiveWriteJob {
            payload: plan.payload,
            spans: executable,
            stats: plan.stats,
        })
    }

    pub(crate) fn write_stats(
        &self,
        piece_index: u32,
        begin: u32,
        length: usize,
    ) -> Result<SelectiveWriteStats, SelectiveStorageError> {
        self.plan_layout_write(piece_index, begin, length)
            .map(|(_, stats)| stats)
    }

    fn plan_layout_write(
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
                SegmentTarget::SkippedFile { file_index, .. } => {
                    if self.files[file_index].is_some() {
                        stats.wanted_bytes = stats.wanted_bytes.saturating_add(segment.length);
                    } else {
                        stats.skipped_bytes = stats.skipped_bytes.saturating_add(segment.length);
                    }
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

    fn prepare_blocking_hash_plan(
        &self,
        piece_index: usize,
        segments: &[LayoutSegment],
        algorithm: PieceHashAlgorithm,
    ) -> Result<SelectiveHashPlan, SelectiveStorageError> {
        let mut spans = Vec::with_capacity(segments.len());
        for segment in segments {
            match segment.target {
                SegmentTarget::WantedFile {
                    file_index,
                    file_offset,
                } => {
                    let part_span = if self.pending_promotions.contains(&file_index) {
                        self.part_file
                            .as_ref()
                            .map(|part_file| {
                                if !part_file.has_piece(piece_index)? {
                                    return Ok(None);
                                }
                                let span = part_file.plan_read_piece_range(
                                    piece_index,
                                    segment.piece_offset,
                                    segment.length,
                                )?;
                                part_file.validate_span(span)?;
                                Ok::<_, PartFileError>(Some((
                                    part_file.checkpoint_reference(),
                                    span,
                                )))
                            })
                            .transpose()?
                            .flatten()
                    } else {
                        None
                    };
                    if let Some((file, span)) = part_span {
                        spans.push(BlockingHashSpan::PartFile { file, span });
                        continue;
                    }
                    let file = self.files[file_index]
                        .as_ref()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    spans.push(BlockingHashSpan::WantedFile {
                        file: file.source.clone(),
                        file_offset,
                        length: segment.length,
                    });
                }
                SegmentTarget::Padding => spans.push(BlockingHashSpan::Padding {
                    length: segment.length,
                }),
                SegmentTarget::SkippedFile {
                    file_index,
                    file_offset,
                } => {
                    let part_span = self
                        .part_file
                        .as_ref()
                        .filter(|part_file| part_file.has_piece(piece_index).unwrap_or(false))
                        .map(|part_file| {
                            let span = part_file.plan_read_piece_range(
                                piece_index,
                                segment.piece_offset,
                                segment.length,
                            )?;
                            part_file.validate_span(span)?;
                            Ok::<_, PartFileError>((part_file.checkpoint_reference(), span))
                        })
                        .transpose()?;
                    if let Some((file, span)) = part_span {
                        spans.push(BlockingHashSpan::PartFile { file, span });
                    } else if let Some(file) = self.skipped_sources[file_index].as_ref() {
                        spans.push(BlockingHashSpan::WantedFile {
                            file: file.clone(),
                            file_offset,
                            length: segment.length,
                        });
                    } else {
                        let part_file = self
                            .part_file
                            .as_ref()
                            .ok_or(SelectiveStorageError::NotPublished)?;
                        let span = part_file.plan_read_piece_range(
                            piece_index,
                            segment.piece_offset,
                            segment.length,
                        )?;
                        part_file.validate_span(span)?;
                        spans.push(BlockingHashSpan::PartFile {
                            file: part_file.checkpoint_reference(),
                            span,
                        });
                    }
                }
            }
        }
        Ok(SelectiveHashPlan { spans, algorithm })
    }

    pub(crate) fn prepare_hash(
        &self,
        piece_index: u32,
    ) -> Result<SelectiveHashPlan, SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let segments = self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?;
        let piece_index_usize = usize::try_from(piece_index)
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        let algorithm = match self
            .content
            .as_ref()
            .map(|content| content.expected_piece(piece_index))
            .transpose()
            .map_err(|_| {
                SelectiveStorageError::InvalidStorageOperation("invalid expected piece geometry")
            })? {
            Some(ExpectedPieceIntegrity::V2Merkle { target_height, .. }) => {
                PieceHashAlgorithm::V2Merkle { target_height }
            }
            Some(ExpectedPieceIntegrity::V1Sha1(_)) | None => PieceHashAlgorithm::Sha1,
        };
        self.prepare_blocking_hash_plan(piece_index_usize, &segments, algorithm)
    }

    async fn hash_piece_with_stats(
        &mut self,
        piece_index: u32,
    ) -> Result<([u8; 20], SelectiveHashIoStats), SelectiveStorageError> {
        let plan = self.prepare_hash(piece_index)?;
        plan.hash().await
    }

    pub async fn hash_piece_content(
        &self,
        piece_index: u32,
    ) -> Result<ComputedPieceHash, SelectiveStorageError> {
        self.prepare_hash(piece_index)?.execute_content().await
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
                SegmentTarget::SkippedFile { file_index, .. } => {
                    if self.files[file_index].is_some() {
                        DurabilityTarget::WantedFile(file_index)
                    } else {
                        DurabilityTarget::PartFile
                    }
                }
                SegmentTarget::Padding => continue,
            };
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    pub(crate) async fn checkpoint_handles(
        &mut self,
    ) -> Result<CheckpointHandles, SelectiveStorageError> {
        let mut handles = BTreeMap::new();
        for (file_index, file) in self.files.iter().enumerate() {
            let Some(file) = file else {
                continue;
            };
            let cell = Arc::new(OnceLock::new());
            cell.set(file.checkpoint_reference())
                .expect("new selected-file checkpoint cell is empty");
            handles.insert(DurabilityTarget::WantedFile(file_index), cell);
        }
        let part_cell = Arc::new(OnceLock::new());
        if let Some(part_file) = self.part_file.as_ref() {
            part_cell
                .set(match part_file.checkpoint_reference() {
                    PartFileCheckpointReference::Dynamic(reference) => {
                        CheckpointFileReference::Dynamic(reference)
                    }
                    PartFileCheckpointReference::Fixed(file) => {
                        CheckpointFileReference::Fixed(file)
                    }
                })
                .expect("new part-file checkpoint cell is empty");
        }
        self.part_checkpoint_handle = Some(part_cell.clone());
        handles.insert(DurabilityTarget::PartFile, part_cell);
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
        self.sync_pieces(&[piece_index]).await
    }

    pub(crate) async fn sync_pieces(
        &mut self,
        piece_indices: &[u32],
    ) -> Result<(), SelectiveStorageError> {
        let mut wanted_files = Vec::new();
        let mut sync_part = false;
        for &piece_index in piece_indices {
            let piece_length = self.layout.piece_length_at(piece_index)?;
            let segments = self
                .layout
                .segments(piece_index, 0, piece_length, &self.selection)?;
            for segment in segments {
                match segment.target {
                    SegmentTarget::WantedFile { file_index, .. } => {
                        if !wanted_files.contains(&file_index) {
                            wanted_files.push(file_index);
                        }
                    }
                    SegmentTarget::SkippedFile { file_index, .. } => {
                        if self.files[file_index].is_some() {
                            if !wanted_files.contains(&file_index) {
                                wanted_files.push(file_index);
                            }
                        } else {
                            sync_part = true;
                        }
                    }
                    SegmentTarget::Padding => {}
                }
            }
        }
        for file_index in wanted_files {
            let file = self.files[file_index]
                .as_ref()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                .acquire(StorageFileAccess::ReadWriteExisting)
                .await?;
            sync_file(file, "flush verified selected piece").await?;
        }
        if sync_part {
            self.part_file_mut()?.sync_payload().await?;
        }
        Ok(())
    }

    pub async fn publish(&mut self) -> Result<(), SelectiveStorageError> {
        self.prepare_path_publication().await?;
        self.commit_path_publication().await
    }

    pub(crate) async fn prepare_path_publication(&mut self) -> Result<(), SelectiveStorageError> {
        let staging_root = match &self.backing {
            StorageBacking::Paths { staging_root, .. } => staging_root.clone(),
            StorageBacking::Platform { .. } | StorageBacking::Descriptors { .. } => {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "path publication",
                ));
            }
        };
        validate_publication_artifact(&staging_root, self.publication_shape).await?;
        let publication_shape = self.publication_shape;
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.padding || !self.selection.is_wanted(file_index) {
                continue;
            }
            let file = self.files[file_index]
                .as_ref()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                .acquire(StorageFileAccess::ReadWriteCreate)
                .await?;
            sync_file(file, "flush normalized selected file").await?;
        }
        validate_publication_artifact(&staging_root, self.publication_shape).await?;
        self.sync_verified().await?;
        if publication_shape == PublicationShape::Tree {
            sync_publication_directories(&staging_root).await?;
        }
        let transition =
            self.decide_namespace_transition(NamespaceAction::PreparePublication, false)?;
        self.commit_namespace_transition(transition);
        Ok(())
    }

    pub(crate) async fn commit_path_publication(&mut self) -> Result<(), SelectiveStorageError> {
        self.rename_path_publication().await?;
        self.sync_path_publication_namespace().await?;
        self.finish_path_publication()?;
        Ok(())
    }

    pub(crate) async fn rename_path_publication(&mut self) -> Result<(), SelectiveStorageError> {
        let (output_root, staging_root, pool, storage_id) = match &self.backing {
            StorageBacking::Paths {
                output_root,
                staging_root,
                part_reference,
                storage_id,
                ..
            } => (
                output_root.clone(),
                staging_root.clone(),
                part_reference.pool().clone(),
                storage_id.clone(),
            ),
            StorageBacking::Platform { .. } | StorageBacking::Descriptors { .. } => {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "path publication",
                ));
            }
        };
        pool.invalidate_storage(&storage_id);
        rename_noreplace(staging_root, output_root).await
    }

    pub(crate) async fn sync_path_publication_namespace(
        &self,
    ) -> Result<(), SelectiveStorageError> {
        let output_root = match &self.backing {
            StorageBacking::Paths { output_root, .. } => output_root,
            StorageBacking::Platform { .. } | StorageBacking::Descriptors { .. } => {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "path publication",
                ));
            }
        };
        let parent = output_root
            .parent()
            .ok_or(SelectiveStorageError::InvalidOutputPath)?
            .to_path_buf();
        sync_publication_directory(parent).await
    }

    pub(crate) fn finish_path_publication(&mut self) -> Result<(), SelectiveStorageError> {
        let transition =
            self.decide_namespace_transition(NamespaceAction::ConfirmPublication, true)?;
        let publication_shape = self.publication_shape;
        let file_count = self.layout.files().len();
        let (output_root, pool, storage_id) = match &self.backing {
            StorageBacking::Paths {
                output_root,
                part_reference,
                storage_id,
                ..
            } => (
                output_root.clone(),
                part_reference.pool().clone(),
                storage_id.clone(),
            ),
            StorageBacking::Platform { .. } | StorageBacking::Descriptors { .. } => {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "path publication",
                ));
            }
        };
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            let Some(file) = self.files[file_index].as_mut() else {
                continue;
            };
            let routing_generation = file.routing_generation;
            *file = RetainedFile::dynamic(
                path_storage_reference(
                    &pool,
                    &storage_id,
                    transition.generation,
                    StorageFileRole::Payload(file_index),
                    payload_path(
                        publication_shape,
                        &output_root,
                        &metainfo_file.path,
                        file_index,
                        file_count,
                    )?,
                ),
                file_index,
                metainfo_file.length,
            );
            file.routing_generation = routing_generation;
        }
        if let StorageBacking::Paths {
            part_path,
            part_reference,
            ..
        } = &mut self.backing
        {
            *part_reference = path_storage_reference(
                &pool,
                &storage_id,
                transition.generation,
                StorageFileRole::Part,
                part_path.clone(),
            );
        }
        self.commit_namespace_transition(transition);
        Ok(())
    }

    pub async fn prepare_descriptors(&mut self) -> Result<(), SelectiveStorageError> {
        if !matches!(self.backing, StorageBacking::Descriptors { .. }) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "descriptor preparation",
            ));
        }
        self.sync_verified().await?;
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.length == 0
                && !metainfo_file.padding
                && self.selection.is_wanted(file_index)
            {
                self.files[file_index]
                    .as_ref()
                    .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                    .acquire(StorageFileAccess::ReadWriteCreate)
                    .await?;
            }
        }
        let transition =
            self.decide_namespace_transition(NamespaceAction::PreparePublication, false)?;
        self.commit_namespace_transition(transition);
        Ok(())
    }

    pub async fn prepare_platform(&mut self) -> Result<(), SelectiveStorageError> {
        if !matches!(self.backing, StorageBacking::Platform { .. }) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "platform preparation",
            ));
        }
        self.sync_verified().await?;
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.length == 0
                && !metainfo_file.padding
                && self.selection.is_wanted(file_index)
            {
                self.files[file_index]
                    .as_ref()
                    .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
                    .acquire(StorageFileAccess::ReadWriteCreate)
                    .await?;
            }
        }
        let transition =
            self.decide_namespace_transition(NamespaceAction::PreparePublication, false)?;
        self.commit_namespace_transition(transition);
        Ok(())
    }

    pub async fn finish_published(&mut self) -> Result<(), SelectiveStorageError> {
        if !self.is_published() {
            return Err(SelectiveStorageError::NotPublished);
        }
        self.ensure_complete_selection()?;
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.padding || !self.selection.is_wanted(file_index) {
                continue;
            }
            let file = self.files[file_index]
                .as_ref()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
            let actual = match file.acquire(StorageFileAccess::ReadExisting).await {
                Ok(file) => file
                    .file()
                    .metadata()
                    .map_err(|source| SelectiveStorageError::Io {
                        operation: "inspect checked published file",
                        source,
                    })?
                    .len(),
                Err(error) if metainfo_file.length == 0 && error.is_missing_or_short_source() => {
                    file.acquire(StorageFileAccess::ReadWriteCreate).await?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if actual > metainfo_file.length {
                file.acquire(StorageFileAccess::ReadWriteCreate).await?;
            } else if actual < metainfo_file.length {
                return Err(SelectiveStorageError::UnexpectedFileLength {
                    file_index,
                    expected: metainfo_file.length,
                    actual,
                });
            }
        }
        Ok(())
    }

    async fn sync_verified(&mut self) -> Result<(), SelectiveStorageError> {
        self.ensure_complete_selection()?;

        for (file_index, file) in self.files.iter().enumerate() {
            let Some(file) = file else { continue };
            if self.layout.files()[file_index].length == 0 {
                continue;
            }
            sync_file(
                file.acquire(StorageFileAccess::ReadWriteCreate).await?,
                "flush selected staging file",
            )
            .await?;
        }
        if let Some(part_file) = self.part_file.as_ref() {
            part_file.sync_payload().await?;
        }
        Ok(())
    }

    fn ensure_complete_selection(&self) -> Result<(), SelectiveStorageError> {
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
        Ok(())
    }

    pub async fn reopen_part_file(&mut self) -> Result<(), SelectiveStorageError> {
        if self.part_file.is_none() {
            return Ok(());
        }
        self.part_file.take();
        self.part_file = Some(match &mut self.backing {
            StorageBacking::Paths {
                part_path,
                part_reference,
                ..
            } => {
                PartFile::open_with_reference(
                    part_reference.clone(),
                    Some(part_path.clone()),
                    self.identity,
                )
                .await?
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
            StorageBacking::Platform { part_reference, .. } => {
                PartFile::open_with_reference(part_reference.clone(), None, self.identity).await?
            }
        });
        Ok(())
    }

    pub(crate) async fn has_piece_sources(
        &mut self,
        piece_index: u32,
    ) -> Result<bool, SelectiveStorageError> {
        let piece_length = self.layout.piece_length_at(piece_index)?;
        let segments = self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?;
        let piece_index = usize::try_from(piece_index)
            .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
        for segment in segments {
            match segment.target {
                SegmentTarget::WantedFile { file_index, .. } => {
                    let file = self.files[file_index]
                        .as_ref()
                        .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
                    if file.source.is_available().await? {
                        continue;
                    }
                    if self.pending_promotions.contains(&file_index)
                        && let Some(part_file) = self.part_file.as_ref()
                        && part_file.has_piece(piece_index)?
                    {
                        continue;
                    }
                    return Ok(false);
                }
                SegmentTarget::SkippedFile { file_index, .. } => {
                    if let Some(source) = self.skipped_sources[file_index].clone() {
                        if source.is_available().await? {
                            continue;
                        }
                        self.skipped_sources[file_index] = None;
                    }
                    if self.part_file.is_none()
                        && let StorageBacking::Platform { part_reference, .. } = &self.backing
                    {
                        self.part_file = PartFile::open_optional_with_reference(
                            part_reference.clone(),
                            None,
                            self.identity,
                        )
                        .await?;
                    }
                    let Some(part_file) = self.part_file.as_ref() else {
                        return Ok(false);
                    };
                    if !part_file.has_piece(piece_index)? {
                        return Ok(false);
                    }
                }
                SegmentTarget::Padding => {}
            }
        }
        Ok(true)
    }

    pub(crate) async fn reconcile_after_recheck(&mut self) -> Result<(), SelectiveStorageError> {
        let pending_promotions = std::mem::take(&mut self.pending_promotions);
        for file_index in pending_promotions {
            let mut recoverable = false;
            for piece_index in self
                .layout
                .file_piece_range(file_index)?
                .into_iter()
                .flatten()
            {
                let piece_index = usize::try_from(piece_index)
                    .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
                if self.verified.get(piece_index).copied().unwrap_or(false)
                    && let Some(part_file) = self.part_file.as_ref()
                    && part_file.has_piece(piece_index)?
                {
                    recoverable = true;
                    break;
                }
            }
            if recoverable {
                self.restore_promoted_file(file_index, false).await?;
            }
        }
        self.release_unused_part_slots().await
    }

    pub async fn reconcile_selection(
        &mut self,
        selection: FileSelection,
    ) -> Result<SelectionReconcileReport, SelectiveStorageError> {
        if selection.file_count() != self.layout.files().len() {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "file selection geometry",
            ));
        }
        let next_epoch = self.route_epoch.checked_add(1).ok_or(
            SelectiveStorageError::InvalidStorageOperation("storage route epoch overflow"),
        )?;
        let promoted_files = self
            .layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| {
                !file.padding && !self.selection.is_wanted(*index) && selection.is_wanted(*index)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let demoted_files = self
            .layout
            .files()
            .iter()
            .enumerate()
            .filter(|(index, file)| {
                !file.padding && self.selection.is_wanted(*index) && !selection.is_wanted(*index)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if promoted_files.is_empty() && demoted_files.is_empty() {
            return Ok(SelectionReconcileReport {
                route_epoch: self.route_epoch,
                promoted_files,
                demoted_files,
                invalidated_pieces: Vec::new(),
            });
        }

        let previous_files = self.files.clone();
        let previous_skipped_sources = self.skipped_sources.clone();
        let previous_selection = self.selection.clone();
        let previous_verified = self.verified.clone();
        let published = self.is_published();
        let namespace_generation = self.namespace_generation;
        let mut promotions_requiring_part = BTreeSet::new();
        for &file_index in &demoted_files {
            if let Some(file) = self.files[file_index].take() {
                self.skipped_sources[file_index] = Some(file.source);
            }
        }
        for &file_index in &promoted_files {
            let metainfo_file = &self.layout.files()[file_index];
            let source = match self.skipped_sources[file_index].take() {
                Some(source) => source,
                None => {
                    promotions_requiring_part.insert(file_index);
                    match &mut self.backing {
                        StorageBacking::Paths {
                            output_root,
                            staging_root,
                            part_reference,
                            storage_id,
                            ..
                        } => {
                            let root = if published { output_root } else { staging_root };
                            let path = payload_path(
                                self.publication_shape,
                                root,
                                &metainfo_file.path,
                                file_index,
                                self.layout.files().len(),
                            )?;
                            RetainedFileSource::Dynamic {
                                reference: path_storage_reference(
                                    part_reference.pool(),
                                    storage_id,
                                    namespace_generation,
                                    StorageFileRole::Payload(file_index),
                                    path,
                                ),
                                file_index,
                                expected_length: metainfo_file.length,
                            }
                        }
                        StorageBacking::Platform { spec, .. } => RetainedFileSource::Dynamic {
                            reference: platform_storage_reference(
                                spec,
                                StorageFileRole::Payload(file_index),
                                metainfo_file.path.clone(),
                            ),
                            file_index,
                            expected_length: metainfo_file.length,
                        },
                        StorageBacking::Descriptors { .. } => {
                            self.files = previous_files;
                            self.skipped_sources = previous_skipped_sources;
                            self.verified = previous_verified;
                            return Err(SelectiveStorageError::InvalidStorageOperation(
                                "dynamic descriptor file selection",
                            ));
                        }
                    }
                }
            };
            self.files[file_index] = Some(RetainedFile {
                source,
                routing_generation: next_epoch,
            });
        }

        let reconcile_result = async {
            let mut invalidated_pieces = BTreeSet::new();
            for &file_index in &promoted_files {
                invalidated_pieces.extend(
                    self.restore_promoted_file(
                        file_index,
                        promotions_requiring_part.contains(&file_index),
                    )
                    .await?,
                );
            }
            self.selection = selection;
            self.release_unused_part_slots().await?;
            Ok::<_, SelectiveStorageError>(invalidated_pieces.into_iter().collect::<Vec<_>>())
        }
        .await;
        let invalidated_pieces = match reconcile_result {
            Ok(invalidated_pieces) => invalidated_pieces,
            Err(error) => {
                self.files = previous_files;
                self.skipped_sources = previous_skipped_sources;
                self.selection = previous_selection;
                self.verified = previous_verified;
                return Err(error);
            }
        };
        self.route_epoch = next_epoch;
        Ok(SelectionReconcileReport {
            route_epoch: next_epoch,
            promoted_files,
            demoted_files,
            invalidated_pieces,
        })
    }

    pub async fn materialize_file(
        &mut self,
        file_index: usize,
    ) -> Result<MaterializationReport, SelectiveStorageError> {
        if !matches!(
            self.namespace_state,
            NamespaceState::Publishing | NamespaceState::Published
        ) {
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
        for piece_index in self
            .layout
            .file_piece_range(file_index)?
            .into_iter()
            .flatten()
        {
            let piece_index_usize = usize::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if !self.verified[piece_index_usize] {
                return Err(SelectiveStorageError::IncompleteMaterialization {
                    file_index,
                    piece_index: piece_index_usize,
                });
            }
        }

        if let StorageBacking::Platform { spec, .. } = &self.backing {
            if !spec.published {
                return Err(SelectiveStorageError::InvalidStorageOperation(
                    "materialize unpublished platform file",
                ));
            }
            self.files[file_index] = Some(RetainedFile::dynamic(
                platform_storage_reference(
                    spec,
                    StorageFileRole::Payload(file_index),
                    metainfo_file.path.clone(),
                ),
                file_index,
                metainfo_file.length,
            ));
            self.restore_promoted_file(file_index, false).await?;
            let slots_before = self.part_slots();
            self.selection
                .set_wanted_content(&self.layout, file_index, true)?;
            self.release_unused_part_slots().await?;
            return Ok(MaterializationReport {
                file_index,
                bytes: metainfo_file.length,
                slots_before,
                slots_after: self.part_slots(),
            });
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
            StorageBacking::Platform { .. } => unreachable!("platform handled above"),
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
                self.files[file_index] =
                    Some(RetainedFile::new(output, "retain materialized descriptor").await?);
            }
        }

        let slots_before = self.part_slots();
        self.selection
            .set_wanted_content(&self.layout, file_index, true)?;
        for piece_index in self
            .layout
            .file_piece_range(file_index)?
            .into_iter()
            .flatten()
        {
            if !self.piece_requires_part_file(piece_index)?
                && let Some(part_file) = self.part_file.as_mut()
            {
                part_file
                    .release_piece(usize::try_from(piece_index).map_err(|_| {
                        SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow)
                    })?)
                    .await?;
            }
        }
        self.remove_empty_part_file().await?;
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
        if !matches!(
            self.backing,
            StorageBacking::Descriptors { .. } | StorageBacking::Platform { .. }
        ) {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "descriptor hash finalization",
            ));
        }
        if !matches!(
            self.namespace_state,
            NamespaceState::Publishing | NamespaceState::Published
        ) {
            return Err(SelectiveStorageError::NotPublished);
        }
        let mut hashes = Vec::new();
        for (file_index, metainfo_file) in self.layout.files().iter().enumerate() {
            if metainfo_file.padding || !self.selection.is_wanted(file_index) {
                continue;
            }
            let file = self.files[file_index]
                .as_ref()
                .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?;
            let file = file.acquire(StorageFileAccess::ReadExisting).await?;
            let file_length = metainfo_file.length;
            let sha1 = tokio::task::spawn_blocking(move || {
                let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
                let mut offset = 0_u64;
                let mut hasher = Sha1::new();
                while offset < file_length {
                    let length = usize::try_from((file_length - offset).min(buffer.len() as u64))
                        .map_err(|_| io::Error::other("prepared file length overflow"))?;
                    read_exact_at(file.file(), &mut buffer[..length], offset)?;
                    hasher.update(&buffer[..length]);
                    offset += length as u64;
                }
                Ok::<[u8; 20], io::Error>(hasher.finalize().into())
            })
            .await
            .map_err(|source| SelectiveStorageError::Io {
                operation: "join prepared descriptor hash",
                source: io::Error::other(source),
            })?
            .map_err(|source| SelectiveStorageError::Io {
                operation: "hash prepared descriptor",
                source,
            })?;
            hashes.push(PreparedFileHash {
                file_index,
                length: metainfo_file.length,
                sha1,
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

    async fn ensure_part_file(&mut self) -> Result<&mut PartFile, SelectiveStorageError> {
        if self.layout.format() == MetainfoFormat::V2 {
            return Err(SelectiveStorageError::InvalidStorageOperation(
                "create v2 part artifact",
            ));
        }
        if self.part_file.is_none() {
            let (part_path, part_reference) = match &self.backing {
                StorageBacking::Paths {
                    part_path,
                    part_reference,
                    ..
                } => (Some(part_path.clone()), part_reference.clone()),
                StorageBacking::Descriptors { .. } => {
                    return Err(SelectiveStorageError::InvalidStorageOperation(
                        "lazy descriptor part-file creation",
                    ));
                }
                StorageBacking::Platform { part_reference, .. } => (None, part_reference.clone()),
            };
            let part_file =
                PartFile::create_with_reference(part_reference, part_path, self.identity).await?;
            if let Some(handle) = self.part_checkpoint_handle.as_ref() {
                handle
                    .set(match part_file.checkpoint_reference() {
                        PartFileCheckpointReference::Dynamic(reference) => {
                            CheckpointFileReference::Dynamic(reference)
                        }
                        PartFileCheckpointReference::Fixed(file) => {
                            CheckpointFileReference::Fixed(file)
                        }
                    })
                    .map_err(|_| {
                        SelectiveStorageError::InvalidStorageOperation(
                            "replace registered part-file checkpoint handle",
                        )
                    })?;
            }
            self.part_file = Some(part_file);
        }
        self.part_file_mut()
    }

    async fn restore_promoted_file(
        &mut self,
        file_index: usize,
        missing_verified_source_invalidates: bool,
    ) -> Result<Vec<usize>, SelectiveStorageError> {
        let metainfo_file = self
            .layout
            .files()
            .get(file_index)
            .ok_or(LayoutError::InvalidFileIndex {
                index: file_index,
                file_count: self.layout.files().len(),
            })?
            .clone();
        let destination = self.files[file_index]
            .as_ref()
            .ok_or(SelectiveStorageError::MissingWantedFile { file_index })?
            .acquire(StorageFileAccess::ReadWriteCreate)
            .await?;
        destination
            .file()
            .set_len(metainfo_file.length)
            .map_err(|source| SelectiveStorageError::Io {
                operation: "size promoted selected file",
                source,
            })?;
        let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
        let mut file_offset = 0_u64;
        let mut invalidated_pieces = BTreeSet::new();
        while file_offset < metainfo_file.length {
            let torrent_offset = metainfo_file.offset.checked_add(file_offset).ok_or(
                SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow),
            )?;
            let piece_index_u64 = torrent_offset / u64::from(self.layout.piece_length());
            let piece_index = usize::try_from(piece_index_u64)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
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
            let part_has_piece = match self.part_file.as_ref() {
                Some(part_file) => part_file.has_piece(piece_index)?,
                None => false,
            };
            if self.verified[piece_index] && part_has_piece {
                self.part_file
                    .as_ref()
                    .expect("part file was checked above")
                    .read_piece_range(piece_index, piece_offset, &mut buffer[..length])
                    .await?;
                let destination = destination.clone();
                let payload: Arc<[u8]> = Arc::from(&buffer[..length]);
                tokio::task::spawn_blocking(move || {
                    write_all_at(destination.file(), &payload, file_offset)
                })
                .await
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "join promoted selected-file write",
                    source: io::Error::other(source),
                })?
                .map_err(|source| SelectiveStorageError::Io {
                    operation: "write promoted selected-file range",
                    source,
                })?;
            } else if self.verified[piece_index] && missing_verified_source_invalidates {
                self.verified[piece_index] = false;
                invalidated_pieces.insert(piece_index);
            }
            file_offset =
                file_offset
                    .checked_add(length as u64)
                    .ok_or(SelectiveStorageError::Layout(
                        LayoutError::ArithmeticOverflow,
                    ))?;
        }
        sync_file(destination, "flush promoted selected file").await?;
        Ok(invalidated_pieces.into_iter().collect())
    }

    async fn release_unused_part_slots(&mut self) -> Result<(), SelectiveStorageError> {
        for piece_index in 0..self.layout.piece_count() {
            let piece_index_u32 = u32::try_from(piece_index)
                .map_err(|_| SelectiveStorageError::Layout(LayoutError::ArithmeticOverflow))?;
            if self.piece_requires_part_file(piece_index_u32)? {
                continue;
            }
            if let Some(part_file) = self.part_file.as_mut() {
                part_file.release_piece(piece_index).await?;
            }
        }
        self.remove_empty_part_file().await
    }

    fn piece_requires_part_file(&self, piece_index: u32) -> Result<bool, SelectiveStorageError> {
        if self.layout.format() == MetainfoFormat::V2 {
            return Ok(false);
        }
        let piece_length = self.layout.piece_length_at(piece_index)?;
        Ok(self
            .layout
            .segments(piece_index, 0, piece_length, &self.selection)?
            .into_iter()
            .any(|segment| {
                matches!(
                    segment.target,
                    SegmentTarget::SkippedFile { file_index, .. }
                        if self.files[file_index].is_none()
                )
            }))
    }

    async fn remove_empty_part_file(&mut self) -> Result<(), SelectiveStorageError> {
        if self
            .part_file
            .as_ref()
            .is_none_or(|part_file| part_file.mapped_piece_count() != 0)
        {
            return Ok(());
        }
        match &mut self.backing {
            StorageBacking::Paths {
                part_path,
                part_reference,
                ..
            } => {
                self.part_checkpoint_handle.take();
                self.part_file.take();
                part_reference.pool().invalidate_key(part_reference.key());
                remove_file_if_present(part_path).await.map_err(|source| {
                    SelectiveStorageError::Io {
                        operation: "remove empty part file",
                        source,
                    }
                })?;
            }
            StorageBacking::Descriptors { .. } => {}
            StorageBacking::Platform { part_reference, .. } => {
                self.part_checkpoint_handle.take();
                self.part_file.take();
                part_reference
                    .delete()
                    .await
                    .map_err(|error| SelectiveStorageError::Io {
                        operation: "delete empty platform part file",
                        source: io::Error::other(error),
                    })?;
            }
        }
        Ok(())
    }
}

#[cfg_attr(not(feature = "descriptor-storage-diagnostics"), allow(dead_code))]
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

#[cfg_attr(not(feature = "descriptor-storage-diagnostics"), allow(dead_code))]
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

pub async fn verify_prepared_platform_files(
    spec: &PlatformStorageSpec,
    metainfo: &Metainfo,
    prepared: &[PreparedFileHash],
) -> Result<(), SelectiveStorageError> {
    let content = TorrentContent::from_v1_metainfo(metainfo.clone());
    verify_prepared_platform_content_files(spec, &content, prepared).await
}

pub async fn verify_prepared_platform_content_files(
    spec: &PlatformStorageSpec,
    content: &TorrentContent,
    prepared: &[PreparedFileHash],
) -> Result<(), SelectiveStorageError> {
    if !spec.published {
        return Err(SelectiveStorageError::InvalidStorageOperation(
            "verify unpublished platform namespace",
        ));
    }
    let layout = ContentLayout::from_content(content);
    for expected in prepared {
        let metainfo_file = layout.files().get(expected.file_index).ok_or(
            SelectiveStorageError::InvalidDescriptorManifest {
                role: "published",
                file_index: expected.file_index,
                reason: "file index is out of range",
            },
        )?;
        let reference = platform_storage_reference(
            spec,
            StorageFileRole::Payload(expected.file_index),
            metainfo_file.path.clone(),
        );
        let file = reference
            .open(StorageFileAccess::ReadExisting)
            .await
            .map(StorageFileLease::from)
            .map_err(|error| SelectiveStorageError::Io {
                operation: "acquire published platform file",
                source: io::Error::other(error),
            })?;
        let actual_length = file
            .file()
            .metadata()
            .map_err(|source| SelectiveStorageError::Io {
                operation: "inspect published platform file",
                source,
            })?
            .len();
        if actual_length != expected.length {
            return Err(SelectiveStorageError::UnexpectedFileLength {
                file_index: expected.file_index,
                expected: expected.length,
                actual: actual_length,
            });
        }
        let expected_hash = expected.sha1;
        let file_index = expected.file_index;
        let length = expected.length;
        let actual_hash = tokio::task::spawn_blocking(move || {
            let mut hasher = Sha1::new();
            let mut buffer = [0_u8; VERIFICATION_CHUNK_LENGTH];
            let mut offset = 0_u64;
            while offset < length {
                let read_length = usize::try_from((length - offset).min(buffer.len() as u64))
                    .map_err(|_| io::Error::other("published file length overflow"))?;
                read_exact_at(file.file(), &mut buffer[..read_length], offset)?;
                hasher.update(&buffer[..read_length]);
                offset += read_length as u64;
            }
            Ok::<[u8; 20], io::Error>(hasher.finalize().into())
        })
        .await
        .map_err(|source| SelectiveStorageError::Io {
            operation: "join published platform verification",
            source: io::Error::other(source),
        })?
        .map_err(|source| SelectiveStorageError::Io {
            operation: "verify published platform file",
            source,
        })?;
        if actual_hash != expected_hash {
            return Err(SelectiveStorageError::PreparedHashMismatch { file_index });
        }
    }
    Ok(())
}

pub async fn validate_published_fast_resume_with_path(
    storage_root: &Path,
    artifact_identity: TorrentArtifactIdentity,
    metainfo: &Metainfo,
    verified: &[bool],
    skipped: &[usize],
    pool: StorageFilePool,
) -> Result<FastResumeValidation, SelectiveStorageError> {
    let layout = TorrentLayout::from_metainfo(metainfo);
    let selection = FileSelection::new(&layout, skipped)?;
    let paths =
        torrent_storage_paths_for_metainfo(storage_root, metainfo, artifact_identity.torrent_id)?;
    let (mut storage, resumed) = SelectiveStorage::resume_with_paths_and_pool_expected(
        paths,
        artifact_identity,
        layout,
        selection,
        verified.to_vec(),
        pool,
        Some(ResumeArtifactState::Published),
    )
    .await?;
    storage.validate_fast_resume(resumed).await
}

pub async fn validate_published_fast_resume_content_with_path(
    storage_root: &Path,
    artifact_identity: TorrentArtifactIdentity,
    content: Arc<TorrentContent>,
    verified: &[bool],
    skipped: &[usize],
    pool: StorageFilePool,
) -> Result<FastResumeValidation, SelectiveStorageError> {
    let layout = ContentLayout::from_content(&content);
    let selection = FileSelection::new_content(&layout, skipped)?;
    let paths = torrent_storage_paths_with_shape(
        storage_root,
        content.name(),
        artifact_identity.torrent_id,
        PublicationShape::from_content(&content),
    )?;
    let (mut storage, resumed) = SelectiveStorage::resume_with_paths_and_pool_expected(
        paths,
        artifact_identity,
        layout,
        selection,
        verified.to_vec(),
        pool,
        Some(ResumeArtifactState::Published),
    )
    .await?;
    storage.content = Some(content);
    storage.validate_fast_resume(resumed).await
}

pub async fn validate_published_fast_resume_with_platform(
    spec: PlatformStorageSpec,
    artifact_identity: TorrentArtifactIdentity,
    metainfo: &Metainfo,
    verified: &[bool],
    skipped: &[usize],
) -> Result<FastResumeValidation, SelectiveStorageError> {
    let layout = TorrentLayout::from_metainfo(metainfo);
    let selection = FileSelection::new(&layout, skipped)?;
    let (mut storage, resumed) = SelectiveStorage::create_with_platform(
        spec,
        artifact_identity,
        metainfo,
        layout,
        selection,
        verified.to_vec(),
    )
    .await?;
    storage.validate_fast_resume(resumed).await
}

pub async fn validate_published_fast_resume_content_with_platform(
    spec: PlatformStorageSpec,
    artifact_identity: TorrentArtifactIdentity,
    content: Arc<TorrentContent>,
    verified: &[bool],
    skipped: &[usize],
) -> Result<FastResumeValidation, SelectiveStorageError> {
    let (mut storage, resumed) = SelectiveStorage::create_content_with_platform(
        spec,
        artifact_identity,
        content,
        skipped,
        verified.to_vec(),
    )
    .await?;
    storage.validate_fast_resume(resumed).await
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

pub fn torrent_storage_paths(
    storage_root: &Path,
    publication_name: &str,
    torrent_id: TorrentId,
) -> Result<TorrentStoragePaths, SelectiveStorageError> {
    torrent_storage_paths_with_shape(
        storage_root,
        publication_name,
        torrent_id,
        PublicationShape::Tree,
    )
}

pub fn torrent_storage_paths_for_metainfo(
    storage_root: &Path,
    metainfo: &Metainfo,
    torrent_id: TorrentId,
) -> Result<TorrentStoragePaths, SelectiveStorageError> {
    torrent_storage_paths_with_shape(
        storage_root,
        &metainfo.name,
        torrent_id,
        PublicationShape::from_metainfo(metainfo),
    )
}

pub fn torrent_storage_paths_with_shape(
    storage_root: &Path,
    publication_name: &str,
    torrent_id: TorrentId,
    publication_shape: PublicationShape,
) -> Result<TorrentStoragePaths, SelectiveStorageError> {
    validate_publication_name(publication_name)?;
    let output = storage_root.join(publication_name);
    torrent_storage_paths_for_output_with_shape(output, torrent_id, publication_shape)
}

pub fn validate_publication_name(publication_name: &str) -> Result<(), SelectiveStorageError> {
    if publication_name.is_empty()
        || publication_name.len() > 255
        || matches!(publication_name, "." | "..")
        || publication_name
            .bytes()
            .any(|byte| matches!(byte, 0 | b'/' | b'\\' | b':'))
        || is_internal_artifact_name(publication_name)
    {
        return Err(SelectiveStorageError::InvalidPublicationName);
    }
    Ok(())
}

fn is_internal_artifact_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix('.') else {
        return false;
    };
    [".rstorrent-staging", ".rstorrent-parts"]
        .into_iter()
        .any(|suffix| {
            name.strip_suffix(suffix)
                .is_some_and(|owner| owner.parse::<TorrentId>().is_ok())
        })
}

pub(crate) fn torrent_storage_paths_for_output_with_shape(
    output: PathBuf,
    torrent_id: TorrentId,
    publication_shape: PublicationShape,
) -> Result<TorrentStoragePaths, SelectiveStorageError> {
    let parent = output
        .parent()
        .ok_or(SelectiveStorageError::InvalidOutputPath)?;
    let artifact_base = parent.join(torrent_id.to_string());
    let staging = selective_staging_path(&artifact_base)?;
    let part = selective_part_path(&artifact_base)?;
    if output == staging || output == part || staging == part {
        return Err(SelectiveStorageError::InvalidPublicationName);
    }
    Ok(TorrentStoragePaths {
        publication_shape,
        output,
        staging,
        part,
    })
}

fn payload_path(
    publication_shape: PublicationShape,
    artifact_root: &Path,
    components: &[String],
    file_index: usize,
    file_count: usize,
) -> Result<PathBuf, SelectiveStorageError> {
    match publication_shape {
        PublicationShape::File if file_index == 0 && file_count == 1 => {
            Ok(artifact_root.to_path_buf())
        }
        PublicationShape::File => Err(SelectiveStorageError::InvalidStorageOperation(
            "single-file publication requires exactly one logical file",
        )),
        PublicationShape::Tree => Ok(joined_path(artifact_root, components)),
    }
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

fn storage_instance_id(torrent_id: TorrentId) -> String {
    torrent_id.to_string()
}

fn path_storage_reference(
    pool: &StorageFilePool,
    storage_id: &str,
    namespace_generation: u64,
    role: StorageFileRole,
    path: PathBuf,
) -> StorageFileReference {
    StorageFileReference::new(
        pool.clone(),
        StorageFileKey {
            storage_id: storage_id.to_owned(),
            namespace_generation,
            role,
        },
        StorageFileLocator::Path(path),
    )
}

fn platform_storage_reference(
    spec: &PlatformStorageSpec,
    role: StorageFileRole,
    components: Vec<String>,
) -> StorageFileReference {
    let path = match role {
        StorageFileRole::Namespace => vec![spec.publication_name.clone()],
        StorageFileRole::Payload(_) => {
            let namespace = if spec.published {
                spec.publication_name.clone()
            } else {
                format!(".{}.rstorrent-staging", spec.storage_id)
            };
            match spec.publication_shape {
                PublicationShape::File => vec![namespace],
                PublicationShape::Tree => std::iter::once(namespace).chain(components).collect(),
            }
        }
        StorageFileRole::Part => vec![format!(".{}.rstorrent-parts", spec.storage_id)],
    };
    StorageFileReference::new(
        spec.pool.clone(),
        StorageFileKey {
            storage_id: spec.storage_id.clone(),
            namespace_generation: spec.namespace_generation,
            role,
        },
        StorageFileLocator::Platform(PlatformStorageTarget {
            root_id: spec.root_id.clone(),
            storage_id: spec.storage_id.clone(),
            namespace_generation: spec.namespace_generation,
            role,
            path,
        }),
    )
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
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use rstorrent_protocol::content::{
        ExpectedPieceIntegrity, TorrentContent, TorrentContentProjection,
    };
    use rstorrent_protocol::merkle::{file_root_from_data, piece_root_from_data};
    use rstorrent_protocol::metainfo::{
        EXPLICIT_IMPORT_METAINFO_LIMITS, Metainfo, MetainfoFile, MetainfoMode,
    };
    use rstorrent_protocol::peer_wire::BlockRequest;
    use rstorrent_protocol::storage_layout::{FileSelection, TorrentLayout};
    use sha1::{Digest, Sha1};

    use crate::checkpoint::DurabilityTarget;
    use crate::identity::{ContentFingerprint, TorrentId};
    use crate::resume_validation::{ResumeStorageEvidence, ResumeValidationRejectReason};
    use crate::storage_file_pool::{StorageFileAccess, StorageFilePool, platform_storage_channel};

    use super::{
        BlockingHashResult, ComputedPieceHash, DescriptorFile, DescriptorStorage,
        PlatformStorageSpec, PreparedFileHash, PublicationShape, ResumeArtifactState,
        ResumedStorage, SelectiveStorage, SelectiveStorageError, SelectiveWriteDestination,
        SelectiveWritePlan, SelectiveWriteSpan, SelectiveWriteStats, TorrentArtifactIdentity,
        VERIFICATION_CHUNK_LENGTH, await_blocking_hash, collect_descriptors, materialization_path,
        remove_selective_part_if_present, remove_selective_staging_if_present,
        selective_staging_path, storage_instance_id, torrent_storage_paths,
        torrent_storage_paths_for_metainfo, torrent_storage_paths_for_output_with_shape,
        verify_prepared_descriptors, verify_prepared_platform_files,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_torrent_id() -> TorrentId {
        TorrentId::new([0x51; 16]).expect("nonzero test owner")
    }

    fn test_artifact_identity() -> TorrentArtifactIdentity {
        TorrentArtifactIdentity {
            torrent_id: test_torrent_id(),
            content_fingerprint: ContentFingerprint::from_digest([0x52; 32]),
        }
    }

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rstorrent-selective-storage-test-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("create isolated test parent");
        root.join(name)
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

    fn bstr(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(value.len().to_string().as_bytes());
        output.push(b':');
        output.extend_from_slice(value);
    }

    fn pure_v2_content(files: &[(&[u8], &[u8])], piece_length: u32) -> TorrentContent {
        let roots = files
            .iter()
            .map(|(_, data)| file_root_from_data(data).expect("nonempty fixture file"))
            .collect::<Vec<_>>();
        let mut info = b"d9:file treed".to_vec();
        for ((name, data), root) in files.iter().zip(&roots) {
            bstr(&mut info, name);
            info.extend_from_slice(b"d0:d6:lengthi");
            info.extend_from_slice(data.len().to_string().as_bytes());
            info.extend_from_slice(b"e11:pieces root32:");
            info.extend_from_slice(root);
            info.extend_from_slice(b"ee");
        }
        info.extend_from_slice(b"e12:meta versioni2e4:name4:root12:piece lengthi");
        info.extend_from_slice(piece_length.to_string().as_bytes());
        info.extend_from_slice(b"ee");
        let mut source = b"d4:info".to_vec();
        source.extend_from_slice(&info);
        source.extend_from_slice(b"12:piece layersd");
        for ((_, data), root) in files.iter().zip(&roots) {
            if data.len() <= piece_length as usize {
                continue;
            }
            bstr(&mut source, root);
            let hashes = data
                .chunks(piece_length as usize)
                .map(|piece| piece_root_from_data(piece, piece_length).expect("piece root"))
                .collect::<Vec<_>>();
            bstr(&mut source, &hashes.concat());
        }
        source.extend_from_slice(b"ee");
        TorrentContentProjection::from_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
            .expect("complete pure-v2 fixture")
            .content
    }

    #[tokio::test]
    async fn pure_v2_writes_and_hashes_file_local_pieces_without_part_or_gap() {
        let skipped = vec![9_u8];
        let selected = (0..40_000)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let selected_small = vec![0x5a; 17];
        let content = Arc::new(pure_v2_content(
            &[(b"a", &skipped), (b"b", &selected), (b"c", &selected_small)],
            32 * 1024,
        ));
        let output = test_path("root");
        let parent = output.parent().expect("test parent").to_path_buf();
        let mut storage = SelectiveStorage::create_content(
            output.clone(),
            test_artifact_identity(),
            content.clone(),
            &[0],
        )
        .await
        .expect("create v2 storage");

        assert_eq!(
            storage.selected_bytes(),
            (selected.len() + selected_small.len()) as u64
        );
        assert_eq!(storage.skipped_bytes(), skipped.len() as u64);
        assert_eq!(storage.padding_bytes(), 0);
        assert_eq!(storage.part_slots(), 0);
        assert!(!storage.has_part_file());
        assert!(matches!(
            storage.write_block(0, 0, skipped.clone()).await,
            Err(SelectiveStorageError::InvalidStorageOperation(
                "write skipped v2 piece"
            ))
        ));

        for (piece, payload) in selected.chunks(32 * 1024).enumerate() {
            for (block, bytes) in payload.chunks(16 * 1024).enumerate() {
                storage
                    .write_block(
                        u32::try_from(piece + 1).expect("piece"),
                        u32::try_from(block * 16 * 1024).expect("begin"),
                        bytes.to_vec(),
                    )
                    .await
                    .expect("write v2 block");
            }
            let piece_index = u32::try_from(piece + 1).expect("piece index");
            let actual = storage
                .hash_piece_content(piece_index)
                .await
                .expect("hash v2 piece");
            let expected = content.expected_piece(piece_index).expect("expected root");
            let (
                ComputedPieceHash::Sha256 {
                    root,
                    retained_hash_high_water,
                },
                ExpectedPieceIntegrity::V2Merkle { expected_root, .. },
            ) = (actual, expected)
            else {
                panic!("v2 integrity variants")
            };
            assert_eq!(root, expected_root);
            assert!(retained_hash_high_water <= 2);
            storage.record_verified(piece + 1).expect("record v2 piece");
        }
        storage
            .write_block(3, 0, selected_small)
            .await
            .expect("write one-piece v2 file");
        let actual = storage
            .hash_piece_content(3)
            .await
            .expect("hash one-piece v2 file");
        let expected = content.expected_piece(3).expect("one-piece expected root");
        assert!(matches!(
            (actual, expected),
            (
                ComputedPieceHash::Sha256 {
                    root,
                    retained_hash_high_water: 1
                },
                ExpectedPieceIntegrity::V2Merkle {
                    expected_root,
                    target_height: 0,
                    ..
                }
            ) if root == expected_root
        ));
        storage.record_verified(3).expect("record one-piece file");
        assert_eq!(storage.part_slots(), 0);
        assert!(!storage.has_part_file());
        let part = storage
            .part_path()
            .expect("path-backed part name")
            .to_path_buf();
        assert!(!part.exists());
        storage.publish().await.expect("publish selected v2 files");
        assert_eq!(
            std::fs::read(output.join("b")).expect("published b"),
            selected
        );
        assert_eq!(
            std::fs::read(output.join("c")).expect("published c"),
            vec![0x5a; 17]
        );
        assert!(!output.join("a").exists());
        drop(storage);
        let (resumed, state) = SelectiveStorage::resume_content(
            output,
            test_artifact_identity(),
            content,
            &[0],
            vec![false, true, true, true],
            Some(ResumeArtifactState::Published),
        )
        .await
        .expect("resume published v2 storage");
        assert_eq!(state, ResumedStorage::Published);
        assert_eq!(resumed.part_slots(), 0);
        assert!(!resumed.has_part_file());
        assert!(matches!(
            resumed.hash_piece_content(2).await,
            Ok(ComputedPieceHash::Sha256 { .. })
        ));
        drop(resumed);
        std::fs::remove_dir_all(parent).expect("remove v2 storage fixture");
    }

    fn single_file_fixture() -> Metainfo {
        Metainfo {
            info_hash: [9; 20],
            piece_hashes: vec![[3; 20]; 2],
            piece_length: 16_384,
            total_length: 20_000,
            name: "single.bin".to_owned(),
            private: false,
            mode: MetainfoMode::SingleFile,
            files: vec![MetainfoFile {
                path: vec!["single.bin".to_owned()],
                length: 20_000,
                offset: 0,
                padding: false,
            }],
        }
    }

    #[test]
    fn torrent_paths_use_visible_name_and_opaque_owner_artifacts() {
        let root = test_path("torrent-paths");
        let paths = torrent_storage_paths(&root, "Visible Name", test_torrent_id())
            .expect("plan torrent storage paths");

        assert_eq!(paths.output, root.join("Visible Name"));
        assert_eq!(paths.publication_shape, PublicationShape::Tree);
        assert_eq!(
            paths.staging,
            root.join(format!(".{}.rstorrent-staging", test_torrent_id()))
        );
        assert_eq!(
            paths.part,
            root.join(format!(".{}.rstorrent-parts", test_torrent_id()))
        );
        assert_eq!(
            torrent_storage_paths(&root, "Каталог", test_torrent_id())
                .expect("plan Unicode publication")
                .output,
            root.join("Каталог")
        );
        let maximum_name = "x".repeat(255);
        assert_eq!(
            torrent_storage_paths(&root, &maximum_name, test_torrent_id())
                .expect("plan maximum publication name")
                .output,
            root.join(&maximum_name)
        );
        assert!(matches!(
            torrent_storage_paths(&root, &"x".repeat(256), test_torrent_id()),
            Err(SelectiveStorageError::InvalidPublicationName)
        ));
        for invalid in [
            "",
            ".",
            "..",
            "nested/name",
            "nested\\name",
            "C:name",
            ".t1-0123456789abcdef0123456789abcdef.rstorrent-staging",
            ".t1-fedcba9876543210fedcba9876543210.rstorrent-parts",
        ] {
            assert!(matches!(
                torrent_storage_paths(&root, invalid, test_torrent_id()),
                Err(SelectiveStorageError::InvalidPublicationName)
            ));
        }

        let single = single_file_fixture();
        let single_paths = torrent_storage_paths_for_metainfo(&root, &single, test_torrent_id())
            .expect("plan single-file storage paths");
        assert_eq!(single_paths.publication_shape, PublicationShape::File);
        assert_eq!(single_paths.output, root.join("single.bin"));

        let multi = fixture();
        assert_eq!(
            torrent_storage_paths_for_metainfo(&root, &multi, test_torrent_id())
                .expect("plan multi-file storage paths")
                .publication_shape,
            PublicationShape::Tree
        );
    }

    #[tokio::test]
    async fn publication_no_replace_preserves_a_racing_destination() {
        let root = test_path("publication-no-replace");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan single-file paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage = SelectiveStorage::create_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout,
            selection,
        )
        .await
        .expect("create storage");
        storage
            .write_block(0, 0, vec![0x31; 16_384])
            .await
            .expect("write first piece");
        storage
            .write_block(1, 0, vec![0x72; 3_616])
            .await
            .expect("write final piece");
        storage.record_verified(0).expect("verify first piece");
        storage.record_verified(1).expect("verify final piece");
        storage
            .prepare_path_publication()
            .await
            .expect("prepare durable publication");
        tokio::fs::write(&paths.output, b"foreign destination")
            .await
            .expect("race final destination");

        assert!(matches!(
            storage.commit_path_publication().await,
            Err(SelectiveStorageError::ExistingOutput(path)) if path == paths.output
        ));
        assert_eq!(
            tokio::fs::read(&paths.output)
                .await
                .expect("foreign destination retained"),
            b"foreign destination"
        );
        assert!(paths.staging.exists());
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn durable_artifact_state_rejects_impossible_namespace_sides() {
        let root = test_path("artifact-reconciliation");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan single-file paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        tokio::fs::write(&paths.output, vec![0; metainfo.total_length as usize])
            .await
            .expect("create final artifact");
        assert!(matches!(
            SelectiveStorage::resume_with_paths_expected(
                paths.clone(),
                test_artifact_identity(),
                layout.clone(),
                selection.clone(),
                vec![false; layout.piece_count()],
                ResumeArtifactState::Staging,
            )
            .await,
            Err(SelectiveStorageError::IncompleteResumeArtifacts)
        ));
        tokio::fs::remove_file(&paths.output)
            .await
            .expect("remove final artifact");
        tokio::fs::write(&paths.staging, vec![0; metainfo.total_length as usize])
            .await
            .expect("create staging artifact");
        assert!(matches!(
            SelectiveStorage::resume_with_paths_expected(
                paths,
                test_artifact_identity(),
                layout.clone(),
                selection,
                vec![false; layout.piece_count()],
                ResumeArtifactState::Published,
            )
            .await,
            Err(SelectiveStorageError::IncompleteResumeArtifacts)
        ));
        tokio::fs::remove_dir_all(root).await.expect("remove root");
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
        if let Some(parent) = output.parent() {
            let artifact_base = parent.join(test_torrent_id().to_string());
            let _ = remove_selective_staging_if_present(&artifact_base).await;
            let _ = remove_selective_part_if_present(&artifact_base).await;
        }
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
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection.clone(),
        )
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
        assert_eq!(stats.wanted_file_duplicates, 0);
        assert_eq!(stats.blocking_jobs, 1);
        clean(&output).await;
    }

    #[tokio::test]
    async fn validates_every_write_route_before_mutating_any_destination() {
        let output = test_path("stale-write-route");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
        .await
        .expect("create storage");
        let route = storage.files[0]
            .as_ref()
            .expect("first wanted file")
            .routing_generation;
        let plan = SelectiveWritePlan {
            payload: Arc::new(Vec::from(&b"abcdefgh"[..])),
            spans: vec![
                SelectiveWriteSpan {
                    destination: SelectiveWriteDestination::WantedFile {
                        file_index: 0,
                        file_offset: 0,
                        routing_generation: route,
                    },
                    block_offset: 0,
                    length: 4,
                },
                SelectiveWriteSpan {
                    destination: SelectiveWriteDestination::WantedFile {
                        file_index: 0,
                        file_offset: 4,
                        routing_generation: route.wrapping_add(1),
                    },
                    block_offset: 4,
                    length: 4,
                },
            ],
            stats: SelectiveWriteStats {
                wanted_bytes: 8,
                skipped_bytes: 0,
            },
        };

        assert!(matches!(
            storage.execute_write_plan(plan).await,
            Err(SelectiveStorageError::StaleWriteRoute { file_index: 0 })
        ));
        assert!(
            !selective_staging_path(&output)
                .expect("staging path")
                .exists(),
            "a rejected immutable plan must not create its destination"
        );
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
        let pool = StorageFilePool::new(1, None).expect("single-handle file pool");
        let mut storage = SelectiveStorage::create_with_pool(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection.clone(),
            pool,
        )
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
        let (actual, stats) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            storage.hash_piece_with_stats(0),
        )
        .await
        .expect("cross-file hash must not retain one lease while acquiring the next")
        .expect("hash cross-file piece");
        assert_eq!(actual, expected);
        assert_eq!(stats.wanted_file_seeks, 0);
        assert_eq!(stats.wanted_file_duplicates, 0);
        assert_eq!(stats.blocking_jobs, 1);
        assert_eq!(
            stats.wanted_file_reads,
            (first_length as usize).div_ceil(VERIFICATION_CHUNK_LENGTH)
                + (second_length as usize).div_ceil(VERIFICATION_CHUNK_LENGTH)
        );
        assert_eq!(stats.part_file_reads, 0);
        storage.files[2]
            .as_ref()
            .expect("second wanted file")
            .acquire(StorageFileAccess::ReadWriteExisting)
            .await
            .expect("acquire second wanted file")
            .file()
            .set_len(second_length - 1)
            .expect("truncate second cross-file span");
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::UnexpectedFileLength {
                file_index: 2,
                expected,
                actual,
            }) if expected == second_length && actual == second_length - 1
        ));
        clean(&output).await;
    }

    #[tokio::test]
    async fn active_upload_read_composes_cross_file_padding_and_staging_bytes() {
        let output = test_path("active-upload-cross-file-padding");
        clean(&output).await;
        let metainfo = Metainfo {
            info_hash: [0x44; 20],
            piece_hashes: vec![[0; 20]],
            piece_length: 12,
            total_length: 12,
            name: "active-upload".to_owned(),
            private: false,
            mode: MetainfoMode::MultiFile,
            files: vec![
                MetainfoFile {
                    path: vec!["first".to_owned()],
                    length: 4,
                    offset: 0,
                    padding: false,
                },
                MetainfoFile {
                    path: vec![".pad".to_owned(), "3".to_owned()],
                    length: 3,
                    offset: 4,
                    padding: true,
                },
                MetainfoFile {
                    path: vec!["second".to_owned()],
                    length: 5,
                    offset: 7,
                    padding: false,
                },
            ],
        };
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
        .await
        .expect("storage");
        storage
            .write_block(0, 0, b"abcd".to_vec())
            .await
            .expect("first file");
        storage
            .write_block(0, 7, b"efghi".to_vec())
            .await
            .expect("second file");
        storage.record_verified(0).expect("verified");
        let plan = storage
            .prepare_upload_read(
                BlockRequest {
                    index: 0,
                    begin: 0,
                    length: 12,
                },
                storage.route_epoch(),
            )
            .expect("read plan");
        assert_eq!(plan.segment_count(), 3);
        assert_eq!(
            plan.execute().await.expect("read"),
            [
                b'a', b'b', b'c', b'd', 0, 0, 0, b'e', b'f', b'g', b'h', b'i'
            ]
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn active_upload_read_serves_verified_part_backed_piece() {
        let output = test_path("active-upload-part");
        clean(&output).await;
        let metainfo = fixture();
        let bytes = torrent_bytes(&metainfo);
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection.clone(),
        )
        .await
        .expect("storage");
        for request in layout.request_ranges(0, &selection).expect("requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("piece write");
        }
        storage.record_verified(0).expect("verified");
        let request = BlockRequest {
            index: 0,
            begin: 16_384,
            length: 16_384,
        };
        let plan = storage
            .prepare_upload_read(request, storage.route_epoch())
            .expect("part read plan");
        assert_eq!(plan.segment_count(), 2);
        assert_eq!(
            plan.execute().await.expect("part-backed read"),
            bytes[16_384..32_768]
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn active_file_read_requires_every_intersecting_verified_piece() {
        let output = test_path("active-file-read");
        clean(&output).await;
        let metainfo = single_file_fixture();
        let bytes = (0..metainfo.total_length as usize)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
        .await
        .expect("storage");
        storage
            .write_block(0, 0, bytes[..16_384].to_vec())
            .await
            .expect("first piece");
        storage
            .write_block(1, 0, bytes[16_384..].to_vec())
            .await
            .expect("second piece");
        storage.record_verified(0).expect("first verified");
        let epoch = storage.route_epoch();
        assert!(matches!(
            storage.prepare_file_read(0, 10_000, 10_000, epoch),
            Err(SelectiveStorageError::InvalidVerifiedPiece { piece_index: 1 })
        ));
        storage.record_verified(1).expect("second verified");
        let plan = storage
            .prepare_file_read(0, 10_000, 10_000, epoch)
            .expect("verified file read plan");
        assert_eq!(plan.file_index(), 0);
        assert_eq!(plan.offset(), 10_000);
        assert_eq!(plan.length(), 10_000);
        assert_eq!(
            plan.execute().await.expect("active file read"),
            bytes[10_000..]
        );
        assert!(matches!(
            storage.prepare_file_read(0, 0, 1, epoch + 1),
            Err(SelectiveStorageError::InvalidStorageOperation(
                "stale active file route epoch"
            ))
        ));
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
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection.clone(),
        )
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
            .acquire(StorageFileAccess::ReadWriteExisting)
            .await
            .expect("acquire wanted file")
            .file()
            .set_len(16_384)
            .expect("truncate staging file");
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::UnexpectedFileLength {
                file_index: 0,
                expected: 32_768,
                actual: 16_384,
            })
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
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
        .await
        .expect("create selected storage");
        let checkpoint_handles = storage
            .checkpoint_handles()
            .await
            .expect("register checkpoint handles");
        assert!(
            checkpoint_handles[&DurabilityTarget::PartFile]
                .get()
                .is_none()
        );

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
        assert!(
            checkpoint_handles[&DurabilityTarget::PartFile]
                .get()
                .is_some()
        );
        assert_eq!(
            storage.hash_piece(0).await.expect("hash coalesced piece"),
            <[u8; 20]>::from(Sha1::digest(&bytes[..metainfo.piece_length as usize]))
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn dynamic_platform_storage_is_lazy_and_verifies_after_publication() {
        let root = test_path("dynamic-platform");
        clean(&root).await;
        tokio::fs::create_dir_all(&root)
            .await
            .expect("create fake provider root");
        let (platform, broker) = platform_storage_channel();
        let pool = StorageFilePool::new(4, Some(platform)).expect("platform pool");
        let provider_root = root.clone();
        let provider = tokio::spawn(async move {
            while let Some(request) = broker.next_request().await {
                let path = request
                    .path
                    .iter()
                    .fold(provider_root.clone(), |path, component| {
                        path.join(component)
                    });
                if matches!(
                    request.operation,
                    crate::storage_file_pool::PlatformStorageOperation::Observe
                ) {
                    let observation = match std::fs::symlink_metadata(&path) {
                        Ok(metadata) => crate::StorageObservation::present(
                            if metadata.is_file() {
                                crate::StorageObjectKind::File
                            } else if metadata.is_dir() {
                                crate::StorageObjectKind::Directory
                            } else {
                                crate::StorageObjectKind::Other
                            },
                            metadata.is_file().then_some(metadata.len()),
                            None,
                        )
                        .expect("provider observation"),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            crate::StorageObservation::missing()
                        }
                        Err(error) => {
                            broker.complete_error(
                                request.request_id,
                                crate::PlatformStorageFailure::new(
                                    crate::PlatformStorageFailureKind::ProviderRefused,
                                    error.to_string(),
                                ),
                            );
                            continue;
                        }
                    };
                    broker.complete_observation(request.request_id, observation);
                    continue;
                }
                if matches!(
                    request.operation,
                    crate::storage_file_pool::PlatformStorageOperation::Delete
                ) {
                    let result = match std::fs::remove_file(&path) {
                        Ok(()) => Ok(()),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                        Err(error) => Err(error),
                    };
                    match result {
                        Ok(()) => {
                            broker.complete_deleted(request.request_id);
                        }
                        Err(error) => {
                            broker.complete_error(
                                request.request_id,
                                crate::storage_file_pool::PlatformStorageFailure::new(
                                    crate::storage_file_pool::PlatformStorageFailureKind::Internal,
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                    continue;
                }
                assert_eq!(
                    request.operation,
                    crate::storage_file_pool::PlatformStorageOperation::Open
                );
                if matches!(request.access, StorageFileAccess::ReadWriteCreate) {
                    std::fs::create_dir_all(path.parent().expect("provider path parent"))
                        .expect("create provider parents");
                }
                let mut options = std::fs::OpenOptions::new();
                options
                    .read(true)
                    .write(!matches!(request.access, StorageFileAccess::ReadExisting));
                options.create(matches!(request.access, StorageFileAccess::ReadWriteCreate));
                match options.open(path) {
                    Ok(file) => {
                        broker.complete_file(request.request_id, file);
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        broker.complete_error(
                            request.request_id,
                            crate::storage_file_pool::PlatformStorageFailure::new(
                                crate::storage_file_pool::PlatformStorageFailureKind::Missing,
                                "missing",
                            ),
                        );
                    }
                    Err(error) => {
                        broker.complete_error(
                            request.request_id,
                            crate::storage_file_pool::PlatformStorageFailure::new(
                                crate::storage_file_pool::PlatformStorageFailureKind::Internal,
                                error.to_string(),
                            ),
                        );
                    }
                }
            }
        });

        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let spec = PlatformStorageSpec {
            pool: pool.clone(),
            root_id: "downloads".to_owned(),
            storage_id: storage_instance_id(test_torrent_id()),
            publication_shape: PublicationShape::from_metainfo(&metainfo),
            publication_name: metainfo.name.clone(),
            namespace_generation: 0,
            managed: false,
            published: false,
        };
        let (mut storage, resumed) = SelectiveStorage::create_with_platform(
            spec.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection,
            vec![false; layout.piece_count()],
        )
        .await
        .expect("create platform storage");
        assert_eq!(resumed, ResumedStorage::Created);
        assert!(
            std::fs::read_dir(&root)
                .expect("empty provider root")
                .next()
                .is_none()
        );
        assert_eq!(pool.snapshot().current_owned, 0);

        for piece_index in [0_u32, 2, 3, 4] {
            for request in layout
                .request_ranges(piece_index, &storage.selection)
                .expect("piece requests")
            {
                let offset =
                    piece_index as usize * layout.piece_length() as usize + request.begin as usize;
                storage
                    .write_block(
                        piece_index,
                        request.begin,
                        bytes[offset..offset + request.length as usize].to_vec(),
                    )
                    .await
                    .expect("write platform block");
            }
            let offset = piece_index as usize * layout.piece_length() as usize;
            let length = layout.piece_length_at(piece_index).expect("piece length") as usize;
            assert_eq!(
                storage
                    .hash_piece(piece_index)
                    .await
                    .expect("hash platform piece"),
                <[u8; 20]>::from(Sha1::digest(&bytes[offset..offset + length]))
            );
            storage
                .record_verified(piece_index as usize)
                .expect("record platform piece");
        }
        storage.prepare_platform().await.expect("prepare platform");
        let prepared = storage
            .finalize_descriptor_hashes()
            .await
            .expect("hash prepared platform files");
        pool.invalidate_storage(&spec.storage_id);
        std::fs::rename(
            root.join(format!(".{}.rstorrent-staging", spec.storage_id)),
            root.join(&metainfo.name),
        )
        .expect("publish fake provider tree");
        let published = PlatformStorageSpec {
            namespace_generation: 1,
            managed: true,
            published: true,
            ..spec
        };
        verify_prepared_platform_files(&published, &metainfo, &prepared)
            .await
            .expect("verify dynamic publication");
        let mut committed = vec![false; layout.piece_count()];
        for piece_index in [0_usize, 2, 3, 4] {
            committed[piece_index] = true;
        }
        let validation = super::validate_published_fast_resume_with_platform(
            published.clone(),
            test_artifact_identity(),
            &metainfo,
            &committed,
            &[1, 2],
        )
        .await
        .expect("validate platform fast resume");
        assert_eq!(validation.evidence, ResumeStorageEvidence::Matches);
        assert_eq!(validation.payload_bytes_read, 0);
        assert_eq!(validation.hash_jobs, 0);
        let selected_path = root.join(&metainfo.name).join("wanted/start.bin");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&selected_path)
            .expect("open platform selected file")
            .set_len(20_001)
            .expect("oversize platform selected file");
        let oversized = super::validate_published_fast_resume_with_platform(
            published.clone(),
            test_artifact_identity(),
            &metainfo,
            &committed,
            &[1, 2],
        )
        .await
        .expect("validate oversized platform resume");
        assert_eq!(
            oversized.evidence,
            ResumeStorageEvidence::ContentMismatch(
                ResumeValidationRejectReason::UnexpectedPayloadLength,
            )
        );
        std::fs::OpenOptions::new()
            .write(true)
            .open(selected_path)
            .expect("reopen platform selected file")
            .set_len(20_000)
            .expect("restore platform selected length");
        let resumed_selection = storage.selection.clone();
        drop(storage);
        let (mut resumed, resumed_state) = SelectiveStorage::create_with_platform(
            published.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            resumed_selection,
            vec![false; layout.piece_count()],
        )
        .await
        .expect("inventory managed platform publication with empty have");
        assert_eq!(resumed_state, ResumedStorage::Published);
        for piece_index in [0_u32, 2, 3, 4] {
            assert!(
                resumed
                    .has_piece_sources(piece_index)
                    .await
                    .expect("inventory managed platform sources")
            );
            let offset = piece_index as usize * layout.piece_length() as usize;
            let length = layout.piece_length_at(piece_index).expect("piece length") as usize;
            assert_eq!(
                resumed
                    .hash_piece(piece_index)
                    .await
                    .expect("recheck managed platform piece"),
                <[u8; 20]>::from(Sha1::digest(&bytes[offset..offset + length]))
            );
        }
        assert!(pool.snapshot().owned_high_water <= 4);

        drop(resumed);
        drop(published);
        pool.shutdown().await.expect("shutdown pool");
        drop(pool);
        provider.await.expect("join fake provider");
        clean(&root).await;
    }

    #[tokio::test]
    async fn stages_hashes_publishes_reopens_and_materializes() {
        let output = test_path("fixture");
        clean(&output).await;
        let metainfo = fixture();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let paths = torrent_storage_paths_for_output_with_shape(
            output.clone(),
            test_torrent_id(),
            PublicationShape::from_metainfo(&metainfo),
        )
        .expect("storage paths");
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout.clone(),
            selection,
        )
        .await
        .expect("create selected storage");

        assert_eq!(storage.selected_bytes(), 73_000);
        assert_eq!(storage.skipped_bytes(), 57_000);
        assert_eq!(storage.padding_bytes(), 3_304);
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        let staging = paths.staging;
        assert!(!tokio::fs::try_exists(&staging).await.expect("lazy staging"));
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
        let part_path = paths.part;
        assert!(
            !tokio::fs::try_exists(&part_path)
                .await
                .expect("part remains lazy")
        );
        assert!(matches!(
            storage.publish().await,
            Err(SelectiveStorageError::IncompleteSelection { piece_index: 0 })
        ));
        assert!(!tokio::fs::try_exists(&output).await.expect("output state"));
        assert!(matches!(
            storage.hash_piece(0).await,
            Err(SelectiveStorageError::NotPublished)
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
                assert_eq!(stats.blocking_jobs, 1);
                assert_eq!(stats.wanted_file_duplicates, 0);
                assert_eq!(stats.wanted_file_seeks, 0);
                assert!(stats.part_file_reads > 0);
            }
            storage
                .record_verified(piece_index as usize)
                .expect("record verified");
        }
        assert!(
            tokio::fs::try_exists(&part_path)
                .await
                .expect("first skipped write created part")
        );
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
            tokio::fs::try_exists(&part_path)
                .await
                .expect("part exists")
        );
        clean(&output).await;
    }

    #[tokio::test]
    async fn resumes_staging_and_published_trees_without_trusting_geometry() {
        let root = test_path("resume");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan resumable storage paths");
        let output = paths.output.clone();
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            vec![false; layout.piece_count()],
        )
        .await
        .expect("create resumable storage");
        assert_eq!(resumed, ResumedStorage::Created);
        assert!(!paths.staging.exists());
        assert!(!paths.part.exists());
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
        assert!(paths.part.exists());
        let expected_hash = storage.hash_piece(0).await.expect("hash first piece");
        storage.sync_piece(0).await.expect("sync first piece");
        storage.record_verified(0).expect("record first piece");
        drop(storage);

        let mut verified = vec![false; layout.piece_count()];
        verified[0] = true;
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
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

        let (storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
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
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths,
            test_artifact_identity(),
            layout,
            selection,
            vec![false; 5],
        )
        .await
        .expect("inventory short published file");
        assert_eq!(resumed, ResumedStorage::Published);
        assert!(
            !storage
                .has_piece_sources(0)
                .await
                .expect("classify short source")
        );
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn published_recheck_accepts_read_only_exact_data_and_normalizes_oversize() {
        let root = test_path("published-geometry");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan published paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let bytes = vec![0x5a; metainfo.total_length as usize];
        tokio::fs::write(&paths.output, &bytes)
            .await
            .expect("write exact published file");
        let mut permissions = tokio::fs::metadata(&paths.output)
            .await
            .expect("read publication metadata")
            .permissions();
        permissions.set_readonly(true);
        tokio::fs::set_permissions(&paths.output, permissions)
            .await
            .expect("make publication read-only");

        let (mut exact, resumed) = SelectiveStorage::resume_with_paths_expected(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            vec![false; layout.piece_count()],
            ResumeArtifactState::Published,
        )
        .await
        .expect("inventory read-only publication");
        assert_eq!(resumed, ResumedStorage::Published);
        for piece_index in 0..layout.piece_count() {
            exact
                .hash_piece(u32::try_from(piece_index).expect("bounded piece"))
                .await
                .expect("hash read-only publication");
            exact
                .record_verified(piece_index)
                .expect("record checked piece");
        }
        exact
            .finish_published()
            .await
            .expect("finish exact read-only check without mutation");
        drop(exact);

        tokio::fs::remove_file(&paths.output)
            .await
            .expect("remove exact read-only fixture");
        let mut oversized = bytes.clone();
        oversized.extend_from_slice(b"ignored suffix");
        tokio::fs::write(&paths.output, oversized)
            .await
            .expect("extend published file");
        let (mut oversized, resumed) = SelectiveStorage::resume_with_paths_expected(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection,
            vec![false; layout.piece_count()],
            ResumeArtifactState::Published,
        )
        .await
        .expect("inventory oversized publication");
        assert_eq!(resumed, ResumedStorage::Published);
        for piece_index in 0..layout.piece_count() {
            oversized
                .hash_piece(u32::try_from(piece_index).expect("bounded piece"))
                .await
                .expect("hash only declared bytes");
            oversized
                .record_verified(piece_index)
                .expect("record checked piece");
        }
        oversized
            .finish_published()
            .await
            .expect("normalize oversized managed file after check");
        assert_eq!(
            tokio::fs::metadata(&paths.output)
                .await
                .expect("normalized metadata")
                .len(),
            metainfo.total_length
        );
        assert_eq!(
            tokio::fs::read(&paths.output)
                .await
                .expect("normalized bytes"),
            bytes
        );
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn fast_resume_accepts_exact_and_same_length_mutated_path_content() {
        let root = test_path("fast-resume-exact");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan published paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        tokio::fs::write(&paths.output, vec![0x41; metainfo.total_length as usize])
            .await
            .expect("write exact publication");

        for byte in [0x41, 0x92] {
            tokio::fs::write(&paths.output, vec![byte; metainfo.total_length as usize])
                .await
                .expect("write same-length publication");
            let (mut storage, resumed) = SelectiveStorage::resume_with_paths_expected(
                paths.clone(),
                test_artifact_identity(),
                layout.clone(),
                selection.clone(),
                vec![true; layout.piece_count()],
                ResumeArtifactState::Published,
            )
            .await
            .expect("inventory exact publication");
            let validation = storage
                .validate_fast_resume(resumed)
                .await
                .expect("validate exact publication");
            assert_eq!(validation.evidence, ResumeStorageEvidence::Matches);
            assert_eq!(validation.committed_pieces, layout.piece_count());
            assert_eq!(validation.relevant_files, 1);
            assert_eq!(validation.artifact_observations, 1);
            assert_eq!(validation.payload_bytes_read, 0);
            assert_eq!(validation.hash_jobs, 0);
        }
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn fast_resume_sends_oversized_path_content_to_full_check() {
        let root = test_path("fast-resume-oversized");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan published paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        tokio::fs::write(
            &paths.output,
            vec![0x51; metainfo.total_length as usize + 1],
        )
        .await
        .expect("write oversized publication");
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths_expected(
            paths,
            test_artifact_identity(),
            layout.clone(),
            selection,
            vec![true; layout.piece_count()],
            ResumeArtifactState::Published,
        )
        .await
        .expect("inventory oversized publication");
        assert_eq!(
            storage
                .validate_fast_resume(resumed)
                .await
                .expect("validate oversized publication")
                .evidence,
            ResumeStorageEvidence::ContentMismatch(
                ResumeValidationRejectReason::UnexpectedPayloadLength,
            ),
        );
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn fast_resume_validates_part_slot_extent_without_reading_payload() {
        let root = test_path("fast-resume-part-extent");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let (mut storage, _) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            vec![false; layout.piece_count()],
        )
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
                .expect("write boundary piece");
        }
        storage.sync_piece(0).await.expect("sync boundary piece");
        storage.record_verified(0).expect("record boundary piece");
        drop(storage);

        let mut committed = vec![false; layout.piece_count()];
        committed[0] = true;
        let (mut exact, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            committed.clone(),
        )
        .await
        .expect("resume exact part slot");
        let validation = exact
            .validate_fast_resume(resumed)
            .await
            .expect("validate exact part slot");
        assert_eq!(validation.evidence, ResumeStorageEvidence::Matches);
        assert!(validation.part_header_bytes > 0);
        assert_eq!(validation.payload_bytes_read, 0);
        assert_eq!(validation.hash_jobs, 0);
        drop(exact);

        let part_length = tokio::fs::metadata(&paths.part)
            .await
            .expect("part metadata")
            .len();
        let part = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&paths.part)
            .await
            .expect("open part for truncation");
        part.set_len(part_length - 1)
            .await
            .expect("truncate part payload");
        drop(part);
        let (mut truncated, resumed) = SelectiveStorage::resume_with_paths(
            paths,
            test_artifact_identity(),
            layout,
            selection,
            committed,
        )
        .await
        .expect("inventory truncated part slot");
        assert_eq!(
            truncated
                .validate_fast_resume(resumed)
                .await
                .expect("validate truncated part slot")
                .evidence,
            ResumeStorageEvidence::ContentMismatch(ResumeValidationRejectReason::TruncatedPartSlot,),
        );
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn resumed_recheck_drops_cached_handles_before_observing_replacement() {
        let root = test_path("published-replaced-handle");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = single_file_fixture();
        let paths = torrent_storage_paths_for_metainfo(&root, &metainfo, test_torrent_id())
            .expect("plan published paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[]).expect("selection");
        let pool = StorageFilePool::new(4, None).expect("file pool");
        let original = vec![0x31; metainfo.total_length as usize];
        let replacement = vec![0x72; metainfo.total_length as usize];
        tokio::fs::write(&paths.output, &original)
            .await
            .expect("write original publication");

        let (mut first, _) = SelectiveStorage::resume_with_paths_and_pool_expected(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            vec![false; layout.piece_count()],
            pool.clone(),
            Some(ResumeArtifactState::Published),
        )
        .await
        .expect("open first generation");
        let first_hash = first.hash_piece(0).await.expect("hash original generation");
        drop(first);

        tokio::fs::remove_file(&paths.output)
            .await
            .expect("unlink original publication");
        tokio::fs::write(&paths.output, &replacement)
            .await
            .expect("write replacement publication");
        let (mut second, _) = SelectiveStorage::resume_with_paths_and_pool_expected(
            paths,
            test_artifact_identity(),
            layout.clone(),
            selection,
            vec![false; layout.piece_count()],
            pool.clone(),
            Some(ResumeArtifactState::Published),
        )
        .await
        .expect("open replacement generation");
        let second_hash = second
            .hash_piece(0)
            .await
            .expect("hash replacement generation");

        assert_ne!(first_hash, second_hash);
        assert_eq!(
            second_hash,
            <[u8; 20]>::from(Sha1::digest(&replacement[..layout.piece_length() as usize]))
        );
        drop(second);
        pool.shutdown().await.expect("shutdown pool");
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn resume_defers_part_promotion_until_recheck_reconciliation() {
        let root = test_path("resume-promote");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let skipped = FileSelection::new(&layout, &[1, 2]).expect("initial selection");
        let bytes = torrent_bytes(&metainfo);
        let (mut storage, _) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            skipped.clone(),
            vec![false; layout.piece_count()],
        )
        .await
        .expect("create initial storage");
        for request in layout.request_ranges(0, &skipped).expect("piece requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write boundary piece");
        }
        storage.record_verified(0).expect("record verified piece");
        storage.sync_piece(0).await.expect("sync verified piece");
        assert!(paths.part.exists());
        drop(storage);

        let promoted = FileSelection::new(&layout, &[2]).expect("promoted selection");
        let mut verified = vec![false; layout.piece_count()];
        verified[0] = true;
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout,
            promoted,
            verified,
        )
        .await
        .expect("resume promoted storage");
        assert_eq!(resumed, ResumedStorage::Staging);
        assert!(!paths.staging.join("skip/large.bin").exists());
        assert_eq!(
            storage.hash_piece(0).await.expect("hash promoted piece"),
            <[u8; 20]>::from(Sha1::digest(&bytes[..metainfo.piece_length as usize]))
        );
        storage
            .reconcile_after_recheck()
            .await
            .expect("promote only after recheck");
        assert_eq!(
            tokio::fs::read(paths.staging.join("skip/large.bin"))
                .await
                .expect("read promoted file")[..12_768],
            bytes[20_000..32_768]
        );
        assert!(!paths.part.exists());
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn resume_reconciliation_keeps_new_boundary_writes_on_wanted_routes() {
        let root = test_path("resume-new-boundary-route");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let selection = FileSelection::new(&layout, &[1, 2]).expect("selection");
        let bytes = torrent_bytes(&metainfo);
        let (mut storage, _) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            vec![false; layout.piece_count()],
        )
        .await
        .expect("create initial storage");
        for request in layout
            .request_ranges(0, &selection)
            .expect("piece requests")
        {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write retained boundary piece");
        }
        storage.record_verified(0).expect("record retained piece");
        storage.sync_piece(0).await.expect("sync retained piece");
        drop(storage);

        let mut committed = vec![false; layout.piece_count()];
        committed[0] = true;
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            selection.clone(),
            committed,
        )
        .await
        .expect("resume partial storage");
        assert_eq!(
            storage
                .validate_fast_resume(resumed)
                .await
                .expect("validate partial resume")
                .evidence,
            ResumeStorageEvidence::Matches
        );
        storage
            .reconcile_after_recheck()
            .await
            .expect("reconcile accepted resume routes");

        let piece = 2_u32;
        let piece_offset = piece as usize * metainfo.piece_length as usize;
        for request in layout
            .request_ranges(piece, &selection)
            .expect("new boundary requests")
        {
            let begin = request.begin as usize;
            storage
                .write_block(
                    piece,
                    request.begin,
                    bytes[piece_offset + begin..piece_offset + begin + request.length as usize]
                        .to_vec(),
                )
                .await
                .expect("write new boundary piece");
        }
        let length = layout.piece_length_at(piece).expect("piece length") as usize;
        assert_eq!(
            storage
                .hash_piece(piece)
                .await
                .expect("hash new boundary piece"),
            <[u8; 20]>::from(Sha1::digest(&bytes[piece_offset..piece_offset + length]))
        );

        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn resume_keeps_existing_file_route_when_it_becomes_skipped() {
        let root = test_path("resume-retain-skipped");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let all_wanted = FileSelection::new(&layout, &[]).expect("initial selection");
        let bytes = torrent_bytes(&metainfo);
        let (mut storage, _) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            all_wanted.clone(),
            vec![false; layout.piece_count()],
        )
        .await
        .expect("create all-wanted storage");
        for request in layout
            .request_ranges(0, &all_wanted)
            .expect("piece requests")
        {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write wanted piece");
        }
        storage.record_verified(0).expect("record verified piece");
        storage.sync_piece(0).await.expect("sync verified piece");
        drop(storage);

        let lowered = FileSelection::new(&layout, &[1]).expect("lowered selection");
        let mut verified = vec![false; layout.piece_count()];
        verified[0] = true;
        let (mut storage, resumed) = SelectiveStorage::resume_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout,
            lowered,
            verified,
        )
        .await
        .expect("resume lowered storage");
        assert_eq!(resumed, ResumedStorage::Staging);
        assert!(storage.has_piece_sources(0).await.expect("piece sources"));
        assert_eq!(
            storage.hash_piece(0).await.expect("hash retained route"),
            <[u8; 20]>::from(Sha1::digest(&bytes[..metainfo.piece_length as usize]))
        );
        assert_eq!(
            storage
                .write_stats(0, 0, metainfo.piece_length as usize)
                .expect("retained route stats"),
            SelectiveWriteStats {
                wanted_bytes: 20_000,
                skipped_bytes: metainfo.piece_length as usize - 20_000,
            }
        );
        assert!(!paths.part.exists());
        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn live_selection_reconcile_promotes_and_demotes_without_losing_verification() {
        let root = test_path("live-selection-reconcile");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let skipped = FileSelection::new(&layout, &[1]).expect("initial selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage = SelectiveStorage::create_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            skipped.clone(),
        )
        .await
        .expect("create selective storage");
        for request in layout.request_ranges(0, &skipped).expect("piece requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write boundary piece");
        }
        storage.record_verified(0).expect("record verified piece");
        storage.sync_piece(0).await.expect("sync verified piece");
        assert!(paths.part.exists());

        let all_wanted = FileSelection::new(&layout, &[]).expect("promoted selection");
        let promoted = storage
            .reconcile_selection(all_wanted)
            .await
            .expect("promote skipped file");
        assert_eq!(promoted.route_epoch, 1);
        assert_eq!(promoted.promoted_files, vec![1]);
        assert!(promoted.demoted_files.is_empty());
        assert!(promoted.invalidated_pieces.is_empty());
        assert!(storage.verified_pieces()[0]);
        assert_eq!(
            tokio::fs::read(paths.staging.join("skip/large.bin"))
                .await
                .expect("read promoted file")[..12_768],
            bytes[20_000..32_768]
        );
        assert!(!paths.part.exists());

        let skipped_again = FileSelection::new(&layout, &[1]).expect("demoted selection");
        let demoted = storage
            .reconcile_selection(skipped_again)
            .await
            .expect("demote promoted file");
        assert_eq!(demoted.route_epoch, 2);
        assert!(demoted.promoted_files.is_empty());
        assert_eq!(demoted.demoted_files, vec![1]);
        assert!(storage.verified_pieces()[0]);
        assert_eq!(
            storage.hash_piece(0).await.expect("hash retained route"),
            <[u8; 20]>::from(Sha1::digest(&bytes[..metainfo.piece_length as usize]))
        );
        assert_eq!(storage.route_epoch(), 2);

        tokio::fs::remove_dir_all(root).await.expect("remove root");
    }

    #[tokio::test]
    async fn promotion_clears_only_piece_evidence_with_a_missing_part_span() {
        let root = test_path("live-selection-missing-span");
        tokio::fs::create_dir(&root).await.expect("create root");
        let metainfo = fixture();
        let paths = torrent_storage_paths(&root, &metainfo.name, test_torrent_id())
            .expect("plan storage paths");
        let layout = TorrentLayout::from_metainfo(&metainfo);
        let skipped = FileSelection::new(&layout, &[1]).expect("initial selection");
        let bytes = torrent_bytes(&metainfo);
        let mut storage = SelectiveStorage::create_with_paths(
            paths.clone(),
            test_artifact_identity(),
            layout.clone(),
            skipped.clone(),
        )
        .await
        .expect("create selective storage");
        for request in layout.request_ranges(0, &skipped).expect("piece requests") {
            let begin = request.begin as usize;
            storage
                .write_block(
                    0,
                    request.begin,
                    bytes[begin..begin + request.length as usize].to_vec(),
                )
                .await
                .expect("write boundary piece");
        }
        storage.record_verified(0).expect("record verified piece");
        storage.sync_piece(0).await.expect("sync verified piece");
        storage
            .part_file
            .as_mut()
            .expect("boundary piece owns a part slot")
            .release_piece(0)
            .await
            .expect("simulate missing part span");

        let all_wanted = FileSelection::new(&layout, &[]).expect("promoted selection");
        let promoted = storage
            .reconcile_selection(all_wanted)
            .await
            .expect("promote with missing part span");

        assert_eq!(promoted.route_epoch, 1);
        assert_eq!(promoted.invalidated_pieces, vec![0]);
        assert!(!storage.verified_pieces()[0]);
        assert_eq!(
            tokio::fs::read(paths.staging.join("skip/large.bin"))
                .await
                .expect("read uncertain promoted file"),
            vec![0; metainfo.files[1].length as usize]
        );
        tokio::fs::remove_dir_all(root).await.expect("remove root");
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
            test_artifact_identity(),
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
            test_artifact_identity(),
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
            test_artifact_identity(),
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
                test_artifact_identity(),
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
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
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
        let mut storage = SelectiveStorage::create(
            output.clone(),
            test_artifact_identity(),
            &metainfo,
            layout,
            selection,
        )
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
        let paths = torrent_storage_paths_for_output_with_shape(
            output.clone(),
            test_torrent_id(),
            PublicationShape::from_metainfo(&metainfo),
        )
        .expect("storage paths");

        tokio::fs::create_dir(&output)
            .await
            .expect("create existing output");
        assert!(matches!(
            SelectiveStorage::create(
                output.clone(),
                test_artifact_identity(),
                &metainfo,
                layout.clone(),
                selection.clone()
            )
            .await,
            Err(SelectiveStorageError::ExistingOutput(_))
        ));
        assert!(tokio::fs::try_exists(&output).await.expect("output state"));
        tokio::fs::remove_dir(&output)
            .await
            .expect("remove existing output");

        let staging = paths.staging;
        tokio::fs::create_dir(&staging)
            .await
            .expect("create existing staging");
        assert!(matches!(
            SelectiveStorage::create(
                output.clone(),
                test_artifact_identity(),
                &metainfo,
                layout.clone(),
                selection.clone()
            )
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

        let part = paths.part;
        tokio::fs::write(&part, b"owned elsewhere")
            .await
            .expect("create existing part");
        assert!(matches!(
            SelectiveStorage::create(
                output.clone(),
                test_artifact_identity(),
                &metainfo,
                layout,
                selection
            )
            .await,
            Err(SelectiveStorageError::ExistingPartFile(_))
        ));
        assert_eq!(
            tokio::fs::read(&part).await.expect("preserved part"),
            b"owned elsewhere"
        );
        clean(&output).await;
    }
}

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use rstorrent_protocol::metainfo::MAX_PIECE_LENGTH;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};

const MAGIC: &[u8; 8] = b"RSPART01";
const VERSION: u32 = 1;
const FIXED_HEADER_LENGTH: usize = 64;
const HEADER_ALIGNMENT: usize = 1024;
const SLOT_ENTRY_LENGTH: usize = 4;
const MISSING_SLOT: i32 = -1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PartFileIdentity {
    pub info_hash: [u8; 20],
    pub piece_count: usize,
    pub piece_length: u32,
    pub total_length: u64,
}

#[derive(Debug)]
pub enum PartFileError {
    InvalidIdentity(&'static str),
    Existing(PathBuf),
    NonemptyDescriptor {
        length: u64,
    },
    InvalidMagic,
    UnsupportedVersion(u32),
    InvalidHeaderLength {
        actual: u32,
        expected: u32,
    },
    NonzeroReserved,
    LayoutMismatch(&'static str),
    InvalidSlot {
        piece_index: usize,
        slot: i32,
    },
    DuplicateSlot {
        slot: u32,
    },
    PieceOutOfRange {
        piece_index: usize,
        piece_count: usize,
    },
    PieceRangeOutOfBounds {
        piece_index: usize,
        offset: u32,
        length: usize,
        piece_length: u32,
    },
    MissingSlot {
        piece_index: usize,
    },
    OffsetOverflow,
    TruncatedPayload {
        piece_index: usize,
        expected_end: u64,
        file_length: u64,
    },
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for PartFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(reason) => {
                write!(formatter, "invalid part-file identity: {reason}")
            }
            Self::Existing(path) => {
                write!(formatter, "part file already exists: {}", path.display())
            }
            Self::NonemptyDescriptor { length } => {
                write!(
                    formatter,
                    "part-file descriptor is not empty: {length} bytes"
                )
            }
            Self::InvalidMagic => write!(formatter, "part file has invalid magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "part file version {version} is unsupported")
            }
            Self::InvalidHeaderLength { actual, expected } => write!(
                formatter,
                "part file header length {actual} does not match expected {expected}"
            ),
            Self::NonzeroReserved => {
                write!(formatter, "part file reserved bytes are not zero")
            }
            Self::LayoutMismatch(field) => {
                write!(formatter, "part file {field} does not match torrent")
            }
            Self::InvalidSlot { piece_index, slot } => write!(
                formatter,
                "part file piece {piece_index} has invalid slot {slot}"
            ),
            Self::DuplicateSlot { slot } => {
                write!(formatter, "part file slot {slot} is mapped more than once")
            }
            Self::PieceOutOfRange {
                piece_index,
                piece_count,
            } => write!(
                formatter,
                "piece index {piece_index} is outside 0..{piece_count}"
            ),
            Self::PieceRangeOutOfBounds {
                piece_index,
                offset,
                length,
                piece_length,
            } => write!(
                formatter,
                "piece {piece_index} range {offset}+{length} exceeds length {piece_length}"
            ),
            Self::MissingSlot { piece_index } => {
                write!(formatter, "piece {piece_index} has no part-file slot")
            }
            Self::OffsetOverflow => write!(formatter, "part-file offset overflow"),
            Self::TruncatedPayload {
                piece_index,
                expected_end,
                file_length,
            } => write!(
                formatter,
                "piece {piece_index} payload ends at {expected_end}, file length is {file_length}"
            ),
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}"),
        }
    }
}

impl Error for PartFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PartFile {
    file: File,
    path: Option<PathBuf>,
    identity: PartFileIdentity,
    header_length: u64,
    slots: Vec<Option<u32>>,
}

impl PartFile {
    pub async fn create(path: PathBuf, identity: PartFileIdentity) -> Result<Self, PartFileError> {
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                return Err(PartFileError::Existing(path));
            }
            Err(source) => {
                return Err(PartFileError::Io {
                    operation: "create part file",
                    source,
                });
            }
        };
        Self::initialize(file, Some(path), identity).await
    }

    pub async fn create_preopened(
        file: std::fs::File,
        identity: PartFileIdentity,
    ) -> Result<Self, PartFileError> {
        Self::initialize(File::from_std(file), None, identity).await
    }

    async fn initialize(
        mut file: File,
        path: Option<PathBuf>,
        identity: PartFileIdentity,
    ) -> Result<Self, PartFileError> {
        let header_length = validate_identity(identity)?;
        let existing_length = file
            .metadata()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "inspect new part-file descriptor",
                source,
            })?
            .len();
        if existing_length != 0 {
            return Err(PartFileError::NonemptyDescriptor {
                length: existing_length,
            });
        }
        file.seek(SeekFrom::Start(0))
            .await
            .map_err(|source| PartFileError::Io {
                operation: "seek new part-file descriptor",
                source,
            })?;
        let header_length_usize =
            usize::try_from(header_length).map_err(|_| PartFileError::OffsetOverflow)?;
        let mut header = vec![0_u8; header_length_usize];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&VERSION.to_be_bytes());
        header[12..16].copy_from_slice(
            &u32::try_from(header_length)
                .map_err(|_| PartFileError::OffsetOverflow)?
                .to_be_bytes(),
        );
        header[16..36].copy_from_slice(&identity.info_hash);
        header[36..40].copy_from_slice(
            &u32::try_from(identity.piece_count)
                .map_err(|_| PartFileError::OffsetOverflow)?
                .to_be_bytes(),
        );
        header[40..44].copy_from_slice(&identity.piece_length.to_be_bytes());
        header[44..52].copy_from_slice(&identity.total_length.to_be_bytes());
        for piece_index in 0..identity.piece_count {
            let entry = slot_entry_offset(piece_index)?;
            header[entry..entry + SLOT_ENTRY_LENGTH].copy_from_slice(&MISSING_SLOT.to_be_bytes());
        }

        file.write_all(&header)
            .await
            .map_err(|source| PartFileError::Io {
                operation: "write part-file header",
                source,
            })?;
        file.sync_data().await.map_err(|source| PartFileError::Io {
            operation: "flush part-file header",
            source,
        })?;

        Ok(Self {
            file,
            path,
            identity,
            header_length,
            slots: vec![None; identity.piece_count],
        })
    }

    pub async fn open(path: PathBuf, expected: PartFileIdentity) -> Result<Self, PartFileError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .await
            .map_err(|source| PartFileError::Io {
                operation: "open part file",
                source,
            })?;
        Self::open_file(file, Some(path), expected).await
    }

    pub async fn open_preopened(
        file: std::fs::File,
        expected: PartFileIdentity,
    ) -> Result<Self, PartFileError> {
        Self::open_file(File::from_std(file), None, expected).await
    }

    async fn open_file(
        mut file: File,
        path: Option<PathBuf>,
        expected: PartFileIdentity,
    ) -> Result<Self, PartFileError> {
        let expected_header_length = validate_identity(expected)?;
        let file_length = file
            .metadata()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "inspect part file",
                source,
            })?
            .len();
        if file_length < FIXED_HEADER_LENGTH as u64 {
            return Err(PartFileError::InvalidHeaderLength {
                actual: u32::try_from(file_length).unwrap_or(u32::MAX),
                expected: u32::try_from(expected_header_length)
                    .map_err(|_| PartFileError::OffsetOverflow)?,
            });
        }
        if file_length < expected_header_length {
            return Err(PartFileError::InvalidHeaderLength {
                actual: u32::try_from(file_length).unwrap_or(u32::MAX),
                expected: u32::try_from(expected_header_length)
                    .map_err(|_| PartFileError::OffsetOverflow)?,
            });
        }

        let expected_header_usize =
            usize::try_from(expected_header_length).map_err(|_| PartFileError::OffsetOverflow)?;
        let mut header = vec![0_u8; expected_header_usize];
        file.seek(SeekFrom::Start(0))
            .await
            .map_err(|source| PartFileError::Io {
                operation: "seek part-file header",
                source,
            })?;
        file.read_exact(&mut header)
            .await
            .map_err(|source| PartFileError::Io {
                operation: "read part-file header",
                source,
            })?;

        if &header[..8] != MAGIC {
            return Err(PartFileError::InvalidMagic);
        }
        let version = read_u32(&header[8..12]);
        if version != VERSION {
            return Err(PartFileError::UnsupportedVersion(version));
        }
        let header_length = read_u32(&header[12..16]);
        let expected_header_u32 =
            u32::try_from(expected_header_length).map_err(|_| PartFileError::OffsetOverflow)?;
        if header_length != expected_header_u32 {
            return Err(PartFileError::InvalidHeaderLength {
                actual: header_length,
                expected: expected_header_u32,
            });
        }
        if header[52..64].iter().any(|byte| *byte != 0) {
            return Err(PartFileError::NonzeroReserved);
        }
        if header[16..36] != expected.info_hash {
            return Err(PartFileError::LayoutMismatch("info hash"));
        }
        if read_u32(&header[36..40])
            != u32::try_from(expected.piece_count).map_err(|_| PartFileError::OffsetOverflow)?
        {
            return Err(PartFileError::LayoutMismatch("piece count"));
        }
        if read_u32(&header[40..44]) != expected.piece_length {
            return Err(PartFileError::LayoutMismatch("piece length"));
        }
        if read_u64(&header[44..52]) != expected.total_length {
            return Err(PartFileError::LayoutMismatch("total length"));
        }

        let table_end = FIXED_HEADER_LENGTH
            .checked_add(
                expected
                    .piece_count
                    .checked_mul(SLOT_ENTRY_LENGTH)
                    .ok_or(PartFileError::OffsetOverflow)?,
            )
            .ok_or(PartFileError::OffsetOverflow)?;
        if header[table_end..].iter().any(|byte| *byte != 0) {
            return Err(PartFileError::NonzeroReserved);
        }

        let mut used_slots = HashSet::new();
        let mut slots = Vec::with_capacity(expected.piece_count);
        for piece_index in 0..expected.piece_count {
            let entry = slot_entry_offset(piece_index)?;
            let slot = read_i32(&header[entry..entry + SLOT_ENTRY_LENGTH]);
            if slot == MISSING_SLOT {
                slots.push(None);
                continue;
            }
            let Ok(slot) = u32::try_from(slot) else {
                return Err(PartFileError::InvalidSlot { piece_index, slot });
            };
            if slot as usize >= expected.piece_count {
                return Err(PartFileError::InvalidSlot {
                    piece_index,
                    slot: slot as i32,
                });
            }
            if !used_slots.insert(slot) {
                return Err(PartFileError::DuplicateSlot { slot });
            }
            slots.push(Some(slot));
        }

        Ok(Self {
            file,
            path,
            identity: expected,
            header_length: expected_header_length,
            slots,
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn mapped_piece_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    pub fn has_piece(&self, piece_index: usize) -> Result<bool, PartFileError> {
        self.validate_piece_index(piece_index)?;
        Ok(self.slots[piece_index].is_some())
    }

    pub async fn write_piece_range(
        &mut self,
        piece_index: usize,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), PartFileError> {
        self.validate_piece_range(piece_index, offset, bytes.len())?;
        let slot = self.ensure_slot(piece_index).await?;
        let file_offset = self.payload_offset(slot, offset, bytes.len())?;
        self.file
            .seek(SeekFrom::Start(file_offset))
            .await
            .map_err(|source| PartFileError::Io {
                operation: "seek part file for write",
                source,
            })?;
        self.file
            .write_all(bytes)
            .await
            .map_err(|source| PartFileError::Io {
                operation: "write part-file payload",
                source,
            })
    }

    pub async fn read_piece_range(
        &mut self,
        piece_index: usize,
        offset: u32,
        bytes: &mut [u8],
    ) -> Result<(), PartFileError> {
        self.validate_piece_range(piece_index, offset, bytes.len())?;
        let slot = self.slots[piece_index].ok_or(PartFileError::MissingSlot { piece_index })?;
        let file_offset = self.payload_offset(slot, offset, bytes.len())?;
        let expected_end = file_offset
            .checked_add(bytes.len() as u64)
            .ok_or(PartFileError::OffsetOverflow)?;
        let file_length = self
            .file
            .metadata()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "inspect part-file payload",
                source,
            })?
            .len();
        if expected_end > file_length {
            return Err(PartFileError::TruncatedPayload {
                piece_index,
                expected_end,
                file_length,
            });
        }
        self.file
            .seek(SeekFrom::Start(file_offset))
            .await
            .map_err(|source| PartFileError::Io {
                operation: "seek part file for read",
                source,
            })?;
        self.file
            .read_exact(bytes)
            .await
            .map(|_| ())
            .map_err(|source| PartFileError::Io {
                operation: "read part-file payload",
                source,
            })
    }

    pub async fn release_piece(&mut self, piece_index: usize) -> Result<bool, PartFileError> {
        self.validate_piece_index(piece_index)?;
        if self.slots[piece_index].is_none() {
            return Ok(false);
        }
        self.write_slot_entry(piece_index, None).await?;
        self.slots[piece_index] = None;
        Ok(true)
    }

    pub async fn sync_payload(&self) -> Result<(), PartFileError> {
        self.file
            .sync_data()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "flush part-file payload",
                source,
            })
    }

    pub(crate) async fn duplicate_for_checkpoint(&self) -> Result<std::fs::File, PartFileError> {
        let file = self
            .file
            .try_clone()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "duplicate part file for durability checkpoint",
                source,
            })?;
        Ok(file.into_std().await)
    }

    async fn ensure_slot(&mut self, piece_index: usize) -> Result<u32, PartFileError> {
        self.validate_piece_index(piece_index)?;
        if let Some(slot) = self.slots[piece_index] {
            return Ok(slot);
        }

        let mut used_slots = vec![false; self.identity.piece_count];
        for slot in self.slots.iter().flatten() {
            used_slots[*slot as usize] = true;
        }
        let slot = used_slots
            .iter()
            .position(|used| !used)
            .ok_or(PartFileError::OffsetOverflow)?;
        let slot = u32::try_from(slot).map_err(|_| PartFileError::OffsetOverflow)?;
        let slot_end = self.payload_offset(slot, self.identity.piece_length, 0)?;
        let current_length = self
            .file
            .metadata()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "inspect part-file slot allocation",
                source,
            })?
            .len();
        if current_length < slot_end {
            self.file
                .set_len(slot_end)
                .await
                .map_err(|source| PartFileError::Io {
                    operation: "size part-file slot allocation",
                    source,
                })?;
        }
        self.write_slot_entry(piece_index, Some(slot)).await?;
        self.slots[piece_index] = Some(slot);
        Ok(slot)
    }

    async fn write_slot_entry(
        &mut self,
        piece_index: usize,
        slot: Option<u32>,
    ) -> Result<(), PartFileError> {
        let offset = u64::try_from(slot_entry_offset(piece_index)?)
            .map_err(|_| PartFileError::OffsetOverflow)?;
        let value = match slot {
            Some(slot) => i32::try_from(slot).map_err(|_| PartFileError::OffsetOverflow)?,
            None => MISSING_SLOT,
        };
        self.file
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|source| PartFileError::Io {
                operation: "seek part-file slot entry",
                source,
            })?;
        self.file
            .write_all(&value.to_be_bytes())
            .await
            .map_err(|source| PartFileError::Io {
                operation: "write part-file slot entry",
                source,
            })?;
        self.file
            .sync_data()
            .await
            .map_err(|source| PartFileError::Io {
                operation: "flush part-file slot entry",
                source,
            })
    }

    fn payload_offset(
        &self,
        slot: u32,
        piece_offset: u32,
        length: usize,
    ) -> Result<u64, PartFileError> {
        let slot_offset = u64::from(slot)
            .checked_mul(u64::from(self.identity.piece_length))
            .ok_or(PartFileError::OffsetOverflow)?;
        let start = self
            .header_length
            .checked_add(slot_offset)
            .and_then(|offset| offset.checked_add(u64::from(piece_offset)))
            .ok_or(PartFileError::OffsetOverflow)?;
        start
            .checked_add(length as u64)
            .ok_or(PartFileError::OffsetOverflow)?;
        Ok(start)
    }

    fn validate_piece_index(&self, piece_index: usize) -> Result<(), PartFileError> {
        if piece_index >= self.identity.piece_count {
            return Err(PartFileError::PieceOutOfRange {
                piece_index,
                piece_count: self.identity.piece_count,
            });
        }
        Ok(())
    }

    fn validate_piece_range(
        &self,
        piece_index: usize,
        offset: u32,
        length: usize,
    ) -> Result<(), PartFileError> {
        self.validate_piece_index(piece_index)?;
        let piece_length = self.piece_length_at(piece_index)?;
        if length == 0
            || u64::from(offset)
                .checked_add(length as u64)
                .is_none_or(|end| end > u64::from(piece_length))
        {
            return Err(PartFileError::PieceRangeOutOfBounds {
                piece_index,
                offset,
                length,
                piece_length,
            });
        }
        Ok(())
    }

    fn piece_length_at(&self, piece_index: usize) -> Result<u32, PartFileError> {
        self.validate_piece_index(piece_index)?;
        let start = u64::try_from(piece_index)
            .map_err(|_| PartFileError::OffsetOverflow)?
            .checked_mul(u64::from(self.identity.piece_length))
            .ok_or(PartFileError::OffsetOverflow)?;
        let remaining = self
            .identity
            .total_length
            .checked_sub(start)
            .ok_or(PartFileError::OffsetOverflow)?;
        u32::try_from(remaining.min(u64::from(self.identity.piece_length)))
            .map_err(|_| PartFileError::OffsetOverflow)
    }
}

fn validate_identity(identity: PartFileIdentity) -> Result<u64, PartFileError> {
    if identity.piece_count == 0 {
        return Err(PartFileError::InvalidIdentity("piece count is zero"));
    }
    if identity.piece_length == 0 || identity.piece_length > MAX_PIECE_LENGTH {
        return Err(PartFileError::InvalidIdentity(
            "piece length is out of range",
        ));
    }
    let minimum_total = u64::try_from(identity.piece_count - 1)
        .map_err(|_| PartFileError::OffsetOverflow)?
        .checked_mul(u64::from(identity.piece_length))
        .and_then(|length| length.checked_add(1))
        .ok_or(PartFileError::OffsetOverflow)?;
    let maximum_total = u64::try_from(identity.piece_count)
        .map_err(|_| PartFileError::OffsetOverflow)?
        .checked_mul(u64::from(identity.piece_length))
        .ok_or(PartFileError::OffsetOverflow)?;
    if identity.total_length < minimum_total || identity.total_length > maximum_total {
        return Err(PartFileError::InvalidIdentity(
            "total length does not match piece geometry",
        ));
    }
    let unaligned = FIXED_HEADER_LENGTH
        .checked_add(
            identity
                .piece_count
                .checked_mul(SLOT_ENTRY_LENGTH)
                .ok_or(PartFileError::OffsetOverflow)?,
        )
        .ok_or(PartFileError::OffsetOverflow)?;
    let aligned = unaligned
        .checked_add(HEADER_ALIGNMENT - 1)
        .ok_or(PartFileError::OffsetOverflow)?
        / HEADER_ALIGNMENT
        * HEADER_ALIGNMENT;
    u64::try_from(aligned).map_err(|_| PartFileError::OffsetOverflow)
}

fn slot_entry_offset(piece_index: usize) -> Result<usize, PartFileError> {
    FIXED_HEADER_LENGTH
        .checked_add(
            piece_index
                .checked_mul(SLOT_ENTRY_LENGTH)
                .ok_or(PartFileError::OffsetOverflow)?,
        )
        .ok_or(PartFileError::OffsetOverflow)
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().expect("four-byte field"))
}

fn read_i32(bytes: &[u8]) -> i32 {
    i32::from_be_bytes(bytes.try_into().expect("four-byte field"))
}

fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().expect("eight-byte field"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};

    use rstorrent_protocol::metainfo::MAX_PIECE_LENGTH;

    use super::{
        FIXED_HEADER_LENGTH, PartFile, PartFileError, PartFileIdentity, slot_entry_offset,
        validate_identity,
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn test_path(name: &str) -> PathBuf {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "rstorrent-part-file-test-{}-{sequence}-{name}",
            std::process::id()
        ))
    }

    fn identity() -> PartFileIdentity {
        PartFileIdentity {
            info_hash: [7; 20],
            piece_count: 5,
            piece_length: 32_768,
            total_length: 133_304,
        }
    }

    async fn clean(path: &Path) {
        let _ = tokio::fs::remove_file(path).await;
    }

    async fn overwrite(path: &Path, offset: u64, bytes: &[u8]) {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await
            .expect("open for corruption");
        file.seek(SeekFrom::Start(offset))
            .await
            .expect("seek for corruption");
        file.write_all(bytes).await.expect("write corruption");
        file.sync_data().await.expect("flush corruption");
    }

    #[tokio::test]
    async fn allocates_reuses_and_reopens_compact_slots() {
        let path = test_path("slots");
        clean(&path).await;
        let mut part = PartFile::create(path.clone(), identity())
            .await
            .expect("create part file");
        part.write_piece_range(0, 20_000, b"boundary")
            .await
            .expect("write piece zero");
        part.write_piece_range(2, 0, b"second slot")
            .await
            .expect("write piece two");
        assert_eq!(part.mapped_piece_count(), 2);
        assert_eq!(
            tokio::fs::metadata(&path)
                .await
                .expect("part metadata")
                .len(),
            part.header_length + 2 * u64::from(part.identity.piece_length)
        );
        part.sync_payload().await.expect("sync payload");
        drop(part);

        let mut reopened = PartFile::open(path.clone(), identity())
            .await
            .expect("reopen part file");
        let mut first = [0_u8; 8];
        reopened
            .read_piece_range(0, 20_000, &mut first)
            .await
            .expect("read first slot");
        assert_eq!(&first, b"boundary");
        let mut second = [0_u8; 11];
        reopened
            .read_piece_range(2, 0, &mut second)
            .await
            .expect("read second slot");
        assert_eq!(&second, b"second slot");

        assert!(reopened.release_piece(0).await.expect("release slot"));
        reopened
            .write_piece_range(4, 0, b"reused")
            .await
            .expect("reuse slot");
        drop(reopened);

        let reopened = PartFile::open(path.clone(), identity())
            .await
            .expect("reopen reused slots");
        assert!(!reopened.has_piece(0).expect("piece zero mapping"));
        assert!(reopened.has_piece(2).expect("piece two mapping"));
        assert!(reopened.has_piece(4).expect("piece four mapping"));
        clean(&path).await;
    }

    #[tokio::test]
    async fn preopened_descriptor_refuses_content_and_reopens_without_a_path() {
        let path = test_path("preopened");
        clean(&path).await;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("create descriptor file");
        let mut part = PartFile::create_preopened(file, identity())
            .await
            .expect("initialize descriptor part file");
        assert_eq!(part.path(), None);
        part.write_piece_range(2, 17, b"descriptor")
            .await
            .expect("write descriptor slot");
        part.sync_payload().await.expect("sync descriptor part");
        drop(part);

        let reopen = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("independently reopen descriptor file");
        let mut reopened = PartFile::open_preopened(reopen, identity())
            .await
            .expect("validate descriptor part file");
        let mut bytes = [0_u8; 10];
        reopened
            .read_piece_range(2, 17, &mut bytes)
            .await
            .expect("read descriptor slot");
        assert_eq!(&bytes, b"descriptor");
        drop(reopened);
        clean(&path).await;

        let nonempty_path = test_path("preopened-nonempty");
        clean(&nonempty_path).await;
        std::fs::write(&nonempty_path, b"sentinel").expect("write sentinel");
        let nonempty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&nonempty_path)
            .expect("open sentinel");
        assert!(matches!(
            PartFile::create_preopened(nonempty, identity()).await,
            Err(PartFileError::NonemptyDescriptor { length: 8 })
        ));
        assert_eq!(
            std::fs::read(&nonempty_path).expect("read preserved sentinel"),
            b"sentinel"
        );
        clean(&nonempty_path).await;
    }

    #[tokio::test]
    async fn rejects_identity_mismatch_and_corrupt_headers() {
        let path = test_path("corruption");
        clean(&path).await;
        drop(
            PartFile::create(path.clone(), identity())
                .await
                .expect("create part file"),
        );

        let mut mismatch = identity();
        mismatch.info_hash[0] = 8;
        assert!(matches!(
            PartFile::open(path.clone(), mismatch).await,
            Err(PartFileError::LayoutMismatch("info hash"))
        ));

        overwrite(&path, 52, &[1]).await;
        assert!(matches!(
            PartFile::open(path.clone(), identity()).await,
            Err(PartFileError::NonzeroReserved)
        ));
        overwrite(&path, 52, &[0]).await;

        let padding_offset =
            u64::try_from(FIXED_HEADER_LENGTH + identity().piece_count * 4).expect("offset");
        overwrite(&path, padding_offset, &[1]).await;
        assert!(matches!(
            PartFile::open(path.clone(), identity()).await,
            Err(PartFileError::NonzeroReserved)
        ));
        clean(&path).await;
    }

    #[tokio::test]
    async fn rejects_magic_version_header_length_and_header_truncation() {
        let path = test_path("fixed-header");
        clean(&path).await;
        drop(
            PartFile::create(path.clone(), identity())
                .await
                .expect("create part file"),
        );
        assert!(matches!(
            PartFile::create(path.clone(), identity()).await,
            Err(PartFileError::Existing(_))
        ));

        overwrite(&path, 0, b"BADMAGIC").await;
        assert!(matches!(
            PartFile::open(path.clone(), identity()).await,
            Err(PartFileError::InvalidMagic)
        ));
        overwrite(&path, 0, super::MAGIC).await;

        overwrite(&path, 8, &2_u32.to_be_bytes()).await;
        assert!(matches!(
            PartFile::open(path.clone(), identity()).await,
            Err(PartFileError::UnsupportedVersion(2))
        ));
        overwrite(&path, 8, &super::VERSION.to_be_bytes()).await;

        overwrite(&path, 12, &2048_u32.to_be_bytes()).await;
        assert!(matches!(
            PartFile::open(path.clone(), identity()).await,
            Err(PartFileError::InvalidHeaderLength { .. })
        ));
        clean(&path).await;

        let truncated = test_path("truncated-header");
        clean(&truncated).await;
        drop(
            PartFile::create(truncated.clone(), identity())
                .await
                .expect("create part file"),
        );
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(&truncated)
            .await
            .expect("open for truncation")
            .set_len(100)
            .await
            .expect("truncate header");
        assert!(matches!(
            PartFile::open(truncated.clone(), identity()).await,
            Err(PartFileError::InvalidHeaderLength { .. })
        ));
        clean(&truncated).await;
    }

    #[tokio::test]
    async fn rejects_every_mismatched_layout_identity_field() {
        let cases = [
            (36_u64, 4_u32.to_be_bytes().to_vec(), "piece count"),
            (
                40,
                (identity().piece_length + 1).to_be_bytes().to_vec(),
                "piece length",
            ),
            (
                44,
                (identity().total_length - 1).to_be_bytes().to_vec(),
                "total length",
            ),
        ];
        for (offset, bytes, field) in cases {
            let path = test_path(field);
            clean(&path).await;
            drop(
                PartFile::create(path.clone(), identity())
                    .await
                    .expect("create part file"),
            );
            overwrite(&path, offset, &bytes).await;
            assert!(matches!(
                PartFile::open(path.clone(), identity()).await,
                Err(PartFileError::LayoutMismatch(actual)) if actual == field
            ));
            clean(&path).await;
        }
    }

    #[tokio::test]
    async fn rejects_duplicate_negative_and_out_of_range_slots() {
        for (name, first, second, expected) in [
            (
                "duplicate",
                0_i32,
                0_i32,
                PartFileError::DuplicateSlot { slot: 0 },
            ),
            (
                "negative",
                -2_i32,
                -1_i32,
                PartFileError::InvalidSlot {
                    piece_index: 0,
                    slot: -2,
                },
            ),
            (
                "range",
                5_i32,
                -1_i32,
                PartFileError::InvalidSlot {
                    piece_index: 0,
                    slot: 5,
                },
            ),
        ] {
            let path = test_path(name);
            clean(&path).await;
            drop(
                PartFile::create(path.clone(), identity())
                    .await
                    .expect("create part file"),
            );
            overwrite(
                &path,
                u64::try_from(slot_entry_offset(0).expect("entry")).expect("offset"),
                &first.to_be_bytes(),
            )
            .await;
            overwrite(
                &path,
                u64::try_from(slot_entry_offset(1).expect("entry")).expect("offset"),
                &second.to_be_bytes(),
            )
            .await;
            let error = PartFile::open(path.clone(), identity())
                .await
                .expect_err("reject slot table");
            assert_eq!(error.to_string(), expected.to_string());
            clean(&path).await;
        }
    }

    #[tokio::test]
    async fn reports_missing_truncated_and_invalid_ranges() {
        let path = test_path("payload");
        clean(&path).await;
        let mut part = PartFile::create(path.clone(), identity())
            .await
            .expect("create part file");
        let mut byte = [0_u8; 1];
        assert!(matches!(
            part.read_piece_range(0, 0, &mut byte).await,
            Err(PartFileError::MissingSlot { piece_index: 0 })
        ));
        assert!(matches!(
            part.write_piece_range(4, 2_232, &[1]).await,
            Err(PartFileError::PieceRangeOutOfBounds { piece_index: 4, .. })
        ));

        part.write_piece_range(0, 100, b"x")
            .await
            .expect("allocate short payload");
        part.file
            .set_len(part.header_length + 101)
            .await
            .expect("truncate allocated slot");
        assert!(matches!(
            part.read_piece_range(0, 101, &mut byte).await,
            Err(PartFileError::TruncatedPayload { piece_index: 0, .. })
        ));
        clean(&path).await;
    }

    #[test]
    fn computes_large_piece_slot_offsets_with_u64_geometry() {
        let large = PartFileIdentity {
            info_hash: [9; 20],
            piece_count: 26_214,
            piece_length: MAX_PIECE_LENGTH,
            total_length: 26_214_u64 * u64::from(MAX_PIECE_LENGTH),
        };
        let header = validate_identity(large).expect("large identity");
        let final_slot_offset =
            header + 26_213_u64 * u64::from(MAX_PIECE_LENGTH) + u64::from(MAX_PIECE_LENGTH - 1);
        assert!(final_slot_offset > u64::from(u32::MAX));
    }
}

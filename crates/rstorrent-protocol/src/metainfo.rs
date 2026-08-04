use std::error::Error;
use std::fmt;

use sha1::{Digest, Sha1};

use crate::bencode::{
    DictionaryEntry, Limits, MAX_BENCODE_DECODED_ITEMS, MAX_BENCODE_INPUT_LENGTH, Node, ParseError,
    Value, parse_with_limits,
};

pub const MAX_PIECE_LENGTH: u32 = 256 * 1024 * 1024;
pub const MAX_METAINFO_FILES: usize = 4096;
pub const MAX_METAINFO_PIECES: usize = 52_428;
pub const MAX_METAINFO_PATH_COMPONENTS: usize = 32;
pub const MAX_METAINFO_PATH_COMPONENT_LENGTH: usize = 255;
pub const MAX_METAINFO_PATH_LENGTH: usize = 4096;

const MAX_PIECE_HASH_STRING_LENGTH: usize = MAX_METAINFO_PIECES * 20;
const MAX_EXPLICIT_METAINFO_LENGTH: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetainfoLimits {
    pub max_outer_bytes: usize,
    pub max_info_bytes: usize,
    pub max_string_bytes: usize,
    pub max_decoded_items: usize,
    pub max_depth: usize,
    pub max_collection_entries: usize,
    pub max_files: usize,
    pub max_pieces: usize,
    pub max_path_components: usize,
    pub max_path_component_bytes: usize,
    pub max_path_bytes: usize,
}

const fn metainfo_limits(max_bytes: usize) -> MetainfoLimits {
    MetainfoLimits {
        max_outer_bytes: max_bytes,
        max_info_bytes: max_bytes,
        max_string_bytes: if max_bytes > MAX_PIECE_HASH_STRING_LENGTH {
            max_bytes
        } else {
            MAX_PIECE_HASH_STRING_LENGTH
        },
        max_decoded_items: MAX_BENCODE_DECODED_ITEMS,
        max_depth: 32,
        max_collection_entries: 4096,
        max_files: MAX_METAINFO_FILES,
        max_pieces: MAX_METAINFO_PIECES,
        max_path_components: MAX_METAINFO_PATH_COMPONENTS,
        max_path_component_bytes: MAX_METAINFO_PATH_COMPONENT_LENGTH,
        max_path_bytes: MAX_METAINFO_PATH_LENGTH,
    }
}

/// The current peer-metadata parsing policy. BEP 9 supplies only `info`, so
/// `max_outer_bytes` is a defensive value rather than a transport contract.
pub const BEP9_METAINFO_LIMITS: MetainfoLimits = metainfo_limits(MAX_BENCODE_INPUT_LENGTH);

/// The parser policy for persisted `raw_info` bytes.
pub const DURABLE_METAINFO_LIMITS: MetainfoLimits = metainfo_limits(MAX_BENCODE_INPUT_LENGTH);

/// Parser-only headroom for a future explicit local or authenticated import.
pub const EXPLICIT_IMPORT_METAINFO_LIMITS: MetainfoLimits =
    metainfo_limits(MAX_EXPLICIT_METAINFO_LENGTH);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetainfoMode {
    SingleFile,
    MultiFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetainfoFile {
    pub path: Vec<String>,
    pub length: u64,
    pub offset: u64,
    pub padding: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metainfo {
    pub info_hash: [u8; 20],
    pub piece_hashes: Vec<[u8; 20]>,
    pub piece_length: u32,
    pub total_length: u64,
    pub name: String,
    pub private: bool,
    pub mode: MetainfoMode,
    pub files: Vec<MetainfoFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetainfoError {
    Bencode(ParseError),
    RootIsNotDictionary,
    MissingField(&'static str),
    InvalidField(&'static str),
    Unsupported(&'static str),
    InfoTooLarge {
        length: usize,
        maximum: usize,
    },
    TooManyFiles {
        actual: usize,
        maximum: usize,
    },
    TooManyPieces {
        actual: usize,
        maximum: usize,
    },
    UnsafePath {
        file: Option<usize>,
        reason: &'static str,
    },
    PathCollision {
        first: usize,
        second: usize,
    },
    TotalLengthOverflow,
    PieceCountMismatch {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for MetainfoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bencode(error) => write!(formatter, "invalid metainfo bencode: {error}"),
            Self::RootIsNotDictionary => write!(formatter, "metainfo root is not a dictionary"),
            Self::MissingField(field) => write!(formatter, "metainfo is missing {field}"),
            Self::InvalidField(field) => write!(formatter, "metainfo has invalid {field}"),
            Self::Unsupported(reason) => {
                write!(formatter, "metainfo uses unsupported {reason}")
            }
            Self::InfoTooLarge { length, maximum } => write!(
                formatter,
                "metainfo info dictionary length {length} exceeds limit {maximum}"
            ),
            Self::TooManyFiles { actual, maximum } => {
                write!(formatter, "metainfo has {actual} files, limit {maximum}")
            }
            Self::TooManyPieces { actual, maximum } => {
                write!(formatter, "metainfo has {actual} pieces, limit {maximum}")
            }
            Self::UnsafePath { file, reason } => match file {
                Some(index) => write!(formatter, "metainfo file {index} has unsafe path: {reason}"),
                None => write!(formatter, "metainfo name is unsafe: {reason}"),
            },
            Self::PathCollision { first, second } => {
                write!(
                    formatter,
                    "metainfo file paths {first} and {second} collide"
                )
            }
            Self::TotalLengthOverflow => write!(formatter, "metainfo total length overflows u64"),
            Self::PieceCountMismatch { expected, actual } => write!(
                formatter,
                "metainfo has {actual} piece hashes, expected {expected}"
            ),
        }
    }
}

impl Error for MetainfoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bencode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for MetainfoError {
    fn from(error: ParseError) -> Self {
        Self::Bencode(error)
    }
}

impl Metainfo {
    pub fn from_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        let root = parse_metainfo(bytes, limits.max_outer_bytes, limits)?;
        let root_entries = dictionary(&root).ok_or(MetainfoError::RootIsNotDictionary)?;
        let info_node = field(root_entries, b"info").ok_or(MetainfoError::MissingField("info"))?;
        enforce_info_length(info_node.span.len(), limits)?;
        Self::from_info_node(bytes, info_node, limits)
    }

    pub fn from_info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        enforce_info_length(bytes.len(), limits)?;
        let info_node = parse_metainfo(bytes, limits.max_info_bytes, limits)?;
        Self::from_info_node(bytes, &info_node, limits)
    }

    pub fn info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<&[u8], MetainfoError> {
        let root = parse_metainfo(bytes, limits.max_outer_bytes, limits)?;
        let root_entries = dictionary(&root).ok_or(MetainfoError::RootIsNotDictionary)?;
        let info_node = field(root_entries, b"info").ok_or(MetainfoError::MissingField("info"))?;
        enforce_info_length(info_node.span.len(), limits)?;
        Ok(&bytes[info_node.span.clone()])
    }

    fn from_info_node(
        bytes: &[u8],
        info_node: &Node<'_>,
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        let info_entries =
            dictionary(info_node).ok_or(MetainfoError::InvalidField("info dictionary"))?;

        if field(info_entries, b"meta version").is_some() {
            return Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"));
        }

        let private = match field(info_entries, b"private").map(|node| &node.value) {
            None | Some(Value::Integer(0)) => false,
            Some(Value::Integer(1)) => true,
            Some(_) => return Err(MetainfoError::InvalidField("info.private")),
        };

        let piece_length = positive_integer(info_entries, b"piece length", "info.piece length")?;
        let piece_length = u32::try_from(piece_length)
            .map_err(|_| MetainfoError::InvalidField("info.piece length"))?;
        if piece_length > MAX_PIECE_LENGTH {
            return Err(MetainfoError::InvalidField("info.piece length"));
        }

        let name = safe_component(
            bytes_field(info_entries, b"name", "info.name")?,
            None,
            limits,
        )?;
        let length = field(info_entries, b"length");
        let files = field(info_entries, b"files");
        let (mode, files, total_length) = match (length, files) {
            (Some(length), None) => {
                let length = node_nonnegative_integer(length, "info.length")?;
                if length == 0 {
                    return Err(MetainfoError::InvalidField("info.length"));
                }
                (
                    MetainfoMode::SingleFile,
                    vec![MetainfoFile {
                        path: vec![name.clone()],
                        length,
                        offset: 0,
                        padding: false,
                    }],
                    length,
                )
            }
            (None, Some(files)) => parse_multi_files(files, limits)?,
            (Some(_), Some(_)) => {
                return Err(MetainfoError::InvalidField(
                    "info must contain exactly one of length or files",
                ));
            }
            (None, None) => {
                return Err(MetainfoError::MissingField("info.length or info.files"));
            }
        };

        if total_length == 0 {
            return Err(MetainfoError::InvalidField("info total length"));
        }

        let expected_piece_count_u64 = total_length.div_ceil(u64::from(piece_length));
        let expected_piece_count = usize::try_from(expected_piece_count_u64).map_err(|_| {
            MetainfoError::TooManyPieces {
                actual: usize::MAX,
                maximum: limits.max_pieces,
            }
        })?;
        if expected_piece_count > limits.max_pieces {
            return Err(MetainfoError::TooManyPieces {
                actual: expected_piece_count,
                maximum: limits.max_pieces,
            });
        }

        let pieces = bytes_field(info_entries, b"pieces", "info.pieces")?;
        if pieces.len() % 20 != 0 {
            return Err(MetainfoError::InvalidField(
                "info.pieces hash string length",
            ));
        }
        let actual_piece_count = pieces.len() / 20;
        if actual_piece_count > limits.max_pieces {
            return Err(MetainfoError::TooManyPieces {
                actual: actual_piece_count,
                maximum: limits.max_pieces,
            });
        }
        if actual_piece_count != expected_piece_count {
            return Err(MetainfoError::PieceCountMismatch {
                expected: expected_piece_count,
                actual: actual_piece_count,
            });
        }
        let piece_hashes = pieces
            .chunks_exact(20)
            .map(|hash| {
                hash.try_into()
                    .expect("piece hash chunk is exactly 20 bytes")
            })
            .collect();

        let info_hash = Sha1::digest(&bytes[info_node.span.clone()]).into();
        Ok(Self {
            info_hash,
            piece_hashes,
            piece_length,
            total_length,
            name,
            private,
            mode,
            files,
        })
    }

    pub fn piece_count(&self) -> usize {
        self.piece_hashes.len()
    }

    pub fn piece_length_at(&self, index: u32) -> Option<u32> {
        let piece_index = usize::try_from(index).ok()?;
        if piece_index >= self.piece_count() {
            return None;
        }
        let begin = u64::from(index).checked_mul(u64::from(self.piece_length))?;
        let remaining = self.total_length.checked_sub(begin)?;
        u32::try_from(remaining.min(u64::from(self.piece_length))).ok()
    }
}

fn enforce_info_length(length: usize, limits: MetainfoLimits) -> Result<(), MetainfoError> {
    if length > limits.max_info_bytes {
        return Err(MetainfoError::InfoTooLarge {
            length,
            maximum: limits.max_info_bytes,
        });
    }
    Ok(())
}

fn parse_metainfo(
    bytes: &[u8],
    max_input_length: usize,
    limits: MetainfoLimits,
) -> Result<Node<'_>, ParseError> {
    parse_with_limits(
        bytes,
        Limits {
            max_input_length,
            max_string_length: limits.max_string_bytes,
            max_decoded_items: limits.max_decoded_items,
            max_depth: limits.max_depth,
            max_collection_entries: limits.max_collection_entries,
        },
    )
}

fn parse_multi_files(
    node: &Node<'_>,
    limits: MetainfoLimits,
) -> Result<(MetainfoMode, Vec<MetainfoFile>, u64), MetainfoError> {
    let Value::List(entries) = &node.value else {
        return Err(MetainfoError::InvalidField("info.files"));
    };
    if entries.is_empty() {
        return Err(MetainfoError::InvalidField("info.files"));
    }
    if entries.len() > limits.max_files {
        return Err(MetainfoError::TooManyFiles {
            actual: entries.len(),
            maximum: limits.max_files,
        });
    }

    let mut files = Vec::with_capacity(entries.len());
    let mut offset = 0_u64;
    for (index, entry) in entries.iter().enumerate() {
        let fields = dictionary(entry).ok_or(MetainfoError::InvalidField("info.files entry"))?;
        let attributes =
            optional_bytes_field(fields, b"attr", "info.files.attr")?.unwrap_or_default();
        if attributes.contains(&b'l') {
            return Err(MetainfoError::Unsupported("BEP 47 symlink file"));
        }
        let padding = attributes.contains(&b'p');
        let length_node =
            field(fields, b"length").ok_or(MetainfoError::MissingField("info.files.length"))?;
        let length = node_nonnegative_integer(length_node, "info.files.length")?;
        let path = match field(fields, b"path") {
            Some(path) => parse_path(path, index, limits)?,
            None if padding => Vec::new(),
            None => return Err(MetainfoError::MissingField("info.files.path")),
        };
        if path.is_empty() && !padding {
            return Err(MetainfoError::UnsafePath {
                file: Some(index),
                reason: "path has no components",
            });
        }

        files.push(MetainfoFile {
            path,
            length,
            offset,
            padding,
        });
        offset = offset
            .checked_add(length)
            .ok_or(MetainfoError::TotalLengthOverflow)?;
    }
    validate_path_collisions(&files)?;
    Ok((MetainfoMode::MultiFile, files, offset))
}

fn parse_path(
    node: &Node<'_>,
    file: usize,
    limits: MetainfoLimits,
) -> Result<Vec<String>, MetainfoError> {
    let Value::List(components) = &node.value else {
        return Err(MetainfoError::InvalidField("info.files.path"));
    };
    if components.is_empty() {
        return Err(MetainfoError::UnsafePath {
            file: Some(file),
            reason: "path has no components",
        });
    }
    if components.len() > limits.max_path_components {
        return Err(MetainfoError::UnsafePath {
            file: Some(file),
            reason: "path has too many components",
        });
    }

    let mut path = Vec::with_capacity(components.len());
    let mut encoded_length = 0_usize;
    for component in components {
        let Value::Bytes(bytes) = component.value else {
            return Err(MetainfoError::InvalidField("info.files.path component"));
        };
        let component = safe_component(bytes, Some(file), limits)?;
        encoded_length = encoded_length
            .checked_add(component.len() + usize::from(!path.is_empty()))
            .ok_or(MetainfoError::UnsafePath {
                file: Some(file),
                reason: "path length overflows",
            })?;
        if encoded_length > limits.max_path_bytes {
            return Err(MetainfoError::UnsafePath {
                file: Some(file),
                reason: "path is too long",
            });
        }
        path.push(component);
    }
    Ok(path)
}

fn safe_component(
    bytes: &[u8],
    file: Option<usize>,
    limits: MetainfoLimits,
) -> Result<String, MetainfoError> {
    let component = std::str::from_utf8(bytes).map_err(|_| MetainfoError::UnsafePath {
        file,
        reason: "component is not UTF-8",
    })?;
    let unsafe_reason = if component.is_empty() {
        Some("component is empty")
    } else if component.len() > limits.max_path_component_bytes {
        Some("component is too long")
    } else if matches!(component, "." | "..") {
        Some("component is dot or dot-dot")
    } else if component
        .bytes()
        .any(|byte| matches!(byte, 0 | b'/' | b'\\' | b':'))
    {
        Some("component contains a reserved separator or prefix character")
    } else {
        None
    };
    if let Some(reason) = unsafe_reason {
        return Err(MetainfoError::UnsafePath { file, reason });
    }
    Ok(component.to_owned())
}

fn validate_path_collisions(files: &[MetainfoFile]) -> Result<(), MetainfoError> {
    let mut paths: Vec<_> = files
        .iter()
        .enumerate()
        .filter(|(_, file)| !file.path.is_empty())
        .map(|(index, file)| (index, &file.path))
        .collect();
    paths.sort_by(|left, right| left.1.cmp(right.1));
    for pair in paths.windows(2) {
        let (first_index, first) = pair[0];
        let (second_index, second) = pair[1];
        if second.starts_with(first) {
            return Err(MetainfoError::PathCollision {
                first: first_index,
                second: second_index,
            });
        }
    }
    Ok(())
}

fn dictionary<'node, 'input>(
    node: &'node Node<'input>,
) -> Option<&'node [DictionaryEntry<'input>]> {
    match &node.value {
        Value::Dictionary(entries) => Some(entries),
        _ => None,
    }
}

fn field<'node, 'input>(
    entries: &'node [DictionaryEntry<'input>],
    key: &[u8],
) -> Option<&'node Node<'input>> {
    entries
        .binary_search_by_key(&key, |entry| entry.key)
        .ok()
        .map(|index| &entries[index].value)
}

fn positive_integer(
    entries: &[DictionaryEntry<'_>],
    key: &[u8],
    field_name: &'static str,
) -> Result<u64, MetainfoError> {
    let node = field(entries, key).ok_or(MetainfoError::MissingField(field_name))?;
    let value = node_nonnegative_integer(node, field_name)?;
    if value == 0 {
        return Err(MetainfoError::InvalidField(field_name));
    }
    Ok(value)
}

fn node_nonnegative_integer(
    node: &Node<'_>,
    field_name: &'static str,
) -> Result<u64, MetainfoError> {
    let Value::Integer(value) = node.value else {
        return Err(MetainfoError::InvalidField(field_name));
    };
    u64::try_from(value).map_err(|_| MetainfoError::InvalidField(field_name))
}

fn bytes_field<'input>(
    entries: &[DictionaryEntry<'input>],
    key: &[u8],
    field_name: &'static str,
) -> Result<&'input [u8], MetainfoError> {
    optional_bytes_field(entries, key, field_name)?.ok_or(MetainfoError::MissingField(field_name))
}

fn optional_bytes_field<'input>(
    entries: &[DictionaryEntry<'input>],
    key: &[u8],
    field_name: &'static str,
) -> Result<Option<&'input [u8]>, MetainfoError> {
    let Some(node) = field(entries, key) else {
        return Ok(None);
    };
    match node.value {
        Value::Bytes(bytes) => Ok(Some(bytes)),
        _ => Err(MetainfoError::InvalidField(field_name)),
    }
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::{
        BEP9_METAINFO_LIMITS, EXPLICIT_IMPORT_METAINFO_LIMITS, MAX_PIECE_LENGTH, Metainfo,
        MetainfoError, MetainfoMode,
    };

    fn parse(bytes: &[u8]) -> Result<Metainfo, MetainfoError> {
        Metainfo::from_bytes_with_limits(bytes, BEP9_METAINFO_LIMITS)
    }

    fn parse_info(bytes: &[u8]) -> Result<Metainfo, MetainfoError> {
        Metainfo::from_info_bytes_with_limits(bytes, BEP9_METAINFO_LIMITS)
    }

    fn extract_info(bytes: &[u8]) -> Result<&[u8], MetainfoError> {
        Metainfo::info_bytes_with_limits(bytes, BEP9_METAINFO_LIMITS)
    }

    fn single_metainfo(file_length: u64, piece_length: u32, hashes: &[[u8; 20]]) -> Vec<u8> {
        let mut bytes = format!(
            "d4:infod6:lengthi{file_length}e4:name1:x12:piece lengthi{piece_length}e6:pieces{}:",
            hashes.len() * 20
        )
        .into_bytes();
        for hash in hashes {
            bytes.extend_from_slice(hash);
        }
        bytes.extend_from_slice(b"ee");
        bytes
    }

    fn multi_metainfo(files: &[u8], total_hashes: &[[u8; 20]], piece_length: u32) -> Vec<u8> {
        let mut bytes = b"d4:infod5:filesl".to_vec();
        bytes.extend_from_slice(files);
        bytes.extend_from_slice(
            format!(
                "e4:name4:root12:piece lengthi{piece_length}e6:pieces{}:",
                total_hashes.len() * 20
            )
            .as_bytes(),
        );
        for hash in total_hashes {
            bytes.extend_from_slice(hash);
        }
        bytes.extend_from_slice(b"ee");
        bytes
    }

    #[test]
    fn parses_single_file_and_hashes_exact_info_span() {
        let mut bytes =
            b"d7:comment7:ignored4:infod6:lengthi3e4:name1:x12:piece lengthi4e6:pieces20:".to_vec();
        bytes.extend_from_slice(&[7; 20]);
        bytes.extend_from_slice(b"ee");
        let info_start = b"d7:comment7:ignored4:info".len();
        let info_end = bytes.len() - 1;
        let expected_info_hash: [u8; 20] = Sha1::digest(&bytes[info_start..info_end]).into();

        let metainfo = parse(&bytes).expect("valid metainfo");

        assert_eq!(metainfo.info_hash, expected_info_hash);
        assert_eq!(metainfo.piece_hashes, vec![[7; 20]]);
        assert_eq!(metainfo.piece_length, 4);
        assert_eq!(metainfo.total_length, 3);
        assert_eq!(metainfo.name, "x");
        assert_eq!(metainfo.mode, MetainfoMode::SingleFile);
        assert_eq!(metainfo.files[0].path, ["x"]);
        assert_eq!(metainfo.piece_length_at(0), Some(3));
        assert_eq!(metainfo.piece_length_at(1), None);

        let raw_info = extract_info(&bytes).expect("extract info dictionary");
        assert_eq!(raw_info, &bytes[info_start..info_end]);
        assert_eq!(
            parse_info(raw_info).expect("parse raw info dictionary"),
            metainfo
        );
    }

    #[test]
    fn retains_only_the_normative_private_flag_values() {
        let mut private = single_metainfo(1, 4, &[[1; 20]]);
        private.splice(
            private.len() - 2..private.len() - 2,
            b"7:privatei1e".iter().copied(),
        );
        assert!(parse(&private).expect("private torrent").private);

        let public = single_metainfo(1, 4, &[[1; 20]]);
        assert!(!parse(&public).expect("public torrent").private);

        for value in [b"i2e".as_slice(), b"1:x".as_slice()] {
            let mut invalid = single_metainfo(1, 4, &[[1; 20]]);
            let mut field = b"7:private".to_vec();
            field.extend_from_slice(value);
            invalid.splice(invalid.len() - 2..invalid.len() - 2, field);
            assert_eq!(
                parse(&invalid),
                Err(MetainfoError::InvalidField("info.private"))
            );
        }
    }

    #[test]
    fn parses_offsets_zero_length_and_padding_without_a_path() {
        let files = concat!(
            "d6:lengthi2e4:pathl1:aee",
            "d6:lengthi0e4:pathl1:zee",
            "d4:attr1:p6:lengthi2ee"
        );
        let metainfo = parse(&multi_metainfo(files.as_bytes(), &[[3; 20]], 4))
            .expect("valid multi-file metainfo");

        assert_eq!(metainfo.mode, MetainfoMode::MultiFile);
        assert_eq!(metainfo.total_length, 4);
        assert_eq!(metainfo.files.len(), 3);
        assert_eq!(metainfo.files[0].offset, 0);
        assert_eq!(metainfo.files[1].offset, 2);
        assert_eq!(metainfo.files[2].offset, 2);
        assert!(metainfo.files[2].padding);
        assert!(metainfo.files[2].path.is_empty());
    }

    #[test]
    fn validates_exact_piece_hash_count_and_final_piece_length() {
        let metainfo =
            parse(&single_metainfo(5, 4, &[[1; 20], [2; 20]])).expect("two-piece metainfo");
        assert_eq!(metainfo.piece_length_at(0), Some(4));
        assert_eq!(metainfo.piece_length_at(1), Some(1));

        assert_eq!(
            parse(&single_metainfo(5, 4, &[[1; 20]])),
            Err(MetainfoError::PieceCountMismatch {
                expected: 2,
                actual: 1,
            })
        );
        let mut malformed = single_metainfo(1, 4, &[]);
        let pieces_offset = malformed
            .windows(b"6:pieces0:".len())
            .position(|window| window == b"6:pieces0:")
            .expect("pieces field");
        malformed.splice(
            pieces_offset..pieces_offset + b"6:pieces0:".len(),
            b"6:pieces1:x".iter().copied(),
        );
        assert_eq!(
            parse(&malformed),
            Err(MetainfoError::InvalidField(
                "info.pieces hash string length"
            ))
        );
    }

    #[test]
    fn rejects_unsafe_colliding_and_symlink_paths() {
        for path in ["l0:e", "l1:.e", "l2:..e", "l3:a/be", "l3:a\\be", "l2:C:e"] {
            let file = format!("d6:lengthi1e4:path{path}e");
            assert!(matches!(
                parse(&multi_metainfo(file.as_bytes(), &[[1; 20]], 4)),
                Err(MetainfoError::UnsafePath { .. })
            ));
        }

        let duplicate = concat!("d6:lengthi1e4:pathl1:aee", "d6:lengthi1e4:pathl1:aee");
        assert!(matches!(
            parse(&multi_metainfo(duplicate.as_bytes(), &[[1; 20]], 4)),
            Err(MetainfoError::PathCollision { .. })
        ));

        let prefix = concat!("d6:lengthi1e4:pathl1:aee", "d6:lengthi1e4:pathl1:a1:bee");
        assert!(matches!(
            parse(&multi_metainfo(prefix.as_bytes(), &[[1; 20]], 4)),
            Err(MetainfoError::PathCollision { .. })
        ));

        let symlink = b"d4:attr1:l6:lengthi0e4:pathl1:aee";
        assert_eq!(
            parse(&multi_metainfo(symlink, &[[1; 20]], 4)),
            Err(MetainfoError::Unsupported("BEP 47 symlink file"))
        );
    }

    #[test]
    fn rejects_invalid_modes_lengths_and_fields() {
        let v2 = b"d4:infod6:lengthi1e12:meta versioni2e4:name1:x12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            parse(v2),
            Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"))
        );

        let empty = b"d4:infod5:filesle4:name4:root12:piece lengthi4e6:pieces0:ee";
        assert_eq!(parse(empty), Err(MetainfoError::InvalidField("info.files")));

        let missing_path = b"d6:lengthi1ee";
        assert_eq!(
            parse(&multi_metainfo(missing_path, &[[1; 20]], 4)),
            Err(MetainfoError::MissingField("info.files.path"))
        );

        let both = b"d4:infod5:filesld6:lengthi1e4:pathl1:aeee6:lengthi1e4:name4:root12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            parse(both),
            Err(MetainfoError::InvalidField(
                "info must contain exactly one of length or files"
            ))
        );
    }

    #[test]
    fn accepts_256_mib_piece_bound_and_rejects_larger_value() {
        let accepted = parse(&single_metainfo(
            u64::from(MAX_PIECE_LENGTH),
            MAX_PIECE_LENGTH,
            &[[3; 20]],
        ))
        .expect("accepted maximum piece");
        assert_eq!(accepted.total_length, u64::from(MAX_PIECE_LENGTH));

        assert_eq!(
            parse(&single_metainfo(1, MAX_PIECE_LENGTH + 1, &[[3; 20]])),
            Err(MetainfoError::InvalidField("info.piece length"))
        );
    }

    #[test]
    fn accepts_ten_gibibytes_with_256_kib_pieces() {
        const TOTAL_LENGTH: u64 = 10 * 1024 * 1024 * 1024;
        const PIECE_LENGTH: u32 = 256 * 1024;
        let piece_count = usize::try_from(TOTAL_LENGTH / u64::from(PIECE_LENGTH))
            .expect("piece count fits usize");
        let hashes = vec![[7; 20]; piece_count];

        let metainfo = parse(&single_metainfo(TOTAL_LENGTH, PIECE_LENGTH, &hashes))
            .expect("ordinary large metainfo");

        assert_eq!(metainfo.total_length, TOTAL_LENGTH);
        assert_eq!(metainfo.piece_count(), 40_960);
    }

    #[test]
    fn explicit_profile_accepts_info_larger_than_peer_metadata() {
        let unknown_length = 1024 * 1024;
        let mut info = format!("d1:a{unknown_length}:").into_bytes();
        info.resize(info.len() + unknown_length, b'x');
        info.extend_from_slice(
            b"6:lengthi1e4:name1:x12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaae",
        );
        let expected_hash: [u8; 20] = Sha1::digest(&info).into();

        assert_eq!(
            Metainfo::from_info_bytes_with_limits(&info, BEP9_METAINFO_LIMITS),
            Err(MetainfoError::InfoTooLarge {
                length: info.len(),
                maximum: BEP9_METAINFO_LIMITS.max_info_bytes,
            })
        );
        let parsed = Metainfo::from_info_bytes_with_limits(&info, EXPLICIT_IMPORT_METAINFO_LIMITS)
            .expect("explicit import profile");
        assert_eq!(parsed.info_hash, expected_hash);
    }

    #[test]
    fn independently_enforces_metainfo_structure_limits() {
        let minimal = single_metainfo(1, 1, &[[1; 20]]);
        let minimal_info = extract_info(&minimal).expect("minimal info");

        let outer_limit = BEP9_METAINFO_LIMITS.max_outer_bytes;
        let limits = super::MetainfoLimits {
            max_outer_bytes: minimal.len() - 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_bytes_with_limits(&minimal, limits),
            Err(MetainfoError::Bencode(super::ParseError::InputTooLarge {
                length,
                maximum
            })) if length == minimal.len() && maximum == minimal.len() - 1
        ));
        assert_eq!(outer_limit, 1024 * 1024);

        let limits = super::MetainfoLimits {
            max_info_bytes: minimal_info.len() - 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert_eq!(
            Metainfo::from_info_bytes_with_limits(minimal_info, limits),
            Err(MetainfoError::InfoTooLarge {
                length: minimal_info.len(),
                maximum: minimal_info.len() - 1,
            })
        );

        let limits = super::MetainfoLimits {
            max_string_bytes: 5,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_info_bytes_with_limits(minimal_info, limits),
            Err(MetainfoError::Bencode(super::ParseError::StringTooLarge {
                maximum: 5,
                ..
            }))
        ));

        let limits = super::MetainfoLimits {
            max_decoded_items: 8,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_info_bytes_with_limits(minimal_info, limits),
            Err(MetainfoError::Bencode(
                super::ParseError::TooManyDecodedItems { maximum: 8, .. }
            ))
        ));

        let nested =
            b"d1:ali1ee6:lengthi1e4:name1:x12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaae";
        let limits = super::MetainfoLimits {
            max_depth: 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_info_bytes_with_limits(nested, limits),
            Err(MetainfoError::Bencode(super::ParseError::NestingTooDeep {
                maximum: 1,
                ..
            }))
        ));

        let limits = super::MetainfoLimits {
            max_collection_entries: 3,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_info_bytes_with_limits(minimal_info, limits),
            Err(MetainfoError::Bencode(
                super::ParseError::CollectionTooLarge { maximum: 3, .. }
            ))
        ));
    }

    #[test]
    fn independently_enforces_file_piece_and_path_limits() {
        let two_files = concat!("d6:lengthi1e4:pathl1:aee", "d6:lengthi1e4:pathl1:bee");
        let limits = super::MetainfoLimits {
            max_files: 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert_eq!(
            Metainfo::from_bytes_with_limits(
                &multi_metainfo(two_files.as_bytes(), &[[1; 20], [2; 20]], 1),
                limits,
            ),
            Err(MetainfoError::TooManyFiles {
                actual: 2,
                maximum: 1,
            })
        );

        let limits = super::MetainfoLimits {
            max_pieces: 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert_eq!(
            Metainfo::from_bytes_with_limits(&single_metainfo(2, 1, &[[1; 20], [2; 20]]), limits,),
            Err(MetainfoError::TooManyPieces {
                actual: 2,
                maximum: 1,
            })
        );
        assert_eq!(
            Metainfo::from_bytes_with_limits(&single_metainfo(1, 1, &[[1; 20], [2; 20]]), limits,),
            Err(MetainfoError::TooManyPieces {
                actual: 2,
                maximum: 1,
            })
        );

        let two_components = b"d6:lengthi1e4:pathl1:a1:bee";
        let limits = super::MetainfoLimits {
            max_path_components: 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_bytes_with_limits(
                &multi_metainfo(two_components, &[[1; 20]], 1),
                limits,
            ),
            Err(MetainfoError::UnsafePath {
                reason: "path has too many components",
                ..
            })
        ));

        let long_component = b"d6:lengthi1e4:pathl2:aaee";
        let limits = super::MetainfoLimits {
            max_path_component_bytes: 1,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_bytes_with_limits(
                &multi_metainfo(long_component, &[[1; 20]], 1),
                limits,
            ),
            Err(MetainfoError::UnsafePath {
                reason: "component is too long",
                ..
            })
        ));

        let limits = super::MetainfoLimits {
            max_path_bytes: 2,
            ..BEP9_METAINFO_LIMITS
        };
        assert!(matches!(
            Metainfo::from_bytes_with_limits(
                &multi_metainfo(two_components, &[[1; 20]], 1),
                limits,
            ),
            Err(MetainfoError::UnsafePath {
                reason: "path is too long",
                ..
            })
        ));
    }
}

use std::error::Error;
use std::fmt;

use sha1::{Digest, Sha1};

use crate::bencode::{DictionaryEntry, Node, ParseError, Value, parse};

pub const MAX_PIECE_LENGTH: u32 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metainfo {
    pub info_hash: [u8; 20],
    pub piece_hash: [u8; 20],
    pub piece_length: u32,
    pub file_length: u64,
    pub name: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetainfoError {
    Bencode(ParseError),
    RootIsNotDictionary,
    MissingField(&'static str),
    InvalidField(&'static str),
    Unsupported(&'static str),
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
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MetainfoError> {
        let root = parse(bytes)?;
        let root_entries = dictionary(&root).ok_or(MetainfoError::RootIsNotDictionary)?;
        let info_node = field(root_entries, b"info").ok_or(MetainfoError::MissingField("info"))?;
        let info_entries =
            dictionary(info_node).ok_or(MetainfoError::InvalidField("info dictionary"))?;

        if field(info_entries, b"meta version").is_some() {
            return Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"));
        }
        if field(info_entries, b"files").is_some() {
            return Err(MetainfoError::Unsupported("multi-file info dictionary"));
        }

        let file_length = positive_integer(info_entries, b"length", "info.length")?;
        let piece_length = positive_integer(info_entries, b"piece length", "info.piece length")?;
        let piece_length = u32::try_from(piece_length)
            .map_err(|_| MetainfoError::InvalidField("info.piece length"))?;
        if piece_length > MAX_PIECE_LENGTH {
            return Err(MetainfoError::InvalidField("info.piece length"));
        }
        if file_length > u64::from(piece_length) {
            return Err(MetainfoError::Unsupported(
                "more than one piece in the controlled fixture",
            ));
        }

        let name = bytes_field(info_entries, b"name", "info.name")?;
        if name.is_empty() {
            return Err(MetainfoError::InvalidField("info.name"));
        }

        let pieces = bytes_field(info_entries, b"pieces", "info.pieces")?;
        let piece_hash: [u8; 20] = pieces
            .try_into()
            .map_err(|_| MetainfoError::InvalidField("single info.pieces hash"))?;

        let info_hash = Sha1::digest(&bytes[info_node.span.clone()]).into();
        Ok(Self {
            info_hash,
            piece_hash,
            piece_length,
            file_length,
            name: name.to_vec(),
        })
    }
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
    let Value::Integer(value) = node.value else {
        return Err(MetainfoError::InvalidField(field_name));
    };
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(MetainfoError::InvalidField(field_name))
}

fn bytes_field<'input>(
    entries: &[DictionaryEntry<'input>],
    key: &[u8],
    field_name: &'static str,
) -> Result<&'input [u8], MetainfoError> {
    let node = field(entries, key).ok_or(MetainfoError::MissingField(field_name))?;
    match node.value {
        Value::Bytes(bytes) => Ok(bytes),
        _ => Err(MetainfoError::InvalidField(field_name)),
    }
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::{MAX_PIECE_LENGTH, Metainfo, MetainfoError};

    fn metainfo_bytes(piece_hash: [u8; 20]) -> Vec<u8> {
        let mut bytes =
            b"d7:comment7:ignored4:infod6:lengthi3e4:name1:x12:piece lengthi4e6:pieces20:".to_vec();
        bytes.extend_from_slice(&piece_hash);
        bytes.extend_from_slice(b"ee");
        bytes
    }

    fn metainfo_with_lengths(file_length: u32, piece_length: u32) -> Vec<u8> {
        let mut bytes = format!(
            "d4:infod6:lengthi{file_length}e4:name1:x12:piece lengthi{piece_length}e6:pieces20:"
        )
        .into_bytes();
        bytes.extend_from_slice(&[3; 20]);
        bytes.extend_from_slice(b"ee");
        bytes
    }

    #[test]
    fn hashes_exact_original_info_dictionary_span() {
        let bytes = metainfo_bytes([7; 20]);
        let info_start = b"d7:comment7:ignored4:info".len();
        let info_end = bytes.len() - 1;
        let expected_info_hash: [u8; 20] = Sha1::digest(&bytes[info_start..info_end]).into();

        let metainfo = Metainfo::from_bytes(&bytes).expect("valid metainfo");

        assert_eq!(metainfo.info_hash, expected_info_hash);
        assert_eq!(metainfo.piece_hash, [7; 20]);
        assert_eq!(metainfo.piece_length, 4);
        assert_eq!(metainfo.file_length, 3);
        assert_eq!(metainfo.name, b"x");
    }

    #[test]
    fn root_fields_do_not_change_info_hash() {
        let with_comment = metainfo_bytes([9; 20]);
        let mut without_comment =
            b"d4:infod6:lengthi3e4:name1:x12:piece lengthi4e6:pieces20:".to_vec();
        without_comment.extend_from_slice(&[9; 20]);
        without_comment.extend_from_slice(b"ee");

        assert_eq!(
            Metainfo::from_bytes(&with_comment)
                .expect("commented metainfo")
                .info_hash,
            Metainfo::from_bytes(&without_comment)
                .expect("plain metainfo")
                .info_hash
        );
    }

    #[test]
    fn rejects_multi_file_v2_and_multiple_piece_inputs() {
        let multi_file = b"d4:infod5:filesle6:lengthi1e4:name1:x12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            Metainfo::from_bytes(multi_file),
            Err(MetainfoError::Unsupported("multi-file info dictionary"))
        );

        let v2 = b"d4:infod6:lengthi1e12:meta versioni2e4:name1:x12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            Metainfo::from_bytes(v2),
            Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"))
        );

        let two_pieces = b"d4:infod6:lengthi5e4:name1:x12:piece lengthi4e6:pieces40:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            Metainfo::from_bytes(two_pieces),
            Err(MetainfoError::Unsupported(
                "more than one piece in the controlled fixture"
            ))
        );
    }

    #[test]
    fn rejects_missing_or_invalid_required_fields() {
        assert!(matches!(
            Metainfo::from_bytes(b"d4:infodee"),
            Err(MetainfoError::MissingField("info.length"))
        ));

        let zero_length =
            b"d4:infod6:lengthi0e4:name1:x12:piece lengthi4e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            Metainfo::from_bytes(zero_length),
            Err(MetainfoError::InvalidField("info.length"))
        );

        let bad_piece_hash =
            b"d4:infod6:lengthi1e4:name1:x12:piece lengthi4e6:pieces19:aaaaaaaaaaaaaaaaaaaee";
        assert_eq!(
            Metainfo::from_bytes(bad_piece_hash),
            Err(MetainfoError::InvalidField("single info.pieces hash"))
        );
    }

    #[test]
    fn accepts_256_mib_piece_bound_and_rejects_larger_value() {
        let accepted =
            Metainfo::from_bytes(&metainfo_with_lengths(MAX_PIECE_LENGTH, MAX_PIECE_LENGTH))
                .expect("accepted maximum piece");
        assert_eq!(accepted.file_length, u64::from(MAX_PIECE_LENGTH));
        assert_eq!(accepted.piece_length, MAX_PIECE_LENGTH);

        assert_eq!(
            Metainfo::from_bytes(&metainfo_with_lengths(1, MAX_PIECE_LENGTH + 1)),
            Err(MetainfoError::InvalidField("info.piece length"))
        );
    }
}

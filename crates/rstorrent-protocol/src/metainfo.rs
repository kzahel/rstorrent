use std::error::Error;
use std::fmt;

use crate::bencode::ParseError;

mod direct;

pub const MAX_PIECE_LENGTH: u32 = 536_854_528;
pub const MAX_METAINFO_FILES: usize = 374_998;
pub const MAX_PEER_METAINFO_FILES: usize = 312_498;
pub const MAX_METAINFO_PIECES: usize = 2_097_152;
pub const MAX_METAINFO_PATH_COMPONENTS: usize = 3_000_000;
pub const MAX_METAINFO_PATH_COMPONENT_LENGTH: usize = 240;
pub const MAX_METAINFO_PATH_LENGTH: usize = 4096;

pub const MAX_PEER_METAINFO_LENGTH: usize = 30 * 1024 * 1024;
pub const MAX_EXPLICIT_METAINFO_LENGTH: usize = 64 * 1024 * 1024;
pub const MAX_PEER_METAINFO_TOKENS: usize = 2_500_000;
pub const MAX_EXPLICIT_METAINFO_TOKENS: usize = 3_000_000;

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

const fn metainfo_limits(
    max_bytes: usize,
    max_decoded_items: usize,
    max_depth: usize,
    max_files: usize,
) -> MetainfoLimits {
    MetainfoLimits {
        max_outer_bytes: max_bytes,
        max_info_bytes: max_bytes,
        max_string_bytes: max_bytes,
        max_decoded_items,
        max_depth,
        max_collection_entries: max_decoded_items,
        max_files,
        max_pieces: MAX_METAINFO_PIECES,
        max_path_components: max_decoded_items,
        max_path_component_bytes: max_bytes,
        max_path_bytes: max_bytes,
    }
}

/// The current peer-metadata parsing policy. BEP 9 supplies only `info`, so
/// `max_outer_bytes` is a defensive value rather than a transport contract.
pub const BEP9_METAINFO_LIMITS: MetainfoLimits = metainfo_limits(
    MAX_PEER_METAINFO_LENGTH,
    MAX_PEER_METAINFO_TOKENS,
    200,
    MAX_PEER_METAINFO_FILES,
);

/// The parser policy for persisted `raw_info` bytes.
pub const DURABLE_METAINFO_LIMITS: MetainfoLimits = metainfo_limits(
    MAX_EXPLICIT_METAINFO_LENGTH,
    MAX_EXPLICIT_METAINFO_TOKENS,
    200,
    MAX_METAINFO_FILES,
);

/// Parser-only headroom for a future explicit local or authenticated import.
pub const EXPLICIT_IMPORT_METAINFO_LIMITS: MetainfoLimits = metainfo_limits(
    MAX_EXPLICIT_METAINFO_LENGTH,
    MAX_EXPLICIT_METAINFO_TOKENS,
    100,
    MAX_METAINFO_FILES,
);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetainfoTrackerTransport {
    Udp,
    Http,
    Https,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetainfoTracker {
    pub tier: u32,
    pub position: u32,
    pub url: String,
    pub transport: MetainfoTrackerTransport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetainfoProjection {
    pub metainfo: Metainfo,
    pub info_span: std::ops::Range<usize>,
    pub trackers: Vec<MetainfoTracker>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetainfoError {
    Bencode(ParseError),
    RootIsNotDictionary,
    MissingField(&'static str),
    InvalidField(&'static str),
    Unsupported(&'static str),
    UnsupportedVersion {
        version: i64,
    },
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
            Self::UnsupportedVersion { version } => {
                write!(formatter, "metainfo version {version} is not supported")
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
    pub fn project_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<MetainfoProjection, MetainfoError> {
        let parsed = direct::parse_outer(bytes, limits)?;
        Ok(MetainfoProjection {
            metainfo: parsed.metainfo,
            info_span: parsed.info_span,
            trackers: parsed.trackers,
        })
    }

    pub fn from_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        Ok(Self::project_bytes_with_limits(bytes, limits)?.metainfo)
    }

    pub fn from_info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<Self, MetainfoError> {
        direct::parse_info(bytes, limits)
    }

    pub fn info_bytes_with_limits(
        bytes: &[u8],
        limits: MetainfoLimits,
    ) -> Result<&[u8], MetainfoError> {
        let parsed = direct::parse_outer(bytes, limits)?;
        Ok(&bytes[parsed.info_span])
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

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::{
        BEP9_METAINFO_LIMITS, EXPLICIT_IMPORT_METAINFO_LIMITS, MAX_PIECE_LENGTH, Metainfo,
        MetainfoError, MetainfoMode, MetainfoTrackerTransport,
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

    fn tracker_metainfo(announce: Option<&str>, tiers: &[&[&str]]) -> Vec<u8> {
        let outer = single_metainfo(1, 1, &[[1; 20]]);
        let info = &outer[b"d4:info".len()..outer.len() - 1];
        let mut bytes = b"d".to_vec();
        if let Some(announce) = announce {
            bytes.extend_from_slice(format!("8:announce{}:{announce}", announce.len()).as_bytes());
        }
        if !tiers.is_empty() {
            bytes.extend_from_slice(b"13:announce-listl");
            for tier in tiers {
                bytes.push(b'l');
                for tracker in *tier {
                    bytes.extend_from_slice(format!("{}:{tracker}", tracker.len()).as_bytes());
                }
                bytes.push(b'e');
            }
            bytes.push(b'e');
        }
        bytes.extend_from_slice(b"4:info");
        bytes.extend_from_slice(info);
        bytes.push(b'e');
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
    fn projects_full_tracker_tiers_and_uses_announce_only_as_fallback() {
        let source = tracker_metainfo(
            Some("udp://fallback.example:80/announce"),
            &[
                &["not-a-tracker"],
                &[
                    "UDP://Tracker.Example:80/passkey",
                    "http://tracker.example/announce?key=secret",
                ],
                &[
                    "udp://tracker.example:80/passkey",
                    "https://secure.example/announce",
                ],
            ],
        );
        let projection =
            Metainfo::project_bytes_with_limits(&source, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("valid tracker catalog");

        assert_eq!(projection.trackers.len(), 3);
        assert_eq!(projection.trackers[0].tier, 0);
        assert_eq!(projection.trackers[0].position, 0);
        assert_eq!(
            &*projection.trackers[0].url,
            "udp://tracker.example:80/passkey"
        );
        assert_eq!(
            projection.trackers[0].transport,
            MetainfoTrackerTransport::Udp
        );
        assert_eq!(projection.trackers[1].tier, 0);
        assert_eq!(
            projection.trackers[1].transport,
            MetainfoTrackerTransport::Http
        );
        assert_eq!(projection.trackers[2].tier, 1);
        assert_eq!(
            projection.trackers[2].transport,
            MetainfoTrackerTransport::Https
        );

        let fallback = tracker_metainfo(
            Some("udp://fallback.example:80/passkey"),
            &[&["ws://unsupported.example/announce"]],
        );
        let fallback =
            Metainfo::project_bytes_with_limits(&fallback, EXPLICIT_IMPORT_METAINFO_LIMITS)
                .expect("fallback announce");
        assert_eq!(fallback.trackers.len(), 1);
        assert_eq!(
            &*fallback.trackers[0].url,
            "udp://fallback.example:80/passkey"
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
    fn projects_unsafe_and_colliding_paths_but_rejects_symlinks() {
        for path in ["l0:e", "l1:.e", "l2:..e", "l3:a/be", "l3:a\\be", "l2:C:e"] {
            let file = format!("d6:lengthi1e4:path{path}e");
            let parsed = parse(&multi_metainfo(file.as_bytes(), &[[1; 20]], 4))
                .expect("unsafe source component is projected");
            assert!(!parsed.files[0].path[0].is_empty());
            assert!(parsed.files[0].path[0].len() <= 240);
            assert!(!parsed.files[0].path[0].contains(['/', '\\', ':']));
        }

        let duplicate = concat!("d6:lengthi1e4:pathl1:aee", "d6:lengthi1e4:pathl1:aee");
        let duplicate = parse(&multi_metainfo(duplicate.as_bytes(), &[[1; 20]], 4))
            .expect("duplicate source paths receive stable suffixes");
        assert_ne!(duplicate.files[0].path, duplicate.files[1].path);

        let prefix = concat!("d6:lengthi1e4:pathl1:aee", "d6:lengthi1e4:pathl1:a1:bee");
        let prefix = parse(&multi_metainfo(prefix.as_bytes(), &[[1; 20]], 4))
            .expect("file/directory source conflict receives a stable suffix");
        assert!(!prefix.files[1].path.starts_with(&prefix.files[0].path));

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

        let future = b"d4:infod9:file treede12:meta versioni3eee";
        assert_eq!(
            parse(future),
            Err(MetainfoError::UnsupportedVersion { version: 3 })
        );

        let missing_version = b"d4:infod9:file treedeee";
        assert_eq!(
            parse(missing_version),
            Err(MetainfoError::InvalidField("info.meta version"))
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
    fn accepts_libtorrent_piece_bound_and_rejects_larger_value() {
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
        let unknown_length = super::MAX_PEER_METAINFO_LENGTH;
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
        assert_eq!(outer_limit, 30 * 1024 * 1024);

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

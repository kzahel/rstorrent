use std::error::Error;
use std::fmt;

use rstorrent_engine::{ContentFingerprint, TorrentId};
use rstorrent_protocol::metainfo::MAX_METAINFO_PIECES;

const HAVE_MAGIC: &[u8; 8] = b"RSTHAVE\0";
const HAVE_VERSION: u16 = 2;
const HAVE_HEADER_LENGTH: usize = 8 + 2 + 16 + 32 + 4;
pub const MAX_DURABLE_PIECES: usize = MAX_METAINFO_PIECES;
pub const MAX_DURABLE_HAVE_STATE_BYTES: usize = HAVE_HEADER_LENGTH + MAX_DURABLE_PIECES.div_ceil(8);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HaveState {
    torrent_id: TorrentId,
    content_fingerprint: ContentFingerprint,
    pieces: Vec<bool>,
}

impl HaveState {
    pub fn empty(
        torrent_id: TorrentId,
        content_fingerprint: ContentFingerprint,
        piece_count: usize,
    ) -> Result<Self, HaveError> {
        validate_piece_count(piece_count)?;
        Ok(Self {
            torrent_id,
            content_fingerprint,
            pieces: vec![false; piece_count],
        })
    }

    pub fn from_pieces(
        torrent_id: TorrentId,
        content_fingerprint: ContentFingerprint,
        pieces: Vec<bool>,
    ) -> Result<Self, HaveError> {
        validate_piece_count(pieces.len())?;
        Ok(Self {
            torrent_id,
            content_fingerprint,
            pieces,
        })
    }

    pub fn torrent_id(&self) -> TorrentId {
        self.torrent_id
    }

    pub fn content_fingerprint(&self) -> ContentFingerprint {
        self.content_fingerprint
    }

    pub fn pieces(&self) -> &[bool] {
        &self.pieces
    }

    pub fn verified_count(&self) -> usize {
        self.pieces.iter().filter(|piece| **piece).count()
    }

    pub fn set(&mut self, piece_index: usize, verified: bool) -> Result<(), HaveError> {
        let piece_count = self.pieces.len();
        let piece = self
            .pieces
            .get_mut(piece_index)
            .ok_or(HaveError::InvalidPieceIndex {
                index: piece_index,
                piece_count,
            })?;
        *piece = verified;
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HAVE_HEADER_LENGTH + self.pieces.len().div_ceil(8));
        bytes.extend_from_slice(HAVE_MAGIC);
        bytes.extend_from_slice(&HAVE_VERSION.to_be_bytes());
        bytes.extend_from_slice(self.torrent_id.as_bytes());
        bytes.extend_from_slice(self.content_fingerprint.as_bytes());
        bytes.extend_from_slice(&(self.pieces.len() as u32).to_be_bytes());
        bytes.resize(HAVE_HEADER_LENGTH + self.pieces.len().div_ceil(8), 0);
        for (index, verified) in self.pieces.iter().enumerate() {
            if *verified {
                bytes[HAVE_HEADER_LENGTH + index / 8] |= 1 << (7 - index % 8);
            }
        }
        bytes
    }

    pub fn decode(
        bytes: &[u8],
        expected_torrent_id: TorrentId,
        expected_content_fingerprint: ContentFingerprint,
        expected_piece_count: usize,
    ) -> Result<Self, HaveError> {
        validate_piece_count(expected_piece_count)?;
        if bytes.len() < HAVE_HEADER_LENGTH {
            return Err(HaveError::InvalidLength);
        }
        if &bytes[..HAVE_MAGIC.len()] != HAVE_MAGIC {
            return Err(HaveError::InvalidMagic);
        }
        let version = u16::from_be_bytes(
            bytes[8..10]
                .try_into()
                .expect("fixed version field is two bytes"),
        );
        if version != HAVE_VERSION {
            return Err(HaveError::UnsupportedVersion(version));
        }
        let torrent_id = TorrentId::new(
            bytes[10..26]
                .try_into()
                .expect("fixed torrent ID field is sixteen bytes"),
        )
        .map_err(|_| HaveError::InvalidTorrentId)?;
        if torrent_id != expected_torrent_id {
            return Err(HaveError::TorrentIdMismatch);
        }
        let content_fingerprint = ContentFingerprint::from_digest(
            bytes[26..58]
                .try_into()
                .expect("fixed content fingerprint field is thirty-two bytes"),
        );
        if content_fingerprint != expected_content_fingerprint {
            return Err(HaveError::ContentFingerprintMismatch);
        }
        let piece_count = u32::from_be_bytes(
            bytes[58..62]
                .try_into()
                .expect("fixed piece-count field is four bytes"),
        ) as usize;
        if piece_count != expected_piece_count {
            return Err(HaveError::PieceCountMismatch {
                expected: expected_piece_count,
                actual: piece_count,
            });
        }
        let expected_length = HAVE_HEADER_LENGTH + piece_count.div_ceil(8);
        if bytes.len() != expected_length {
            return Err(HaveError::InvalidLength);
        }
        if !piece_count.is_multiple_of(8)
            && let Some(last) = bytes.last()
        {
            let unused_mask = (1_u8 << (8 - piece_count % 8)) - 1;
            if last & unused_mask != 0 {
                return Err(HaveError::NonzeroPadding);
            }
        }
        let mut pieces = vec![false; piece_count];
        for (index, piece) in pieces.iter_mut().enumerate() {
            *piece = bytes[HAVE_HEADER_LENGTH + index / 8] & (1 << (7 - index % 8)) != 0;
        }
        Ok(Self {
            torrent_id,
            content_fingerprint,
            pieces,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HaveError {
    InvalidPieceCount { actual: usize, maximum: usize },
    InvalidPieceIndex { index: usize, piece_count: usize },
    InvalidLength,
    InvalidMagic,
    UnsupportedVersion(u16),
    InvalidTorrentId,
    TorrentIdMismatch,
    ContentFingerprintMismatch,
    PieceCountMismatch { expected: usize, actual: usize },
    NonzeroPadding,
}

impl fmt::Display for HaveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPieceCount { actual, maximum } => {
                write!(formatter, "piece count {actual} exceeds bound {maximum}")
            }
            Self::InvalidPieceIndex { index, piece_count } => {
                write!(
                    formatter,
                    "piece index {index} is outside count {piece_count}"
                )
            }
            Self::InvalidLength => write!(formatter, "have state has an invalid length"),
            Self::InvalidMagic => write!(formatter, "have state has an invalid magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "have state version {version} is unsupported")
            }
            Self::InvalidTorrentId => write!(formatter, "have state torrent ID is invalid"),
            Self::TorrentIdMismatch => write!(formatter, "have state torrent ID does not match"),
            Self::ContentFingerprintMismatch => {
                write!(formatter, "have state content fingerprint does not match")
            }
            Self::PieceCountMismatch { expected, actual } => write!(
                formatter,
                "have state piece count {actual} does not match expected {expected}"
            ),
            Self::NonzeroPadding => write!(formatter, "have state sets unused padding bits"),
        }
    }
}

impl Error for HaveError {}

fn validate_piece_count(piece_count: usize) -> Result<(), HaveError> {
    if piece_count == 0 || piece_count > MAX_DURABLE_PIECES {
        return Err(HaveError::InvalidPieceCount {
            actual: piece_count,
            maximum: MAX_DURABLE_PIECES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstorrent_engine::{ContentFingerprint, TorrentId};

    use super::{
        HAVE_HEADER_LENGTH, HaveError, HaveState, MAX_DURABLE_HAVE_STATE_BYTES, MAX_DURABLE_PIECES,
    };

    #[test]
    fn round_trips_boundary_bit_counts() {
        let owner = TorrentId::new([7; 16]).expect("owner");
        let fingerprint = ContentFingerprint::from_digest([8; 32]);
        for piece_count in [1, 7, 8, 9, 15, 16, 17] {
            let mut state =
                HaveState::empty(owner, fingerprint, piece_count).expect("bounded state");
            state.set(0, true).expect("first piece");
            state
                .set(piece_count - 1, true)
                .expect("last piece is in range");
            let encoded = state.encode();
            assert_eq!(encoded.len(), HAVE_HEADER_LENGTH + piece_count.div_ceil(8));
            assert_eq!(
                HaveState::decode(&encoded, owner, fingerprint, piece_count).expect("decode"),
                state
            );
        }
    }

    #[test]
    fn rejects_shape_identity_version_and_padding() {
        let owner = TorrentId::new([1; 16]).expect("owner");
        let other_owner = TorrentId::new([2; 16]).expect("other owner");
        let fingerprint = ContentFingerprint::from_digest([3; 32]);
        let other_fingerprint = ContentFingerprint::from_digest([4; 32]);
        let state = HaveState::empty(owner, fingerprint, 9).expect("bounded state");
        let encoded = state.encode();

        assert_eq!(
            HaveState::decode(&encoded, other_owner, fingerprint, 9),
            Err(HaveError::TorrentIdMismatch)
        );
        assert_eq!(
            HaveState::decode(&encoded, owner, other_fingerprint, 9),
            Err(HaveError::ContentFingerprintMismatch)
        );
        assert!(matches!(
            HaveState::decode(&encoded, owner, fingerprint, 8),
            Err(HaveError::PieceCountMismatch { .. })
        ));

        let mut invalid = encoded.clone();
        invalid[8..10].copy_from_slice(&3_u16.to_be_bytes());
        assert_eq!(
            HaveState::decode(&invalid, owner, fingerprint, 9),
            Err(HaveError::UnsupportedVersion(3))
        );

        let mut invalid = encoded;
        *invalid.last_mut().expect("bitfield byte") = 1;
        assert_eq!(
            HaveState::decode(&invalid, owner, fingerprint, 9),
            Err(HaveError::NonzeroPadding)
        );

        let mut zero_owner = state.encode();
        zero_owner[10..26].fill(0);
        assert_eq!(
            HaveState::decode(&zero_owner, owner, fingerprint, 9),
            Err(HaveError::InvalidTorrentId)
        );
    }

    #[test]
    fn accepts_exact_durable_piece_boundary_and_rejects_the_next_piece() {
        let owner = TorrentId::new([9; 16]).expect("owner");
        let fingerprint = ContentFingerprint::from_digest([10; 32]);
        let state =
            HaveState::empty(owner, fingerprint, MAX_DURABLE_PIECES).expect("exact maximum");
        let encoded = state.encode();
        assert_eq!(encoded.len(), MAX_DURABLE_HAVE_STATE_BYTES);
        assert_eq!(
            HaveState::decode(&encoded, owner, fingerprint, MAX_DURABLE_PIECES)
                .expect("decode maximum"),
            state
        );
        assert_eq!(
            HaveState::empty(owner, fingerprint, MAX_DURABLE_PIECES + 1),
            Err(HaveError::InvalidPieceCount {
                actual: MAX_DURABLE_PIECES + 1,
                maximum: MAX_DURABLE_PIECES,
            })
        );
    }
}

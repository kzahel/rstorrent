//! Runtime-free BEP 52 Merkle arithmetic and proof validation.

use std::error::Error;
use std::fmt;

use sha2::{Digest, Sha256};

pub type Sha256Hash = [u8; 32];

pub const MERKLE_BLOCK_SIZE: usize = 16 * 1024;
pub const MAX_MERKLE_LEAVES: u64 = 1_u64 << 35;
pub const MAX_MERKLE_HEIGHT: u8 = 35;
pub const MAX_MERKLE_SCRATCH_HASHES: usize = MAX_MERKLE_HEIGHT as usize + 1;
pub const MAX_MERKLE_PROOF_HASHES: usize = MAX_MERKLE_HEIGHT as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MerkleTreeShape {
    leaf_count: u64,
    padded_leaf_count: u64,
    height: u8,
}

impl MerkleTreeShape {
    pub fn new(leaf_count: u64) -> Result<Self, MerkleError> {
        if leaf_count == 0 {
            return Err(MerkleError::EmptyTree);
        }
        if leaf_count > MAX_MERKLE_LEAVES {
            return Err(MerkleError::TooManyLeaves {
                actual: leaf_count,
                maximum: MAX_MERKLE_LEAVES,
            });
        }
        let padded_leaf_count = leaf_count
            .checked_next_power_of_two()
            .ok_or(MerkleError::ArithmeticOverflow)?;
        let height = u8::try_from(padded_leaf_count.trailing_zeros())
            .map_err(|_| MerkleError::ArithmeticOverflow)?;
        Ok(Self {
            leaf_count,
            padded_leaf_count,
            height,
        })
    }

    pub const fn leaf_count(self) -> u64 {
        self.leaf_count
    }

    pub const fn padded_leaf_count(self) -> u64 {
        self.padded_leaf_count
    }

    pub const fn height(self) -> u8 {
        self.height
    }

    pub fn node_count(self) -> Result<u64, MerkleError> {
        self.padded_leaf_count
            .checked_mul(2)
            .and_then(|value| value.checked_sub(1))
            .ok_or(MerkleError::ArithmeticOverflow)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MerkleError {
    EmptyTree,
    TooManyLeaves {
        actual: u64,
        maximum: u64,
    },
    InvalidBlockLength {
        length: usize,
        maximum: usize,
    },
    InvalidPieceLength {
        length: u32,
    },
    PieceTooLarge {
        length: usize,
        piece_length: u32,
    },
    LayerOutOfRange {
        layer: u8,
        maximum: u8,
    },
    SubjectOutOfRange {
        index: u64,
        layer: u8,
        leaf_count: u64,
    },
    InvalidProofLength {
        expected: usize,
        actual: usize,
    },
    InvalidPaddingHash {
        layer: u8,
        index: u64,
    },
    RootMismatch,
    ArithmeticOverflow,
}

impl fmt::Display for MerkleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTree => formatter.write_str("Merkle tree has no leaves"),
            Self::TooManyLeaves { actual, maximum } => {
                write!(
                    formatter,
                    "Merkle tree has {actual} leaves, limit {maximum}"
                )
            }
            Self::InvalidBlockLength { length, maximum } => write!(
                formatter,
                "Merkle block length {length} is outside 1..={maximum}"
            ),
            Self::InvalidPieceLength { length } => {
                write!(formatter, "invalid BEP 52 piece length {length}")
            }
            Self::PieceTooLarge {
                length,
                piece_length,
            } => write!(
                formatter,
                "piece payload length {length} exceeds piece length {piece_length}"
            ),
            Self::LayerOutOfRange { layer, maximum } => {
                write!(formatter, "Merkle layer {layer} exceeds limit {maximum}")
            }
            Self::SubjectOutOfRange {
                index,
                layer,
                leaf_count,
            } => write!(
                formatter,
                "Merkle subject {index} at layer {layer} is outside {leaf_count} leaves"
            ),
            Self::InvalidProofLength { expected, actual } => write!(
                formatter,
                "Merkle proof has {actual} siblings, expected {expected}"
            ),
            Self::InvalidPaddingHash { layer, index } => write!(
                formatter,
                "Merkle proof has invalid padding hash {index} at layer {layer}"
            ),
            Self::RootMismatch => formatter.write_str("Merkle proof root does not match"),
            Self::ArithmeticOverflow => formatter.write_str("Merkle arithmetic overflow"),
        }
    }
}

impl Error for MerkleError {}

pub fn hash_block(block: &[u8]) -> Result<Sha256Hash, MerkleError> {
    if block.is_empty() || block.len() > MERKLE_BLOCK_SIZE {
        return Err(MerkleError::InvalidBlockLength {
            length: block.len(),
            maximum: MERKLE_BLOCK_SIZE,
        });
    }
    Ok(Sha256::digest(block).into())
}

pub fn hash_pair(left: &Sha256Hash, right: &Sha256Hash) -> Sha256Hash {
    let mut hasher = Sha256::new();
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

pub fn zero_hash(layer: u8) -> Result<Sha256Hash, MerkleError> {
    if layer > MAX_MERKLE_HEIGHT {
        return Err(MerkleError::LayerOutOfRange {
            layer,
            maximum: MAX_MERKLE_HEIGHT,
        });
    }
    let mut hash = [0_u8; 32];
    for _ in 0..layer {
        hash = hash_pair(&hash, &hash);
    }
    Ok(hash)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MerkleAccumulator {
    base_layer: u8,
    count: u64,
    slots: [Option<Sha256Hash>; MAX_MERKLE_SCRATCH_HASHES],
    high_water: usize,
}

impl MerkleAccumulator {
    pub fn new(base_layer: u8) -> Result<Self, MerkleError> {
        if base_layer > MAX_MERKLE_HEIGHT {
            return Err(MerkleError::LayerOutOfRange {
                layer: base_layer,
                maximum: MAX_MERKLE_HEIGHT,
            });
        }
        Ok(Self {
            base_layer,
            count: 0,
            slots: [None; MAX_MERKLE_SCRATCH_HASHES],
            high_water: 0,
        })
    }

    pub const fn count(&self) -> u64 {
        self.count
    }

    pub const fn retained_hash_high_water(&self) -> usize {
        self.high_water
    }

    pub fn push(&mut self, mut hash: Sha256Hash) -> Result<(), MerkleError> {
        let maximum = MAX_MERKLE_LEAVES
            .checked_shr(u32::from(self.base_layer))
            .ok_or(MerkleError::ArithmeticOverflow)?;
        if self.count == maximum {
            return Err(MerkleError::TooManyLeaves {
                actual: self.count.saturating_add(1),
                maximum,
            });
        }

        let mut slot = 0_usize;
        loop {
            let entry = self
                .slots
                .get_mut(slot)
                .ok_or(MerkleError::ArithmeticOverflow)?;
            if let Some(left) = entry.take() {
                hash = hash_pair(&left, &hash);
                slot = slot.checked_add(1).ok_or(MerkleError::ArithmeticOverflow)?;
            } else {
                *entry = Some(hash);
                break;
            }
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or(MerkleError::ArithmeticOverflow)?;
        self.high_water = self
            .high_water
            .max(self.slots.iter().filter(|slot| slot.is_some()).count());
        Ok(())
    }

    pub fn finish(self) -> Result<Sha256Hash, MerkleError> {
        self.finish_at_height(None)
    }

    pub fn finish_padded_to(self, target_height: u8) -> Result<Sha256Hash, MerkleError> {
        self.finish_at_height(Some(target_height))
    }

    fn finish_at_height(self, target_height: Option<u8>) -> Result<Sha256Hash, MerkleError> {
        if self.count == 0 {
            return Err(MerkleError::EmptyTree);
        }

        let mut root = None;
        let mut root_height = 0_u8;
        for (slot_index, slot) in self.slots.iter().copied().enumerate() {
            let Some(left) = slot else {
                continue;
            };
            let slot_height =
                u8::try_from(slot_index).map_err(|_| MerkleError::ArithmeticOverflow)?;
            match root {
                None => {
                    root = Some(left);
                    root_height = slot_height;
                }
                Some(mut right) => {
                    while root_height < slot_height {
                        let padding_layer = self
                            .base_layer
                            .checked_add(root_height)
                            .ok_or(MerkleError::ArithmeticOverflow)?;
                        right = hash_pair(&right, &zero_hash(padding_layer)?);
                        root_height += 1;
                    }
                    root = Some(hash_pair(&left, &right));
                    root_height = slot_height
                        .checked_add(1)
                        .ok_or(MerkleError::ArithmeticOverflow)?;
                }
            }
        }

        let mut root = root.expect("nonempty accumulator has a retained root");
        let minimum_height = u8::try_from(self.count.next_power_of_two().trailing_zeros())
            .map_err(|_| MerkleError::ArithmeticOverflow)?;
        debug_assert_eq!(root_height, minimum_height);
        let target_height = target_height.unwrap_or(minimum_height);
        if target_height < minimum_height
            || self
                .base_layer
                .checked_add(target_height)
                .is_none_or(|layer| layer > MAX_MERKLE_HEIGHT)
        {
            return Err(MerkleError::LayerOutOfRange {
                layer: self.base_layer.saturating_add(target_height),
                maximum: MAX_MERKLE_HEIGHT,
            });
        }
        while root_height < target_height {
            let padding_layer = self
                .base_layer
                .checked_add(root_height)
                .ok_or(MerkleError::ArithmeticOverflow)?;
            root = hash_pair(&root, &zero_hash(padding_layer)?);
            root_height += 1;
        }
        Ok(root)
    }
}

pub fn root_from_hashes(
    hashes: impl IntoIterator<Item = Sha256Hash>,
    base_layer: u8,
) -> Result<Sha256Hash, MerkleError> {
    let mut accumulator = MerkleAccumulator::new(base_layer)?;
    for hash in hashes {
        accumulator.push(hash)?;
    }
    accumulator.finish()
}

pub fn file_root_from_data(data: &[u8]) -> Result<Sha256Hash, MerkleError> {
    if data.is_empty() {
        return Err(MerkleError::EmptyTree);
    }
    let mut accumulator = MerkleAccumulator::new(0)?;
    for block in data.chunks(MERKLE_BLOCK_SIZE) {
        accumulator.push(hash_block(block)?)?;
    }
    accumulator.finish()
}

pub fn piece_root_from_data(data: &[u8], piece_length: u32) -> Result<Sha256Hash, MerkleError> {
    let piece_layer = piece_layer(piece_length)?;
    if data.len() > piece_length as usize {
        return Err(MerkleError::PieceTooLarge {
            length: data.len(),
            piece_length,
        });
    }
    if data.is_empty() {
        return Err(MerkleError::EmptyTree);
    }
    let mut accumulator = MerkleAccumulator::new(0)?;
    for block in data.chunks(MERKLE_BLOCK_SIZE) {
        accumulator.push(hash_block(block)?)?;
    }
    accumulator.finish_padded_to(piece_layer)
}

pub fn file_root_from_piece_hashes(
    piece_hashes: impl IntoIterator<Item = Sha256Hash>,
    piece_length: u32,
) -> Result<Sha256Hash, MerkleError> {
    root_from_hashes(piece_hashes, piece_layer(piece_length)?)
}

pub fn piece_layer(piece_length: u32) -> Result<u8, MerkleError> {
    if piece_length < MERKLE_BLOCK_SIZE as u32
        || !piece_length.is_power_of_two()
        || piece_length > 256 * 1024 * 1024
    {
        return Err(MerkleError::InvalidPieceLength {
            length: piece_length,
        });
    }
    let blocks = piece_length / MERKLE_BLOCK_SIZE as u32;
    u8::try_from(blocks.trailing_zeros()).map_err(|_| MerkleError::ArithmeticOverflow)
}

pub fn verify_proof(
    subject: Sha256Hash,
    subject_layer: u8,
    subject_index: u64,
    leaf_count: u64,
    siblings: &[Sha256Hash],
    expected_root: Sha256Hash,
) -> Result<(), MerkleError> {
    let shape = MerkleTreeShape::new(leaf_count)?;
    if subject_layer > shape.height {
        return Err(MerkleError::LayerOutOfRange {
            layer: subject_layer,
            maximum: shape.height,
        });
    }
    let subject_width = 1_u64
        .checked_shl(u32::from(subject_layer))
        .ok_or(MerkleError::ArithmeticOverflow)?;
    let subject_start = subject_index
        .checked_mul(subject_width)
        .ok_or(MerkleError::ArithmeticOverflow)?;
    if subject_start >= leaf_count {
        return Err(MerkleError::SubjectOutOfRange {
            index: subject_index,
            layer: subject_layer,
            leaf_count,
        });
    }

    let expected_siblings = usize::from(shape.height - subject_layer);
    if siblings.len() != expected_siblings {
        return Err(MerkleError::InvalidProofLength {
            expected: expected_siblings,
            actual: siblings.len(),
        });
    }

    let mut hash = subject;
    let mut index = subject_index;
    for (layer, sibling) in (subject_layer..).zip(siblings) {
        let sibling_index = index ^ 1;
        let sibling_start = sibling_index
            .checked_shl(u32::from(layer))
            .ok_or(MerkleError::ArithmeticOverflow)?;
        if sibling_start >= leaf_count && *sibling != zero_hash(layer)? {
            return Err(MerkleError::InvalidPaddingHash {
                layer,
                index: sibling_index,
            });
        }
        hash = if index & 1 == 0 {
            hash_pair(&hash, sibling)
        } else {
            hash_pair(sibling, &hash)
        };
        index >>= 1;
    }

    if hash != expected_root {
        return Err(MerkleError::RootMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_hash(hex: &str) -> Sha256Hash {
        assert_eq!(hex.len(), 64);
        let mut output = [0_u8; 32];
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex byte");
        }
        output
    }

    #[test]
    fn fixed_sha256_and_padding_vectors_are_exact() {
        let a = hash_block(b"a").expect("one-byte block");
        let b = hash_block(b"b").expect("one-byte block");
        let c = hash_block(b"c").expect("one-byte block");
        assert_eq!(
            a,
            decode_hash("ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb")
        );
        assert_eq!(
            hash_pair(&a, &b),
            decode_hash("e5a01fee14e0ed5c48714f22180f25ad8365b53f9779f79dc4a3d7e93963f94a")
        );
        assert_eq!(
            zero_hash(1),
            Ok(decode_hash(
                "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b"
            ))
        );
        assert_eq!(
            root_from_hashes([a, b, c], 0),
            Ok(decode_hash(
                "d0a664079d491a97357efa1ce1eab5aeb566adef78a2b910e8d13e901e192832"
            ))
        );
    }

    #[test]
    fn file_root_is_piece_size_independent_and_piece_layers_reconstruct_it() {
        let data = (0..MERKLE_BLOCK_SIZE * 17 + 17)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let expected = file_root_from_data(&data).expect("file root");
        for piece_length in [64 * 1024_u32, 256 * 1024] {
            let pieces = data
                .chunks(piece_length as usize)
                .map(|piece| piece_root_from_data(piece, piece_length).expect("piece root"))
                .collect::<Vec<_>>();
            assert_eq!(
                file_root_from_piece_hashes(pieces, piece_length),
                Ok(expected)
            );
        }
    }

    #[test]
    fn proof_validation_is_exact_and_padding_aware() {
        let a = hash_block(b"a").expect("a");
        let b = hash_block(b"b").expect("b");
        let c = hash_block(b"c").expect("c");
        let left = hash_pair(&a, &b);
        let right = hash_pair(&c, &[0; 32]);
        let root = hash_pair(&left, &right);

        assert_eq!(verify_proof(c, 0, 2, 3, &[[0; 32], left], root), Ok(()));
        assert_eq!(
            verify_proof(c, 0, 2, 3, &[[1; 32], left], root),
            Err(MerkleError::InvalidPaddingHash { layer: 0, index: 3 })
        );
        assert!(matches!(
            verify_proof(c, 0, 2, 3, &[[0; 32]], root),
            Err(MerkleError::InvalidProofLength { .. })
        ));
        assert!(matches!(
            verify_proof(c, 0, 3, 3, &[[0; 32], left], root),
            Err(MerkleError::SubjectOutOfRange { .. })
        ));
        assert_eq!(
            verify_proof(c, 0, 2, 3, &[[0; 32], left], [0; 32]),
            Err(MerkleError::RootMismatch)
        );
    }

    #[test]
    fn shape_and_accumulator_enforce_the_fixed_resource_ceiling() {
        let maximum = MerkleTreeShape::new(MAX_MERKLE_LEAVES).expect("maximum shape");
        assert_eq!(maximum.height(), MAX_MERKLE_HEIGHT);
        assert_eq!(maximum.node_count(), Ok((1_u64 << 36) - 1));
        assert!(matches!(
            MerkleTreeShape::new(MAX_MERKLE_LEAVES + 1),
            Err(MerkleError::TooManyLeaves { .. })
        ));

        let mut accumulator = MerkleAccumulator::new(0).expect("accumulator");
        for value in 0_u16..1000 {
            let mut hash = [0_u8; 32];
            hash[..2].copy_from_slice(&value.to_be_bytes());
            accumulator.push(hash).expect("bounded hash");
        }
        assert!(accumulator.retained_hash_high_water() <= MAX_MERKLE_SCRATCH_HASHES);
        accumulator.finish().expect("root");
        assert_eq!(MAX_MERKLE_SCRATCH_HASHES * 32, 1_152);
        assert_eq!(MAX_MERKLE_PROOF_HASHES * 32, 1_120);
    }

    #[test]
    fn invalid_blocks_pieces_layers_and_empty_trees_fail_closed() {
        assert!(matches!(
            hash_block(&[]),
            Err(MerkleError::InvalidBlockLength { .. })
        ));
        assert!(matches!(
            hash_block(&vec![0; MERKLE_BLOCK_SIZE + 1]),
            Err(MerkleError::InvalidBlockLength { .. })
        ));
        assert_eq!(root_from_hashes([], 0), Err(MerkleError::EmptyTree));
        assert!(matches!(
            piece_root_from_data(b"x", 32 * 1024 - 1),
            Err(MerkleError::InvalidPieceLength { .. })
        ));
        assert!(matches!(
            piece_root_from_data(&vec![0; 32 * 1024 + 1], 32 * 1024),
            Err(MerkleError::PieceTooLarge { .. })
        ));
        assert!(matches!(
            zero_hash(MAX_MERKLE_HEIGHT + 1),
            Err(MerkleError::LayerOutOfRange { .. })
        ));
    }
}

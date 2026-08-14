//! Runtime-free BEP 52 hash-exchange values and authenticated sparse state.

use std::error::Error;
use std::fmt;
use std::mem::size_of;

use crate::merkle::{
    MAX_MERKLE_HEIGHT, MerkleAccumulator, MerkleError, MerkleTreeShape, Sha256Hash,
    file_root_from_piece_hashes, piece_layer, verify_proof, zero_hash,
};

pub const MAX_HASH_REQUEST_COUNT: u32 = 512;
pub const MAX_HASH_PROOF_LAYERS: u32 = MAX_MERKLE_HEIGHT as u32;
pub const MAX_HASHES_PER_RESPONSE: usize =
    MAX_HASH_REQUEST_COUNT as usize + MAX_HASH_PROOF_LAYERS as usize;
pub const MAX_HASH_RESPONSE_BYTES: usize = MAX_HASHES_PER_RESPONSE * size_of::<Sha256Hash>();
pub const HASH_REQUEST_PAYLOAD_LENGTH: usize = 32 + 4 * size_of::<u32>();
pub const MAX_HASH_MESSAGE_LENGTH: usize =
    1 + HASH_REQUEST_PAYLOAD_LENGTH + MAX_HASH_RESPONSE_BYTES;

pub const MAX_AUTHENTICATED_PIECE_ROOTS: usize = 2_097_152;
pub const MAX_RETAINED_PROOF_NODES: usize = 131_072;
pub const MAX_HASH_CATALOG_BYTES: usize = 80 * 1024 * 1024;

const PIECE_ROOT_CHUNK: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HashRequest {
    pub pieces_root: Sha256Hash,
    pub base_layer: u32,
    pub index: u32,
    pub count: u32,
    pub proof_layers: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashResponse {
    pub request: HashRequest,
    pub hashes: Vec<Sha256Hash>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2FileHashGeometry {
    pub pieces_root: Sha256Hash,
    pub file_length: u64,
    pub piece_length: u32,
    pub first_piece: u32,
    pub piece_count: u32,
}

impl V2FileHashGeometry {
    pub fn new(
        pieces_root: Sha256Hash,
        file_length: u64,
        piece_length: u32,
        first_piece: u32,
        piece_count: u32,
    ) -> Result<Self, HashExchangeError> {
        if file_length == 0 || piece_count == 0 {
            return Err(HashExchangeError::EmptyFile);
        }
        piece_layer(piece_length).map_err(HashExchangeError::Merkle)?;
        let expected_pieces = file_length.div_ceil(u64::from(piece_length));
        if expected_pieces != u64::from(piece_count) {
            return Err(HashExchangeError::InvalidFileGeometry);
        }
        let end = usize::try_from(first_piece)
            .ok()
            .and_then(|start| start.checked_add(piece_count as usize))
            .ok_or(HashExchangeError::ArithmeticOverflow)?;
        if end > MAX_AUTHENTICATED_PIECE_ROOTS {
            return Err(HashExchangeError::TooManyPieceRoots {
                actual: end,
                maximum: MAX_AUTHENTICATED_PIECE_ROOTS,
            });
        }
        Ok(Self {
            pieces_root,
            file_length,
            piece_length,
            first_piece,
            piece_count,
        })
    }

    pub fn leaf_count(self) -> Result<u64, HashExchangeError> {
        Ok(self
            .file_length
            .div_ceil(crate::merkle::MERKLE_BLOCK_SIZE as u64))
    }

    pub fn piece_layer(self) -> Result<u8, HashExchangeError> {
        piece_layer(self.piece_length).map_err(HashExchangeError::Merkle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValidatedRange {
    base_layer: u8,
    subject_layer: u8,
    subject_index: u64,
    real_base_nodes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct HashNodeKey {
    pieces_root: Sha256Hash,
    layer: u8,
    index: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HashNode {
    key: HashNodeKey,
    hash: Sha256Hash,
}

#[derive(Clone, Debug)]
struct PieceRoots {
    total: usize,
    chunks: Vec<Option<Box<[[u8; 32]; PIECE_ROOT_CHUNK]>>>,
    known: Vec<u64>,
    known_count: usize,
}

impl PieceRoots {
    fn new(total: usize) -> Result<Self, HashExchangeError> {
        if total > MAX_AUTHENTICATED_PIECE_ROOTS {
            return Err(HashExchangeError::TooManyPieceRoots {
                actual: total,
                maximum: MAX_AUTHENTICATED_PIECE_ROOTS,
            });
        }
        Ok(Self {
            total,
            chunks: vec![None; total.div_ceil(PIECE_ROOT_CHUNK)],
            known: vec![0; total.div_ceil(64)],
            known_count: 0,
        })
    }

    fn get(&self, index: usize) -> Option<Sha256Hash> {
        if index >= self.total || self.known[index / 64] & (1_u64 << (index % 64)) == 0 {
            return None;
        }
        self.chunks[index / PIECE_ROOT_CHUNK]
            .as_ref()
            .map(|chunk| chunk[index % PIECE_ROOT_CHUNK])
    }

    fn check(&self, index: usize, hash: Sha256Hash) -> Result<bool, HashExchangeError> {
        if index >= self.total {
            return Err(HashExchangeError::PieceIndexOutOfRange);
        }
        match self.get(index) {
            Some(existing) if existing != hash => Err(HashExchangeError::ConflictingHash),
            Some(_) => Ok(false),
            None => Ok(true),
        }
    }

    fn set_new(&mut self, index: usize, hash: Sha256Hash) {
        let chunk = self.chunks[index / PIECE_ROOT_CHUNK]
            .get_or_insert_with(|| Box::new([[0; 32]; PIECE_ROOT_CHUNK]));
        chunk[index % PIECE_ROOT_CHUNK] = hash;
        self.known[index / 64] |= 1_u64 << (index % 64);
        self.known_count += 1;
    }

    fn allocated_chunk_count(&self) -> usize {
        self.chunks.iter().filter(|chunk| chunk.is_some()).count()
    }

    fn clear(&mut self) {
        self.chunks.fill_with(|| None);
        self.known.fill(0);
        self.known_count = 0;
    }
}

/// Authenticated v2 hash knowledge shared by one torrent generation.
///
/// Piece roots use lazily allocated fixed chunks plus a compact presence
/// bitmap. Proof and leaf nodes are sorted flat values so the maximum retained
/// allocation remains below the tactical's per-torrent ceiling.
#[derive(Clone, Debug)]
pub struct V2HashCatalog {
    piece_roots: PieceRoots,
    proof_nodes: Vec<HashNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HashCatalogAccounting {
    pub known_piece_roots: usize,
    pub retained_proof_nodes: usize,
    pub retained_raw_hash_bytes: usize,
    pub allocated_bytes: usize,
}

impl V2HashCatalog {
    pub fn new(total_pieces: usize) -> Result<Self, HashExchangeError> {
        let catalog = Self {
            piece_roots: PieceRoots::new(total_pieces)?,
            proof_nodes: Vec::new(),
        };
        if maximum_catalog_allocation(total_pieces)? > MAX_HASH_CATALOG_BYTES {
            return Err(HashExchangeError::CatalogResourceLimit {
                maximum: MAX_HASH_CATALOG_BYTES,
            });
        }
        Ok(catalog)
    }

    pub fn piece_root(&self, piece: u32) -> Option<Sha256Hash> {
        usize::try_from(piece)
            .ok()
            .and_then(|index| self.piece_roots.get(index))
    }

    pub fn accounting(&self) -> HashCatalogAccounting {
        let raw_hashes = self
            .piece_roots
            .known_count
            .saturating_add(self.proof_nodes.len());
        HashCatalogAccounting {
            known_piece_roots: self.piece_roots.known_count,
            retained_proof_nodes: self.proof_nodes.len(),
            retained_raw_hash_bytes: raw_hashes.saturating_mul(size_of::<Sha256Hash>()),
            allocated_bytes: self
                .piece_roots
                .allocated_chunk_count()
                .saturating_mul(PIECE_ROOT_CHUNK * size_of::<Sha256Hash>())
                .saturating_add(self.piece_roots.known.capacity() * size_of::<u64>())
                .saturating_add(
                    self.piece_roots.chunks.capacity()
                        * size_of::<Option<Box<[[u8; 32]; PIECE_ROOT_CHUNK]>>>(),
                )
                .saturating_add(self.proof_nodes.capacity() * size_of::<HashNode>()),
        }
    }

    pub fn release(&mut self) {
        self.piece_roots.clear();
        self.proof_nodes = Vec::new();
    }

    pub fn seed_complete_piece_layer(
        &mut self,
        geometry: V2FileHashGeometry,
        hashes: &[Sha256Hash],
    ) -> Result<(), HashExchangeError> {
        if hashes.len() != geometry.piece_count as usize {
            return Err(HashExchangeError::InvalidHashCount {
                expected: geometry.piece_count as usize,
                actual: hashes.len(),
            });
        }
        let root = file_root_from_piece_hashes(hashes.iter().copied(), geometry.piece_length)
            .map_err(HashExchangeError::Merkle)?;
        if root != geometry.pieces_root {
            return Err(HashExchangeError::BadProof);
        }
        let first = geometry.first_piece as usize;
        for (offset, hash) in hashes.iter().copied().enumerate() {
            self.piece_roots.check(first + offset, hash)?;
        }
        for (offset, hash) in hashes.iter().copied().enumerate() {
            if self.piece_roots.get(first + offset).is_none() {
                self.piece_roots.set_new(first + offset, hash);
            }
        }
        Ok(())
    }

    pub fn insert_response(
        &mut self,
        geometry: V2FileHashGeometry,
        response: &HashResponse,
    ) -> Result<usize, HashExchangeError> {
        let validated = validate_request_range(geometry, response.request, false)?;
        let expected = response.request.count as usize + response.request.proof_layers as usize;
        if response.hashes.len() != expected {
            return Err(HashExchangeError::InvalidHashCount {
                expected,
                actual: response.hashes.len(),
            });
        }
        if response.hashes.len() > MAX_HASHES_PER_RESPONSE {
            return Err(HashExchangeError::TooManyResponseHashes {
                actual: response.hashes.len(),
                maximum: MAX_HASHES_PER_RESPONSE,
            });
        }

        let count = response.request.count as usize;
        let base_hashes = &response.hashes[..count];
        for (offset, hash) in base_hashes.iter().copied().enumerate() {
            let index = u64::from(response.request.index)
                .checked_add(offset as u64)
                .ok_or(HashExchangeError::ArithmeticOverflow)?;
            if index >= validated.real_base_nodes && hash != zero_hash(validated.base_layer)? {
                return Err(HashExchangeError::InvalidPaddingHash);
            }
        }

        let mut accumulator = MerkleAccumulator::new(validated.base_layer)?;
        for hash in base_hashes {
            accumulator.push(*hash)?;
        }
        let subject = accumulator.finish()?;
        let proof = &response.hashes[count..];
        verify_proof(
            subject,
            validated.subject_layer,
            validated.subject_index,
            geometry.leaf_count()?,
            proof,
            geometry.pieces_root,
        )
        .map_err(|_| HashExchangeError::BadProof)?;

        let mut base_inserts = Vec::new();
        let mut base_node_inserts = Vec::new();
        for (offset, hash) in base_hashes.iter().copied().enumerate() {
            let base_index = u64::from(response.request.index) + offset as u64;
            if base_index >= validated.real_base_nodes {
                continue;
            }
            if validated.base_layer == geometry.piece_layer()? {
                let global = usize::try_from(u64::from(geometry.first_piece) + base_index)
                    .map_err(|_| HashExchangeError::ArithmeticOverflow)?;
                if self.piece_roots.check(global, hash)? {
                    base_inserts.push((global, hash));
                }
            } else {
                let key = HashNodeKey {
                    pieces_root: geometry.pieces_root,
                    layer: validated.base_layer,
                    index: base_index,
                };
                if self.check_node(key, hash)? {
                    base_node_inserts.push((key, hash));
                }
            }
        }

        let mut proof_inserts = Vec::new();
        let mut proof_index = validated.subject_index;
        for (layer, hash) in (validated.subject_layer..).zip(proof.iter().copied()) {
            let key = HashNodeKey {
                pieces_root: geometry.pieces_root,
                layer,
                index: proof_index ^ 1,
            };
            if self.check_node(key, hash)? {
                proof_inserts.push((key, hash));
            }
            proof_index >>= 1;
        }
        let new_node_count = base_node_inserts.len().saturating_add(proof_inserts.len());
        if self.proof_nodes.len().saturating_add(new_node_count) > MAX_RETAINED_PROOF_NODES {
            return Err(HashExchangeError::TooManyProofNodes {
                maximum: MAX_RETAINED_PROOF_NODES,
            });
        }
        self.proof_nodes
            .try_reserve_exact(new_node_count)
            .map_err(|_| HashExchangeError::CatalogResourceLimit {
                maximum: MAX_HASH_CATALOG_BYTES,
            })?;

        for (global, hash) in &base_inserts {
            self.piece_roots.set_new(*global, *hash);
        }
        for (key, hash) in base_node_inserts {
            self.insert_node(key, hash);
        }
        for (key, hash) in proof_inserts {
            self.insert_node(key, hash);
        }
        let inserted = base_inserts.len();
        if self.accounting().allocated_bytes > MAX_HASH_CATALOG_BYTES {
            return Err(HashExchangeError::CatalogResourceLimit {
                maximum: MAX_HASH_CATALOG_BYTES,
            });
        }
        Ok(inserted)
    }

    pub fn response_for(
        &self,
        geometry: V2FileHashGeometry,
        request: HashRequest,
        allow_count_one: bool,
    ) -> Result<HashResponse, HashExchangeError> {
        let validated = validate_request_range(geometry, request, allow_count_one)?;
        let mut hashes = Vec::with_capacity(request.count as usize + request.proof_layers as usize);
        for offset in 0..request.count {
            let index = u64::from(request.index) + u64::from(offset);
            hashes.push(self.node_hash(geometry, validated.base_layer, index)?);
        }
        let mut proof_index = validated.subject_index;
        for layer in validated.subject_layer..validated.subject_layer + request.proof_layers as u8 {
            let sibling = proof_index ^ 1;
            let sibling_start = sibling
                .checked_shl(u32::from(layer))
                .ok_or(HashExchangeError::ArithmeticOverflow)?;
            let hash = if sibling_start >= geometry.leaf_count()? {
                zero_hash(layer)?
            } else {
                self.find_node(HashNodeKey {
                    pieces_root: geometry.pieces_root,
                    layer,
                    index: sibling,
                })
                .ok_or(HashExchangeError::HashesUnavailable)?
            };
            hashes.push(hash);
            proof_index >>= 1;
        }
        Ok(HashResponse { request, hashes })
    }

    fn node_hash(
        &self,
        geometry: V2FileHashGeometry,
        layer: u8,
        index: u64,
    ) -> Result<Sha256Hash, HashExchangeError> {
        let width = 1_u64
            .checked_shl(u32::from(layer))
            .ok_or(HashExchangeError::ArithmeticOverflow)?;
        if index
            .checked_mul(width)
            .ok_or(HashExchangeError::ArithmeticOverflow)?
            >= geometry.leaf_count()?
        {
            return zero_hash(layer).map_err(HashExchangeError::Merkle);
        }
        if layer == geometry.piece_layer()? {
            let global = usize::try_from(u64::from(geometry.first_piece) + index)
                .map_err(|_| HashExchangeError::ArithmeticOverflow)?;
            return self
                .piece_roots
                .get(global)
                .ok_or(HashExchangeError::HashesUnavailable);
        }
        self.find_node(HashNodeKey {
            pieces_root: geometry.pieces_root,
            layer,
            index,
        })
        .ok_or(HashExchangeError::HashesUnavailable)
    }

    fn find_node(&self, key: HashNodeKey) -> Option<Sha256Hash> {
        self.proof_nodes
            .binary_search_by_key(&key, |node| node.key)
            .ok()
            .map(|index| self.proof_nodes[index].hash)
    }

    fn check_node(&self, key: HashNodeKey, hash: Sha256Hash) -> Result<bool, HashExchangeError> {
        match self.find_node(key) {
            Some(existing) if existing != hash => Err(HashExchangeError::ConflictingHash),
            Some(_) => Ok(false),
            None => Ok(true),
        }
    }

    fn insert_node(&mut self, key: HashNodeKey, hash: Sha256Hash) {
        let index = self
            .proof_nodes
            .binary_search_by_key(&key, |node| node.key)
            .expect_err("new proof node was checked before insertion");
        self.proof_nodes.insert(index, HashNode { key, hash });
    }
}

pub fn validate_request(
    geometry: V2FileHashGeometry,
    request: HashRequest,
    allow_count_one: bool,
) -> Result<(), HashExchangeError> {
    validate_request_range(geometry, request, allow_count_one).map(|_| ())
}

fn validate_request_range(
    geometry: V2FileHashGeometry,
    request: HashRequest,
    allow_count_one: bool,
) -> Result<ValidatedRange, HashExchangeError> {
    if request.pieces_root != geometry.pieces_root {
        return Err(HashExchangeError::UnknownRoot);
    }
    if request.count == 0
        || request.count > MAX_HASH_REQUEST_COUNT
        || (!request.count.is_power_of_two())
        || (request.count == 1 && !allow_count_one)
    {
        return Err(HashExchangeError::InvalidCount {
            count: request.count,
        });
    }
    if !request.index.is_multiple_of(request.count) {
        return Err(HashExchangeError::MisalignedIndex);
    }
    if request.proof_layers > MAX_HASH_PROOF_LAYERS {
        return Err(HashExchangeError::InvalidProofLayers);
    }
    let base_layer =
        u8::try_from(request.base_layer).map_err(|_| HashExchangeError::UnsupportedBaseLayer)?;
    let piece_layer = geometry.piece_layer()?;
    if base_layer != 0 && base_layer != piece_layer {
        return Err(HashExchangeError::UnsupportedBaseLayer);
    }
    let shape = MerkleTreeShape::new(geometry.leaf_count()?)?;
    if base_layer > shape.height() {
        return Err(HashExchangeError::UnsupportedBaseLayer);
    }
    let range_height = u8::try_from(request.count.trailing_zeros())
        .map_err(|_| HashExchangeError::ArithmeticOverflow)?;
    let subject_layer = base_layer
        .checked_add(range_height)
        .ok_or(HashExchangeError::ArithmeticOverflow)?;
    if subject_layer > shape.height()
        || u32::from(shape.height() - subject_layer) != request.proof_layers
    {
        return Err(HashExchangeError::InvalidProofLayers);
    }
    let padded_base_nodes = shape.padded_leaf_count() >> base_layer;
    let end = u64::from(request.index)
        .checked_add(u64::from(request.count))
        .ok_or(HashExchangeError::ArithmeticOverflow)?;
    let base_width = 1_u64
        .checked_shl(u32::from(base_layer))
        .ok_or(HashExchangeError::ArithmeticOverflow)?;
    let real_base_nodes = geometry.leaf_count()?.div_ceil(base_width);
    if end > padded_base_nodes || u64::from(request.index) >= real_base_nodes {
        return Err(HashExchangeError::RangeOutOfBounds);
    }
    Ok(ValidatedRange {
        base_layer,
        subject_layer,
        subject_index: u64::from(request.index / request.count),
        real_base_nodes,
    })
}

pub fn maximum_catalog_allocation(total_pieces: usize) -> Result<usize, HashExchangeError> {
    if total_pieces > MAX_AUTHENTICATED_PIECE_ROOTS {
        return Err(HashExchangeError::TooManyPieceRoots {
            actual: total_pieces,
            maximum: MAX_AUTHENTICATED_PIECE_ROOTS,
        });
    }
    total_pieces
        .checked_mul(size_of::<Sha256Hash>())
        .and_then(|bytes| bytes.checked_add(total_pieces.div_ceil(64) * size_of::<u64>()))
        .and_then(|bytes| {
            bytes.checked_add(
                total_pieces.div_ceil(PIECE_ROOT_CHUNK)
                    * size_of::<Option<Box<[[u8; 32]; PIECE_ROOT_CHUNK]>>>(),
            )
        })
        .and_then(|bytes| bytes.checked_add(MAX_RETAINED_PROOF_NODES * size_of::<HashNode>()))
        .ok_or(HashExchangeError::ArithmeticOverflow)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashExchangeError {
    EmptyFile,
    InvalidFileGeometry,
    UnknownRoot,
    UnsupportedBaseLayer,
    InvalidCount { count: u32 },
    MisalignedIndex,
    InvalidProofLayers,
    RangeOutOfBounds,
    InvalidHashCount { expected: usize, actual: usize },
    TooManyResponseHashes { actual: usize, maximum: usize },
    InvalidPaddingHash,
    BadProof,
    ConflictingHash,
    HashesUnavailable,
    PieceIndexOutOfRange,
    TooManyPieceRoots { actual: usize, maximum: usize },
    TooManyProofNodes { maximum: usize },
    CatalogResourceLimit { maximum: usize },
    ArithmeticOverflow,
    Merkle(MerkleError),
}

impl fmt::Display for HashExchangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFile => formatter.write_str("empty v2 file has no hash tree"),
            Self::InvalidFileGeometry => formatter.write_str("v2 file hash geometry is invalid"),
            Self::UnknownRoot => formatter.write_str("hash request names an unknown file root"),
            Self::UnsupportedBaseLayer => {
                formatter.write_str("hash request base layer is unsupported")
            }
            Self::InvalidCount { count } => {
                write!(formatter, "hash request count {count} is invalid")
            }
            Self::MisalignedIndex => formatter.write_str("hash request index is not count-aligned"),
            Self::InvalidProofLayers => {
                formatter.write_str("hash request proof layer count is invalid")
            }
            Self::RangeOutOfBounds => {
                formatter.write_str("hash request range is outside the file tree")
            }
            Self::InvalidHashCount { expected, actual } => write!(
                formatter,
                "hash response has {actual} hashes, expected {expected}"
            ),
            Self::TooManyResponseHashes { actual, maximum } => write!(
                formatter,
                "hash response has {actual} hashes, limit {maximum}"
            ),
            Self::InvalidPaddingHash => {
                formatter.write_str("hash response contains invalid padding")
            }
            Self::BadProof => {
                formatter.write_str("hash response does not prove the authenticated file root")
            }
            Self::ConflictingHash => {
                formatter.write_str("authenticated hash conflicts with retained truth")
            }
            Self::HashesUnavailable => {
                formatter.write_str("requested authenticated hashes are unavailable")
            }
            Self::PieceIndexOutOfRange => {
                formatter.write_str("piece hash index is outside the torrent")
            }
            Self::TooManyPieceRoots { actual, maximum } => write!(
                formatter,
                "hash catalog has {actual} piece roots, limit {maximum}"
            ),
            Self::TooManyProofNodes { maximum } => {
                write!(formatter, "hash catalog exceeds {maximum} proof nodes")
            }
            Self::CatalogResourceLimit { maximum } => {
                write!(formatter, "hash catalog exceeds {maximum} bytes")
            }
            Self::ArithmeticOverflow => formatter.write_str("hash exchange arithmetic overflow"),
            Self::Merkle(error) => write!(formatter, "Merkle hash exchange: {error}"),
        }
    }
}

impl Error for HashExchangeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Merkle(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MerkleError> for HashExchangeError {
    fn from(value: MerkleError) -> Self {
        Self::Merkle(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::{hash_block, hash_pair};

    fn geometry() -> (V2FileHashGeometry, Vec<Sha256Hash>) {
        let hashes = [b"a", b"b", b"c"]
            .map(|data| hash_block(data).expect("block hash"))
            .to_vec();
        let root = hash_pair(
            &hash_pair(&hashes[0], &hashes[1]),
            &hash_pair(&hashes[2], &[0; 32]),
        );
        (
            V2FileHashGeometry::new(root, 3 * 16 * 1024, 16 * 1024, 4, 3).expect("geometry"),
            hashes,
        )
    }

    #[test]
    fn exact_resource_and_frame_ceilings_fit_the_tactical() {
        assert_eq!(MAX_HASHES_PER_RESPONSE, 547);
        assert_eq!(MAX_HASH_RESPONSE_BYTES, 17_504);
        assert_eq!(MAX_HASH_MESSAGE_LENGTH, 17_553);
        assert_eq!(size_of::<HashNode>(), 80);
        let maximum = maximum_catalog_allocation(MAX_AUTHENTICATED_PIECE_ROOTS)
            .expect("maximum catalog model");
        assert_eq!(maximum, 77_873_152);
        assert!(maximum < MAX_HASH_CATALOG_BYTES);
        assert!(maximum_catalog_allocation(MAX_AUTHENTICATED_PIECE_ROOTS + 1).is_err());
    }

    #[test]
    fn proved_tail_inserts_real_hashes_once_and_rejects_conflicts() {
        let (geometry, hashes) = geometry();
        let request = HashRequest {
            pieces_root: geometry.pieces_root,
            base_layer: 0,
            index: 2,
            count: 2,
            proof_layers: 1,
        };
        let response = HashResponse {
            request,
            hashes: vec![hashes[2], [0; 32], hash_pair(&hashes[0], &hashes[1])],
        };
        let mut catalog = V2HashCatalog::new(8).expect("catalog");
        assert_eq!(catalog.insert_response(geometry, &response), Ok(1));
        assert_eq!(catalog.insert_response(geometry, &response), Ok(0));
        let mut conflict = response.clone();
        conflict.hashes[0] = [9; 32];
        assert_eq!(
            catalog.insert_response(geometry, &conflict),
            Err(HashExchangeError::BadProof)
        );
    }

    #[test]
    fn complete_piece_layer_is_authenticated_before_catalog_mutation() {
        let piece_hashes = [hash_block(b"a").unwrap(), hash_block(b"b").unwrap()];
        let root = file_root_from_piece_hashes(piece_hashes, 16 * 1024).unwrap();
        let geometry = V2FileHashGeometry::new(root, 2 * 16 * 1024, 16 * 1024, 2, 2).unwrap();
        let mut catalog = V2HashCatalog::new(8).unwrap();
        catalog
            .seed_complete_piece_layer(geometry, &piece_hashes)
            .unwrap();
        assert_eq!(catalog.piece_root(2), Some(piece_hashes[0]));
        assert_eq!(catalog.piece_root(3), Some(piece_hashes[1]));
        assert_eq!(catalog.accounting().known_piece_roots, 2);
        catalog.release();
        assert_eq!(catalog.accounting().known_piece_roots, 0);
        assert_eq!(catalog.accounting().retained_raw_hash_bytes, 0);
    }

    #[test]
    fn request_shape_rejects_hostile_counts_layers_and_ranges() {
        let (geometry, _) = geometry();
        let valid = HashRequest {
            pieces_root: geometry.pieces_root,
            base_layer: 0,
            index: 0,
            count: 2,
            proof_layers: 1,
        };
        assert_eq!(validate_request(geometry, valid, false), Ok(()));
        for count in [0, 1, 3, 513] {
            assert!(matches!(
                validate_request(geometry, HashRequest { count, ..valid }, false),
                Err(HashExchangeError::InvalidCount { .. })
            ));
        }
        assert_eq!(
            validate_request(geometry, HashRequest { index: 1, ..valid }, false),
            Err(HashExchangeError::MisalignedIndex)
        );
        assert_eq!(
            validate_request(
                geometry,
                HashRequest {
                    base_layer: 7,
                    ..valid
                },
                false
            ),
            Err(HashExchangeError::UnsupportedBaseLayer)
        );
        assert_eq!(
            validate_request(
                geometry,
                HashRequest {
                    proof_layers: 0,
                    ..valid
                },
                false
            ),
            Err(HashExchangeError::InvalidProofLayers)
        );
        let singleton = HashRequest {
            count: 1,
            proof_layers: 2,
            ..valid
        };
        assert_eq!(validate_request(geometry, singleton, true), Ok(()));
    }

    #[test]
    fn maximum_catalog_constructs_inserts_looks_up_conflicts_and_releases() {
        let mut catalog =
            V2HashCatalog::new(MAX_AUTHENTICATED_PIECE_ROOTS).expect("maximum compact catalog");
        for index in 0..MAX_AUTHENTICATED_PIECE_ROOTS {
            let mut hash = [0; 32];
            hash[..8].copy_from_slice(&(index as u64).to_be_bytes());
            assert!(catalog.piece_roots.check(index, hash).unwrap());
            catalog.piece_roots.set_new(index, hash);
        }
        let last = (MAX_AUTHENTICATED_PIECE_ROOTS - 1) as u64;
        let mut expected = [0; 32];
        expected[..8].copy_from_slice(&last.to_be_bytes());
        assert_eq!(catalog.piece_root(last as u32), Some(expected));
        assert_eq!(
            catalog.piece_roots.check(last as usize, [0xff; 32]),
            Err(HashExchangeError::ConflictingHash)
        );
        let accounting = catalog.accounting();
        assert_eq!(accounting.known_piece_roots, MAX_AUTHENTICATED_PIECE_ROOTS);
        assert_eq!(accounting.retained_raw_hash_bytes, 64 * 1024 * 1024);
        assert!(accounting.allocated_bytes < MAX_HASH_CATALOG_BYTES);
        catalog.release();
        assert_eq!(catalog.accounting().retained_raw_hash_bytes, 0);
        assert_eq!(catalog.accounting().allocated_bytes, 278_528);
    }
}

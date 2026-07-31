use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use sha1::{Digest, Sha1};

use crate::bencode::{
    DictionaryEntry, Limits, Node, ParseError, Value, parse_prefix_with_limits, parse_with_limits,
};
use crate::peer_wire::MAX_EXTENSION_PAYLOAD_LENGTH;

pub const UT_METADATA_LOCAL_ID: u8 = 1;
pub const METADATA_BLOCK_LENGTH: usize = 16 * 1024;
pub const MAX_METADATA_LENGTH: usize = 1024 * 1024;
pub const MAX_METADATA_BLOCKS: usize = MAX_METADATA_LENGTH / METADATA_BLOCK_LENGTH;
pub const MAX_METADATA_REQUESTS_IN_FLIGHT: usize = 2;
pub const MAX_METADATA_UPLOAD_REQUESTS: usize = 256;
pub const MAX_TORRENT_METADATA_PEERS: usize = 32;
pub const METADATA_ASSIGNMENT_TIMEOUT_MILLIS: u64 = 3_000;
pub const METADATA_REJECT_COOLDOWN_MILLIS: u64 = 20_000;
pub const METADATA_HASH_FAILURE_COOLDOWN_MILLIS: u64 = 20_000;
pub const METADATA_SINGLE_BLOCK_HASH_FAILURE_COOLDOWN_MILLIS: u64 = 5 * 60 * 1_000;

/// A caller-supplied monotonic time for deterministic metadata scheduling.
///
/// The value has no wall-clock meaning. Runtime owners convert their elapsed
/// monotonic clock to milliseconds at the protocol boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetadataInstant(u64);

impl MetadataInstant {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(u64::MAX);

    pub const fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub const fn as_millis(self) -> u64 {
        self.0
    }

    const fn saturating_add(self, millis: u64) -> Self {
        Self(self.0.saturating_add(millis))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataExtensionUpdate {
    Unchanged,
    Disabled,
    Enabled(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionHandshake {
    pub metadata_extension: MetadataExtensionUpdate,
    pub metadata_size: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataMessage<'a> {
    Request {
        piece: i64,
    },
    Data {
        piece: i64,
        total_size: usize,
        block: &'a [u8],
    },
    Reject {
        piece: i64,
    },
    Unknown {
        message_type: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataError {
    Bencode(ParseError),
    RootIsNotDictionary,
    MissingField(&'static str),
    InvalidField(&'static str),
    InvalidSize {
        size: i64,
        maximum: usize,
    },
    SizeChanged {
        expected: usize,
        actual: usize,
    },
    InvalidPiece {
        piece: i64,
    },
    InvalidBlockLength {
        piece: u32,
        actual: usize,
        expected: usize,
    },
    UnsolicitedPiece {
        piece: i64,
    },
    ConflictingDuplicate {
        piece: u32,
    },
    Rejected {
        piece: u32,
    },
    HashMismatch,
    AlreadyStarted,
    AlreadyComplete,
    UploadRequestLimit {
        maximum: usize,
    },
    MetadataPeerLimit {
        maximum: usize,
    },
    UnknownMetadataPeer {
        peer: u64,
    },
}

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bencode(error) => write!(formatter, "invalid metadata bencode: {error}"),
            Self::RootIsNotDictionary => write!(formatter, "metadata message is not a dictionary"),
            Self::MissingField(field) => write!(formatter, "metadata message is missing {field}"),
            Self::InvalidField(field) => write!(formatter, "metadata message has invalid {field}"),
            Self::InvalidSize { size, maximum } => {
                write!(formatter, "metadata size {size} is outside 1..={maximum}")
            }
            Self::SizeChanged { expected, actual } => write!(
                formatter,
                "peer changed metadata size from {expected} to {actual}"
            ),
            Self::InvalidPiece { piece } => write!(formatter, "invalid metadata piece {piece}"),
            Self::InvalidBlockLength {
                piece,
                actual,
                expected,
            } => write!(
                formatter,
                "metadata piece {piece} has length {actual}, expected {expected}"
            ),
            Self::UnsolicitedPiece { piece } => {
                write!(formatter, "peer sent unsolicited metadata piece {piece}")
            }
            Self::ConflictingDuplicate { piece } => {
                write!(formatter, "peer changed duplicate metadata piece {piece}")
            }
            Self::Rejected { piece } => {
                write!(formatter, "peer rejected metadata piece {piece}")
            }
            Self::HashMismatch => write!(formatter, "assembled metadata does not match info hash"),
            Self::AlreadyStarted => write!(formatter, "metadata download was already started"),
            Self::AlreadyComplete => write!(formatter, "metadata download is already complete"),
            Self::UploadRequestLimit { maximum } => {
                write!(formatter, "metadata upload exceeded {maximum} requests")
            }
            Self::MetadataPeerLimit { maximum } => {
                write!(formatter, "metadata download exceeded {maximum} peers")
            }
            Self::UnknownMetadataPeer { peer } => {
                write!(formatter, "unknown metadata peer {peer}")
            }
        }
    }
}

impl Error for MetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bencode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for MetadataError {
    fn from(error: ParseError) -> Self {
        Self::Bencode(error)
    }
}

pub fn parse_extension_handshake(payload: &[u8]) -> Result<ExtensionHandshake, MetadataError> {
    let root = parse_with_limits(payload, extension_limits())?;
    let entries = dictionary(&root).ok_or(MetadataError::RootIsNotDictionary)?;
    let metadata_extension = match field(entries, b"m") {
        None => MetadataExtensionUpdate::Unchanged,
        Some(mapping) => {
            let mapping = dictionary(mapping).ok_or(MetadataError::InvalidField("handshake.m"))?;
            match field(mapping, b"ut_metadata") {
                None => MetadataExtensionUpdate::Unchanged,
                Some(node) => {
                    let id = integer(node, "handshake.m.ut_metadata")?;
                    match id {
                        0 => MetadataExtensionUpdate::Disabled,
                        1..=255 => MetadataExtensionUpdate::Enabled(id as u8),
                        _ => {
                            return Err(MetadataError::InvalidField("handshake.m.ut_metadata"));
                        }
                    }
                }
            }
        }
    };
    let metadata_size = match field(entries, b"metadata_size") {
        None => None,
        Some(node) => Some(validate_size(integer(node, "handshake.metadata_size")?)?),
    };
    Ok(ExtensionHandshake {
        metadata_extension,
        metadata_size,
    })
}

pub fn encode_extension_handshake(metadata_size: Option<usize>) -> Vec<u8> {
    encode_extension_handshake_with_id(UT_METADATA_LOCAL_ID, metadata_size)
        .expect("the stable local metadata extension ID is nonzero")
}

pub fn encode_extension_handshake_with_id(
    local_metadata_id: u8,
    metadata_size: Option<usize>,
) -> Result<Vec<u8>, MetadataError> {
    if local_metadata_id == 0 {
        return Err(MetadataError::InvalidField(
            "local ut_metadata extension ID",
        ));
    }
    if let Some(size) = metadata_size {
        validate_size(i64::try_from(size).map_err(|_| MetadataError::InvalidSize {
            size: i64::MAX,
            maximum: MAX_METADATA_LENGTH,
        })?)?;
    }
    let mut encoded = b"d1:md11:ut_metadatai".to_vec();
    push_integer(&mut encoded, i64::from(local_metadata_id));
    encoded.push(b'e');
    if let Some(size) = metadata_size {
        encoded.extend_from_slice(b"13:metadata_sizei");
        push_integer(&mut encoded, size as i64);
    }
    encoded.push(b'e');
    Ok(encoded)
}

pub fn parse_metadata_message(payload: &[u8]) -> Result<MetadataMessage<'_>, MetadataError> {
    let (root, consumed) = parse_prefix_with_limits(payload, extension_limits())?;
    let entries = dictionary(&root).ok_or(MetadataError::RootIsNotDictionary)?;
    let message_type = integer(
        field(entries, b"msg_type").ok_or(MetadataError::MissingField("msg_type"))?,
        "msg_type",
    )?;
    if !matches!(message_type, 0..=2) {
        return Ok(MetadataMessage::Unknown { message_type });
    }
    let piece = integer(
        field(entries, b"piece").ok_or(MetadataError::MissingField("piece"))?,
        "piece",
    )?;
    match message_type {
        0 => {
            reject_trailing(consumed, payload.len())?;
            Ok(MetadataMessage::Request { piece })
        }
        1 => {
            let total_size = integer(
                field(entries, b"total_size").ok_or(MetadataError::MissingField("total_size"))?,
                "total_size",
            )?;
            Ok(MetadataMessage::Data {
                piece,
                total_size: validate_size(total_size)?,
                block: &payload[consumed..],
            })
        }
        2 => {
            reject_trailing(consumed, payload.len())?;
            Ok(MetadataMessage::Reject { piece })
        }
        _ => unreachable!("message type was checked"),
    }
}

pub fn encode_metadata_request(piece: u32) -> Vec<u8> {
    encode_control_message(0, i64::from(piece))
}

pub fn encode_metadata_reject(piece: i64) -> Vec<u8> {
    encode_control_message(2, piece)
}

pub fn encode_metadata_data(
    piece: u32,
    total_size: usize,
    block: &[u8],
) -> Result<Vec<u8>, MetadataError> {
    let total_size =
        validate_size(
            i64::try_from(total_size).map_err(|_| MetadataError::InvalidSize {
                size: i64::MAX,
                maximum: MAX_METADATA_LENGTH,
            })?,
        )?;
    let expected = metadata_block_length(total_size, piece)?;
    if block.len() != expected {
        return Err(MetadataError::InvalidBlockLength {
            piece,
            actual: block.len(),
            expected,
        });
    }
    let mut encoded = b"d8:msg_typei1e5:piecei".to_vec();
    push_integer(&mut encoded, i64::from(piece));
    encoded.extend_from_slice(b"10:total_sizei");
    push_integer(&mut encoded, total_size as i64);
    encoded.push(b'e');
    encoded.extend_from_slice(block);
    Ok(encoded)
}

fn encode_control_message(message_type: i64, piece: i64) -> Vec<u8> {
    let mut encoded = b"d8:msg_typei".to_vec();
    push_integer(&mut encoded, message_type);
    encoded.extend_from_slice(b"5:piecei");
    push_integer(&mut encoded, piece);
    encoded.push(b'e');
    encoded
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataDownloadAction {
    Request(u32),
    Complete(Vec<u8>),
}

#[derive(Debug)]
pub struct MetadataDownload {
    expected_info_hash: [u8; 20],
    size: Option<usize>,
    blocks: Vec<Option<Vec<u8>>>,
    pending: BTreeSet<u32>,
    started: bool,
    complete: bool,
}

impl MetadataDownload {
    pub fn new(expected_info_hash: [u8; 20]) -> Self {
        Self {
            expected_info_hash,
            size: None,
            blocks: Vec::new(),
            pending: BTreeSet::new(),
            started: false,
            complete: false,
        }
    }

    pub fn start(
        &mut self,
        advertised_size: Option<usize>,
    ) -> Result<Vec<MetadataDownloadAction>, MetadataError> {
        if self.started {
            return Err(MetadataError::AlreadyStarted);
        }
        self.started = true;
        if let Some(size) = advertised_size {
            self.accept_size(size)?;
        }
        self.schedule_requests()
    }

    pub fn accept_advertised_size(
        &mut self,
        size: usize,
    ) -> Result<Vec<MetadataDownloadAction>, MetadataError> {
        if self.complete {
            return Err(MetadataError::AlreadyComplete);
        }
        self.accept_size(size)?;
        if self.started {
            self.schedule_requests()
        } else {
            Ok(Vec::new())
        }
    }

    pub fn on_message(
        &mut self,
        message: MetadataMessage<'_>,
    ) -> Result<Vec<MetadataDownloadAction>, MetadataError> {
        if self.complete {
            return Err(MetadataError::AlreadyComplete);
        }
        match message {
            MetadataMessage::Data {
                piece,
                total_size,
                block,
            } => self.on_data(piece, total_size, block),
            MetadataMessage::Reject { piece } => {
                let piece = valid_piece_number(piece)?;
                if !self.pending.remove(&piece) {
                    return Err(MetadataError::UnsolicitedPiece {
                        piece: i64::from(piece),
                    });
                }
                Err(MetadataError::Rejected { piece })
            }
            MetadataMessage::Unknown { .. } => Ok(Vec::new()),
            MetadataMessage::Request { .. } => {
                Err(MetadataError::InvalidField("unexpected metadata request"))
            }
        }
    }

    pub fn metadata_size(&self) -> Option<usize> {
        self.size
    }

    pub fn pending_requests(&self) -> usize {
        self.pending.len()
    }

    pub fn allocated_blocks(&self) -> usize {
        self.blocks.len()
    }

    pub fn received_blocks(&self) -> usize {
        self.blocks.iter().filter(|block| block.is_some()).count()
    }

    fn on_data(
        &mut self,
        piece: i64,
        total_size: usize,
        block: &[u8],
    ) -> Result<Vec<MetadataDownloadAction>, MetadataError> {
        let piece = valid_piece_number(piece)?;
        if let Some(size) = self.size
            && size != total_size
        {
            return Err(MetadataError::SizeChanged {
                expected: size,
                actual: total_size,
            });
        }

        if let Some(existing) = self.blocks.get(piece as usize).and_then(Option::as_deref) {
            return if existing == block {
                Ok(Vec::new())
            } else {
                Err(MetadataError::ConflictingDuplicate { piece })
            };
        }
        if !self.pending.contains(&piece) {
            return Err(MetadataError::UnsolicitedPiece {
                piece: i64::from(piece),
            });
        }

        self.accept_size(total_size)?;
        let expected = metadata_block_length(total_size, piece)?;
        if block.len() != expected {
            return Err(MetadataError::InvalidBlockLength {
                piece,
                actual: block.len(),
                expected,
            });
        }
        self.pending.remove(&piece);
        self.blocks[piece as usize] = Some(block.to_vec());

        if self.blocks.iter().all(Option::is_some) {
            let mut bytes = Vec::with_capacity(total_size);
            for block in &self.blocks {
                bytes.extend_from_slice(block.as_deref().expect("all blocks are present"));
            }
            if bytes.len() != total_size
                || <[u8; 20]>::from(Sha1::digest(&bytes)) != self.expected_info_hash
            {
                return Err(MetadataError::HashMismatch);
            }
            self.complete = true;
            self.pending.clear();
            return Ok(vec![MetadataDownloadAction::Complete(bytes)]);
        }
        self.schedule_requests()
    }

    fn accept_size(&mut self, size: usize) -> Result<(), MetadataError> {
        let size = validate_size(i64::try_from(size).map_err(|_| MetadataError::InvalidSize {
            size: i64::MAX,
            maximum: MAX_METADATA_LENGTH,
        })?)?;
        if let Some(expected) = self.size {
            if expected != size {
                return Err(MetadataError::SizeChanged {
                    expected,
                    actual: size,
                });
            }
            return Ok(());
        }
        self.size = Some(size);
        self.blocks = vec![None; metadata_block_count(size)];
        Ok(())
    }

    fn schedule_requests(&mut self) -> Result<Vec<MetadataDownloadAction>, MetadataError> {
        let mut actions = Vec::new();
        if self.size.is_none() {
            if self.pending.is_empty() {
                self.pending.insert(0);
                actions.push(MetadataDownloadAction::Request(0));
            }
            return Ok(actions);
        }
        for piece in 0..self.blocks.len() {
            if self.pending.len() == MAX_METADATA_REQUESTS_IN_FLIGHT {
                break;
            }
            let piece = u32::try_from(piece)
                .map_err(|_| MetadataError::InvalidPiece { piece: i64::MAX })?;
            if self.blocks[piece as usize].is_none() && self.pending.insert(piece) {
                actions.push(MetadataDownloadAction::Request(piece));
            }
        }
        Ok(actions)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentMetadataEvent {
    BlockAccepted { piece: u32 },
    Duplicate { piece: u32 },
    HashMismatch { contributors: Vec<u64> },
    Complete(Vec<u8>),
}

#[derive(Debug, Default)]
struct TorrentMetadataBlock {
    bytes: Option<Vec<u8>>,
    source: Option<u64>,
    assignments: BTreeMap<u64, MetadataInstant>,
    requested_by: BTreeSet<u64>,
    request_count: usize,
}

#[derive(Debug, Default)]
struct TorrentMetadataPeer {
    pending: BTreeSet<u32>,
    cooldown_until: Option<MetadataInstant>,
}

#[derive(Debug)]
pub struct TorrentMetadataDownload {
    expected_info_hash: [u8; 20],
    size: Option<usize>,
    blocks: Vec<TorrentMetadataBlock>,
    peers: BTreeMap<u64, TorrentMetadataPeer>,
    complete: bool,
    hash_failures: usize,
}

impl TorrentMetadataDownload {
    pub fn new(expected_info_hash: [u8; 20]) -> Self {
        Self {
            expected_info_hash,
            size: None,
            blocks: vec![TorrentMetadataBlock::default()],
            peers: BTreeMap::new(),
            complete: false,
            hash_failures: 0,
        }
    }

    pub fn register_peer(
        &mut self,
        peer: u64,
        advertised_size: Option<usize>,
    ) -> Result<(), MetadataError> {
        if !self.peers.contains_key(&peer) {
            if self.peers.len() == MAX_TORRENT_METADATA_PEERS {
                return Err(MetadataError::MetadataPeerLimit {
                    maximum: MAX_TORRENT_METADATA_PEERS,
                });
            }
            self.peers.insert(peer, TorrentMetadataPeer::default());
        }
        if let Some(size) = advertised_size {
            self.accept_size(size)?;
        }
        Ok(())
    }

    pub fn accept_peer_size(&mut self, peer: u64, size: usize) -> Result<(), MetadataError> {
        self.peer(peer)?;
        self.accept_size(size)
    }

    pub fn requests_for_peer(
        &mut self,
        peer: u64,
        now: MetadataInstant,
    ) -> Result<Vec<u32>, MetadataError> {
        if self.complete {
            return Err(MetadataError::AlreadyComplete);
        }
        self.expire_assignments(now);
        let cooling_down = self
            .peer(peer)?
            .cooldown_until
            .is_some_and(|cooldown| cooldown > now);
        if cooling_down {
            return Ok(Vec::new());
        }

        let mut requests = Vec::new();
        loop {
            let pending = self.peer(peer)?.pending.len();
            if pending >= MAX_METADATA_REQUESTS_IN_FLIGHT {
                break;
            }
            let candidate = self
                .blocks
                .iter()
                .enumerate()
                .filter(|(_, block)| {
                    if block.bytes.is_some() || !block.assignments.is_empty() {
                        return false;
                    }
                    if !block.requested_by.contains(&peer) {
                        return true;
                    }
                    !self.peers.iter().any(|(&other, state)| {
                        other != peer
                            && !block.requested_by.contains(&other)
                            && state.pending.len() < MAX_METADATA_REQUESTS_IN_FLIGHT
                            && state.cooldown_until.is_none_or(|cooldown| cooldown <= now)
                    })
                })
                .min_by_key(|(index, block)| (block.request_count, *index))
                .map(|(index, _)| index);
            let Some(index) = candidate else {
                break;
            };
            let piece = u32::try_from(index)
                .map_err(|_| MetadataError::InvalidPiece { piece: i64::MAX })?;
            let block = &mut self.blocks[index];
            block.assignments.insert(peer, now);
            block.requested_by.insert(peer);
            block.request_count = block.request_count.saturating_add(1);
            self.peers
                .get_mut(&peer)
                .expect("registered metadata peer")
                .pending
                .insert(piece);
            requests.push(piece);
        }
        Ok(requests)
    }

    pub fn on_data(
        &mut self,
        peer: u64,
        piece: i64,
        total_size: usize,
        block: &[u8],
        now: MetadataInstant,
    ) -> Result<TorrentMetadataEvent, MetadataError> {
        if self.complete {
            return Err(MetadataError::AlreadyComplete);
        }
        self.peer(peer)?;
        let piece = valid_piece_number(piece)?;
        if self.size.is_none() && piece != 0 {
            return Err(MetadataError::UnsolicitedPiece {
                piece: i64::from(piece),
            });
        }
        self.accept_size(total_size)?;
        let index = usize::try_from(piece).map_err(|_| MetadataError::InvalidPiece {
            piece: i64::from(piece),
        })?;
        let Some(state) = self.blocks.get(index) else {
            return Err(MetadataError::InvalidPiece {
                piece: i64::from(piece),
            });
        };
        if !state.requested_by.contains(&peer) {
            return Err(MetadataError::UnsolicitedPiece {
                piece: i64::from(piece),
            });
        }
        if let Some(existing) = state.bytes.as_deref() {
            return if existing == block {
                Ok(TorrentMetadataEvent::Duplicate { piece })
            } else {
                Err(MetadataError::ConflictingDuplicate { piece })
            };
        }
        let expected = metadata_block_length(total_size, piece)?;
        if block.len() != expected {
            return Err(MetadataError::InvalidBlockLength {
                piece,
                actual: block.len(),
                expected,
            });
        }

        self.release_piece(piece);
        let state = &mut self.blocks[index];
        state.bytes = Some(block.to_vec());
        state.source = Some(peer);
        if self
            .blocks
            .iter()
            .any(|candidate| candidate.bytes.is_none())
        {
            return Ok(TorrentMetadataEvent::BlockAccepted { piece });
        }

        let mut bytes = Vec::with_capacity(total_size);
        for candidate in &self.blocks {
            bytes.extend_from_slice(
                candidate
                    .bytes
                    .as_deref()
                    .expect("all torrent metadata blocks are present"),
            );
        }
        if bytes.len() == total_size
            && <[u8; 20]>::from(Sha1::digest(&bytes)) == self.expected_info_hash
        {
            self.complete = true;
            return Ok(TorrentMetadataEvent::Complete(bytes));
        }

        self.hash_failures = self.hash_failures.saturating_add(1);
        let contributors: BTreeSet<u64> = self
            .blocks
            .iter()
            .filter_map(|candidate| candidate.source)
            .collect();
        let cooldown = if self.blocks.len() == 1 {
            METADATA_SINGLE_BLOCK_HASH_FAILURE_COOLDOWN_MILLIS
        } else {
            METADATA_HASH_FAILURE_COOLDOWN_MILLIS
        };
        let cooldown_until = now.saturating_add(cooldown);
        for contributor in &contributors {
            if let Some(peer_state) = self.peers.get_mut(contributor) {
                peer_state.cooldown_until = Some(cooldown_until);
            }
        }
        for candidate in &mut self.blocks {
            *candidate = TorrentMetadataBlock::default();
        }
        for peer_state in self.peers.values_mut() {
            peer_state.pending.clear();
        }
        Ok(TorrentMetadataEvent::HashMismatch {
            contributors: contributors.into_iter().collect(),
        })
    }

    pub fn on_reject(
        &mut self,
        peer: u64,
        piece: i64,
        now: MetadataInstant,
    ) -> Result<(), MetadataError> {
        let piece = valid_piece_number(piece)?;
        if !self.peer(peer)?.pending.contains(&piece) {
            return Err(MetadataError::UnsolicitedPiece {
                piece: i64::from(piece),
            });
        }
        let cooldown_until = now.saturating_add(METADATA_REJECT_COOLDOWN_MILLIS);
        self.release_peer_assignments(peer);
        self.peers
            .get_mut(&peer)
            .expect("registered metadata peer")
            .cooldown_until = Some(cooldown_until);
        Ok(())
    }

    pub fn remove_peer(&mut self, peer: u64) -> bool {
        if self.peers.remove(&peer).is_none() {
            return false;
        }
        for block in &mut self.blocks {
            block.assignments.remove(&peer);
            block.requested_by.remove(&peer);
        }
        true
    }

    pub fn next_assignment_expiry(&self) -> Option<MetadataInstant> {
        self.blocks
            .iter()
            .flat_map(|block| block.assignments.values())
            .map(|issued| issued.saturating_add(METADATA_ASSIGNMENT_TIMEOUT_MILLIS))
            .min()
    }

    pub fn metadata_size(&self) -> Option<usize> {
        self.size
    }

    pub fn allocated_blocks(&self) -> usize {
        self.size.map_or(0, |_| self.blocks.len())
    }

    pub fn received_blocks(&self) -> usize {
        self.blocks
            .iter()
            .filter(|block| block.bytes.is_some())
            .count()
    }

    pub fn pending_requests(&self) -> usize {
        self.peers.values().map(|peer| peer.pending.len()).sum()
    }

    pub fn pending_requests_for_peer(&self, peer: u64) -> Option<usize> {
        self.peers.get(&peer).map(|state| state.pending.len())
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn hash_failures(&self) -> usize {
        self.hash_failures
    }

    fn peer(&self, peer: u64) -> Result<&TorrentMetadataPeer, MetadataError> {
        self.peers
            .get(&peer)
            .ok_or(MetadataError::UnknownMetadataPeer { peer })
    }

    fn accept_size(&mut self, size: usize) -> Result<(), MetadataError> {
        let size = validate_size(i64::try_from(size).map_err(|_| MetadataError::InvalidSize {
            size: i64::MAX,
            maximum: MAX_METADATA_LENGTH,
        })?)?;
        if let Some(expected) = self.size {
            return if expected == size {
                Ok(())
            } else {
                Err(MetadataError::SizeChanged {
                    expected,
                    actual: size,
                })
            };
        }
        let count = metadata_block_count(size);
        let first = std::mem::take(&mut self.blocks[0]);
        self.blocks = (0..count)
            .map(|index| {
                if index == 0 {
                    TorrentMetadataBlock {
                        assignments: first.assignments.clone(),
                        requested_by: first.requested_by.clone(),
                        request_count: first.request_count,
                        ..TorrentMetadataBlock::default()
                    }
                } else {
                    TorrentMetadataBlock::default()
                }
            })
            .collect();
        self.size = Some(size);
        Ok(())
    }

    fn expire_assignments(&mut self, now: MetadataInstant) {
        let expired: Vec<(usize, u64)> = self
            .blocks
            .iter()
            .enumerate()
            .flat_map(|(index, block)| {
                block
                    .assignments
                    .iter()
                    .filter_map(move |(&peer, &issued)| {
                        (issued.saturating_add(METADATA_ASSIGNMENT_TIMEOUT_MILLIS) <= now)
                            .then_some((index, peer))
                    })
            })
            .collect();
        for (index, peer) in expired {
            self.blocks[index].assignments.remove(&peer);
            if let Some(peer_state) = self.peers.get_mut(&peer)
                && let Ok(piece) = u32::try_from(index)
            {
                peer_state.pending.remove(&piece);
            }
        }
    }

    fn release_piece(&mut self, piece: u32) {
        let Some(block) = self.blocks.get_mut(piece as usize) else {
            return;
        };
        let assigned: Vec<u64> = block.assignments.keys().copied().collect();
        block.assignments.clear();
        for peer in assigned {
            if let Some(peer_state) = self.peers.get_mut(&peer) {
                peer_state.pending.remove(&piece);
            }
        }
    }

    fn release_peer_assignments(&mut self, peer: u64) {
        if let Some(peer_state) = self.peers.get_mut(&peer) {
            peer_state.pending.clear();
        }
        for block in &mut self.blocks {
            block.assignments.remove(&peer);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataUploadAction {
    Data {
        piece: u32,
        total_size: usize,
        block: Vec<u8>,
    },
    Reject {
        piece: i64,
    },
}

#[derive(Debug)]
pub struct MetadataUpload {
    bytes: Vec<u8>,
    served: Vec<bool>,
    request_count: usize,
}

impl MetadataUpload {
    pub fn new(bytes: Vec<u8>) -> Result<Self, MetadataError> {
        let size =
            validate_size(
                i64::try_from(bytes.len()).map_err(|_| MetadataError::InvalidSize {
                    size: i64::MAX,
                    maximum: MAX_METADATA_LENGTH,
                })?,
            )?;
        Ok(Self {
            bytes,
            served: vec![false; metadata_block_count(size)],
            request_count: 0,
        })
    }

    pub fn on_request(&mut self, piece: i64) -> Result<MetadataUploadAction, MetadataError> {
        self.request_count += 1;
        if self.request_count > MAX_METADATA_UPLOAD_REQUESTS {
            return Err(MetadataError::UploadRequestLimit {
                maximum: MAX_METADATA_UPLOAD_REQUESTS,
            });
        }
        let Ok(piece_index) = usize::try_from(piece) else {
            return Ok(MetadataUploadAction::Reject { piece });
        };
        if piece_index >= self.served.len() {
            return Ok(MetadataUploadAction::Reject { piece });
        }
        let begin = piece_index * METADATA_BLOCK_LENGTH;
        let end = (begin + METADATA_BLOCK_LENGTH).min(self.bytes.len());
        self.served[piece_index] = true;
        Ok(MetadataUploadAction::Data {
            piece: piece_index as u32,
            total_size: self.bytes.len(),
            block: self.bytes[begin..end].to_vec(),
        })
    }

    pub fn is_complete(&self) -> bool {
        self.served.iter().all(|served| *served)
    }

    pub fn request_count(&self) -> usize {
        self.request_count
    }
}

pub fn metadata_block_count(total_size: usize) -> usize {
    total_size.div_ceil(METADATA_BLOCK_LENGTH)
}

pub fn metadata_block_length(total_size: usize, piece: u32) -> Result<usize, MetadataError> {
    let count = metadata_block_count(total_size);
    let piece_index = usize::try_from(piece).map_err(|_| MetadataError::InvalidPiece {
        piece: i64::from(piece),
    })?;
    if piece_index >= count {
        return Err(MetadataError::InvalidPiece {
            piece: i64::from(piece),
        });
    }
    let begin = piece_index * METADATA_BLOCK_LENGTH;
    Ok((total_size - begin).min(METADATA_BLOCK_LENGTH))
}

fn extension_limits() -> Limits {
    Limits {
        max_input_length: MAX_EXTENSION_PAYLOAD_LENGTH,
        max_string_length: 4096,
        max_depth: 8,
        max_collection_entries: 128,
    }
}

fn validate_size(size: i64) -> Result<usize, MetadataError> {
    let converted = usize::try_from(size).ok();
    match converted {
        Some(size @ 1..=MAX_METADATA_LENGTH) => Ok(size),
        _ => Err(MetadataError::InvalidSize {
            size,
            maximum: MAX_METADATA_LENGTH,
        }),
    }
}

fn valid_piece_number(piece: i64) -> Result<u32, MetadataError> {
    u32::try_from(piece).map_err(|_| MetadataError::InvalidPiece { piece })
}

fn reject_trailing(consumed: usize, length: usize) -> Result<(), MetadataError> {
    if consumed != length {
        return Err(MetadataError::Bencode(ParseError::TrailingData {
            position: consumed,
        }));
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

fn integer(node: &Node<'_>, field_name: &'static str) -> Result<i64, MetadataError> {
    match node.value {
        Value::Integer(value) => Ok(value),
        _ => Err(MetadataError::InvalidField(field_name)),
    }
}

fn push_integer(output: &mut Vec<u8>, value: i64) {
    output.extend_from_slice(value.to_string().as_bytes());
    output.push(b'e');
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::{
        ExtensionHandshake, MAX_METADATA_LENGTH, MAX_METADATA_REQUESTS_IN_FLIGHT,
        MAX_METADATA_UPLOAD_REQUESTS, MAX_TORRENT_METADATA_PEERS,
        METADATA_ASSIGNMENT_TIMEOUT_MILLIS, METADATA_BLOCK_LENGTH, METADATA_REJECT_COOLDOWN_MILLIS,
        MetadataDownload, MetadataDownloadAction, MetadataError, MetadataExtensionUpdate,
        MetadataInstant, MetadataMessage, MetadataUpload, MetadataUploadAction,
        TorrentMetadataDownload, TorrentMetadataEvent, encode_extension_handshake,
        encode_extension_handshake_with_id, encode_metadata_data, encode_metadata_reject,
        encode_metadata_request, parse_extension_handshake, parse_metadata_message,
    };

    const fn instant(millis: u64) -> MetadataInstant {
        MetadataInstant::from_millis(millis)
    }

    #[test]
    fn extension_handshake_round_trip_and_additive_updates_are_directional() {
        assert_eq!(
            parse_extension_handshake(&encode_extension_handshake(Some(32_000))),
            Ok(ExtensionHandshake {
                metadata_extension: MetadataExtensionUpdate::Enabled(1),
                metadata_size: Some(32_000)
            })
        );
        assert_eq!(
            parse_extension_handshake(
                &encode_extension_handshake_with_id(19, None).expect("nonzero local ID")
            ),
            Ok(ExtensionHandshake {
                metadata_extension: MetadataExtensionUpdate::Enabled(19),
                metadata_size: None
            })
        );
        assert!(encode_extension_handshake_with_id(0, None).is_err());
        assert_eq!(
            parse_extension_handshake(b"d1:md11:ut_metadatai0eee"),
            Ok(ExtensionHandshake {
                metadata_extension: MetadataExtensionUpdate::Disabled,
                metadata_size: None
            })
        );
        assert_eq!(
            parse_extension_handshake(b"d1:md3:fooi7eee"),
            Ok(ExtensionHandshake {
                metadata_extension: MetadataExtensionUpdate::Unchanged,
                metadata_size: None
            })
        );
        assert_eq!(
            parse_extension_handshake(b"d1:v4:teste"),
            Ok(ExtensionHandshake {
                metadata_extension: MetadataExtensionUpdate::Unchanged,
                metadata_size: None
            })
        );
    }

    #[test]
    fn extension_handshake_rejects_bad_mappings_and_sizes() {
        for payload in [
            b"d1:mi1ee".as_slice(),
            b"d1:md11:ut_metadatai-1eee".as_slice(),
            b"d1:md11:ut_metadatai256eee".as_slice(),
            b"d13:metadata_sizei0ee".as_slice(),
            b"d13:metadata_sizei-1ee".as_slice(),
            format!("d13:metadata_sizei{}ee", MAX_METADATA_LENGTH + 1).as_bytes(),
        ] {
            assert!(parse_extension_handshake(payload).is_err());
        }
    }

    #[test]
    fn metadata_codec_separates_the_data_suffix_and_rejects_control_suffixes() {
        let block = vec![7; METADATA_BLOCK_LENGTH];
        let encoded =
            encode_metadata_data(0, METADATA_BLOCK_LENGTH + 3, &block).expect("full first block");
        assert_eq!(
            parse_metadata_message(&encoded),
            Ok(MetadataMessage::Data {
                piece: 0,
                total_size: METADATA_BLOCK_LENGTH + 3,
                block: &block
            })
        );
        assert_eq!(
            parse_metadata_message(&encode_metadata_request(4)),
            Ok(MetadataMessage::Request { piece: 4 })
        );
        assert_eq!(
            parse_metadata_message(&encode_metadata_reject(-1)),
            Ok(MetadataMessage::Reject { piece: -1 })
        );
        assert!(parse_metadata_message(b"d8:msg_typei0e5:piecei0eejunk").is_err());
        assert!(parse_metadata_message(b"d8:msg_typei2e5:piecei0eejunk").is_err());
        assert_eq!(
            parse_metadata_message(b"d8:msg_typei99eeDATA"),
            Ok(MetadataMessage::Unknown { message_type: 99 })
        );
    }

    #[test]
    fn downloader_limits_requests_and_completes_out_of_order_after_hashing() {
        let mut bytes = vec![3; METADATA_BLOCK_LENGTH + 7];
        bytes[0] = b'd';
        let hash = Sha1::digest(&bytes).into();
        let mut download = MetadataDownload::new(hash);

        let actions = download
            .start(Some(bytes.len()))
            .expect("advertised bounded size");
        assert_eq!(
            actions,
            [
                MetadataDownloadAction::Request(0),
                MetadataDownloadAction::Request(1)
            ]
        );
        assert_eq!(download.pending_requests(), MAX_METADATA_REQUESTS_IN_FLIGHT);
        assert_eq!(download.allocated_blocks(), 2);

        assert!(
            download
                .on_message(MetadataMessage::Data {
                    piece: 1,
                    total_size: bytes.len(),
                    block: &bytes[METADATA_BLOCK_LENGTH..]
                })
                .expect("final block first")
                .is_empty()
        );
        assert_eq!(
            download
                .on_message(MetadataMessage::Data {
                    piece: 0,
                    total_size: bytes.len(),
                    block: &bytes[..METADATA_BLOCK_LENGTH]
                })
                .expect("first block completes"),
            [MetadataDownloadAction::Complete(bytes)]
        );
    }

    #[test]
    fn downloader_defers_allocation_without_size_and_checks_every_transition() {
        let bytes = b"tiny metadata";
        let mut download = MetadataDownload::new(Sha1::digest(bytes).into());
        assert_eq!(
            download.start(None).expect("fallback request"),
            [MetadataDownloadAction::Request(0)]
        );
        assert_eq!(download.allocated_blocks(), 0);

        assert_eq!(
            download
                .on_message(MetadataMessage::Data {
                    piece: 0,
                    total_size: bytes.len(),
                    block: bytes
                })
                .expect("fallback response"),
            [MetadataDownloadAction::Complete(bytes.to_vec())]
        );

        let mut oversized = MetadataDownload::new([0; 20]);
        assert!(oversized.start(None).is_ok());
        assert!(matches!(
            oversized.on_message(MetadataMessage::Data {
                piece: 0,
                total_size: MAX_METADATA_LENGTH + 1,
                block: &[]
            }),
            Err(MetadataError::InvalidSize { .. })
        ));
        assert_eq!(oversized.allocated_blocks(), 0);
    }

    #[test]
    fn downloader_rejects_unsolicited_bad_geometry_duplicates_sizes_and_hashes() {
        let bytes = vec![5; METADATA_BLOCK_LENGTH + 1];
        let mut unsolicited = MetadataDownload::new([0; 20]);
        unsolicited.start(None).expect("start fallback");
        assert!(matches!(
            unsolicited.on_message(MetadataMessage::Data {
                piece: 1,
                total_size: bytes.len(),
                block: &bytes[METADATA_BLOCK_LENGTH..]
            }),
            Err(MetadataError::UnsolicitedPiece { piece: 1 })
        ));
        assert_eq!(unsolicited.allocated_blocks(), 0);

        let mut bad_length = MetadataDownload::new([0; 20]);
        bad_length.start(Some(bytes.len())).expect("start");
        assert!(matches!(
            bad_length.on_message(MetadataMessage::Data {
                piece: 0,
                total_size: bytes.len(),
                block: &bytes[..10]
            }),
            Err(MetadataError::InvalidBlockLength { .. })
        ));

        let first = vec![1; METADATA_BLOCK_LENGTH];
        let mut duplicate = MetadataDownload::new([0; 20]);
        duplicate
            .start(Some(METADATA_BLOCK_LENGTH * 3))
            .expect("start");
        duplicate
            .on_message(MetadataMessage::Data {
                piece: 0,
                total_size: METADATA_BLOCK_LENGTH * 3,
                block: &first,
            })
            .expect("first copy");
        assert!(
            duplicate
                .on_message(MetadataMessage::Data {
                    piece: 0,
                    total_size: METADATA_BLOCK_LENGTH * 3,
                    block: &first
                })
                .expect("identical duplicate")
                .is_empty()
        );
        assert!(matches!(
            duplicate.on_message(MetadataMessage::Data {
                piece: 0,
                total_size: METADATA_BLOCK_LENGTH * 3,
                block: &vec![2; METADATA_BLOCK_LENGTH]
            }),
            Err(MetadataError::ConflictingDuplicate { piece: 0 })
        ));
        assert!(matches!(
            duplicate.accept_advertised_size(METADATA_BLOCK_LENGTH * 2),
            Err(MetadataError::SizeChanged { .. })
        ));

        let wrong_hash_bytes = b"wrong";
        let mut wrong_hash = MetadataDownload::new([0; 20]);
        wrong_hash
            .start(Some(wrong_hash_bytes.len()))
            .expect("start");
        assert_eq!(
            wrong_hash.on_message(MetadataMessage::Data {
                piece: 0,
                total_size: wrong_hash_bytes.len(),
                block: wrong_hash_bytes
            }),
            Err(MetadataError::HashMismatch)
        );
    }

    #[test]
    fn torrent_download_combines_disjoint_peer_blocks() {
        let mut bytes = vec![3; METADATA_BLOCK_LENGTH * 2 + 7];
        bytes[0] = b'd';
        let mut download = TorrentMetadataDownload::new(Sha1::digest(&bytes).into());
        download
            .register_peer(1, Some(bytes.len()))
            .expect("register first peer");
        download
            .register_peer(2, None)
            .expect("register second peer");
        assert_eq!(
            download
                .requests_for_peer(1, MetadataInstant::ZERO)
                .expect("first peer requests"),
            [0, 1]
        );
        assert_eq!(
            download
                .requests_for_peer(2, MetadataInstant::ZERO)
                .expect("second peer requests"),
            [2]
        );
        assert_eq!(
            download
                .on_data(
                    1,
                    0,
                    bytes.len(),
                    &bytes[..METADATA_BLOCK_LENGTH],
                    MetadataInstant::ZERO,
                )
                .expect("first block"),
            TorrentMetadataEvent::BlockAccepted { piece: 0 }
        );
        assert_eq!(
            download
                .on_data(
                    2,
                    2,
                    bytes.len(),
                    &bytes[METADATA_BLOCK_LENGTH * 2..],
                    MetadataInstant::ZERO,
                )
                .expect("third block"),
            TorrentMetadataEvent::BlockAccepted { piece: 2 }
        );
        assert_eq!(
            download
                .on_data(
                    1,
                    1,
                    bytes.len(),
                    &bytes[METADATA_BLOCK_LENGTH..METADATA_BLOCK_LENGTH * 2],
                    MetadataInstant::ZERO,
                )
                .expect("combined completion"),
            TorrentMetadataEvent::Complete(bytes)
        );
    }

    #[test]
    fn torrent_download_expires_assignments_and_accepts_late_duplicates() {
        let bytes = vec![4; METADATA_BLOCK_LENGTH + 3];
        let mut download = TorrentMetadataDownload::new(Sha1::digest(&bytes).into());
        for peer in [1, 2] {
            download
                .register_peer(peer, Some(bytes.len()))
                .expect("register peer");
        }
        assert_eq!(
            download
                .requests_for_peer(1, MetadataInstant::ZERO)
                .expect("initial requests"),
            [0, 1]
        );
        assert!(
            download
                .requests_for_peer(2, instant(METADATA_ASSIGNMENT_TIMEOUT_MILLIS - 1))
                .expect("suppressed duplicate")
                .is_empty()
        );
        assert_eq!(
            download
                .requests_for_peer(2, instant(METADATA_ASSIGNMENT_TIMEOUT_MILLIS))
                .expect("expired requests reassign"),
            [0, 1]
        );
        assert_eq!(
            download
                .on_data(
                    1,
                    0,
                    bytes.len(),
                    &bytes[..METADATA_BLOCK_LENGTH],
                    instant(METADATA_ASSIGNMENT_TIMEOUT_MILLIS),
                )
                .expect("late original response"),
            TorrentMetadataEvent::BlockAccepted { piece: 0 }
        );
        assert_eq!(
            download
                .on_data(
                    2,
                    0,
                    bytes.len(),
                    &bytes[..METADATA_BLOCK_LENGTH],
                    instant(METADATA_ASSIGNMENT_TIMEOUT_MILLIS),
                )
                .expect("identical losing response"),
            TorrentMetadataEvent::Duplicate { piece: 0 }
        );
        assert_eq!(
            download
                .on_data(
                    2,
                    1,
                    bytes.len(),
                    &bytes[METADATA_BLOCK_LENGTH..],
                    instant(METADATA_ASSIGNMENT_TIMEOUT_MILLIS),
                )
                .expect("reassigned completion"),
            TorrentMetadataEvent::Complete(bytes)
        );
    }

    #[test]
    fn torrent_download_reject_cools_peer_and_releases_all_work() {
        let bytes = vec![5; METADATA_BLOCK_LENGTH + 1];
        let mut download = TorrentMetadataDownload::new(Sha1::digest(&bytes).into());
        for peer in [1, 2] {
            download
                .register_peer(peer, Some(bytes.len()))
                .expect("register peer");
        }
        assert_eq!(
            download
                .requests_for_peer(1, MetadataInstant::ZERO)
                .expect("first requests"),
            [0, 1]
        );
        download
            .on_reject(1, 0, MetadataInstant::ZERO)
            .expect("reject assigned block");
        assert!(
            download
                .requests_for_peer(1, instant(METADATA_REJECT_COOLDOWN_MILLIS - 1))
                .expect("cooling peer")
                .is_empty()
        );
        assert_eq!(
            download
                .requests_for_peer(2, MetadataInstant::ZERO)
                .expect("replacement peer"),
            [0, 1]
        );
        assert!(download.remove_peer(2));
        assert_eq!(download.pending_requests(), 0);
    }

    #[test]
    fn torrent_download_hash_failure_attributes_resets_and_mixes_sources() {
        let mut correct = vec![6; METADATA_BLOCK_LENGTH * 2 + 11];
        correct[0] = b'd';
        let mut corrupt = correct.clone();
        corrupt[METADATA_BLOCK_LENGTH * 2] ^= 0xff;
        let mut download = TorrentMetadataDownload::new(Sha1::digest(&correct).into());
        for peer in [1, 2, 3] {
            download
                .register_peer(peer, Some(correct.len()))
                .expect("register peer");
        }
        assert_eq!(
            download
                .requests_for_peer(1, MetadataInstant::ZERO)
                .unwrap(),
            [0, 1]
        );
        assert_eq!(
            download
                .requests_for_peer(2, MetadataInstant::ZERO)
                .unwrap(),
            [2]
        );
        assert!(matches!(
            download
                .on_data(
                    1,
                    0,
                    correct.len(),
                    &correct[..METADATA_BLOCK_LENGTH],
                    MetadataInstant::ZERO,
                )
                .unwrap(),
            TorrentMetadataEvent::BlockAccepted { piece: 0 }
        ));
        assert!(matches!(
            download
                .on_data(
                    1,
                    1,
                    correct.len(),
                    &correct[METADATA_BLOCK_LENGTH..METADATA_BLOCK_LENGTH * 2],
                    MetadataInstant::ZERO,
                )
                .unwrap(),
            TorrentMetadataEvent::BlockAccepted { piece: 1 }
        ));
        assert_eq!(
            download
                .on_data(
                    2,
                    2,
                    correct.len(),
                    &corrupt[METADATA_BLOCK_LENGTH * 2..],
                    MetadataInstant::ZERO,
                )
                .expect("hash mismatch is recoverable"),
            TorrentMetadataEvent::HashMismatch {
                contributors: vec![1, 2]
            }
        );
        assert_eq!(download.hash_failures(), 1);
        assert!(
            download
                .requests_for_peer(1, MetadataInstant::ZERO)
                .unwrap()
                .is_empty()
        );
        assert!(
            download
                .requests_for_peer(2, MetadataInstant::ZERO)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            download
                .requests_for_peer(3, MetadataInstant::ZERO)
                .unwrap(),
            [0, 1]
        );
        for piece in [0_u32, 1] {
            let begin = piece as usize * METADATA_BLOCK_LENGTH;
            let end = (begin + METADATA_BLOCK_LENGTH).min(correct.len());
            assert!(matches!(
                download
                    .on_data(
                        3,
                        i64::from(piece),
                        correct.len(),
                        &correct[begin..end],
                        MetadataInstant::ZERO,
                    )
                    .unwrap(),
                TorrentMetadataEvent::BlockAccepted { .. }
            ));
        }
        assert_eq!(
            download
                .requests_for_peer(3, MetadataInstant::ZERO)
                .unwrap(),
            [2]
        );
        assert_eq!(
            download
                .on_data(
                    3,
                    2,
                    correct.len(),
                    &correct[METADATA_BLOCK_LENGTH * 2..],
                    MetadataInstant::ZERO,
                )
                .unwrap(),
            TorrentMetadataEvent::Complete(correct)
        );
    }

    #[test]
    fn torrent_download_preserves_geometry_and_peer_bounds() {
        let mut download = TorrentMetadataDownload::new([0; 20]);
        download.register_peer(1, Some(17)).expect("initial size");
        assert!(matches!(
            download.accept_peer_size(1, 18),
            Err(MetadataError::SizeChanged {
                expected: 17,
                actual: 18
            })
        ));
        assert_eq!(download.metadata_size(), Some(17));
        assert_eq!(download.allocated_blocks(), 1);
        for peer in 2..=MAX_TORRENT_METADATA_PEERS as u64 {
            download.register_peer(peer, None).expect("bounded peer");
        }
        assert!(matches!(
            download.register_peer(MAX_TORRENT_METADATA_PEERS as u64 + 1, None),
            Err(MetadataError::MetadataPeerLimit { .. })
        ));
        assert_eq!(download.peer_count(), MAX_TORRENT_METADATA_PEERS);
    }

    #[test]
    fn upload_serves_exact_blocks_rejects_indices_and_bounds_repeats() {
        let bytes = vec![8; METADATA_BLOCK_LENGTH + 3];
        let mut upload = MetadataUpload::new(bytes.clone()).expect("bounded metadata");

        assert_eq!(
            upload.on_request(-1).expect("reject negative"),
            MetadataUploadAction::Reject { piece: -1 }
        );
        assert_eq!(
            upload.on_request(9).expect("reject overflow"),
            MetadataUploadAction::Reject { piece: 9 }
        );
        assert_eq!(
            upload.on_request(1).expect("last block"),
            MetadataUploadAction::Data {
                piece: 1,
                total_size: bytes.len(),
                block: vec![8; 3]
            }
        );
        assert!(!upload.is_complete());
        assert!(matches!(
            upload.on_request(0).expect("first block"),
            MetadataUploadAction::Data { piece: 0, .. }
        ));
        assert!(upload.is_complete());

        let mut flooded = MetadataUpload::new(vec![1]).expect("one block");
        for _ in 0..MAX_METADATA_UPLOAD_REQUESTS {
            flooded.on_request(0).expect("bounded repeated request");
        }
        assert_eq!(
            flooded.on_request(0),
            Err(MetadataError::UploadRequestLimit {
                maximum: MAX_METADATA_UPLOAD_REQUESTS
            })
        );
    }
}

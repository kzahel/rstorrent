//! Runtime-independent payload upload admission and cancellation state.

use std::collections::{BTreeSet, VecDeque};
use std::net::Ipv4Addr;
use std::sync::Arc;

use rstorrent_protocol::peer_wire::{BlockRequest, MAX_REQUEST_BLOCK_LENGTH, PeerMessage};
use sha1::{Digest, Sha1};

pub const MAX_QUEUED_UPLOAD_REQUESTS: usize = 2_000;
pub const MAX_QUEUED_UPLOAD_BYTES: usize =
    MAX_QUEUED_UPLOAD_REQUESTS * MAX_REQUEST_BLOCK_LENGTH as usize;
pub const MAX_GENERATED_ALLOWED_FAST_PIECES: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadRead {
    pub generation: u64,
    pub request: BlockRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadAction {
    Send(PeerMessage),
    Read(UploadRead),
    Close(UploadCloseReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadCloseReason {
    InvalidRequest,
    RequestLimit,
    ReadFailed,
    ShortRead,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UploadPeerSnapshot {
    pub interested: bool,
    pub choking: bool,
    pub queued_requests: usize,
    pub queued_bytes: usize,
    pub queued_requests_high_water: usize,
    pub queued_bytes_high_water: usize,
    pub read_in_flight: bool,
    pub read_enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingRequest {
    generation: u64,
    request: BlockRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InFlightRequest {
    pending: PendingRequest,
    cancelled: bool,
    terminal_sent: bool,
}

#[derive(Debug)]
pub struct UploadPeerState {
    piece_lengths: Arc<[u32]>,
    available: Arc<[bool]>,
    interested: bool,
    choking: bool,
    fast_extension: bool,
    allowed_fast: BTreeSet<u32>,
    queued: VecDeque<PendingRequest>,
    in_flight: Option<InFlightRequest>,
    read_enabled: bool,
    pending_bytes: usize,
    next_generation: u64,
    queued_requests_high_water: usize,
    queued_bytes_high_water: usize,
}

impl UploadPeerState {
    pub fn new(piece_lengths: Vec<u32>, available: Vec<bool>) -> Result<Self, &'static str> {
        Self::from_shared(piece_lengths.into(), available.into())
    }

    pub fn from_shared(
        piece_lengths: Arc<[u32]>,
        available: Arc<[bool]>,
    ) -> Result<Self, &'static str> {
        if piece_lengths.len() != available.len() {
            return Err("piece lengths and availability must have equal lengths");
        }
        if piece_lengths.contains(&0) {
            return Err("piece lengths must be nonzero");
        }
        Ok(Self {
            piece_lengths,
            available,
            interested: false,
            choking: true,
            fast_extension: false,
            allowed_fast: BTreeSet::new(),
            queued: VecDeque::new(),
            in_flight: None,
            read_enabled: true,
            pending_bytes: 0,
            next_generation: 1,
            queued_requests_high_water: 0,
            queued_bytes_high_water: 0,
        })
    }

    pub fn bitfield(&self) -> Vec<u8> {
        let mut bitfield = vec![0; self.available.len().div_ceil(8)];
        for (index, available) in self.available.iter().copied().enumerate() {
            if available {
                bitfield[index / 8] |= 1 << (7 - index % 8);
            }
        }
        bitfield
    }

    pub fn initial_availability_message(&self, fast_extension: bool) -> PeerMessage {
        if fast_extension && self.available.iter().all(|available| *available) {
            PeerMessage::HaveAll
        } else if fast_extension && self.available.iter().all(|available| !*available) {
            PeerMessage::HaveNone
        } else {
            PeerMessage::Bitfield(self.bitfield())
        }
    }

    pub fn snapshot(&self) -> UploadPeerSnapshot {
        UploadPeerSnapshot {
            interested: self.interested,
            choking: self.choking,
            queued_requests: self.pending_count(),
            queued_bytes: self.pending_bytes,
            queued_requests_high_water: self.queued_requests_high_water,
            queued_bytes_high_water: self.queued_bytes_high_water,
            read_in_flight: self.in_flight.is_some(),
            read_enabled: self.read_enabled,
        }
    }

    pub fn enable_fast_extension(
        &mut self,
        allowed_fast: impl IntoIterator<Item = u32>,
    ) -> Result<(), &'static str> {
        if self.pending_count() != 0 {
            return Err("Fast capability cannot change after upload requests begin");
        }
        let mut retained = BTreeSet::new();
        for piece in allowed_fast {
            let index = usize::try_from(piece).map_err(|_| "allowed-fast piece is invalid")?;
            if index >= self.piece_lengths.len() {
                return Err("allowed-fast piece is outside torrent geometry");
            }
            retained.insert(piece);
        }
        self.fast_extension = true;
        self.allowed_fast = retained;
        Ok(())
    }

    pub fn set_granted(&mut self, granted: bool) -> Vec<UploadAction> {
        if granted {
            if !self.interested || !self.choking {
                return Vec::new();
            }
            self.choking = false;
            let mut actions = vec![UploadAction::Send(PeerMessage::Unchoke)];
            self.start_next_read(&mut actions);
            actions
        } else if self.choking {
            Vec::new()
        } else {
            self.choking = true;
            let mut actions = vec![UploadAction::Send(PeerMessage::Choke)];
            if self.fast_extension {
                self.reject_disallowed_requests(&mut actions);
                self.start_next_read(&mut actions);
            } else {
                self.clear_pending_requests();
            }
            actions
        }
    }

    pub fn set_read_enabled(&mut self, enabled: bool) -> Vec<UploadAction> {
        self.read_enabled = enabled;
        let mut actions = Vec::new();
        self.start_next_read(&mut actions);
        actions
    }

    pub fn on_message(&mut self, message: &PeerMessage) -> Vec<UploadAction> {
        match message {
            PeerMessage::Interested => self.on_interested(),
            PeerMessage::NotInterested => self.on_not_interested(),
            PeerMessage::Request(request) => self.on_request(*request),
            PeerMessage::Cancel(request) => self.on_cancel(*request),
            _ => Vec::new(),
        }
    }

    pub fn on_read_complete(
        &mut self,
        read: UploadRead,
        result: Result<Vec<u8>, ()>,
    ) -> Vec<UploadAction> {
        let Some(in_flight) = self.in_flight.take() else {
            return Vec::new();
        };
        if in_flight.pending.generation != read.generation
            || in_flight.pending.request != read.request
        {
            self.in_flight = Some(in_flight);
            return Vec::new();
        }
        self.pending_bytes = self
            .pending_bytes
            .saturating_sub(read.request.length as usize);
        let mut actions = Vec::new();
        match result {
            Err(()) => {
                if self.fast_extension && !in_flight.terminal_sent {
                    actions.push(UploadAction::Send(PeerMessage::RejectRequest(read.request)));
                }
                actions.push(UploadAction::Close(UploadCloseReason::ReadFailed));
            }
            Ok(block) if block.len() != read.request.length as usize => {
                if self.fast_extension && !in_flight.terminal_sent {
                    actions.push(UploadAction::Send(PeerMessage::RejectRequest(read.request)));
                }
                actions.push(UploadAction::Close(UploadCloseReason::ShortRead));
            }
            Ok(block)
                if !in_flight.cancelled
                    && !in_flight.terminal_sent
                    && self.interested
                    && (!self.choking || self.allowed_fast.contains(&read.request.index)) =>
            {
                actions.push(UploadAction::Send(PeerMessage::Piece {
                    index: read.request.index,
                    begin: read.request.begin,
                    block,
                }));
            }
            Ok(_) if self.fast_extension && !in_flight.terminal_sent => {
                actions.push(UploadAction::Send(PeerMessage::RejectRequest(read.request)));
            }
            Ok(_) => {}
        }
        if !actions
            .iter()
            .any(|action| matches!(action, UploadAction::Close(_)))
        {
            self.start_next_read(&mut actions);
        }
        actions
    }

    fn on_interested(&mut self) -> Vec<UploadAction> {
        self.interested = true;
        Vec::new()
    }

    fn on_not_interested(&mut self) -> Vec<UploadAction> {
        self.interested = false;
        if self.choking {
            return Vec::new();
        }
        self.set_granted(false)
    }

    fn on_request(&mut self, request: BlockRequest) -> Vec<UploadAction> {
        if !self.valid_request(request) {
            return vec![UploadAction::Close(UploadCloseReason::InvalidRequest)];
        }
        if self.choking && (!self.fast_extension || !self.allowed_fast.contains(&request.index))
            || !self.interested
        {
            return self
                .fast_extension
                .then_some(UploadAction::Send(PeerMessage::RejectRequest(request)))
                .into_iter()
                .collect();
        }
        let requested_bytes = request.length as usize;
        if self.pending_count() == MAX_QUEUED_UPLOAD_REQUESTS
            || self
                .pending_bytes
                .checked_add(requested_bytes)
                .is_none_or(|bytes| bytes > MAX_QUEUED_UPLOAD_BYTES)
        {
            return vec![if self.fast_extension {
                UploadAction::Send(PeerMessage::RejectRequest(request))
            } else {
                UploadAction::Close(UploadCloseReason::RequestLimit)
            }];
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.queued.push_back(PendingRequest {
            generation,
            request,
        });
        self.pending_bytes += requested_bytes;
        self.queued_requests_high_water = self.queued_requests_high_water.max(self.pending_count());
        self.queued_bytes_high_water = self.queued_bytes_high_water.max(self.pending_bytes);
        let mut actions = Vec::new();
        self.start_next_read(&mut actions);
        actions
    }

    fn on_cancel(&mut self, request: BlockRequest) -> Vec<UploadAction> {
        let mut removed_bytes = 0_usize;
        let mut removed = 0_usize;
        self.queued.retain(|pending| {
            if pending.request == request {
                removed_bytes = removed_bytes.saturating_add(pending.request.length as usize);
                removed = removed.saturating_add(1);
                false
            } else {
                true
            }
        });
        self.pending_bytes = self.pending_bytes.saturating_sub(removed_bytes);
        let mut actions = Vec::new();
        if self.fast_extension {
            actions.extend(
                (0..removed).map(|_| UploadAction::Send(PeerMessage::RejectRequest(request))),
            );
        }
        if let Some(in_flight) = &mut self.in_flight
            && in_flight.pending.request == request
        {
            in_flight.cancelled = true;
            if self.fast_extension && !in_flight.terminal_sent {
                in_flight.terminal_sent = true;
                actions.push(UploadAction::Send(PeerMessage::RejectRequest(request)));
            }
        }
        actions
    }

    fn start_next_read(&mut self, actions: &mut Vec<UploadAction>) {
        if self.in_flight.is_some() || !self.interested || !self.read_enabled {
            return;
        }
        let position = self.queued.iter().position(|pending| {
            !self.choking || self.allowed_fast.contains(&pending.request.index)
        });
        let Some(pending) = position.and_then(|position| self.queued.remove(position)) else {
            return;
        };
        self.in_flight = Some(InFlightRequest {
            pending,
            cancelled: false,
            terminal_sent: false,
        });
        actions.push(UploadAction::Read(UploadRead {
            generation: pending.generation,
            request: pending.request,
        }));
    }

    fn pending_count(&self) -> usize {
        self.queued.len() + usize::from(self.in_flight.is_some())
    }

    fn clear_pending_requests(&mut self) {
        self.pending_bytes = self
            .in_flight
            .as_ref()
            .map_or(0, |in_flight| in_flight.pending.request.length as usize);
        self.queued.clear();
        if let Some(in_flight) = &mut self.in_flight {
            in_flight.cancelled = true;
        }
    }

    fn reject_disallowed_requests(&mut self, actions: &mut Vec<UploadAction>) {
        let allowed_fast = &self.allowed_fast;
        let mut retained = VecDeque::new();
        while let Some(pending) = self.queued.pop_front() {
            if allowed_fast.contains(&pending.request.index) {
                retained.push_back(pending);
            } else {
                self.pending_bytes = self
                    .pending_bytes
                    .saturating_sub(pending.request.length as usize);
                actions.push(UploadAction::Send(PeerMessage::RejectRequest(
                    pending.request,
                )));
            }
        }
        self.queued = retained;
        if let Some(in_flight) = &mut self.in_flight
            && !allowed_fast.contains(&in_flight.pending.request.index)
            && !in_flight.terminal_sent
        {
            in_flight.cancelled = true;
            in_flight.terminal_sent = true;
            actions.push(UploadAction::Send(PeerMessage::RejectRequest(
                in_flight.pending.request,
            )));
        }
    }

    fn valid_request(&self, request: BlockRequest) -> bool {
        if request.length == 0 || request.length > MAX_REQUEST_BLOCK_LENGTH {
            return false;
        }
        let Ok(index) = usize::try_from(request.index) else {
            return false;
        };
        let Some(&piece_length) = self.piece_lengths.get(index) else {
            return false;
        };
        if !self.available.get(index).copied().unwrap_or(false) || request.begin >= piece_length {
            return false;
        }
        request
            .begin
            .checked_add(request.length)
            .is_some_and(|end| end <= piece_length)
    }
}

pub fn generate_allowed_fast_set(
    info_hash: [u8; 20],
    remote_ip: Ipv4Addr,
    piece_count: usize,
    requested_size: usize,
) -> Result<Vec<u32>, &'static str> {
    if piece_count == 0 || piece_count > u32::MAX as usize {
        return Err("allowed-fast piece count is outside supported geometry");
    }
    let target = requested_size
        .min(MAX_GENERATED_ALLOWED_FAST_PIECES)
        .min(piece_count);
    let mut seed = Vec::with_capacity(24);
    let octets = remote_ip.octets();
    seed.extend_from_slice(&[octets[0], octets[1], octets[2], 0]);
    seed.extend_from_slice(&info_hash);
    let mut retained = BTreeSet::new();
    let mut ordered = Vec::with_capacity(target);
    while ordered.len() < target {
        let digest: [u8; 20] = Sha1::digest(&seed).into();
        seed.clear();
        seed.extend_from_slice(&digest);
        for chunk in digest.chunks_exact(4) {
            let value = u32::from_be_bytes(chunk.try_into().expect("four-byte SHA-1 chunk"));
            let piece = value % piece_count as u32;
            if retained.insert(piece) {
                ordered.push(piece);
                if ordered.len() == target {
                    break;
                }
            }
        }
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use rstorrent_protocol::peer_wire::{BlockRequest, PeerMessage};

    use super::{
        MAX_QUEUED_UPLOAD_BYTES, MAX_QUEUED_UPLOAD_REQUESTS, UploadAction, UploadCloseReason,
        UploadPeerState, UploadRead, generate_allowed_fast_set,
    };

    fn request(index: u32, begin: u32, length: u32) -> BlockRequest {
        BlockRequest {
            index,
            begin,
            length,
        }
    }

    fn interested(state: &mut UploadPeerState) {
        assert!(state.on_message(&PeerMessage::Interested).is_empty());
        assert_eq!(
            state.set_granted(true),
            [UploadAction::Send(PeerMessage::Unchoke)]
        );
    }

    #[test]
    fn bitfield_is_exact_and_zeroes_spare_bits() {
        let state = UploadPeerState::new(
            vec![16; 10],
            vec![
                true, false, true, true, false, false, false, true, true, false,
            ],
        )
        .expect("valid state");
        assert_eq!(state.bitfield(), [0b1011_0001, 0b1000_0000]);
    }

    #[test]
    fn fast_initial_availability_chooses_exactly_one_compact_form() {
        let all = UploadPeerState::new(vec![4; 3], vec![true; 3]).expect("all state");
        let none = UploadPeerState::new(vec![4; 3], vec![false; 3]).expect("none state");
        let mixed = UploadPeerState::new(vec![4; 3], vec![true, false, true]).expect("mixed state");
        assert_eq!(all.initial_availability_message(true), PeerMessage::HaveAll);
        assert_eq!(
            none.initial_availability_message(true),
            PeerMessage::HaveNone
        );
        assert_eq!(
            mixed.initial_availability_message(true),
            PeerMessage::Bitfield(vec![0b1010_0000])
        );
        assert!(matches!(
            all.initial_availability_message(false),
            PeerMessage::Bitfield(_)
        ));
    }

    #[test]
    fn canonical_allowed_fast_generation_matches_bep_6_vectors() {
        let info_hash = [0xaa; 20];
        let address = "80.4.4.200".parse().expect("IPv4 vector");
        assert_eq!(
            generate_allowed_fast_set(info_hash, address, 1_313, 7).expect("seven pieces"),
            [1059, 431, 808, 1217, 287, 376, 1188]
        );
        assert_eq!(
            generate_allowed_fast_set(info_hash, address, 1_313, 9).expect("nine pieces"),
            [1059, 431, 808, 1217, 287, 376, 1188, 353, 508]
        );
        assert_eq!(
            generate_allowed_fast_set(info_hash, address, 3, 32).expect("capped pieces"),
            [1, 2, 0]
        );
        assert!(generate_allowed_fast_set(info_hash, address, 0, 10).is_err());
    }

    #[test]
    fn ignores_requests_until_interested_and_unchoked() {
        let mut state = UploadPeerState::new(vec![16], vec![true]).expect("valid state");
        assert!(
            state
                .on_message(&PeerMessage::Request(request(0, 0, 16)))
                .is_empty()
        );
        interested(&mut state);
        assert_eq!(
            state.on_message(&PeerMessage::Request(request(0, 0, 16))),
            [UploadAction::Read(UploadRead {
                generation: 1,
                request: request(0, 0, 16),
            })]
        );
    }

    #[test]
    fn validates_every_request_boundary_without_overflow() {
        let mut state =
            UploadPeerState::new(vec![16_384, 7], vec![true, false]).expect("valid state");
        interested(&mut state);
        for invalid in [
            request(0, 0, 0),
            request(0, 0, 16_385),
            request(0, 16_384, 1),
            request(0, 16_383, 2),
            request(1, 0, 1),
            request(2, 0, 1),
            request(0, u32::MAX, 2),
        ] {
            assert_eq!(
                state.on_message(&PeerMessage::Request(invalid)),
                [UploadAction::Close(UploadCloseReason::InvalidRequest)]
            );
        }
    }

    #[test]
    fn duplicate_requests_consume_budget_and_cancel_removes_every_match() {
        let mut state = UploadPeerState::new(vec![16_384], vec![true]).expect("valid state");
        interested(&mut state);
        let block = request(0, 0, 16_384);
        for _ in 0..MAX_QUEUED_UPLOAD_REQUESTS {
            let _ = state.on_message(&PeerMessage::Request(block));
        }
        assert_eq!(state.snapshot().queued_requests, MAX_QUEUED_UPLOAD_REQUESTS);
        assert_eq!(state.snapshot().queued_bytes, MAX_QUEUED_UPLOAD_BYTES);
        assert_eq!(
            state.on_message(&PeerMessage::Request(block)),
            [UploadAction::Close(UploadCloseReason::RequestLimit)]
        );
        state.on_message(&PeerMessage::Cancel(block));
        assert_eq!(state.snapshot().queued_requests, 1);
        assert_eq!(state.snapshot().queued_bytes, 16_384);
        let actions = state.on_read_complete(
            UploadRead {
                generation: 1,
                request: block,
            },
            Ok(vec![1; 16_384]),
        );
        assert!(actions.is_empty());
        assert_eq!(state.snapshot().queued_requests, 0);
    }

    #[test]
    fn late_or_cancelled_reads_cannot_serialize_payload() {
        let mut state = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
        interested(&mut state);
        let block = request(0, 0, 4);
        let read = match state.on_message(&PeerMessage::Request(block)).as_slice() {
            [UploadAction::Read(read)] => *read,
            actions => panic!("unexpected actions: {actions:?}"),
        };
        assert!(
            state
                .on_read_complete(
                    UploadRead {
                        generation: read.generation + 1,
                        request: block,
                    },
                    Ok(vec![9; 4]),
                )
                .is_empty()
        );
        state.on_message(&PeerMessage::Cancel(block));
        assert!(state.on_read_complete(read, Ok(vec![9; 4])).is_empty());
    }

    #[test]
    fn successful_read_serializes_exact_piece_and_starts_next() {
        let mut state = UploadPeerState::new(vec![8], vec![true]).expect("valid state");
        interested(&mut state);
        let first = request(0, 0, 4);
        let second = request(0, 4, 4);
        state.on_message(&PeerMessage::Request(first));
        assert!(state.on_message(&PeerMessage::Request(second)).is_empty());
        assert_eq!(
            state.on_read_complete(
                UploadRead {
                    generation: 1,
                    request: first,
                },
                Ok(vec![1, 2, 3, 4]),
            ),
            [
                UploadAction::Send(PeerMessage::Piece {
                    index: 0,
                    begin: 0,
                    block: vec![1, 2, 3, 4],
                }),
                UploadAction::Read(UploadRead {
                    generation: 2,
                    request: second,
                }),
            ]
        );
    }

    #[test]
    fn read_failures_and_short_reads_close_without_payload() {
        for (result, reason) in [
            (Err(()), UploadCloseReason::ReadFailed),
            (Ok(vec![1; 3]), UploadCloseReason::ShortRead),
        ] {
            let mut state = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
            interested(&mut state);
            let block = request(0, 0, 4);
            state.on_message(&PeerMessage::Request(block));
            assert_eq!(
                state.on_read_complete(
                    UploadRead {
                        generation: 1,
                        request: block,
                    },
                    result,
                ),
                [UploadAction::Close(reason)]
            );
        }
    }

    #[test]
    fn not_interested_chokes_clears_queue_and_suppresses_read() {
        let mut state = UploadPeerState::new(vec![8], vec![true]).expect("valid state");
        interested(&mut state);
        state.on_message(&PeerMessage::Request(request(0, 0, 4)));
        state.on_message(&PeerMessage::Request(request(0, 4, 4)));
        assert_eq!(
            state.on_message(&PeerMessage::NotInterested),
            [UploadAction::Send(PeerMessage::Choke)]
        );
        assert_eq!(state.snapshot().queued_requests, 1);
        assert!(
            state
                .on_read_complete(
                    UploadRead {
                        generation: 1,
                        request: request(0, 0, 4),
                    },
                    Ok(vec![1; 4]),
                )
                .is_empty()
        );
    }

    #[test]
    fn fast_choke_precedes_rejects_and_retains_allowed_read() {
        let mut state = UploadPeerState::new(vec![4, 4], vec![true, true]).expect("valid state");
        state
            .enable_fast_extension([1])
            .expect("enable Fast upload");
        interested(&mut state);
        let in_flight = request(0, 0, 4);
        let allowed = request(1, 0, 4);
        let queued = request(0, 0, 4);
        assert!(matches!(
            state
                .on_message(&PeerMessage::Request(in_flight))
                .as_slice(),
            [UploadAction::Read(_)]
        ));
        assert!(state.on_message(&PeerMessage::Request(allowed)).is_empty());
        assert!(state.on_message(&PeerMessage::Request(queued)).is_empty());

        assert_eq!(
            state.set_granted(false),
            [
                UploadAction::Send(PeerMessage::Choke),
                UploadAction::Send(PeerMessage::RejectRequest(queued)),
                UploadAction::Send(PeerMessage::RejectRequest(in_flight)),
            ]
        );
        assert_eq!(state.snapshot().queued_requests, 2);
        assert_eq!(
            state.on_read_complete(
                UploadRead {
                    generation: 1,
                    request: in_flight,
                },
                Ok(vec![1; 4]),
            ),
            [UploadAction::Read(UploadRead {
                generation: 2,
                request: allowed,
            })]
        );
        assert_eq!(
            state.on_read_complete(
                UploadRead {
                    generation: 2,
                    request: allowed,
                },
                Ok(vec![2; 4]),
            ),
            [UploadAction::Send(PeerMessage::Piece {
                index: 1,
                begin: 0,
                block: vec![2; 4],
            })]
        );
        assert_eq!(state.snapshot().queued_requests, 0);
    }

    #[test]
    fn fast_requests_while_choked_are_rejected_unless_allowed() {
        let mut state = UploadPeerState::new(vec![4, 4], vec![true, true]).expect("valid state");
        state
            .enable_fast_extension([1])
            .expect("enable Fast upload");
        assert!(state.on_message(&PeerMessage::Interested).is_empty());
        let ordinary = request(0, 0, 4);
        let allowed = request(1, 0, 4);
        assert_eq!(
            state.on_message(&PeerMessage::Request(ordinary)),
            [UploadAction::Send(PeerMessage::RejectRequest(ordinary))]
        );
        assert_eq!(
            state.on_message(&PeerMessage::Request(allowed)),
            [UploadAction::Read(UploadRead {
                generation: 1,
                request: allowed,
            })]
        );
    }

    #[test]
    fn fast_cancel_and_read_failure_each_emit_one_terminal_response() {
        let block = request(0, 0, 4);
        let mut cancelled = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
        cancelled
            .enable_fast_extension([])
            .expect("enable Fast upload");
        interested(&mut cancelled);
        let read = match cancelled
            .on_message(&PeerMessage::Request(block))
            .as_slice()
        {
            [UploadAction::Read(read)] => *read,
            actions => panic!("unexpected read actions: {actions:?}"),
        };
        assert_eq!(
            cancelled.on_message(&PeerMessage::Cancel(block)),
            [UploadAction::Send(PeerMessage::RejectRequest(block))]
        );
        assert!(cancelled.on_read_complete(read, Ok(vec![1; 4])).is_empty());

        let mut failed = UploadPeerState::new(vec![4], vec![true]).expect("valid state");
        failed
            .enable_fast_extension([])
            .expect("enable Fast upload");
        interested(&mut failed);
        let read = match failed.on_message(&PeerMessage::Request(block)).as_slice() {
            [UploadAction::Read(read)] => *read,
            actions => panic!("unexpected read actions: {actions:?}"),
        };
        assert_eq!(
            failed.on_read_complete(read, Err(())),
            [
                UploadAction::Send(PeerMessage::RejectRequest(block)),
                UploadAction::Close(UploadCloseReason::ReadFailed),
            ]
        );
    }
}

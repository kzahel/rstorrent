use core::fmt;

use super::{
    DH_PRIVATE_EXPONENT_LEN, DH_PUBLIC_KEY_LEN, DhPrivateExponent, DhPublicKey, DhSharedSecret,
    MSE_KNOWN_METHODS, MseCipherPair, MseMethod, MseMethodError, MseRole, obfuscated_skey,
    req1_hash, req2_hash, req3_hash, select_method, validate_selected_method,
};

pub const MSE_MAX_PADDING_LEN: usize = 512;
pub const MSE_HANDSHAKE_BUFFER_LEN: usize = 2048;
const BITTORRENT_HANDSHAKE_LEN: usize = 68;
const VC_LEN: usize = 8;
const PE_CRYPTO_HEADER_LEN: usize = VC_LEN + 4 + 2;

pub struct MseBytes {
    bytes: [u8; MSE_HANDSHAKE_BUFFER_LEN],
    len: usize,
}

impl MseBytes {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; MSE_HANDSHAKE_BUFFER_LEN],
            len: 0,
        }
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, MseHandshakeError> {
        let mut output = Self::new();
        output.append(bytes)?;
        Ok(output)
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes[..self.len]
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), MseHandshakeError> {
        let end = self
            .len
            .checked_add(bytes.len())
            .ok_or(MseHandshakeError::BufferOverflow)?;
        if end > self.bytes.len() {
            return Err(MseHandshakeError::BufferOverflow);
        }
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    fn consume_prefix(&mut self, count: usize) {
        debug_assert!(count <= self.len);
        self.bytes.copy_within(count..self.len, 0);
        self.len -= count;
        self.bytes[self.len..self.len + count].fill(0);
    }
}

impl Default for MseBytes {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MseBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MseBytes")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

pub struct MsePadding {
    bytes: [u8; MSE_MAX_PADDING_LEN],
    len: usize,
}

impl MsePadding {
    pub fn new(bytes: &[u8]) -> Result<Self, MseHandshakeError> {
        if bytes.len() > MSE_MAX_PADDING_LEN {
            return Err(MseHandshakeError::InvalidPaddingLength {
                actual: bytes.len(),
            });
        }
        let mut padding = Self {
            bytes: [0; MSE_MAX_PADDING_LEN],
            len: bytes.len(),
        };
        padding.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(padding)
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            bytes: [0; MSE_MAX_PADDING_LEN],
            len: 0,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl Drop for MsePadding {
    fn drop(&mut self) {
        self.bytes.fill(0);
        self.len = 0;
    }
}

impl fmt::Debug for MsePadding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MsePadding")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

// Keeping the fixed-size send buffer inline makes the state machine's memory
// bound visible and avoids a fallible heap allocation on hostile input. The
// larger enum is deliberate: at most one action exists per handshake.
#[allow(clippy::large_enum_variant)]
pub enum MseAction {
    ComputePublicKey {
        private: DhPrivateExponent,
    },
    ComputeSharedSecret {
        private: DhPrivateExponent,
        remote_public: [u8; DH_PUBLIC_KEY_LEN],
    },
    IdentifyTorrent {
        req2_hash: [u8; 20],
    },
    Send(MseBytes),
}

impl fmt::Debug for MseAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComputePublicKey { .. } => formatter.write_str("ComputePublicKey([REDACTED])"),
            Self::ComputeSharedSecret { .. } => {
                formatter.write_str("ComputeSharedSecret([REDACTED])")
            }
            Self::IdentifyTorrent { .. } => formatter.write_str("IdentifyTorrent([REDACTED])"),
            Self::Send(bytes) => formatter.debug_tuple("Send").field(bytes).finish(),
        }
    }
}

pub enum MseResume {
    PublicKeyComputed {
        private: DhPrivateExponent,
        public: DhPublicKey,
    },
    SharedSecretComputed(DhSharedSecret),
    TorrentIdentified(Option<[u8; 20]>),
    Sent,
}

// Completion carries the same bounded inline storage as a send action. Boxing
// either variant would trade a small, fixed stack cost for fallible allocation.
#[allow(clippy::large_enum_variant)]
pub enum MseStep {
    NeedInput,
    Action(MseAction),
    Complete(MseHandshakeComplete),
}

pub struct MseFeed {
    pub consumed: usize,
    pub step: MseStep,
}

pub struct MseHandshakeComplete {
    pub method: MseMethod,
    pub info_hash: [u8; 20],
    pub ciphers: Option<MseCipherPair>,
    pub carried: MseBytes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MseHandshakeError {
    AlreadyStarted,
    NotStarted,
    ActionOutstanding,
    UnexpectedResume,
    Terminal,
    BufferOverflow,
    InvalidPaddingLength { actual: usize },
    InvalidInitialPayloadLength { actual: usize },
    SyncNotFound,
    InvalidVerificationConstant,
    Method(MseMethodError),
    UnknownTorrent,
    TorrentLookupMismatch,
}

impl From<MseMethodError> for MseHandshakeError {
    fn from(error: MseMethodError) -> Self {
        Self::Method(error)
    }
}

impl fmt::Display for MseHandshakeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyStarted => formatter.write_str("MSE handshake already started"),
            Self::NotStarted => formatter.write_str("MSE handshake has not started"),
            Self::ActionOutstanding => formatter.write_str("MSE action is still outstanding"),
            Self::UnexpectedResume => formatter.write_str("unexpected MSE action result"),
            Self::Terminal => formatter.write_str("MSE handshake is terminal"),
            Self::BufferOverflow => formatter.write_str("MSE handshake buffer limit exceeded"),
            Self::InvalidPaddingLength { actual } => {
                write!(formatter, "invalid MSE padding length {actual}")
            }
            Self::InvalidInitialPayloadLength { actual } => {
                write!(formatter, "invalid MSE initial payload length {actual}")
            }
            Self::SyncNotFound => formatter.write_str("MSE synchronization marker not found"),
            Self::InvalidVerificationConstant => {
                formatter.write_str("invalid MSE verification constant")
            }
            Self::Method(error) => write!(formatter, "invalid MSE method: {error}"),
            Self::UnknownTorrent => formatter.write_str("MSE torrent lookup found no candidate"),
            Self::TorrentLookupMismatch => {
                formatter.write_str("MSE torrent lookup returned a mismatched candidate")
            }
        }
    }
}

enum Configuration {
    Initiator {
        info_hash: [u8; 20],
        offered: u32,
        local_handshake: [u8; BITTORRENT_HANDSHAKE_LEN],
        ia_len: usize,
    },
    Responder {
        allowed: u32,
        prefer_rc4: bool,
    },
}

#[derive(Clone, Copy)]
enum Stage {
    NeedPublicKey,
    WaitRemotePublic,
    InitiatorSearchVc,
    InitiatorPe4Header,
    InitiatorPe4Padding { len: usize, method: MseMethod },
    ResponderSearchReq1,
    ResponderPe3Header,
    ResponderPe3Padding { len: usize, method: MseMethod },
    ResponderPe3Ia { len: usize, method: MseMethod },
    AwaitRemoteHandshake { method: MseMethod },
}

#[derive(Clone, Copy)]
enum Pending {
    PublicKey,
    SharedSecret,
    TorrentLookup,
    Send(SendContinuation),
}

#[derive(Clone, Copy)]
enum SendContinuation {
    InitiatorPe1,
    InitiatorPe3,
    InitiatorRemainder { method: MseMethod },
    ResponderPe2,
    ResponderPe4 { method: MseMethod },
}

pub struct MseHandshake {
    role: MseRole,
    configuration: Configuration,
    stage: Stage,
    pending: Option<Pending>,
    started: bool,
    terminal: bool,
    private: Option<DhPrivateExponent>,
    local_public: Option<DhPublicKey>,
    shared: Option<DhSharedSecret>,
    ciphers: Option<MseCipherPair>,
    pad_ab: Option<MsePadding>,
    pad_cd: Option<MsePadding>,
    sync_marker: [u8; 20],
    sync_marker_len: usize,
    lookup_req2: Option<[u8; 20]>,
    info_hash: Option<[u8; 20]>,
    remote_initial: MseBytes,
    buffer: MseBytes,
}

impl MseHandshake {
    pub fn new_initiator(
        private_entropy: [u8; DH_PRIVATE_EXPONENT_LEN],
        pad_a: MsePadding,
        pad_c: MsePadding,
        info_hash: [u8; 20],
        offered: u32,
        local_handshake: [u8; BITTORRENT_HANDSHAKE_LEN],
        ia_len: usize,
    ) -> Result<Self, MseHandshakeError> {
        if offered & MSE_KNOWN_METHODS == 0 {
            return Err(MseHandshakeError::Method(MseMethodError::NoSupportedMethod));
        }
        if ia_len > BITTORRENT_HANDSHAKE_LEN {
            return Err(MseHandshakeError::InvalidInitialPayloadLength { actual: ia_len });
        }
        Ok(Self {
            role: MseRole::Initiator,
            configuration: Configuration::Initiator {
                info_hash,
                offered,
                local_handshake,
                ia_len,
            },
            stage: Stage::NeedPublicKey,
            pending: None,
            started: false,
            terminal: false,
            private: Some(DhPrivateExponent::from_entropy(private_entropy)),
            local_public: None,
            shared: None,
            ciphers: None,
            pad_ab: Some(pad_a),
            pad_cd: Some(pad_c),
            sync_marker: [0; 20],
            sync_marker_len: 0,
            lookup_req2: None,
            info_hash: Some(info_hash),
            remote_initial: MseBytes::new(),
            buffer: MseBytes::new(),
        })
    }

    pub fn new_responder(
        private_entropy: [u8; DH_PRIVATE_EXPONENT_LEN],
        pad_b: MsePadding,
        pad_d: MsePadding,
        allowed: u32,
        prefer_rc4: bool,
    ) -> Result<Self, MseHandshakeError> {
        if allowed & MSE_KNOWN_METHODS == 0 {
            return Err(MseHandshakeError::Method(MseMethodError::NoSupportedMethod));
        }
        Ok(Self {
            role: MseRole::Responder,
            configuration: Configuration::Responder {
                allowed,
                prefer_rc4,
            },
            stage: Stage::NeedPublicKey,
            pending: None,
            started: false,
            terminal: false,
            private: Some(DhPrivateExponent::from_entropy(private_entropy)),
            local_public: None,
            shared: None,
            ciphers: None,
            pad_ab: Some(pad_b),
            pad_cd: Some(pad_d),
            sync_marker: [0; 20],
            sync_marker_len: 0,
            lookup_req2: None,
            info_hash: None,
            remote_initial: MseBytes::new(),
            buffer: MseBytes::new(),
        })
    }

    pub fn start(&mut self) -> Result<MseStep, MseHandshakeError> {
        if self.started {
            return Err(MseHandshakeError::AlreadyStarted);
        }
        self.started = true;
        let private = self.private.take().ok_or(MseHandshakeError::Terminal)?;
        self.pending = Some(Pending::PublicKey);
        Ok(MseStep::Action(MseAction::ComputePublicKey { private }))
    }

    pub fn feed(&mut self, input: &[u8]) -> Result<MseFeed, MseHandshakeError> {
        self.ensure_drivable()?;
        let held = self
            .buffer
            .len()
            .checked_add(self.remote_initial.len())
            .ok_or(MseHandshakeError::BufferOverflow)?;
        let available = MSE_HANDSHAKE_BUFFER_LEN
            .checked_sub(held)
            .ok_or(MseHandshakeError::BufferOverflow)?;
        let consumed = available.min(input.len());
        self.buffer.append(&input[..consumed])?;
        let step = match self.drive() {
            Ok(step) => step,
            Err(error) => {
                self.terminal = true;
                return Err(error);
            }
        };
        if consumed == 0 && !input.is_empty() && matches!(&step, MseStep::NeedInput) {
            self.terminal = true;
            return Err(MseHandshakeError::BufferOverflow);
        }
        Ok(MseFeed { consumed, step })
    }

    pub fn resume(&mut self, resume: MseResume) -> Result<MseStep, MseHandshakeError> {
        if !self.started {
            return Err(MseHandshakeError::NotStarted);
        }
        if self.terminal {
            return Err(MseHandshakeError::Terminal);
        }
        let pending = self
            .pending
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let result = match (pending, resume) {
            (Pending::PublicKey, MseResume::PublicKeyComputed { private, public }) => {
                self.resume_public_key(private, public)
            }
            (Pending::SharedSecret, MseResume::SharedSecretComputed(shared)) => {
                self.resume_shared_secret(shared)
            }
            (Pending::TorrentLookup, MseResume::TorrentIdentified(info_hash)) => {
                self.resume_torrent_lookup(info_hash)
            }
            (Pending::Send(continuation), MseResume::Sent) => self.resume_send(continuation),
            _ => Err(MseHandshakeError::UnexpectedResume),
        };
        if result.is_err() {
            self.terminal = true;
        }
        result
    }

    fn ensure_drivable(&self) -> Result<(), MseHandshakeError> {
        if !self.started {
            return Err(MseHandshakeError::NotStarted);
        }
        if self.terminal {
            return Err(MseHandshakeError::Terminal);
        }
        if self.pending.is_some() {
            return Err(MseHandshakeError::ActionOutstanding);
        }
        Ok(())
    }

    fn resume_public_key(
        &mut self,
        private: DhPrivateExponent,
        public: DhPublicKey,
    ) -> Result<MseStep, MseHandshakeError> {
        self.private = Some(private);
        self.local_public = Some(public);
        match self.role {
            MseRole::Initiator => {
                let output = self.build_pe1()?;
                self.issue_send(output, SendContinuation::InitiatorPe1)
            }
            MseRole::Responder => {
                self.stage = Stage::WaitRemotePublic;
                self.drive()
            }
        }
    }

    fn resume_shared_secret(
        &mut self,
        shared: DhSharedSecret,
    ) -> Result<MseStep, MseHandshakeError> {
        self.shared = Some(shared);
        match self.role {
            MseRole::Initiator => {
                self.initialize_initiator_ciphers()?;
                let output = self.build_pe3()?;
                self.issue_send(output, SendContinuation::InitiatorPe3)
            }
            MseRole::Responder => {
                let marker = req1_hash(
                    self.shared
                        .as_ref()
                        .ok_or(MseHandshakeError::UnexpectedResume)?,
                );
                self.sync_marker.copy_from_slice(&marker);
                self.sync_marker_len = marker.len();
                let output = self.build_pe2()?;
                self.issue_send(output, SendContinuation::ResponderPe2)
            }
        }
    }

    fn resume_torrent_lookup(
        &mut self,
        info_hash: Option<[u8; 20]>,
    ) -> Result<MseStep, MseHandshakeError> {
        let info_hash = info_hash.ok_or(MseHandshakeError::UnknownTorrent)?;
        let expected = self
            .lookup_req2
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        if req2_hash(&info_hash) != expected {
            return Err(MseHandshakeError::TorrentLookupMismatch);
        }
        self.info_hash = Some(info_hash);
        let shared = self
            .shared
            .as_ref()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        self.ciphers = Some(MseCipherPair::new(MseRole::Responder, shared, &info_hash));
        self.shared = None;
        self.buffer.consume_prefix(40);
        self.stage = Stage::ResponderPe3Header;
        self.drive()
    }

    fn resume_send(
        &mut self,
        continuation: SendContinuation,
    ) -> Result<MseStep, MseHandshakeError> {
        self.stage = match continuation {
            SendContinuation::InitiatorPe1 => Stage::WaitRemotePublic,
            SendContinuation::InitiatorPe3 => Stage::InitiatorSearchVc,
            SendContinuation::InitiatorRemainder { method }
            | SendContinuation::ResponderPe4 { method } => Stage::AwaitRemoteHandshake { method },
            SendContinuation::ResponderPe2 => Stage::ResponderSearchReq1,
        };
        self.drive()
    }

    fn drive(&mut self) -> Result<MseStep, MseHandshakeError> {
        if self.pending.is_some() {
            return Err(MseHandshakeError::ActionOutstanding);
        }
        loop {
            match self.stage {
                Stage::NeedPublicKey => return Err(MseHandshakeError::NotStarted),
                Stage::WaitRemotePublic => {
                    if self.buffer.len() < DH_PUBLIC_KEY_LEN {
                        return Ok(MseStep::NeedInput);
                    }
                    let mut remote_public = [0_u8; DH_PUBLIC_KEY_LEN];
                    remote_public.copy_from_slice(&self.buffer.as_slice()[..DH_PUBLIC_KEY_LEN]);
                    self.buffer.consume_prefix(DH_PUBLIC_KEY_LEN);
                    let private = self
                        .private
                        .take()
                        .ok_or(MseHandshakeError::UnexpectedResume)?;
                    self.pending = Some(Pending::SharedSecret);
                    return Ok(MseStep::Action(MseAction::ComputeSharedSecret {
                        private,
                        remote_public,
                    }));
                }
                Stage::InitiatorSearchVc => {
                    let marker = self.sync_marker;
                    let Some(offset) =
                        find_sync(self.buffer.as_slice(), &marker[..self.sync_marker_len])?
                    else {
                        return Ok(MseStep::NeedInput);
                    };
                    self.buffer.consume_prefix(offset);
                    self.stage = Stage::InitiatorPe4Header;
                }
                Stage::InitiatorPe4Header => {
                    if self.buffer.len() < PE_CRYPTO_HEADER_LEN {
                        return Ok(MseStep::NeedInput);
                    }
                    self.decrypt_prefix(PE_CRYPTO_HEADER_LEN)?;
                    if self.buffer.as_slice()[..VC_LEN] != [0_u8; VC_LEN] {
                        return Err(MseHandshakeError::InvalidVerificationConstant);
                    }
                    let selected = read_u32(&self.buffer.as_slice()[VC_LEN..VC_LEN + 4]);
                    let offered = match self.configuration {
                        Configuration::Initiator { offered, .. } => offered,
                        Configuration::Responder { .. } => {
                            return Err(MseHandshakeError::UnexpectedResume);
                        }
                    };
                    let method = validate_selected_method(offered, selected)?;
                    let pad_len = usize::from(read_u16(
                        &self.buffer.as_slice()[VC_LEN + 4..PE_CRYPTO_HEADER_LEN],
                    ));
                    if pad_len > MSE_MAX_PADDING_LEN {
                        return Err(MseHandshakeError::InvalidPaddingLength { actual: pad_len });
                    }
                    self.buffer.consume_prefix(PE_CRYPTO_HEADER_LEN);
                    self.stage = Stage::InitiatorPe4Padding {
                        len: pad_len,
                        method,
                    };
                }
                Stage::InitiatorPe4Padding { len, method } => {
                    if self.buffer.len() < len {
                        return Ok(MseStep::NeedInput);
                    }
                    self.decrypt_prefix(len)?;
                    self.buffer.consume_prefix(len);
                    let (local_handshake, ia_len) = match self.configuration {
                        Configuration::Initiator {
                            local_handshake,
                            ia_len,
                            ..
                        } => (local_handshake, ia_len),
                        Configuration::Responder { .. } => {
                            return Err(MseHandshakeError::UnexpectedResume);
                        }
                    };
                    if ia_len < BITTORRENT_HANDSHAKE_LEN {
                        let mut remainder = MseBytes::from_slice(&local_handshake[ia_len..])?;
                        if method == MseMethod::Rc4 {
                            self.ciphers
                                .as_mut()
                                .ok_or(MseHandshakeError::UnexpectedResume)?
                                .apply_send(remainder.as_mut_slice());
                        }
                        return self.issue_send(
                            remainder,
                            SendContinuation::InitiatorRemainder { method },
                        );
                    }
                    self.stage = Stage::AwaitRemoteHandshake { method };
                }
                Stage::ResponderSearchReq1 => {
                    let marker = self.sync_marker;
                    let Some(offset) =
                        find_sync(self.buffer.as_slice(), &marker[..self.sync_marker_len])?
                    else {
                        return Ok(MseStep::NeedInput);
                    };
                    self.buffer.consume_prefix(offset);
                    if self.buffer.len() < 40 {
                        return Ok(MseStep::NeedInput);
                    }
                    let shared = self
                        .shared
                        .as_ref()
                        .ok_or(MseHandshakeError::UnexpectedResume)?;
                    let req3 = req3_hash(shared);
                    let mut candidate = [0_u8; 20];
                    for index in 0..candidate.len() {
                        candidate[index] = self.buffer.as_slice()[20 + index] ^ req3[index];
                    }
                    self.lookup_req2 = Some(candidate);
                    self.pending = Some(Pending::TorrentLookup);
                    return Ok(MseStep::Action(MseAction::IdentifyTorrent {
                        req2_hash: candidate,
                    }));
                }
                Stage::ResponderPe3Header => {
                    if self.buffer.len() < PE_CRYPTO_HEADER_LEN {
                        return Ok(MseStep::NeedInput);
                    }
                    self.decrypt_prefix(PE_CRYPTO_HEADER_LEN)?;
                    if self.buffer.as_slice()[..VC_LEN] != [0_u8; VC_LEN] {
                        return Err(MseHandshakeError::InvalidVerificationConstant);
                    }
                    let offered = read_u32(&self.buffer.as_slice()[VC_LEN..VC_LEN + 4]);
                    let (allowed, prefer_rc4) = match self.configuration {
                        Configuration::Responder {
                            allowed,
                            prefer_rc4,
                        } => (allowed, prefer_rc4),
                        Configuration::Initiator { .. } => {
                            return Err(MseHandshakeError::UnexpectedResume);
                        }
                    };
                    let method = select_method(offered, allowed, prefer_rc4)?;
                    let pad_len = usize::from(read_u16(
                        &self.buffer.as_slice()[VC_LEN + 4..PE_CRYPTO_HEADER_LEN],
                    ));
                    if pad_len > MSE_MAX_PADDING_LEN {
                        return Err(MseHandshakeError::InvalidPaddingLength { actual: pad_len });
                    }
                    self.buffer.consume_prefix(PE_CRYPTO_HEADER_LEN);
                    self.stage = Stage::ResponderPe3Padding {
                        len: pad_len,
                        method,
                    };
                }
                Stage::ResponderPe3Padding { len, method } => {
                    let needed = len + 2;
                    if self.buffer.len() < needed {
                        return Ok(MseStep::NeedInput);
                    }
                    self.decrypt_prefix(needed)?;
                    let ia_len = usize::from(read_u16(&self.buffer.as_slice()[len..needed]));
                    if ia_len > BITTORRENT_HANDSHAKE_LEN {
                        return Err(MseHandshakeError::InvalidInitialPayloadLength {
                            actual: ia_len,
                        });
                    }
                    self.buffer.consume_prefix(needed);
                    self.stage = Stage::ResponderPe3Ia {
                        len: ia_len,
                        method,
                    };
                }
                Stage::ResponderPe3Ia { len, method } => {
                    if self.buffer.len() < len {
                        return Ok(MseStep::NeedInput);
                    }
                    self.decrypt_prefix(len)?;
                    self.remote_initial.append(&self.buffer.as_slice()[..len])?;
                    self.buffer.consume_prefix(len);
                    let output = self.build_pe4(method)?;
                    return self.issue_send(output, SendContinuation::ResponderPe4 { method });
                }
                Stage::AwaitRemoteHandshake { method } => {
                    if method == MseMethod::Rc4 && !self.buffer.is_empty() {
                        let len = self.buffer.len();
                        self.decrypt_prefix(len)?;
                    }
                    let needed = BITTORRENT_HANDSHAKE_LEN - self.remote_initial.len();
                    let consumed = needed.min(self.buffer.len());
                    self.remote_initial
                        .append(&self.buffer.as_slice()[..consumed])?;
                    self.buffer.consume_prefix(consumed);
                    if self.remote_initial.len() < BITTORRENT_HANDSHAKE_LEN {
                        return Ok(MseStep::NeedInput);
                    }
                    return self.finish(method);
                }
            }
        }
    }

    fn initialize_initiator_ciphers(&mut self) -> Result<(), MseHandshakeError> {
        let info_hash = self.info_hash.ok_or(MseHandshakeError::UnexpectedResume)?;
        let shared = self
            .shared
            .as_ref()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let mut marker_pair = MseCipherPair::new(MseRole::Initiator, shared, &info_hash);
        let mut marker = [0_u8; VC_LEN];
        marker_pair.apply_receive(&mut marker);
        self.sync_marker[..VC_LEN].copy_from_slice(&marker);
        self.sync_marker_len = VC_LEN;
        self.ciphers = Some(MseCipherPair::new(MseRole::Initiator, shared, &info_hash));
        Ok(())
    }

    fn build_pe1(&mut self) -> Result<MseBytes, MseHandshakeError> {
        let public = self
            .local_public
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let padding = self
            .pad_ab
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let mut output = MseBytes::from_slice(public.as_bytes())?;
        output.append(padding.as_slice())?;
        Ok(output)
    }

    fn build_pe2(&mut self) -> Result<MseBytes, MseHandshakeError> {
        let public = self
            .local_public
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let padding = self
            .pad_ab
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let mut output = MseBytes::from_slice(public.as_bytes())?;
        output.append(padding.as_slice())?;
        Ok(output)
    }

    fn build_pe3(&mut self) -> Result<MseBytes, MseHandshakeError> {
        let (info_hash, offered, local_handshake, ia_len) = match self.configuration {
            Configuration::Initiator {
                info_hash,
                offered,
                local_handshake,
                ia_len,
            } => (info_hash, offered, local_handshake, ia_len),
            Configuration::Responder { .. } => return Err(MseHandshakeError::UnexpectedResume),
        };
        let shared = self
            .shared
            .as_ref()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let clear_req1 = req1_hash(shared);
        let clear_skey = obfuscated_skey(shared, &info_hash);
        let padding = self
            .pad_cd
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let mut encrypted = MseBytes::new();
        encrypted.append(&[0; VC_LEN])?;
        encrypted.append(&offered.to_be_bytes())?;
        encrypted.append(&(padding.len as u16).to_be_bytes())?;
        encrypted.append(padding.as_slice())?;
        encrypted.append(&(ia_len as u16).to_be_bytes())?;
        encrypted.append(&local_handshake[..ia_len])?;
        self.ciphers
            .as_mut()
            .ok_or(MseHandshakeError::UnexpectedResume)?
            .apply_send(encrypted.as_mut_slice());

        let mut output = MseBytes::from_slice(&clear_req1)?;
        output.append(&clear_skey)?;
        output.append(encrypted.as_slice())?;
        self.shared = None;
        Ok(output)
    }

    fn build_pe4(&mut self, method: MseMethod) -> Result<MseBytes, MseHandshakeError> {
        let padding = self
            .pad_cd
            .take()
            .ok_or(MseHandshakeError::UnexpectedResume)?;
        let mut output = MseBytes::new();
        output.append(&[0; VC_LEN])?;
        output.append(&method.wire_bit().to_be_bytes())?;
        output.append(&(padding.len as u16).to_be_bytes())?;
        output.append(padding.as_slice())?;
        self.ciphers
            .as_mut()
            .ok_or(MseHandshakeError::UnexpectedResume)?
            .apply_send(output.as_mut_slice());
        Ok(output)
    }

    fn decrypt_prefix(&mut self, len: usize) -> Result<(), MseHandshakeError> {
        self.ciphers
            .as_mut()
            .ok_or(MseHandshakeError::UnexpectedResume)?
            .apply_receive(&mut self.buffer.as_mut_slice()[..len]);
        Ok(())
    }

    fn issue_send(
        &mut self,
        output: MseBytes,
        continuation: SendContinuation,
    ) -> Result<MseStep, MseHandshakeError> {
        self.pending = Some(Pending::Send(continuation));
        Ok(MseStep::Action(MseAction::Send(output)))
    }

    fn finish(&mut self, method: MseMethod) -> Result<MseStep, MseHandshakeError> {
        let info_hash = self.info_hash.ok_or(MseHandshakeError::UnknownTorrent)?;
        let mut carried = MseBytes::new();
        carried.append(self.remote_initial.as_slice())?;
        carried.append(self.buffer.as_slice())?;
        let ciphers = if method == MseMethod::Rc4 {
            self.ciphers.take()
        } else {
            self.ciphers = None;
            None
        };
        self.shared = None;
        self.terminal = true;
        Ok(MseStep::Complete(MseHandshakeComplete {
            method,
            info_hash,
            ciphers,
            carried,
        }))
    }
}

fn find_sync(buffer: &[u8], marker: &[u8]) -> Result<Option<usize>, MseHandshakeError> {
    if buffer.len() < marker.len() {
        return Ok(None);
    }
    let last_start = (buffer.len() - marker.len()).min(MSE_MAX_PADDING_LEN);
    for offset in 0..=last_start {
        if &buffer[offset..offset + marker.len()] == marker {
            return Ok(Some(offset));
        }
    }
    if buffer.len() >= MSE_MAX_PADDING_LEN + marker.len() {
        return Err(MseHandshakeError::SyncNotFound);
    }
    Ok(None)
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use crate::mse::{
        MSE_METHOD_PLAINTEXT, MSE_METHOD_RC4, compute_public_key, compute_shared_secret,
    };
    use crate::peer_wire::{decode_handshake, encode_handshake};

    use super::*;

    const INFO_HASH: [u8; 20] = [0x44; 20];
    const INITIATOR_ID: [u8; 20] = *b"-RS-MSE-A--000000000";
    const RESPONDER_ID: [u8; 20] = *b"-RS-MSE-B--000000000";
    const FIRST_FRAME: [u8; 5] = [0, 0, 0, 1, 2];

    #[test]
    fn both_methods_and_every_initial_payload_length_survive_fragmentation() {
        for method in [MseMethod::PlaintextPayload, MseMethod::Rc4] {
            for ia_len in 0..=BITTORRENT_HANDSHAKE_LEN {
                let outcome = run_pair(method, ia_len, 1, 0);
                assert_eq!(outcome.initiator_method, method);
                assert_eq!(outcome.responder_method, method);
                assert_eq!(
                    &outcome.initiator_carried[..68],
                    &outcome.responder_handshake
                );
                assert_eq!(
                    &outcome.responder_carried[..68],
                    &outcome.initiator_handshake
                );
            }
        }
    }

    #[test]
    fn zero_and_maximum_padding_work_with_coalesced_and_split_input() {
        for pad_len in [0, MSE_MAX_PADDING_LEN] {
            for chunk in [1, 7, 68, MSE_HANDSHAKE_BUFFER_LEN] {
                let outcome = run_pair(MseMethod::Rc4, 68, chunk, pad_len);
                assert_eq!(outcome.initiator_method, MseMethod::Rc4);
                assert_eq!(outcome.responder_method, MseMethod::Rc4);
            }
        }
    }

    #[test]
    fn coalesced_post_handshake_frame_is_carried_once_in_both_methods() {
        for method in [MseMethod::PlaintextPayload, MseMethod::Rc4] {
            let outcome = run_pair(method, 68, MSE_HANDSHAKE_BUFFER_LEN, 512);
            assert_eq!(
                &outcome.initiator_carried[BITTORRENT_HANDSHAKE_LEN..],
                &FIRST_FRAME
            );
        }
    }

    #[test]
    fn seeded_chunk_splits_cover_handshake_field_boundaries() {
        for method in [MseMethod::PlaintextPayload, MseMethod::Rc4] {
            for ia_len in [0, 1, 18, 19, 20, 21, 47, 48, 49, 67, 68] {
                let mut seed = 0x9e37_79b9_7f4a_7c15_u64 ^ ia_len as u64;
                let outcome = run_pair_with_chunker(method, ia_len, 37, || {
                    seed = seed
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    ((seed >> 32) as usize % 97) + 1
                });
                assert_eq!(outcome.initiator_method, method);
                assert_eq!(outcome.responder_method, method);
            }
        }
    }

    #[test]
    fn sync_accepts_every_offset_and_fails_only_after_offset_512_is_excluded() {
        let marker = [0x91; 20];
        for offset in 0..=MSE_MAX_PADDING_LEN {
            let mut buffer = vec![0x22; offset];
            buffer.extend_from_slice(&marker);
            assert_eq!(find_sync(&buffer, &marker), Ok(Some(offset)));
        }
        let absent = vec![0x22; MSE_MAX_PADDING_LEN + marker.len() - 1];
        assert_eq!(find_sync(&absent, &marker), Ok(None));
        let absent = vec![0x22; MSE_MAX_PADDING_LEN + marker.len()];
        assert_eq!(
            find_sync(&absent, &marker),
            Err(MseHandshakeError::SyncNotFound)
        );
    }

    #[test]
    fn action_contract_rejects_feed_and_wrong_resume_while_outstanding() {
        let mut handshake = initiator(MseMethod::Rc4, 68, 0);
        assert!(matches!(
            handshake.start(),
            Ok(MseStep::Action(MseAction::ComputePublicKey { .. }))
        ));
        assert!(matches!(
            handshake.feed(&[0; 96]),
            Err(MseHandshakeError::ActionOutstanding)
        ));
        assert!(matches!(
            handshake.resume(MseResume::Sent),
            Err(MseHandshakeError::UnexpectedResume)
        ));
        assert!(matches!(
            handshake.feed(&[]),
            Err(MseHandshakeError::Terminal)
        ));
    }

    #[test]
    fn torrent_lookup_none_and_mismatch_are_terminal() {
        for (candidate, expected) in [
            (None, MseHandshakeError::UnknownTorrent),
            (Some([0x55; 20]), MseHandshakeError::TorrentLookupMismatch),
        ] {
            let mut handshake = responder(MseMethod::Rc4, 0);
            handshake.started = true;
            handshake.pending = Some(Pending::TorrentLookup);
            handshake.lookup_req2 = Some(req2_hash(&INFO_HASH));
            assert!(matches!(
                handshake.resume(MseResume::TorrentIdentified(candidate)),
                Err(error) if error == expected
            ));
            assert!(matches!(
                handshake.feed(&[]),
                Err(MseHandshakeError::Terminal)
            ));
        }
    }

    #[test]
    fn responder_rejects_hostile_pe3_fields_and_accepts_extension_bits() {
        for (verification, offered, pad_len, expected) in [
            (
                [1; VC_LEN],
                MSE_METHOD_RC4,
                0,
                MseHandshakeError::InvalidVerificationConstant,
            ),
            (
                [0; VC_LEN],
                0,
                0,
                MseHandshakeError::Method(MseMethodError::NoSupportedMethod),
            ),
            (
                [0; VC_LEN],
                0x8000_0000,
                0,
                MseHandshakeError::Method(MseMethodError::NoSupportedMethod),
            ),
            (
                [0; VC_LEN],
                MSE_METHOD_RC4,
                513,
                MseHandshakeError::InvalidPaddingLength { actual: 513 },
            ),
            (
                [0; VC_LEN],
                MSE_METHOD_RC4,
                u16::MAX,
                MseHandshakeError::InvalidPaddingLength {
                    actual: usize::from(u16::MAX),
                },
            ),
        ] {
            let (mut handshake, mut sender) = responder_pe3_parser();
            let mut wire = encrypted_crypto_header(&mut sender, verification, offered, pad_len);
            assert_eq!(feed_error(&mut handshake, &wire), expected);
            wire.fill(0);
            assert!(matches!(
                handshake.feed(&[]),
                Err(MseHandshakeError::Terminal)
            ));
        }

        let (mut handshake, mut sender) = responder_pe3_parser();
        let mut wire =
            encrypted_crypto_header(&mut sender, [0; VC_LEN], MSE_METHOD_RC4 | 0x8000_0000, 0);
        let mut ia_len = 0_u16.to_be_bytes();
        sender.apply_send(&mut ia_len);
        wire.extend_from_slice(&ia_len);
        assert!(matches!(
            handshake.feed(&wire),
            Ok(MseFeed {
                step: MseStep::Action(MseAction::Send(_)),
                ..
            })
        ));
    }

    #[test]
    fn responder_rejects_initial_payload_length_above_bit_torrent_handshake() {
        let (mut handshake, mut sender) = responder_pe3_parser();
        let mut wire = encrypted_crypto_header(&mut sender, [0; VC_LEN], MSE_METHOD_RC4, 0);
        let mut ia_len = 69_u16.to_be_bytes();
        sender.apply_send(&mut ia_len);
        wire.extend_from_slice(&ia_len);
        assert_eq!(
            feed_error(&mut handshake, &wire),
            MseHandshakeError::InvalidInitialPayloadLength { actual: 69 }
        );
    }

    #[test]
    fn initiator_rejects_hostile_pe4_fields_and_accepts_extension_bits() {
        for (verification, selected, expected) in [
            (
                [1; VC_LEN],
                MSE_METHOD_RC4,
                MseHandshakeError::InvalidVerificationConstant,
            ),
            (
                [0; VC_LEN],
                0,
                MseHandshakeError::Method(MseMethodError::AmbiguousSelection),
            ),
            (
                [0; VC_LEN],
                MSE_KNOWN_METHODS,
                MseHandshakeError::Method(MseMethodError::AmbiguousSelection),
            ),
            (
                [0; VC_LEN],
                0x8000_0000,
                MseHandshakeError::Method(MseMethodError::AmbiguousSelection),
            ),
            (
                [0; VC_LEN],
                MSE_METHOD_PLAINTEXT,
                MseHandshakeError::Method(MseMethodError::SelectedMethodNotOffered),
            ),
        ] {
            let (mut handshake, mut sender) = initiator_pe4_parser(MSE_METHOD_RC4);
            let wire = encrypted_crypto_header(&mut sender, verification, selected, 0);
            assert_eq!(feed_error(&mut handshake, &wire), expected);
        }

        let (mut handshake, mut sender) = initiator_pe4_parser(MSE_METHOD_RC4);
        let wire =
            encrypted_crypto_header(&mut sender, [0; VC_LEN], MSE_METHOD_RC4 | 0x8000_0000, 0);
        assert!(matches!(
            handshake.feed(&wire),
            Ok(MseFeed {
                step: MseStep::NeedInput,
                ..
            })
        ));
    }

    #[test]
    fn initiator_rejects_hostile_pe4_padding_lengths() {
        for pad_len in [513, u16::MAX] {
            let (mut handshake, mut sender) = initiator_pe4_parser(MSE_METHOD_RC4);
            let wire = encrypted_crypto_header(&mut sender, [0; VC_LEN], MSE_METHOD_RC4, pad_len);
            assert_eq!(
                feed_error(&mut handshake, &wire),
                MseHandshakeError::InvalidPaddingLength {
                    actual: usize::from(pad_len),
                }
            );
        }
    }

    #[test]
    fn constructors_reject_local_bounds_and_unknown_only_methods() {
        assert_eq!(
            MsePadding::new(&[0; MSE_MAX_PADDING_LEN + 1]).unwrap_err(),
            MseHandshakeError::InvalidPaddingLength {
                actual: MSE_MAX_PADDING_LEN + 1
            }
        );
        assert!(matches!(
            MseHandshake::new_initiator(
                [1; 20],
                MsePadding::empty(),
                MsePadding::empty(),
                INFO_HASH,
                0x8000_0000,
                encode_handshake(INFO_HASH, INITIATOR_ID),
                68,
            ),
            Err(MseHandshakeError::Method(MseMethodError::NoSupportedMethod))
        ));
        assert!(matches!(
            MseHandshake::new_initiator(
                [1; 20],
                MsePadding::empty(),
                MsePadding::empty(),
                INFO_HASH,
                MSE_KNOWN_METHODS,
                encode_handshake(INFO_HASH, INITIATOR_ID),
                69,
            ),
            Err(MseHandshakeError::InvalidInitialPayloadLength { actual: 69 })
        ));
    }

    struct PairOutcome {
        initiator_method: MseMethod,
        responder_method: MseMethod,
        initiator_handshake: [u8; 68],
        responder_handshake: [u8; 68],
        initiator_carried: Vec<u8>,
        responder_carried: Vec<u8>,
    }

    fn responder_pe3_parser() -> (MseHandshake, MseCipherPair) {
        let (initiator_shared, responder_shared) = deterministic_shared_pair();
        let mut handshake = responder(MseMethod::Rc4, 0);
        handshake.started = true;
        handshake.stage = Stage::ResponderPe3Header;
        handshake.info_hash = Some(INFO_HASH);
        handshake.ciphers = Some(MseCipherPair::new(
            MseRole::Responder,
            &responder_shared,
            &INFO_HASH,
        ));
        let sender = MseCipherPair::new(MseRole::Initiator, &initiator_shared, &INFO_HASH);
        (handshake, sender)
    }

    fn initiator_pe4_parser(offered: u32) -> (MseHandshake, MseCipherPair) {
        let (initiator_shared, responder_shared) = deterministic_shared_pair();
        let mut handshake = MseHandshake::new_initiator(
            [0x11; 20],
            MsePadding::empty(),
            MsePadding::empty(),
            INFO_HASH,
            offered,
            encode_handshake(INFO_HASH, INITIATOR_ID),
            BITTORRENT_HANDSHAKE_LEN,
        )
        .expect("initiator parser");
        handshake.started = true;
        handshake.stage = Stage::InitiatorPe4Header;
        handshake.ciphers = Some(MseCipherPair::new(
            MseRole::Initiator,
            &initiator_shared,
            &INFO_HASH,
        ));
        let sender = MseCipherPair::new(MseRole::Responder, &responder_shared, &INFO_HASH);
        (handshake, sender)
    }

    fn deterministic_shared_pair() -> (DhSharedSecret, DhSharedSecret) {
        let initiator_private = DhPrivateExponent::from_entropy([0x11; 20]);
        let responder_private = DhPrivateExponent::from_entropy([0x91; 20]);
        let initiator_public = compute_public_key(&initiator_private);
        let responder_public = compute_public_key(&responder_private);
        let initiator_shared =
            compute_shared_secret(&initiator_private, responder_public.as_bytes())
                .expect("valid responder public key");
        let responder_shared =
            compute_shared_secret(&responder_private, initiator_public.as_bytes())
                .expect("valid initiator public key");
        (initiator_shared, responder_shared)
    }

    fn encrypted_crypto_header(
        sender: &mut MseCipherPair,
        verification: [u8; VC_LEN],
        method: u32,
        pad_len: u16,
    ) -> Vec<u8> {
        let mut wire = Vec::with_capacity(PE_CRYPTO_HEADER_LEN);
        wire.extend_from_slice(&verification);
        wire.extend_from_slice(&method.to_be_bytes());
        wire.extend_from_slice(&pad_len.to_be_bytes());
        sender.apply_send(&mut wire);
        wire
    }

    fn feed_error(handshake: &mut MseHandshake, wire: &[u8]) -> MseHandshakeError {
        match handshake.feed(wire) {
            Err(error) => error,
            Ok(_) => panic!("hostile input unexpectedly accepted"),
        }
    }

    fn run_pair(method: MseMethod, ia_len: usize, chunk: usize, pad_len: usize) -> PairOutcome {
        run_pair_with_chunker(method, ia_len, pad_len, || chunk)
    }

    fn run_pair_with_chunker(
        method: MseMethod,
        ia_len: usize,
        pad_len: usize,
        mut next_chunk: impl FnMut() -> usize,
    ) -> PairOutcome {
        let initiator_handshake = encode_handshake(INFO_HASH, INITIATOR_ID);
        let responder_handshake = encode_handshake(INFO_HASH, RESPONDER_ID);
        let mut initiator = initiator(method, ia_len, pad_len);
        let mut responder = responder(method, pad_len);
        let mut to_initiator = Vec::new();
        let mut to_responder = Vec::new();
        let mut initiator_complete = None;
        let mut responder_complete = None;

        let step = initiator.start().expect("start initiator");
        settle(
            &mut initiator,
            step,
            &mut to_responder,
            &mut initiator_complete,
        );
        let step = responder.start().expect("start responder");
        settle(
            &mut responder,
            step,
            &mut to_initiator,
            &mut responder_complete,
        );

        for _ in 0..20_000 {
            let mut progressed = false;
            progressed |= feed_wire(
                &mut responder,
                &mut to_responder,
                next_chunk(),
                &mut to_initiator,
                &mut responder_complete,
            );
            if let Some(complete) = responder_complete.as_mut()
                && complete.carried.len() >= 68
                && to_initiator.is_empty()
            {
                let mut response = Vec::from(responder_handshake);
                response.extend_from_slice(&FIRST_FRAME);
                if let Some(ciphers) = complete.ciphers.as_mut() {
                    ciphers.apply_send(&mut response);
                }
                to_initiator.extend_from_slice(&response);
                progressed = true;
            }
            progressed |= feed_wire(
                &mut initiator,
                &mut to_initiator,
                next_chunk(),
                &mut to_responder,
                &mut initiator_complete,
            );
            if initiator_complete.is_some() && responder_complete.is_some() {
                break;
            }
            assert!(progressed || !to_initiator.is_empty() || !to_responder.is_empty());
        }

        let initiator_complete = initiator_complete.expect("initiator completed");
        let responder_complete = responder_complete.expect("responder completed");
        decode_handshake(&initiator_complete.carried.as_slice()[..68], INFO_HASH)
            .expect("valid responder handshake");
        decode_handshake(&responder_complete.carried.as_slice()[..68], INFO_HASH)
            .expect("valid initiator handshake");
        PairOutcome {
            initiator_method: initiator_complete.method,
            responder_method: responder_complete.method,
            initiator_handshake,
            responder_handshake,
            initiator_carried: initiator_complete.carried.as_slice().to_vec(),
            responder_carried: responder_complete.carried.as_slice().to_vec(),
        }
    }

    fn initiator(method: MseMethod, ia_len: usize, pad_len: usize) -> MseHandshake {
        MseHandshake::new_initiator(
            [0x11; 20],
            MsePadding::new(&vec![0x21; pad_len]).expect("pad A"),
            MsePadding::new(&vec![0x31; pad_len]).expect("pad C"),
            INFO_HASH,
            method.wire_bit(),
            encode_handshake(INFO_HASH, INITIATOR_ID),
            ia_len,
        )
        .expect("initiator")
    }

    fn responder(method: MseMethod, pad_len: usize) -> MseHandshake {
        MseHandshake::new_responder(
            [0x91; 20],
            MsePadding::new(&vec![0x41; pad_len]).expect("pad B"),
            MsePadding::new(&vec![0x51; pad_len]).expect("pad D"),
            method.wire_bit(),
            method == MseMethod::Rc4,
        )
        .expect("responder")
    }

    fn settle(
        handshake: &mut MseHandshake,
        mut step: MseStep,
        output: &mut Vec<u8>,
        complete: &mut Option<MseHandshakeComplete>,
    ) {
        loop {
            step = match step {
                MseStep::NeedInput => return,
                MseStep::Complete(value) => {
                    *complete = Some(value);
                    return;
                }
                MseStep::Action(MseAction::ComputePublicKey { private }) => {
                    let public = compute_public_key(&private);
                    handshake
                        .resume(MseResume::PublicKeyComputed { private, public })
                        .expect("resume public")
                }
                MseStep::Action(MseAction::ComputeSharedSecret {
                    private,
                    remote_public,
                }) => {
                    let shared = compute_shared_secret(&private, &remote_public)
                        .expect("valid deterministic peer key");
                    handshake
                        .resume(MseResume::SharedSecretComputed(shared))
                        .expect("resume shared")
                }
                MseStep::Action(MseAction::IdentifyTorrent { req2_hash: value }) => {
                    assert_eq!(value, req2_hash(&INFO_HASH));
                    handshake
                        .resume(MseResume::TorrentIdentified(Some(INFO_HASH)))
                        .expect("resume lookup")
                }
                MseStep::Action(MseAction::Send(bytes)) => {
                    output.extend_from_slice(bytes.as_slice());
                    handshake.resume(MseResume::Sent).expect("resume send")
                }
            };
        }
    }

    fn feed_wire(
        handshake: &mut MseHandshake,
        input: &mut Vec<u8>,
        chunk: usize,
        output: &mut Vec<u8>,
        complete: &mut Option<MseHandshakeComplete>,
    ) -> bool {
        if input.is_empty() || complete.is_some() {
            return false;
        }
        let count = input.len().min(chunk.max(1));
        let feed = handshake.feed(&input[..count]).expect("feed wire");
        input.drain(..feed.consumed);
        settle(handshake, feed.step, output, complete);
        feed.consumed != 0
    }
}

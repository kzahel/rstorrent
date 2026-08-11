//! One-generation peer socket ownership and bounded task messaging.

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use rstorrent_protocol::extension::{ExtensionHandshake, ExtensionMap};
use rstorrent_protocol::mse::{
    DH_PRIVATE_EXPONENT_LEN, MSE_KNOWN_METHODS, MSE_MAX_PADDING_LEN, MSE_METHOD_RC4, MseAction,
    MseHandshake, MseHandshakeComplete, MseMethod, MsePadding, MseResume, MseRole, MseStep,
};
use rstorrent_protocol::peer_wire::{
    EXTENSION_PROTOCOL_RESERVED_BIT, EXTENSION_PROTOCOL_RESERVED_INDEX,
    FAST_EXTENSION_RESERVED_BIT, FAST_EXTENSION_RESERVED_INDEX, HANDSHAKE_LENGTH, Handshake,
    NegotiatedPeerCapabilities, PeerMessage, decode_handshake, encode_handshake_with_reserved,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::{Instant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::mse::{
    MseDhWorkOwner, MseHandshakeAccounting, MseHandshakeFailure, MseHandshakeOutcome,
    MseHandshakeSink, record_mse_handshake,
};
use crate::network::{AddressFamilyPolicy, NetworkConfig, PeerEncryptionPolicy};
use crate::peer::{DialAttempt, DialAttemptId, MseEndpointState, PeerFailure, UtpDialDecision};
use crate::peer_budget::{PeerBudget, PeerBudgetDirection, PeerBudgetPermit, PeerBudgetRejection};
use crate::peer_io::{NETWORK_READ_LENGTH, PeerIo, PeerIoError, record_bytes};
use crate::peer_runtime::{PeerTransport, connection_id};
use crate::swarm::ConnectionId;
use crate::{ByteMetric, ByteMetricSink};
use crate::{UtpHandle, UtpStream};

pub(crate) fn advertised_reserved_bits(advertise_extensions: bool) -> [u8; 8] {
    let mut reserved = [0; 8];
    if advertise_extensions {
        reserved[EXTENSION_PROTOCOL_RESERVED_INDEX] = EXTENSION_PROTOCOL_RESERVED_BIT;
    }
    reserved[FAST_EXTENSION_RESERVED_INDEX] = FAST_EXTENSION_RESERVED_BIT;
    reserved
}

pub(crate) const PEER_COMMAND_QUEUE: usize = 16;
pub(crate) const PEER_EVENT_QUEUE: usize = 64;

#[derive(Debug)]
pub(crate) struct PeerConnection {
    attempt: DialAttempt,
    io: PeerIo,
    fast_extension: bool,
    initial_availability_sent: bool,
    extension_map: ExtensionMap,
    mse_method: Option<MseMethod>,
    mse_endpoint_update: Option<MseEndpointState>,
    transport: PeerTransport,
    _budget_permit: Option<Box<PeerBudgetPermit>>,
}

impl PeerConnection {
    pub(crate) const fn attempt(&self) -> DialAttempt {
        self.attempt
    }

    pub(crate) const fn io_timeout(&self) -> Duration {
        self.io.io_timeout
    }

    pub(crate) const fn supports_fast_extension(&self) -> bool {
        self.fast_extension
    }

    pub(crate) const fn initial_availability_sent(&self) -> bool {
        self.initial_availability_sent
    }

    pub(crate) fn mark_initial_availability_sent(&mut self) {
        self.initial_availability_sent = true;
    }

    pub(crate) const fn extension_map(&self) -> ExtensionMap {
        self.extension_map
    }

    pub(crate) const fn mse_method(&self) -> Option<MseMethod> {
        self.mse_method
    }

    pub(crate) const fn mse_endpoint_update(&self) -> Option<MseEndpointState> {
        self.mse_endpoint_update
    }

    pub(crate) const fn transport(&self) -> PeerTransport {
        self.transport
    }

    pub(crate) fn apply_extension_handshake(&mut self, handshake: ExtensionHandshake) {
        self.extension_map.apply(handshake);
    }

    pub(crate) fn prepend_messages(&mut self, messages: VecDeque<PeerMessage>) {
        self.io.prepend_messages(messages);
    }

    pub(crate) fn budget_cancellation(&self) -> Option<CancellationToken> {
        self._budget_permit
            .as_ref()
            .map(|permit| permit.cancellation_token())
    }

    #[cfg(test)]
    pub(crate) fn for_test(attempt: DialAttempt, stream: TcpStream, io_timeout: Duration) -> Self {
        Self {
            attempt,
            io: PeerIo::new(stream, io_timeout, None),
            fast_extension: false,
            initial_availability_sent: false,
            extension_map: ExtensionMap::default(),
            mse_method: None,
            mse_endpoint_update: None,
            transport: PeerTransport::Tcp,
            _budget_permit: None,
        }
    }
}

pub(crate) type PeerSocketError = PeerIoError;

impl PeerSocketError {
    pub(crate) fn peer_failure(&self) -> PeerFailure {
        match self {
            Self::MseEndpointUpdate { source, .. } => source.peer_failure(),
            Self::Cancelled
            | Self::NetworkPolicyDenied { .. }
            | Self::Io {
                operation: "connect to peer",
                ..
            }
            | Self::TimedOut {
                operation: "connect",
                ..
            } => PeerFailure::Connect,
            Self::TimedOut {
                operation: "handshake read" | "handshake write",
                ..
            }
            | Self::Handshake(_)
            | Self::MseHandshake(_)
            | Self::MseDh(_)
            | Self::Entropy(_) => PeerFailure::Handshake,
            Self::UtpEncryptionRequired => PeerFailure::Handshake,
            Self::Closed => PeerFailure::RemoteClosed,
            Self::Io { .. } | Self::TimedOut { .. } | Self::Frame(_) => PeerFailure::Protocol,
        }
    }

    pub(crate) const fn mse_endpoint_update(&self) -> Option<MseEndpointState> {
        match self {
            Self::MseEndpointUpdate { state, .. } => Some(*state),
            _ => None,
        }
    }

    fn with_mse_endpoint_update(self, state: MseEndpointState) -> Self {
        Self::MseEndpointUpdate {
            state,
            source: Box::new(self),
        }
    }
}

#[cfg(test)]
pub(crate) async fn connect(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    let mse_dh = MseDhWorkOwner::new();
    let result = connect_with_progress(
        attempt,
        info_hash,
        advertise_extensions,
        network,
        None,
        ConnectResources {
            progress: None,
            byte_metric_sink: None,
            mse_handshake_sink: None,
            budget_permit: None,
            mse_dh: mse_dh.clone(),
        },
    )
    .await;
    mse_dh.shutdown().await;
    result
}

async fn connect_with_progress(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
    utp: Option<UtpHandle>,
    resources: ConnectResources<'_>,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    let address = attempt.endpoint().address();
    if !network.policy.allows(address) {
        return Err(PeerSocketError::NetworkPolicyDenied {
            address,
            policy: network.policy,
        });
    }
    if !network.address_families.permits(address.ip()) {
        return Err(PeerSocketError::NetworkPolicyDenied {
            address,
            policy: network.policy,
        });
    }
    let preferred_transport = preferred_transport(
        address,
        network.encryption,
        utp.is_some(),
        attempt.utp_decision(),
    );
    if let Some(utp) = utp
        .as_ref()
        .filter(|_| preferred_transport == PeerTransport::Utp)
    {
        let stream = utp
            .connect_with_timeout(address, network.peer_connect_timeout)
            .await;
        if let Ok(stream) = stream {
            return connect_utp_with_progress(
                stream,
                attempt,
                info_hash,
                advertise_extensions,
                network,
                resources,
            )
            .await;
        }
    }
    connect_tcp_with_progress(attempt, info_hash, advertise_extensions, network, resources).await
}

fn utp_dial_eligible(address: std::net::SocketAddr, encryption: PeerEncryptionPolicy) -> bool {
    address.is_ipv4()
        && matches!(
            encryption,
            PeerEncryptionPolicy::Disabled | PeerEncryptionPolicy::Allow
        )
}

pub(crate) fn preferred_transport(
    address: std::net::SocketAddr,
    encryption: PeerEncryptionPolicy,
    utp_available: bool,
    utp_decision: UtpDialDecision,
) -> PeerTransport {
    if utp_available
        && utp_decision == UtpDialDecision::Try
        && utp_dial_eligible(address, encryption)
    {
        PeerTransport::Utp
    } else {
        PeerTransport::Tcp
    }
}

async fn connect_tcp_with_progress(
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
    resources: ConnectResources<'_>,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    let ConnectResources {
        progress,
        byte_metric_sink,
        mse_handshake_sink,
        mut budget_permit,
        mse_dh,
    } = resources;
    let address = attempt.endpoint().address();
    let mut stream = timeout(network.peer_connect_timeout, TcpStream::connect(address))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation: "connect",
            timeout: network.peer_connect_timeout,
        })?
        .map_err(|source| PeerSocketError::Io {
            operation: "connect to peer",
            source,
        })?;
    if let Some(progress) = progress {
        let _ = progress
            .send(PeerDialProgress {
                attempt,
                transport: PeerTransport::Tcp,
            })
            .await;
    }
    let local_handshake = encode_handshake_with_reserved(
        info_hash,
        network.peer_id,
        advertised_reserved_bits(advertise_extensions),
    );
    let try_mse = match network.encryption {
        PeerEncryptionPolicy::Disabled | PeerEncryptionPolicy::Allow => false,
        PeerEncryptionPolicy::Prefer => attempt.mse_endpoint() != MseEndpointState::PlainPreferred,
        PeerEncryptionPolicy::Required => true,
    };
    let (handshake, io, mse_method, mse_endpoint_update) = if try_mse {
        let attempt = run_outgoing_mse(
            &mut stream,
            info_hash,
            local_handshake,
            OutgoingMseConfig {
                io_timeout: network.peer_io_timeout,
                byte_metric_sink: byte_metric_sink.as_ref(),
                mse_dh: &mse_dh,
                policy: network.encryption,
                rc4_only: network.mse_rc4_only,
            },
        )
        .await;
        match attempt.result {
            Ok(negotiated) => {
                record_mse_handshake(
                    mse_handshake_sink.as_ref(),
                    attempt.accounting.finish(
                        MseHandshakeOutcome::Negotiated(negotiated.complete.method),
                        false,
                    ),
                );
                let mut io = PeerIo::new(stream, network.peer_io_timeout, byte_metric_sink.clone());
                if let Some(ciphers) = negotiated.complete.ciphers {
                    io.attach_ciphers(ciphers);
                }
                io.push_decrypted(&negotiated.carried)?;
                (
                    negotiated.handshake,
                    io,
                    Some(negotiated.complete.method),
                    Some(MseEndpointState::MseCapable),
                )
            }
            Err(failure)
                if network.encryption == PeerEncryptionPolicy::Prefer
                    && failure.downgrade_eligible =>
            {
                record_mse_handshake(
                    mse_handshake_sink.as_ref(),
                    attempt.accounting.finish(
                        MseHandshakeOutcome::Failed(mse_outgoing_failure(&failure.error)),
                        true,
                    ),
                );
                drop(stream);
                stream = timeout(network.peer_connect_timeout, TcpStream::connect(address))
                    .await
                    .map_err(|_| PeerSocketError::TimedOut {
                        operation: "connect",
                        timeout: network.peer_connect_timeout,
                    })
                    .and_then(|result| {
                        result.map_err(|source| PeerSocketError::Io {
                            operation: "connect to peer",
                            source,
                        })
                    })
                    .map_err(|error| {
                        error.with_mse_endpoint_update(MseEndpointState::PlainPreferred)
                    })?;
                let handshake = run_outgoing_plain(
                    &mut stream,
                    info_hash,
                    &local_handshake,
                    network.peer_io_timeout,
                    byte_metric_sink.as_ref(),
                )
                .await
                .map_err(|error| error.with_mse_endpoint_update(MseEndpointState::Unknown))?;
                (
                    handshake,
                    PeerIo::new(stream, network.peer_io_timeout, byte_metric_sink.clone()),
                    None,
                    Some(MseEndpointState::PlainPreferred),
                )
            }
            Err(failure) => {
                record_mse_handshake(
                    mse_handshake_sink.as_ref(),
                    attempt.accounting.finish(
                        MseHandshakeOutcome::Failed(mse_outgoing_failure(&failure.error)),
                        false,
                    ),
                );
                return Err(failure.error);
            }
        }
    } else {
        let plain = run_outgoing_plain(
            &mut stream,
            info_hash,
            &local_handshake,
            network.peer_io_timeout,
            byte_metric_sink.as_ref(),
        )
        .await;
        let handshake = match plain {
            Ok(handshake) => handshake,
            Err(error)
                if network.encryption == PeerEncryptionPolicy::Prefer
                    && attempt.mse_endpoint() == MseEndpointState::PlainPreferred =>
            {
                return Err(error.with_mse_endpoint_update(MseEndpointState::Unknown));
            }
            Err(error) => return Err(error),
        };
        (
            handshake,
            PeerIo::new(stream, network.peer_io_timeout, byte_metric_sink.clone()),
            None,
            None,
        )
    };
    let capabilities = NegotiatedPeerCapabilities::negotiate(
        advertised_reserved_bits(advertise_extensions),
        &handshake,
    );
    if let Some(permit) = budget_permit.as_mut() {
        permit.mark_established();
    }
    Ok((
        PeerConnection {
            attempt,
            io,
            fast_extension: capabilities.fast_extension,
            initial_availability_sent: false,
            extension_map: ExtensionMap::default(),
            mse_method,
            mse_endpoint_update,
            transport: PeerTransport::Tcp,
            _budget_permit: budget_permit.map(Box::new),
        },
        handshake,
    ))
}

async fn connect_utp_with_progress(
    stream: UtpStream,
    attempt: DialAttempt,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
    resources: ConnectResources<'_>,
) -> Result<(PeerConnection, Handshake), PeerSocketError> {
    let ConnectResources {
        progress,
        byte_metric_sink,
        mse_handshake_sink: _,
        mut budget_permit,
        mse_dh: _,
    } = resources;
    if let Some(progress) = progress {
        let _ = progress
            .send(PeerDialProgress {
                attempt,
                transport: PeerTransport::Utp,
            })
            .await;
    }
    let (io, handshake) = handshake_over_utp_with_sink(
        stream,
        info_hash,
        advertise_extensions,
        network,
        byte_metric_sink,
    )
    .await?;
    let capabilities = NegotiatedPeerCapabilities::negotiate(
        advertised_reserved_bits(advertise_extensions),
        &handshake,
    );
    if let Some(permit) = budget_permit.as_mut() {
        permit.mark_established();
    }
    Ok((
        PeerConnection {
            attempt,
            io,
            fast_extension: capabilities.fast_extension,
            initial_availability_sent: false,
            extension_map: ExtensionMap::default(),
            mse_method: None,
            mse_endpoint_update: None,
            transport: PeerTransport::Utp,
            _budget_permit: budget_permit.map(Box::new),
        },
        handshake,
    ))
}

pub(crate) async fn handshake_over_utp(
    stream: UtpStream,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
) -> Result<(PeerIo, Handshake), PeerSocketError> {
    handshake_over_utp_with_sink(stream, info_hash, advertise_extensions, network, None).await
}

async fn handshake_over_utp_with_sink(
    mut stream: UtpStream,
    info_hash: [u8; 20],
    advertise_extensions: bool,
    network: NetworkConfig,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
) -> Result<(PeerIo, Handshake), PeerSocketError> {
    let address = stream.peer_addr();
    if !network.policy.allows(address) || !network.address_families.permits(address.ip()) {
        return Err(PeerSocketError::NetworkPolicyDenied {
            address,
            policy: network.policy,
        });
    }
    if network.encryption == PeerEncryptionPolicy::Required {
        return Err(PeerSocketError::UtpEncryptionRequired);
    }
    let local_handshake = encode_handshake_with_reserved(
        info_hash,
        network.peer_id,
        advertised_reserved_bits(advertise_extensions),
    );
    let handshake = run_outgoing_plain(
        &mut stream,
        info_hash,
        &local_handshake,
        network.peer_io_timeout,
        byte_metric_sink.as_ref(),
    )
    .await?;
    let io = PeerIo::new(stream, network.peer_io_timeout, byte_metric_sink);
    Ok((io, handshake))
}

struct ConnectResources<'a> {
    progress: Option<&'a mpsc::Sender<PeerDialProgress>>,
    byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    mse_handshake_sink: Option<Arc<dyn MseHandshakeSink>>,
    budget_permit: Option<PeerBudgetPermit>,
    mse_dh: MseDhWorkOwner,
}

#[derive(Default)]
pub(crate) struct PeerDialServices {
    pub(crate) byte_metric_sink: Option<Arc<dyn ByteMetricSink>>,
    pub(crate) mse_handshake_sink: Option<Arc<dyn MseHandshakeSink>>,
    pub(crate) utp: Option<UtpHandle>,
}

struct OutgoingMse {
    handshake: Handshake,
    complete: MseHandshakeComplete,
    carried: Vec<u8>,
}

struct OutgoingMseFailure {
    error: PeerSocketError,
    downgrade_eligible: bool,
}

struct OutgoingMseAttempt {
    result: Result<OutgoingMse, OutgoingMseFailure>,
    accounting: MseHandshakeAccounting,
}

#[derive(Clone, Copy)]
struct OutgoingMseConfig<'a> {
    io_timeout: Duration,
    byte_metric_sink: Option<&'a Arc<dyn ByteMetricSink>>,
    mse_dh: &'a MseDhWorkOwner,
    policy: PeerEncryptionPolicy,
    rc4_only: bool,
}

async fn run_outgoing_plain<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    info_hash: [u8; 20],
    local_handshake: &[u8; HANDSHAKE_LENGTH],
    io_timeout: Duration,
    byte_metric_sink: Option<&Arc<dyn ByteMetricSink>>,
) -> Result<Handshake, PeerSocketError> {
    write_all_recorded(
        stream,
        local_handshake,
        Instant::now() + io_timeout,
        io_timeout,
        "handshake write",
        byte_metric_sink,
        true,
        None,
    )
    .await?;
    let mut remote_handshake = [0_u8; HANDSHAKE_LENGTH];
    read_exact_recorded(
        stream,
        &mut remote_handshake,
        Instant::now() + io_timeout,
        io_timeout,
        "handshake read",
        byte_metric_sink,
        true,
        None,
    )
    .await?;
    decode_handshake(&remote_handshake, info_hash).map_err(PeerSocketError::Handshake)
}

async fn run_outgoing_mse(
    stream: &mut TcpStream,
    info_hash: [u8; 20],
    local_handshake: [u8; HANDSHAKE_LENGTH],
    config: OutgoingMseConfig<'_>,
) -> OutgoingMseAttempt {
    let mut accounting = MseHandshakeAccounting::new(MseRole::Initiator, config.policy);
    let result =
        run_outgoing_mse_inner(stream, info_hash, local_handshake, config, &mut accounting).await;
    OutgoingMseAttempt { result, accounting }
}

async fn run_outgoing_mse_inner(
    stream: &mut TcpStream,
    info_hash: [u8; 20],
    local_handshake: [u8; HANDSHAKE_LENGTH],
    config: OutgoingMseConfig<'_>,
    accounting: &mut MseHandshakeAccounting,
) -> Result<OutgoingMse, OutgoingMseFailure> {
    let OutgoingMseConfig {
        io_timeout,
        byte_metric_sink,
        mse_dh,
        rc4_only,
        ..
    } = config;
    let mut private_entropy = [0_u8; DH_PRIVATE_EXPONENT_LEN];
    getrandom::fill(&mut private_entropy).map_err(|error| OutgoingMseFailure {
        error: PeerSocketError::Entropy(error),
        downgrade_eligible: false,
    })?;
    let pad_a = random_mse_padding().map_err(|error| OutgoingMseFailure {
        error,
        downgrade_eligible: false,
    })?;
    let pad_c = random_mse_padding().map_err(|error| OutgoingMseFailure {
        error,
        downgrade_eligible: false,
    })?;
    let mut handshake = MseHandshake::new_initiator(
        private_entropy,
        pad_a,
        pad_c,
        info_hash,
        if rc4_only {
            MSE_METHOD_RC4
        } else {
            MSE_KNOWN_METHODS
        },
        local_handshake,
        HANDSHAKE_LENGTH,
    )
    .map_err(|error| OutgoingMseFailure {
        error: PeerSocketError::MseHandshake(error),
        downgrade_eligible: false,
    })?;
    let mut step = handshake.start().map_err(|error| OutgoingMseFailure {
        error: PeerSocketError::MseHandshake(error),
        downgrade_eligible: false,
    })?;
    let mut remote_key_valid = false;
    let handshake_deadline = Instant::now() + io_timeout;
    let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
    let mut buffered = 0;
    let mut consumed = 0;

    loop {
        step = match step {
            MseStep::NeedInput => {
                if consumed == buffered {
                    buffered = read_some_recorded(
                        stream,
                        &mut network_buffer,
                        handshake_deadline,
                        io_timeout,
                        "handshake read",
                        byte_metric_sink,
                        Some(accounting),
                    )
                    .await
                    .map_err(|error| OutgoingMseFailure {
                        downgrade_eligible: !remote_key_valid && is_downgrade_transport(&error),
                        error,
                    })?;
                    consumed = 0;
                }
                let feed = handshake
                    .feed(&network_buffer[consumed..buffered])
                    .map_err(|error| OutgoingMseFailure {
                        error: PeerSocketError::MseHandshake(error),
                        downgrade_eligible: false,
                    })?;
                consumed += feed.consumed;
                feed.step
            }
            MseStep::Action(MseAction::ComputePublicKey { private }) => {
                accounting.exponentiation_started();
                let (private, public) =
                    mse_dh.compute_public_key(private).await.map_err(|error| {
                        OutgoingMseFailure {
                            error: PeerSocketError::MseDh(error),
                            downgrade_eligible: false,
                        }
                    })?;
                handshake
                    .resume(MseResume::PublicKeyComputed { private, public })
                    .map_err(|error| OutgoingMseFailure {
                        error: PeerSocketError::MseHandshake(error),
                        downgrade_eligible: false,
                    })?
            }
            MseStep::Action(MseAction::ComputeSharedSecret {
                private,
                remote_public,
            }) => {
                accounting.exponentiation_started();
                let shared = mse_dh
                    .compute_shared_secret(private, remote_public)
                    .await
                    .map_err(|error| OutgoingMseFailure {
                        error: PeerSocketError::MseDh(error),
                        downgrade_eligible: false,
                    })?;
                remote_key_valid = true;
                handshake
                    .resume(MseResume::SharedSecretComputed(shared))
                    .map_err(|error| OutgoingMseFailure {
                        error: PeerSocketError::MseHandshake(error),
                        downgrade_eligible: false,
                    })?
            }
            MseStep::Action(MseAction::IdentifyTorrent { .. }) => {
                return Err(OutgoingMseFailure {
                    error: PeerSocketError::MseHandshake(
                        rstorrent_protocol::mse::MseHandshakeError::UnexpectedResume,
                    ),
                    downgrade_eligible: false,
                });
            }
            MseStep::Action(MseAction::Send(bytes)) => {
                write_all_recorded(
                    stream,
                    bytes.as_slice(),
                    handshake_deadline,
                    io_timeout,
                    "handshake write",
                    byte_metric_sink,
                    false,
                    Some(accounting),
                )
                .await
                .map_err(|error| OutgoingMseFailure {
                    downgrade_eligible: !remote_key_valid && is_downgrade_transport(&error),
                    error,
                })?;
                handshake
                    .resume(MseResume::Sent)
                    .map_err(|error| OutgoingMseFailure {
                        error: PeerSocketError::MseHandshake(error),
                        downgrade_eligible: false,
                    })?
            }
            MseStep::Complete(mut complete) => {
                let carried = complete.carried.as_slice();
                let remote_handshake: [u8; HANDSHAKE_LENGTH] = carried
                    .get(..HANDSHAKE_LENGTH)
                    .and_then(|bytes| bytes.try_into().ok())
                    .ok_or_else(|| OutgoingMseFailure {
                        error: PeerSocketError::MseHandshake(
                            rstorrent_protocol::mse::MseHandshakeError::BufferOverflow,
                        ),
                        downgrade_eligible: false,
                    })?;
                let decoded = decode_handshake(&remote_handshake, info_hash).map_err(|error| {
                    OutgoingMseFailure {
                        error: PeerSocketError::Handshake(error),
                        downgrade_eligible: false,
                    }
                })?;
                let mut post_handshake = carried[HANDSHAKE_LENGTH..].to_vec();
                if consumed < buffered {
                    let unread = &mut network_buffer[consumed..buffered];
                    if let Some(ciphers) = complete.ciphers.as_mut() {
                        ciphers.apply_receive(unread);
                    }
                    post_handshake.extend_from_slice(unread);
                }
                accounting.carried_wire(post_handshake.len());
                record_bytes(
                    byte_metric_sink,
                    ByteMetric::PeerProtocolSent,
                    HANDSHAKE_LENGTH,
                );
                accounting.protocol_sent(HANDSHAKE_LENGTH);
                record_bytes(
                    byte_metric_sink,
                    ByteMetric::PeerProtocolReceived,
                    HANDSHAKE_LENGTH,
                );
                accounting.protocol_received(HANDSHAKE_LENGTH);
                return Ok(OutgoingMse {
                    handshake: decoded,
                    complete,
                    carried: post_handshake,
                });
            }
        };
    }
}

fn random_mse_padding() -> Result<MsePadding, PeerSocketError> {
    let mut selector = [0_u8; 2];
    getrandom::fill(&mut selector).map_err(PeerSocketError::Entropy)?;
    let len = usize::from(u16::from_ne_bytes(selector)) % (MSE_MAX_PADDING_LEN + 1);
    let mut bytes = [0_u8; MSE_MAX_PADDING_LEN];
    getrandom::fill(&mut bytes[..len]).map_err(PeerSocketError::Entropy)?;
    MsePadding::new(&bytes[..len]).map_err(PeerSocketError::MseHandshake)
}

fn is_downgrade_transport(error: &PeerSocketError) -> bool {
    matches!(
        error,
        PeerSocketError::Closed | PeerSocketError::Io { .. } | PeerSocketError::TimedOut { .. }
    )
}

fn mse_outgoing_failure(error: &PeerSocketError) -> MseHandshakeFailure {
    match error {
        PeerSocketError::Cancelled => MseHandshakeFailure::Cancelled,
        PeerSocketError::TimedOut { .. } => MseHandshakeFailure::TimedOut,
        PeerSocketError::Closed => MseHandshakeFailure::TransportClosed,
        PeerSocketError::Io { .. } => MseHandshakeFailure::TransportIo,
        PeerSocketError::Entropy(_) => MseHandshakeFailure::Entropy,
        PeerSocketError::MseDh(_) => MseHandshakeFailure::DiffieHellman,
        PeerSocketError::MseHandshake(error) => MseHandshakeFailure::Protocol(*error),
        PeerSocketError::Handshake(_) => MseHandshakeFailure::BitTorrentHandshake,
        PeerSocketError::MseEndpointUpdate { source, .. } => mse_outgoing_failure(source),
        PeerSocketError::NetworkPolicyDenied { .. }
        | PeerSocketError::UtpEncryptionRequired
        | PeerSocketError::Frame(_) => MseHandshakeFailure::TransportIo,
    }
}

#[allow(clippy::too_many_arguments)]
async fn write_all_recorded(
    stream: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    deadline: Instant,
    io_timeout: Duration,
    operation: &'static str,
    byte_metric_sink: Option<&Arc<dyn ByteMetricSink>>,
    protocol: bool,
    mut accounting: Option<&mut MseHandshakeAccounting>,
) -> Result<(), PeerSocketError> {
    let mut written = 0;
    while written < bytes.len() {
        let count = timeout_at(deadline, stream.write(&bytes[written..]))
            .await
            .map_err(|_| PeerSocketError::TimedOut {
                operation,
                timeout: io_timeout,
            })?
            .map_err(|source| PeerSocketError::Io { operation, source })?;
        if count == 0 {
            return Err(PeerSocketError::Closed);
        }
        record_bytes(byte_metric_sink, ByteMetric::PeerWireSent, count);
        if let Some(accounting) = accounting.as_deref_mut() {
            accounting.wire_sent(count);
        }
        if protocol {
            record_bytes(byte_metric_sink, ByteMetric::PeerProtocolSent, count);
        }
        written += count;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn read_exact_recorded(
    stream: &mut (impl AsyncRead + Unpin),
    bytes: &mut [u8],
    deadline: Instant,
    io_timeout: Duration,
    operation: &'static str,
    byte_metric_sink: Option<&Arc<dyn ByteMetricSink>>,
    protocol: bool,
    mut accounting: Option<&mut MseHandshakeAccounting>,
) -> Result<(), PeerSocketError> {
    let mut read = 0;
    while read < bytes.len() {
        let count = timeout_at(deadline, stream.read(&mut bytes[read..]))
            .await
            .map_err(|_| PeerSocketError::TimedOut {
                operation,
                timeout: io_timeout,
            })?
            .map_err(|source| PeerSocketError::Io { operation, source })?;
        if count == 0 {
            return Err(PeerSocketError::Closed);
        }
        record_bytes(byte_metric_sink, ByteMetric::PeerWireReceived, count);
        if let Some(accounting) = accounting.as_deref_mut() {
            accounting.wire_received(count);
        }
        if protocol {
            record_bytes(byte_metric_sink, ByteMetric::PeerProtocolReceived, count);
        }
        read += count;
    }
    Ok(())
}

async fn read_some_recorded(
    stream: &mut (impl AsyncRead + Unpin),
    bytes: &mut [u8],
    deadline: Instant,
    io_timeout: Duration,
    operation: &'static str,
    byte_metric_sink: Option<&Arc<dyn ByteMetricSink>>,
    accounting: Option<&mut MseHandshakeAccounting>,
) -> Result<usize, PeerSocketError> {
    let read = timeout_at(deadline, stream.read(bytes))
        .await
        .map_err(|_| PeerSocketError::TimedOut {
            operation,
            timeout: io_timeout,
        })?
        .map_err(|source| PeerSocketError::Io { operation, source })?;
    if read == 0 {
        return Err(PeerSocketError::Closed);
    }
    record_bytes(byte_metric_sink, ByteMetric::PeerWireReceived, read);
    if let Some(accounting) = accounting {
        accounting.wire_received(read);
    }
    Ok(read)
}

pub(crate) async fn next_message(
    peer: &mut PeerConnection,
) -> Result<PeerMessage, PeerSocketError> {
    peer.io.next_message().await
}

pub(crate) async fn send_message(
    peer: &mut PeerConnection,
    message: &PeerMessage,
) -> Result<(), PeerSocketError> {
    peer.io.send_message(message).await
}

#[derive(Debug)]
pub(crate) enum PeerTaskCommand {
    Send(PeerMessage),
}

#[derive(Debug)]
pub(crate) enum PeerTaskEvent {
    Message {
        attempt: DialAttempt,
        message: PeerMessage,
    },
    Stopped {
        attempt: DialAttempt,
        result: Result<(), PeerSocketError>,
    },
}

#[derive(Debug)]
pub(crate) struct PeerSocketTask {
    attempt: DialAttempt,
    commands: mpsc::Sender<PeerTaskCommand>,
    cancellation: CancellationToken,
    join: JoinHandle<()>,
}

impl PeerSocketTask {
    pub(crate) fn spawn(connection: PeerConnection, events: mpsc::Sender<PeerTaskEvent>) -> Self {
        let attempt = connection.attempt;
        let (commands, command_rx) = mpsc::channel(PEER_COMMAND_QUEUE);
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let join = tokio::spawn(async move {
            let result =
                run_peer_task(connection, command_rx, events.clone(), &task_cancellation).await;
            let stopped = PeerTaskEvent::Stopped { attempt, result };
            tokio::select! {
                biased;
                _ = task_cancellation.cancelled() => {}
                _ = events.send(stopped) => {}
            }
        });
        Self {
            attempt,
            commands,
            cancellation,
            join,
        }
    }

    pub(crate) const fn attempt(&self) -> DialAttempt {
        self.attempt
    }

    pub(crate) async fn send(&self, message: PeerMessage) -> Result<(), PeerTaskSendError> {
        self.commands
            .send(PeerTaskCommand::Send(message))
            .await
            .map_err(|_| PeerTaskSendError)
    }

    pub(crate) async fn shutdown(self) -> Result<(), JoinError> {
        self.cancellation.cancel();
        self.join.await
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PeerTaskSendError;

impl fmt::Display for PeerTaskSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("peer socket task stopped before accepting command")
    }
}

impl Error for PeerTaskSendError {}

#[derive(Debug)]
pub(crate) enum PeerSetEvent {
    DialPhase {
        attempt: DialAttempt,
        transport: PeerTransport,
    },
    DialCompleted {
        attempt: DialAttempt,
        result: Box<ConnectedPeerResult>,
    },
    Peer(PeerTaskEvent),
}

type ConnectedPeerResult = Result<(PeerConnection, Handshake), PeerSocketError>;
type PendingDialResult = (DialAttempt, ConnectedPeerResult);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PeerDialProgress {
    attempt: DialAttempt,
    transport: PeerTransport,
}

#[derive(Debug)]
pub(crate) enum PeerSetError {
    ConnectionLimit(PeerBudgetRejection),
    DuplicateDial(DialAttemptId),
    DuplicateConnection(ConnectionId),
    UnknownConnection(ConnectionId),
    EventQueueClosed,
    TaskJoin(JoinError),
}

impl fmt::Display for PeerSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionLimit(error) => error.fmt(formatter),
            Self::DuplicateDial(id) => write!(formatter, "duplicate pending dial {id}"),
            Self::DuplicateConnection(id) => {
                write!(formatter, "duplicate peer connection {}", id.get())
            }
            Self::UnknownConnection(id) => {
                write!(formatter, "unknown peer connection {}", id.get())
            }
            Self::EventQueueClosed => formatter.write_str("peer event queue closed"),
            Self::TaskJoin(error) => write!(formatter, "peer task join: {error}"),
        }
    }
}

impl Error for PeerSetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ConnectionLimit(error) => Some(error),
            Self::TaskJoin(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PeerSocketSet {
    peer_budget: PeerBudget,
    mse_dh: MseDhWorkOwner,
    events_tx: mpsc::Sender<PeerTaskEvent>,
    events_rx: mpsc::Receiver<PeerTaskEvent>,
    tasks: BTreeMap<ConnectionId, PeerSocketTask>,
    pending: JoinSet<PendingDialResult>,
    pending_attempts: BTreeMap<DialAttemptId, (DialAttempt, CancellationToken)>,
    dial_progress_tx: mpsc::Sender<PeerDialProgress>,
    dial_progress_rx: mpsc::Receiver<PeerDialProgress>,
}

impl PeerSocketSet {
    pub(crate) fn new() -> Self {
        Self::with_budget(PeerBudget::system_default())
    }

    pub(crate) fn with_budget(peer_budget: PeerBudget) -> Self {
        Self::with_owners(peer_budget, MseDhWorkOwner::new())
    }

    pub(crate) fn with_owners(peer_budget: PeerBudget, mse_dh: MseDhWorkOwner) -> Self {
        let (events_tx, events_rx) = mpsc::channel(PEER_EVENT_QUEUE);
        let (dial_progress_tx, dial_progress_rx) = mpsc::channel(PEER_EVENT_QUEUE);
        Self {
            peer_budget,
            mse_dh,
            events_tx,
            events_rx,
            tasks: BTreeMap::new(),
            pending: JoinSet::new(),
            pending_attempts: BTreeMap::new(),
            dial_progress_tx,
            dial_progress_rx,
        }
    }

    pub(crate) fn established_len(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending_attempts.len()
    }

    pub(crate) fn pending_attempts(&self) -> Vec<DialAttempt> {
        self.pending_attempts
            .values()
            .map(|(attempt, _)| *attempt)
            .collect()
    }

    pub(crate) fn connection_attempts(&self) -> Vec<DialAttempt> {
        self.tasks.values().map(PeerSocketTask::attempt).collect()
    }

    pub(crate) fn cancel_disallowed(&self, policy: AddressFamilyPolicy) -> usize {
        let mut cancelled = 0;
        for (attempt, cancellation) in self.pending_attempts.values() {
            if !policy.permits(attempt.endpoint().address().ip()) {
                cancellation.cancel();
                cancelled += 1;
            }
        }
        for task in self.tasks.values() {
            if !policy.permits(task.attempt().endpoint().address().ip()) {
                task.cancel();
                cancelled += 1;
            }
        }
        cancelled
    }

    pub(crate) fn contains(&self, id: ConnectionId) -> bool {
        self.tasks.contains_key(&id)
    }

    pub(crate) fn attempt(&self, id: ConnectionId) -> Option<DialAttempt> {
        self.tasks.get(&id).map(PeerSocketTask::attempt)
    }

    pub(crate) fn begin_dial(
        &mut self,
        attempt: DialAttempt,
        info_hash: [u8; 20],
        advertise_extensions: bool,
        network: NetworkConfig,
        services: PeerDialServices,
    ) -> Result<(), PeerSetError> {
        let PeerDialServices {
            byte_metric_sink,
            mse_handshake_sink,
            utp,
        } = services;
        if self.pending_attempts.contains_key(&attempt.id()) {
            return Err(PeerSetError::DuplicateDial(attempt.id()));
        }
        let budget_permit = self
            .peer_budget
            .try_acquire(PeerBudgetDirection::Outgoing)
            .map_err(PeerSetError::ConnectionLimit)?;
        let budget_cancellation = budget_permit.cancellation_token();
        let cancellation = CancellationToken::new();
        let progress = self.dial_progress_tx.clone();
        let mse_dh = self.mse_dh.clone();
        self.pending_attempts
            .insert(attempt.id(), (attempt, cancellation.clone()));
        self.pending.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(PeerSocketError::Cancelled),
                _ = budget_cancellation.cancelled() => Err(PeerSocketError::Cancelled),
                result = connect_with_progress(
                    attempt,
                    info_hash,
                    advertise_extensions,
                    network,
                    utp,
                    ConnectResources {
                        progress: Some(&progress),
                        byte_metric_sink,
                        mse_handshake_sink,
                        budget_permit: Some(budget_permit),
                        mse_dh,
                    },
                ) => result,
            };
            (attempt, result)
        });
        Ok(())
    }

    pub(crate) fn add_connection(
        &mut self,
        connection: PeerConnection,
    ) -> Result<ConnectionId, PeerSetError> {
        let id = connection_id(connection.attempt);
        if self.tasks.contains_key(&id) {
            return Err(PeerSetError::DuplicateConnection(id));
        }
        self.tasks.insert(
            id,
            PeerSocketTask::spawn(connection, self.events_tx.clone()),
        );
        Ok(id)
    }

    pub(crate) async fn send(
        &self,
        id: ConnectionId,
        message: PeerMessage,
    ) -> Result<(), PeerSetError> {
        self.tasks
            .get(&id)
            .ok_or(PeerSetError::UnknownConnection(id))?
            .send(message)
            .await
            .map_err(|_| PeerSetError::UnknownConnection(id))
    }

    pub(crate) async fn next_event(&mut self) -> Result<PeerSetEvent, PeerSetError> {
        if self.pending_attempts.is_empty() {
            return self
                .events_rx
                .recv()
                .await
                .map(PeerSetEvent::Peer)
                .ok_or(PeerSetError::EventQueueClosed);
        }
        tokio::select! {
            progress = self.dial_progress_rx.recv() => {
                let progress = progress.ok_or(PeerSetError::EventQueueClosed)?;
                Ok(PeerSetEvent::DialPhase {
                    attempt: progress.attempt,
                    transport: progress.transport,
                })
            }
            event = self.events_rx.recv() => event
                .map(PeerSetEvent::Peer)
                .ok_or(PeerSetError::EventQueueClosed),
            joined = self.pending.join_next() => {
                let (attempt, result) = joined
                    .expect("pending dial set is nonempty")
                    .map_err(PeerSetError::TaskJoin)?;
                self.pending_attempts.remove(&attempt.id());
                Ok(PeerSetEvent::DialCompleted {
                    attempt,
                    result: Box::new(result),
                })
            }
        }
    }

    pub(crate) async fn remove_connection(
        &mut self,
        id: ConnectionId,
    ) -> Result<DialAttempt, PeerSetError> {
        let task = self
            .tasks
            .remove(&id)
            .ok_or(PeerSetError::UnknownConnection(id))?;
        let attempt = task.attempt();
        task.shutdown().await.map_err(PeerSetError::TaskJoin)?;
        Ok(attempt)
    }

    pub(crate) async fn shutdown(mut self) -> Result<Vec<DialAttempt>, PeerSetError> {
        for task in self.tasks.values() {
            task.cancel();
        }
        for (_, task) in self.tasks {
            task.join.await.map_err(PeerSetError::TaskJoin)?;
        }
        let pending = self
            .pending_attempts
            .values()
            .map(|(attempt, _)| *attempt)
            .collect::<Vec<_>>();
        for (_, cancellation) in self.pending_attempts.into_values() {
            cancellation.cancel();
        }
        while let Some(joined) = self.pending.join_next().await {
            drop(joined.map_err(PeerSetError::TaskJoin)?);
        }
        Ok(pending)
    }
}

impl Default for PeerSocketSet {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_peer_task(
    mut peer: PeerConnection,
    mut commands: mpsc::Receiver<PeerTaskCommand>,
    events: mpsc::Sender<PeerTaskEvent>,
    cancellation: &CancellationToken,
) -> Result<(), PeerSocketError> {
    let budget_cancellation = peer.budget_cancellation();
    let mut pending_messages = std::mem::take(&mut peer.io.queued_messages);
    let mut read_deadline = Instant::now() + peer.io.io_timeout;
    let mut network_buffer = [0_u8; NETWORK_READ_LENGTH];
    loop {
        if !pending_messages.is_empty() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Ok(()),
                _ = async {
                    if let Some(cancellation) = &budget_cancellation {
                        cancellation.cancelled().await;
                    }
                }, if budget_cancellation.is_some() => return Ok(()),
                permit = events.reserve() => {
                    let permit = permit.map_err(|_| PeerSocketError::Io {
                        operation: "deliver peer event",
                        source: io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "torrent supervisor stopped",
                        ),
                    })?;
                    let message = pending_messages
                        .pop_front()
                        .expect("pending peer message queue is nonempty");
                    permit.send(PeerTaskEvent::Message {
                        attempt: peer.attempt,
                        message,
                    });
                }
                command = commands.recv() => match command {
                    Some(PeerTaskCommand::Send(message)) => {
                        send_message(&mut peer, &message).await?;
                    }
                    None => return Ok(()),
                },
            }
            continue;
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Ok(()),
            _ = async {
                if let Some(cancellation) = &budget_cancellation {
                    cancellation.cancelled().await;
                }
            }, if budget_cancellation.is_some() => return Ok(()),
            command = commands.recv() => match command {
                Some(PeerTaskCommand::Send(message)) => send_message(&mut peer, &message).await?,
                None => return Ok(()),
            },
            read = timeout_at(read_deadline, peer.io.stream.read(&mut network_buffer)) => {
                let read = read
                    .map_err(|_| PeerSocketError::TimedOut {
                        operation: "message read",
                        timeout: peer.io.io_timeout,
                    })?
                    .map_err(|source| PeerSocketError::Io {
                        operation: "read peer message",
                        source,
                    })?;
                if read == 0 {
                    return Err(PeerSocketError::Closed);
                }
                let messages = peer.io.decode_received(&mut network_buffer[..read])?;
                for message in &messages {
                    peer.io.record_incoming_message(message)?;
                }
                if !messages.is_empty() {
                    read_deadline = Instant::now() + peer.io.io_timeout;
                }
                pending_messages.extend(messages);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use rstorrent_protocol::mse::{
        MseAction, MseHandshake, MseMethod, MsePadding, MseResume, MseStep, compute_public_key,
        compute_shared_secret, req2_hash,
    };
    use rstorrent_protocol::peer_wire::{
        HANDSHAKE_LENGTH, PeerMessage, decode_handshake, encode_handshake, encode_message,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
    use tokio::sync::{mpsc, oneshot};
    use tokio::time::timeout;

    use super::{
        ConnectResources, PEER_COMMAND_QUEUE, PeerConnection, PeerDialServices, PeerSetEvent,
        PeerSocketError, PeerSocketSet, PeerSocketTask, PeerTaskEvent, connect,
        connect_with_progress, next_message, preferred_transport, send_message, utp_dial_eligible,
    };
    use crate::metrics::{ByteMetric, ByteMetricSink};
    use crate::mse::{
        MseDhWorkOwner, MseHandshakeFailure, MseHandshakeObservation, MseHandshakeOutcome,
        MseHandshakeSink,
    };
    use crate::network::{NetworkConfig, NetworkPolicy, PeerEncryptionPolicy};
    use crate::peer::{
        DialAttempt, MseEndpointState, PeerEndpoint, PeerObservation, PeerRegistry,
        PeerRegistryConfig, PeerSelectionContext, PeerSelector, PeerSource,
    };
    use crate::peer_budget::{PeerBudget, PeerBudgetConfig, PeerBudgetPhase};
    use crate::{PeerTransport, SessionUdpService, UtpService};

    #[derive(Debug, Default)]
    struct RecordingMseSink {
        bytes: Mutex<BTreeMap<ByteMetric, u64>>,
        handshakes: Mutex<Vec<MseHandshakeObservation>>,
    }

    impl ByteMetricSink for RecordingMseSink {
        fn record(&self, metric: ByteMetric, bytes: u64) {
            *self
                .bytes
                .lock()
                .expect("byte metrics")
                .entry(metric)
                .or_default() += bytes;
        }
    }

    impl MseHandshakeSink for RecordingMseSink {
        fn record(&self, observation: MseHandshakeObservation) {
            self.handshakes
                .lock()
                .expect("MSE observations")
                .push(observation);
        }
    }

    async fn connect_observed(
        attempt: DialAttempt,
        info_hash: [u8; 20],
        advertise_extensions: bool,
        network: NetworkConfig,
        sink: Arc<RecordingMseSink>,
    ) -> Result<(PeerConnection, rstorrent_protocol::peer_wire::Handshake), PeerSocketError> {
        let mse_dh = MseDhWorkOwner::new();
        let result = connect_with_progress(
            attempt,
            info_hash,
            advertise_extensions,
            network,
            None,
            ConnectResources {
                progress: None,
                byte_metric_sink: Some(sink.clone()),
                mse_handshake_sink: Some(sink),
                budget_permit: None,
                mse_dh: mse_dh.clone(),
            },
        )
        .await;
        mse_dh.shutdown().await;
        result
    }

    fn test_attempt_for(address: std::net::SocketAddr) -> DialAttempt {
        let endpoint = PeerEndpoint::new(address).expect("valid endpoint");
        let mut registry = PeerRegistry::new(PeerRegistryConfig::default()).expect("registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Manual),
                Duration::ZERO,
            )
            .expect("observation");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector.select(&registry, context).expect("candidate");
        registry.begin_dial(candidate, context).expect("attempt")
    }

    fn test_attempt() -> DialAttempt {
        test_attempt_for("127.0.0.1:6881".parse().expect("test address"))
    }

    async fn connected_pair(io_timeout: Duration) -> (PeerConnection, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let client = TcpStream::connect(address).await.expect("connect");
        let (server, _) = listener.accept().await.expect("accept");
        (
            PeerConnection::for_test(test_attempt(), client, io_timeout),
            server,
        )
    }

    #[tokio::test]
    async fn outgoing_mse_negotiates_both_methods_and_carries_framed_io() {
        const INFO_HASH: [u8; 20] = [0x44; 20];
        for method in [MseMethod::PlaintextPayload, MseMethod::Rc4] {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let address = listener.local_addr().expect("address");
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                run_test_mse_responder(stream, method, INFO_HASH).await;
            });
            let attempt = test_attempt_for(address);
            let network = NetworkConfig::new(
                NetworkPolicy::LoopbackOnly,
                Duration::from_secs(1),
                Duration::from_secs(1),
            )
            .with_encryption(PeerEncryptionPolicy::Prefer);
            let sink = Arc::new(RecordingMseSink::default());
            let (mut connection, handshake) =
                connect_observed(attempt, INFO_HASH, true, network, sink.clone())
                    .await
                    .expect("MSE connection");
            let observation = sink.handshakes.lock().expect("observations")[0];
            assert_eq!(observation.policy, PeerEncryptionPolicy::Prefer);
            assert_eq!(observation.outcome, MseHandshakeOutcome::Negotiated(method));
            assert!(!observation.fallback_socket_used);
            assert_eq!(observation.exponentiations, 2);
            assert_eq!(observation.protocol_bytes_sent, HANDSHAKE_LENGTH as u64);
            assert_eq!(observation.protocol_bytes_received, HANDSHAKE_LENGTH as u64);
            {
                let bytes = sink.bytes.lock().expect("byte metrics");
                assert_eq!(
                    bytes[&ByteMetric::PeerWireSent],
                    observation.wire_bytes_sent
                );
                assert_eq!(
                    bytes[&ByteMetric::PeerWireReceived],
                    observation.wire_bytes_received
                );
                assert_eq!(
                    bytes[&ByteMetric::PeerProtocolSent],
                    observation.protocol_bytes_sent
                );
                assert_eq!(
                    bytes[&ByteMetric::PeerProtocolReceived],
                    observation.protocol_bytes_received
                );
            }
            assert_eq!(connection.mse_method(), Some(method));
            assert_eq!(
                connection.mse_endpoint_update(),
                Some(MseEndpointState::MseCapable)
            );
            assert_eq!(handshake.peer_id, [0x66; 20]);
            assert_eq!(
                next_message(&mut connection)
                    .await
                    .expect("carried message"),
                PeerMessage::Unchoke
            );
            send_message(&mut connection, &PeerMessage::Interested)
                .await
                .expect("encrypted framed send");
            server.await.expect("server join");
        }
    }

    #[tokio::test]
    async fn outgoing_mse_can_offer_only_rc4_for_a_forced_comparison() {
        const INFO_HASH: [u8; 20] = [0x45; 20];
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            run_test_mse_responder(stream, MseMethod::Rc4, INFO_HASH).await;
        });
        let network = NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .with_encryption(PeerEncryptionPolicy::Required)
        .with_mse_rc4_only(true);
        let sink = Arc::new(RecordingMseSink::default());
        let (mut connection, _) = connect_observed(
            test_attempt_for(address),
            INFO_HASH,
            true,
            network,
            sink.clone(),
        )
        .await
        .expect("forced RC4 connection");
        assert_eq!(connection.mse_method(), Some(MseMethod::Rc4));
        assert_eq!(
            sink.handshakes.lock().expect("observations")[0].outcome,
            MseHandshakeOutcome::Negotiated(MseMethod::Rc4)
        );
        assert_eq!(
            next_message(&mut connection)
                .await
                .expect("carried message"),
            PeerMessage::Unchoke
        );
        send_message(&mut connection, &PeerMessage::Interested)
            .await
            .expect("encrypted framed send");
        server.await.expect("server join");
    }

    #[tokio::test]
    async fn prefer_falls_back_once_only_when_peer_closes_before_valid_dh_key() {
        const INFO_HASH: [u8; 20] = [0x55; 20];
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.expect("accept MSE attempt");
            let mut first_byte = [0; 1];
            first
                .read_exact(&mut first_byte)
                .await
                .expect("read MSE prefix");
            drop(first);

            let (mut second, _) = listener.accept().await.expect("accept plain fallback");
            let mut request = [0; HANDSHAKE_LENGTH];
            second
                .read_exact(&mut request)
                .await
                .expect("read plain handshake");
            decode_handshake(&request, INFO_HASH).expect("plain request");
            second
                .write_all(&encode_handshake(INFO_HASH, [0x77; 20]))
                .await
                .expect("write plain response");
        });
        let attempt = test_attempt_for(address);
        let network = NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .with_encryption(PeerEncryptionPolicy::Prefer);
        let sink = Arc::new(RecordingMseSink::default());
        let (connection, _) = connect_observed(attempt, INFO_HASH, false, network, sink.clone())
            .await
            .expect("plain fallback connection");
        let observation = sink.handshakes.lock().expect("observations")[0];
        assert!(observation.fallback_socket_used);
        assert_eq!(
            observation.outcome,
            MseHandshakeOutcome::Failed(MseHandshakeFailure::TransportIo)
        );
        assert_eq!(connection.mse_method(), None);
        assert_eq!(
            connection.mse_endpoint_update(),
            Some(MseEndpointState::PlainPreferred)
        );
        server.await.expect("server join");
    }

    #[tokio::test]
    async fn prefer_does_not_downgrade_after_a_complete_invalid_dh_key() {
        const INFO_HASH: [u8; 20] = [0x56; 20];
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept MSE attempt");
            let mut public_key = [0; 96];
            stream
                .read_exact(&mut public_key)
                .await
                .expect("read initiator public key");
            stream
                .write_all(&[0; 96])
                .await
                .expect("write invalid public key");
            drop(stream);
            assert!(
                timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err(),
                "malformed MSE must not open a fallback socket"
            );
        });
        let attempt = test_attempt_for(address);
        let network = NetworkConfig::new(
            NetworkPolicy::LoopbackOnly,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .with_encryption(PeerEncryptionPolicy::Prefer);
        assert!(matches!(
            connect(attempt, INFO_HASH, false, network).await,
            Err(PeerSocketError::MseDh(_))
        ));
        server.await.expect("server join");
    }

    async fn run_test_mse_responder(mut stream: TcpStream, method: MseMethod, info_hash: [u8; 20]) {
        let mut handshake = MseHandshake::new_responder(
            [0x91; 20],
            MsePadding::new(&[0x41; 17]).expect("PadB"),
            MsePadding::new(&[0x51; 23]).expect("PadD"),
            method.wire_bit(),
            method == MseMethod::Rc4,
        )
        .expect("responder state");
        let mut step = handshake.start().expect("start responder");
        let mut buffer = [0_u8; 137];
        loop {
            step = match step {
                MseStep::NeedInput => {
                    let read = stream.read(&mut buffer).await.expect("read MSE bytes");
                    assert_ne!(read, 0, "initiator closed during MSE");
                    let feed = handshake.feed(&buffer[..read]).expect("feed responder");
                    assert_eq!(feed.consumed, read);
                    feed.step
                }
                MseStep::Action(MseAction::ComputePublicKey { private }) => {
                    let public = compute_public_key(&private);
                    handshake
                        .resume(MseResume::PublicKeyComputed { private, public })
                        .expect("resume public key")
                }
                MseStep::Action(MseAction::ComputeSharedSecret {
                    private,
                    remote_public,
                }) => {
                    let shared = compute_shared_secret(&private, &remote_public)
                        .expect("valid initiator key");
                    handshake
                        .resume(MseResume::SharedSecretComputed(shared))
                        .expect("resume shared secret")
                }
                MseStep::Action(MseAction::IdentifyTorrent {
                    req2_hash: candidate,
                }) => {
                    assert_eq!(candidate, req2_hash(&info_hash));
                    handshake
                        .resume(MseResume::TorrentIdentified(Some(info_hash)))
                        .expect("resume torrent lookup")
                }
                MseStep::Action(MseAction::Send(bytes)) => {
                    stream
                        .write_all(bytes.as_slice())
                        .await
                        .expect("write MSE action");
                    handshake.resume(MseResume::Sent).expect("resume send")
                }
                MseStep::Complete(mut complete) => {
                    decode_handshake(&complete.carried.as_slice()[..HANDSHAKE_LENGTH], info_hash)
                        .expect("initiator handshake");
                    let response_handshake = encode_handshake(info_hash, [0x66; 20]);
                    let response_message =
                        encode_message(&PeerMessage::Unchoke).expect("encode carried response");
                    let mut response =
                        [response_handshake.as_slice(), response_message.as_slice()].concat();
                    if let Some(ciphers) = complete.ciphers.as_mut() {
                        ciphers.apply_send(&mut response);
                    }
                    stream
                        .write_all(&response)
                        .await
                        .expect("write responder handshake and frame");

                    let mut request =
                        encode_message(&PeerMessage::Interested).expect("encode request shape");
                    stream
                        .read_exact(&mut request)
                        .await
                        .expect("read framed request");
                    if let Some(ciphers) = complete.ciphers.as_mut() {
                        ciphers.apply_receive(&mut request);
                    }
                    assert_eq!(
                        request,
                        encode_message(&PeerMessage::Interested).expect("expected request")
                    );
                    return;
                }
            };
        }
    }

    #[tokio::test]
    async fn socket_set_reports_transport_before_handshake_completion() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let attempt = test_attempt_for(address);
        let info_hash = [7; 20];
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut request)
                .await
                .expect("read handshake");
            let request = decode_handshake(&request, info_hash).expect("decode handshake");
            assert!(request.supports_extensions());
            assert!(request.supports_fast_extension());
            release_rx.await.expect("release handshake");
            stream
                .write_all(&encode_handshake(info_hash, [8; 20]))
                .await
                .expect("write handshake");
        });

        let budget = PeerBudget::new(PeerBudgetConfig {
            configured_limit: 1,
            incoming_slack: 0,
            max_open_files: 10_000,
        });
        let mut sockets = PeerSocketSet::with_budget(budget.clone());
        sockets
            .begin_dial(
                attempt,
                info_hash,
                true,
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
                PeerDialServices::default(),
            )
            .expect("begin dial");
        assert_eq!(budget.snapshot().outgoing_connecting, 1);
        assert!(matches!(
            timeout(Duration::from_secs(1), sockets.next_event())
                .await
                .expect("transport phase deadline")
                .expect("transport phase"),
            PeerSetEvent::DialPhase {
                attempt: actual,
                transport: crate::peer_runtime::PeerTransport::Tcp,
            } if actual == attempt
        ));

        release_tx.send(()).expect("release server");
        let connection = match timeout(Duration::from_secs(1), sockets.next_event())
            .await
            .expect("handshake deadline")
            .expect("handshake event")
        {
            PeerSetEvent::DialCompleted {
                attempt: actual,
                result,
            } => {
                assert_eq!(actual, attempt);
                let Ok((connection, _)) = *result else {
                    panic!("dial unexpectedly failed");
                };
                connection
            }
            event => panic!("unexpected event {event:?}"),
        };
        assert_eq!(budget.snapshot().outgoing_connecting, 0);
        assert_eq!(budget.snapshot().outgoing_established, 1);
        assert_eq!(
            connection
                ._budget_permit
                .as_ref()
                .map(|permit| permit.phase()),
            Some(PeerBudgetPhase::Established)
        );
        assert!(matches!(
            sockets.begin_dial(
                attempt,
                info_hash,
                true,
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                ),
                PeerDialServices::default(),
            ),
            Err(super::PeerSetError::ConnectionLimit(_))
        ));
        sockets.add_connection(connection).expect("own connection");
        assert!(sockets.shutdown().await.expect("shutdown").is_empty());
        assert_eq!(budget.snapshot().total, 0);
        server.await.expect("server task");
    }

    #[test]
    fn utp_selection_is_ipv4_and_plaintext_only() {
        let ipv4 = "127.0.0.1:6881".parse().expect("IPv4 endpoint");
        let ipv6 = "[::1]:6881".parse().expect("IPv6 endpoint");
        assert!(utp_dial_eligible(ipv4, PeerEncryptionPolicy::Disabled));
        assert!(utp_dial_eligible(ipv4, PeerEncryptionPolicy::Allow));
        assert!(!utp_dial_eligible(ipv4, PeerEncryptionPolicy::Prefer));
        assert!(!utp_dial_eligible(ipv4, PeerEncryptionPolicy::Required));
        assert!(!utp_dial_eligible(ipv6, PeerEncryptionPolicy::Disabled));
        assert_eq!(
            preferred_transport(
                ipv4,
                PeerEncryptionPolicy::Disabled,
                true,
                crate::peer::UtpDialDecision::Try,
            ),
            PeerTransport::Utp
        );
        assert_eq!(
            preferred_transport(
                ipv4,
                PeerEncryptionPolicy::Disabled,
                false,
                crate::peer::UtpDialDecision::Try,
            ),
            PeerTransport::Tcp
        );
        assert_eq!(
            preferred_transport(
                ipv4,
                PeerEncryptionPolicy::Disabled,
                true,
                crate::peer::UtpDialDecision::TcpWhileSuppressed,
            ),
            PeerTransport::Tcp
        );
    }

    #[tokio::test]
    async fn socket_set_selects_utp_under_one_peer_budget_permit() {
        const INFO_HASH: [u8; 20] = [0xa1; 20];
        let client_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind client UDP");
        let server_socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind server UDP");
        let server_address = server_socket.local_addr().expect("server address");
        let (mut client_udp, _) = SessionUdpService::start(client_socket).expect("client UDP");
        let (mut server_udp, _) = SessionUdpService::start(server_socket).expect("server UDP");
        let client_utp = UtpService::start(&mut client_udp).expect("client uTP");
        let mut server_utp = UtpService::start(&mut server_udp).expect("server uTP");
        let server = tokio::spawn(async move {
            let mut stream = timeout(Duration::from_secs(2), server_utp.accept())
                .await
                .expect("uTP accept deadline")
                .expect("uTP stream");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read uTP handshake");
            decode_handshake(&handshake, INFO_HASH).expect("decode uTP handshake");
            stream
                .write_all(&encode_handshake(INFO_HASH, [0xb2; 20]))
                .await
                .expect("write uTP handshake");
            stream.flush().await.expect("flush uTP handshake");
            server_utp
        });
        let attempt = test_attempt_for(server_address);
        let budget = PeerBudget::new(PeerBudgetConfig {
            configured_limit: 1,
            incoming_slack: 0,
            max_open_files: 10_000,
        });
        let mut sockets = PeerSocketSet::with_budget(budget.clone());
        sockets
            .begin_dial(
                attempt,
                INFO_HASH,
                false,
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_secs(2),
                    Duration::from_secs(2),
                ),
                PeerDialServices {
                    utp: Some(client_utp.handle()),
                    ..PeerDialServices::default()
                },
            )
            .expect("begin uTP dial");
        assert!(matches!(
            timeout(Duration::from_secs(2), sockets.next_event())
                .await
                .expect("uTP phase deadline")
                .expect("uTP phase"),
            PeerSetEvent::DialPhase {
                attempt: actual,
                transport: PeerTransport::Utp,
            } if actual == attempt
        ));
        let connection = match timeout(Duration::from_secs(2), sockets.next_event())
            .await
            .expect("uTP handshake deadline")
            .expect("uTP handshake event")
        {
            PeerSetEvent::DialCompleted { result, .. } => result.expect("uTP dial succeeds").0,
            event => panic!("unexpected uTP event {event:?}"),
        };
        assert_eq!(connection.transport(), PeerTransport::Utp);
        assert_eq!(budget.snapshot().total_high_water, 1);
        drop(connection);
        assert!(
            sockets
                .shutdown()
                .await
                .expect("socket shutdown")
                .is_empty()
        );
        let server_utp = server.await.expect("server task");
        let client_terminal = client_utp.shutdown().await.expect("client uTP shutdown");
        let server_terminal = server_utp.shutdown().await.expect("server uTP shutdown");
        assert_eq!(client_terminal.active_connections, 0);
        assert_eq!(server_terminal.active_connections, 0);
        client_udp.shutdown().await.expect("client UDP shutdown");
        server_udp.shutdown().await.expect("server UDP shutdown");
    }

    #[tokio::test]
    async fn utp_connect_timeout_falls_back_to_tcp_in_same_attempt() {
        const INFO_HASH: [u8; 20] = [0xc3; 20];
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind TCP");
        let target = listener.local_addr().expect("TCP target");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept TCP fallback");
            let mut handshake = [0; HANDSHAKE_LENGTH];
            stream
                .read_exact(&mut handshake)
                .await
                .expect("read TCP handshake");
            decode_handshake(&handshake, INFO_HASH).expect("decode TCP handshake");
            stream
                .write_all(&encode_handshake(INFO_HASH, [0xd4; 20]))
                .await
                .expect("write TCP handshake");
        });
        let socket = UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind client UDP");
        let (mut udp, _) = SessionUdpService::start(socket).expect("session UDP");
        let utp = UtpService::start(&mut udp).expect("client uTP");
        let attempt = test_attempt_for(target);
        let budget = PeerBudget::new(PeerBudgetConfig {
            configured_limit: 1,
            incoming_slack: 0,
            max_open_files: 10_000,
        });
        let mut sockets = PeerSocketSet::with_budget(budget.clone());
        sockets
            .begin_dial(
                attempt,
                INFO_HASH,
                false,
                NetworkConfig::new(
                    NetworkPolicy::LoopbackOnly,
                    Duration::from_millis(200),
                    Duration::from_secs(1),
                ),
                PeerDialServices {
                    utp: Some(utp.handle()),
                    ..PeerDialServices::default()
                },
            )
            .expect("begin fallback dial");
        assert!(matches!(
            timeout(Duration::from_secs(2), sockets.next_event())
                .await
                .expect("fallback phase deadline")
                .expect("fallback phase"),
            PeerSetEvent::DialPhase {
                attempt: actual,
                transport: PeerTransport::Tcp,
            } if actual == attempt
        ));
        assert_eq!(utp.snapshot().active_connections, 0);
        let connection = match timeout(Duration::from_secs(2), sockets.next_event())
            .await
            .expect("fallback handshake deadline")
            .expect("fallback handshake event")
        {
            PeerSetEvent::DialCompleted { result, .. } => result.expect("TCP fallback succeeds").0,
            event => panic!("unexpected fallback event {event:?}"),
        };
        assert_eq!(connection.transport(), PeerTransport::Tcp);
        assert_eq!(budget.snapshot().total_high_water, 1);
        assert!(utp.snapshot().datagrams_sent > 0);
        drop(connection);
        assert!(
            sockets
                .shutdown()
                .await
                .expect("socket shutdown")
                .is_empty()
        );
        let terminal = utp.shutdown().await.expect("uTP shutdown");
        assert_eq!(terminal.active_connections, 0);
        udp.shutdown().await.expect("UDP shutdown");
        server.await.expect("TCP server task");
    }

    #[tokio::test]
    async fn task_routes_bounded_commands_and_generation_tagged_events() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let attempt = connection.attempt();
        let (event_tx, mut events) = mpsc::channel(4);
        let task = PeerSocketTask::spawn(connection, event_tx);
        assert_eq!(task.attempt(), attempt);

        task.send(PeerMessage::Interested)
            .await
            .expect("send command");
        let expected = encode_message(&PeerMessage::Interested).expect("frame");
        let mut received = vec![0; expected.len()];
        server
            .read_exact(&mut received)
            .await
            .expect("read command");
        assert_eq!(received, expected);

        server
            .write_all(&encode_message(&PeerMessage::Unchoke).expect("frame"))
            .await
            .expect("write event");
        match events.recv().await.expect("message event") {
            PeerTaskEvent::Message {
                attempt: actual,
                message: PeerMessage::Unchoke,
            } => assert_eq!(actual, attempt),
            event => panic!("unexpected event {event:?}"),
        }
        task.shutdown().await.expect("join task");
    }

    #[tokio::test]
    async fn cancellation_joins_when_the_event_queue_is_saturated() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let (event_tx, _events) = mpsc::channel(1);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let mut frames = Vec::new();
        for _ in 0..3 {
            frames.extend(encode_message(&PeerMessage::KeepAlive).expect("frame"));
        }
        server.write_all(&frames).await.expect("fill event queue");
        tokio::task::yield_now().await;
        timeout(Duration::from_millis(200), task.shutdown())
            .await
            .expect("bounded shutdown")
            .expect("join task");
    }

    #[tokio::test]
    async fn outbound_commands_drain_while_inbound_event_delivery_is_backpressured() {
        let (connection, mut server) = connected_pair(Duration::from_secs(1)).await;
        let (event_tx, mut events) = mpsc::channel(1);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let keepalive = encode_message(&PeerMessage::KeepAlive).expect("keepalive");
        let mut inbound = Vec::new();
        for _ in 0..3 {
            inbound.extend_from_slice(&keepalive);
        }
        server
            .write_all(&inbound)
            .await
            .expect("saturate inbound event delivery");
        assert!(matches!(
            timeout(Duration::from_millis(200), events.recv())
                .await
                .expect("first inbound event")
                .expect("event channel"),
            PeerTaskEvent::Message {
                message: PeerMessage::KeepAlive,
                ..
            }
        ));
        tokio::task::yield_now().await;

        timeout(Duration::from_millis(200), async {
            for _ in 0..=PEER_COMMAND_QUEUE {
                task.send(PeerMessage::Interested)
                    .await
                    .expect("bounded outbound command");
            }
        })
        .await
        .expect("event backpressure must not block outbound commands");

        let interested = encode_message(&PeerMessage::Interested).expect("interested");
        let mut outbound = vec![0; interested.len() * (PEER_COMMAND_QUEUE + 1)];
        timeout(Duration::from_millis(200), server.read_exact(&mut outbound))
            .await
            .expect("outbound commands reached socket")
            .expect("read outbound commands");
        assert!(
            outbound
                .chunks_exact(interested.len())
                .all(|frame| frame == interested)
        );
        for _ in 0..2 {
            assert!(matches!(
                timeout(Duration::from_millis(200), events.recv())
                    .await
                    .expect("pending inbound event")
                    .expect("event channel"),
                PeerTaskEvent::Message {
                    message: PeerMessage::KeepAlive,
                    ..
                }
            ));
        }
        task.shutdown().await.expect("join task");
    }

    #[tokio::test]
    async fn fragmented_input_cannot_refresh_the_task_message_deadline() {
        let (connection, mut server) = connected_pair(Duration::from_millis(50)).await;
        let attempt = connection.attempt();
        let (event_tx, mut events) = mpsc::channel(4);
        let task = PeerSocketTask::spawn(connection, event_tx);
        let frame = encode_message(&PeerMessage::KeepAlive).expect("frame");
        let writer = tokio::spawn(async move {
            for byte in frame {
                if server.write_all(&[byte]).await.is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        match timeout(Duration::from_millis(300), events.recv())
            .await
            .expect("task deadline")
            .expect("stopped event")
        {
            PeerTaskEvent::Stopped {
                attempt: actual,
                result:
                    Err(PeerSocketError::TimedOut {
                        operation: "message read",
                        ..
                    }),
            } => assert_eq!(actual, attempt),
            event => panic!("unexpected event {event:?}"),
        }
        writer.await.expect("writer join");
        task.shutdown().await.expect("task join");
    }
}

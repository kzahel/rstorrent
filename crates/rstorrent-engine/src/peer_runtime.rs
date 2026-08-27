//! Runtime-independent active peer connection lifecycle and observation.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use rstorrent_protocol::mse::MseMethod;
use rstorrent_protocol::peer_wire::Handshake;

use crate::peer::{DialAttempt, PeerFailure, PeerRecordId, PeerSources};
use crate::swarm::ConnectionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerConnectionDirection {
    Incoming,
    Outgoing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PeerAdmissionRejection {
    SelfConnection,
    DuplicatePeerId { winner: ConnectionId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PeerAdmissionOutcome {
    Admitted { evicted: Option<ConnectionId> },
    Rejected(PeerAdmissionRejection),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PeerIdConnection {
    connection: ConnectionId,
    direction: PeerConnectionDirection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerTransport {
    Tcp,
    Utp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerConnectionLifecycle {
    TransportConnecting,
    ProtocolHandshaking,
    Connected,
    Disconnecting,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerConnectionRole {
    Metadata,
    Content,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerRequestWindowPhase {
    SlowStart,
    Steady,
    Stalled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerContentActivity {
    pub choking: bool,
    pub wanted_piece_count: usize,
    pub pending_requests: usize,
    pub target_requests: usize,
    pub queued_payload_bytes: usize,
    pub useful_payload_bytes: usize,
    pub observed_payload_rate: usize,
    pub connected_age: Duration,
    pub last_useful_age: Option<Duration>,
    pub last_payload_age: Option<Duration>,
    pub request_timeout: Duration,
    pub oldest_request_age: Option<Duration>,
    pub request_window_phase: PeerRequestWindowPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerUploadGrant {
    Choked,
    Regular,
    Optimistic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerUploadActivity {
    pub interested: bool,
    pub grant: PeerUploadGrant,
    pub queued_requests: usize,
    pub queued_bytes: usize,
    pub read_active: bool,
    pub writer_bytes: usize,
    pub payload_bytes: u64,
    pub payload_rate: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerConnectionObservation {
    pub connection_id: ConnectionId,
    pub record_id: Option<PeerRecordId>,
    pub endpoint: SocketAddr,
    pub local_endpoint: Option<SocketAddr>,
    pub sources: PeerSources,
    pub direction: PeerConnectionDirection,
    pub transport: PeerTransport,
    pub lifecycle: PeerConnectionLifecycle,
    pub role: PeerConnectionRole,
    pub started_at: Duration,
    pub lifecycle_changed_at: Duration,
    pub peer_id: Option<[u8; 20]>,
    pub supports_extensions: Option<bool>,
    pub supports_ut_metadata: Option<bool>,
    pub mse_method: Option<MseMethod>,
    pub content: Option<PeerContentActivity>,
    pub upload: Option<PeerUploadActivity>,
    pub close_reason: Option<PeerFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IncomingPeerStart {
    pub(crate) record_id: PeerRecordId,
    pub(crate) endpoint: SocketAddr,
    pub(crate) local_endpoint: SocketAddr,
    pub(crate) transport: PeerTransport,
    pub(crate) role: PeerConnectionRole,
    pub(crate) peer_id: [u8; 20],
    pub(crate) supports_extensions: bool,
    pub(crate) mse_method: Option<MseMethod>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeerRuntimeError {
    DuplicateConnection(ConnectionId),
    UnknownConnection(ConnectionId),
    InvalidTransition {
        connection: ConnectionId,
        from: PeerConnectionLifecycle,
        to: PeerConnectionLifecycle,
    },
}

impl fmt::Display for PeerRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateConnection(connection) => {
                write!(formatter, "duplicate peer connection {}", connection.get())
            }
            Self::UnknownConnection(connection) => {
                write!(formatter, "unknown peer connection {}", connection.get())
            }
            Self::InvalidTransition {
                connection,
                from,
                to,
            } => write!(
                formatter,
                "peer connection {} cannot transition from {from:?} to {to:?}",
                connection.get()
            ),
        }
    }
}

impl Error for PeerRuntimeError {}

#[derive(Debug, Default)]
pub(crate) struct PeerRuntime {
    connections: BTreeMap<ConnectionId, PeerConnectionObservation>,
    peer_ids: BTreeMap<[u8; 20], PeerIdConnection>,
}

impl PeerRuntime {
    pub(crate) fn set_transport(
        &mut self,
        connection: ConnectionId,
        transport: PeerTransport,
    ) -> Result<(), PeerRuntimeError> {
        self.connection_mut(connection)?.transport = transport;
        Ok(())
    }

    pub(crate) fn begin_outgoing(
        &mut self,
        attempt: DialAttempt,
        role: PeerConnectionRole,
        now: Duration,
    ) -> Result<ConnectionId, PeerRuntimeError> {
        let connection = connection_id(attempt);
        self.insert(PeerConnectionObservation {
            connection_id: connection,
            record_id: Some(attempt.record_id()),
            endpoint: attempt.endpoint().address(),
            local_endpoint: None,
            sources: PeerSources::default(),
            direction: PeerConnectionDirection::Outgoing,
            transport: PeerTransport::Tcp,
            lifecycle: PeerConnectionLifecycle::TransportConnecting,
            role,
            started_at: now,
            lifecycle_changed_at: now,
            peer_id: None,
            supports_extensions: None,
            supports_ut_metadata: None,
            mse_method: None,
            content: None,
            upload: None,
            close_reason: None,
        })?;
        Ok(connection)
    }

    pub(crate) fn begin_incoming(
        &mut self,
        connection: ConnectionId,
        start: IncomingPeerStart,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        self.insert(PeerConnectionObservation {
            connection_id: connection,
            record_id: Some(start.record_id),
            endpoint: start.endpoint,
            local_endpoint: Some(start.local_endpoint),
            sources: PeerSources::from_source(crate::peer::PeerSource::Incoming),
            direction: PeerConnectionDirection::Incoming,
            transport: start.transport,
            lifecycle: PeerConnectionLifecycle::ProtocolHandshaking,
            role: start.role,
            started_at: now,
            lifecycle_changed_at: now,
            peer_id: Some(start.peer_id),
            supports_extensions: Some(start.supports_extensions),
            supports_ut_metadata: None,
            mse_method: start.mse_method,
            content: None,
            upload: None,
            close_reason: None,
        })
    }

    pub(crate) fn transport_connected(
        &mut self,
        connection: ConnectionId,
        transport: PeerTransport,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        self.set_transport(connection, transport)?;
        self.transition(
            connection,
            PeerConnectionLifecycle::ProtocolHandshaking,
            now,
            &[PeerConnectionLifecycle::TransportConnecting],
        )
    }

    pub(crate) fn handshake_completed(
        &mut self,
        connection: ConnectionId,
        handshake: &Handshake,
        local_peer_id: [u8; 20],
        now: Duration,
    ) -> Result<PeerAdmissionOutcome, PeerRuntimeError> {
        let outcome = self.admit_peer_id(connection, handshake.peer_id, local_peer_id, now)?;
        if matches!(outcome, PeerAdmissionOutcome::Admitted { .. }) {
            self.connection_mut(connection)?.supports_extensions =
                Some(handshake.supports_extensions());
        }
        Ok(outcome)
    }

    pub(crate) fn set_mse_method(
        &mut self,
        connection: ConnectionId,
        method: Option<MseMethod>,
    ) -> Result<(), PeerRuntimeError> {
        self.connection_mut(connection)?.mse_method = method;
        Ok(())
    }

    pub(crate) fn incoming_handshake_completed(
        &mut self,
        connection: ConnectionId,
        local_peer_id: [u8; 20],
        now: Duration,
    ) -> Result<PeerAdmissionOutcome, PeerRuntimeError> {
        let peer_id = self
            .connection_mut(connection)?
            .peer_id
            .expect("routed incoming handshakes retain the validated peer ID");
        self.admit_peer_id(connection, peer_id, local_peer_id, now)
    }

    fn admit_peer_id(
        &mut self,
        connection: ConnectionId,
        peer_id: [u8; 20],
        local_peer_id: [u8; 20],
        now: Duration,
    ) -> Result<PeerAdmissionOutcome, PeerRuntimeError> {
        let candidate = self.connection_mut(connection)?;
        if !matches!(
            candidate.lifecycle,
            PeerConnectionLifecycle::TransportConnecting
                | PeerConnectionLifecycle::ProtocolHandshaking
        ) {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: candidate.lifecycle,
                to: PeerConnectionLifecycle::Connected,
            });
        }
        let candidate_direction = candidate.direction;
        candidate.peer_id = Some(peer_id);
        if peer_id == local_peer_id {
            self.reject(connection, PeerFailure::SelfConnection, now)?;
            return Ok(PeerAdmissionOutcome::Rejected(
                PeerAdmissionRejection::SelfConnection,
            ));
        }
        let existing = self.peer_ids.get(&peer_id).copied();
        if let Some(existing) = existing {
            let candidate_wins = existing.direction != candidate_direction
                && candidate_direction == preferred_direction(local_peer_id, peer_id);
            if !candidate_wins {
                self.reject(connection, PeerFailure::DuplicatePeerId, now)?;
                return Ok(PeerAdmissionOutcome::Rejected(
                    PeerAdmissionRejection::DuplicatePeerId {
                        winner: existing.connection,
                    },
                ));
            }
            self.reject(existing.connection, PeerFailure::DuplicatePeerId, now)?;
        }
        self.peer_ids.insert(
            peer_id,
            PeerIdConnection {
                connection,
                direction: candidate_direction,
            },
        );
        let candidate = self.connection_mut(connection)?;
        candidate.lifecycle = PeerConnectionLifecycle::Connected;
        candidate.lifecycle_changed_at = now;
        candidate.peer_id = Some(peer_id);
        Ok(PeerAdmissionOutcome::Admitted {
            evicted: existing.map(|existing| existing.connection),
        })
    }

    pub(crate) fn set_sources(
        &mut self,
        connection: ConnectionId,
        sources: PeerSources,
    ) -> Result<(), PeerRuntimeError> {
        self.connection_mut(connection)?.sources = sources;
        Ok(())
    }

    pub(crate) fn set_role(
        &mut self,
        connection: ConnectionId,
        role: PeerConnectionRole,
    ) -> Result<(), PeerRuntimeError> {
        self.connection_mut(connection)?.role = role;
        Ok(())
    }

    pub(crate) fn set_content_activity(
        &mut self,
        connection: ConnectionId,
        activity: PeerContentActivity,
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        if peer.lifecycle != PeerConnectionLifecycle::Connected {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: peer.lifecycle,
                to: PeerConnectionLifecycle::Connected,
            });
        }
        peer.content = Some(activity);
        Ok(())
    }

    pub(crate) fn set_upload_activity(
        &mut self,
        connection: ConnectionId,
        activity: PeerUploadActivity,
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        if peer.lifecycle != PeerConnectionLifecycle::Connected {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: peer.lifecycle,
                to: PeerConnectionLifecycle::Connected,
            });
        }
        peer.upload = Some(activity);
        Ok(())
    }

    pub(crate) fn finalize_upload_transfer(
        &mut self,
        connection: ConnectionId,
        payload_bytes: u64,
        payload_rate: u64,
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        let upload = peer
            .upload
            .as_mut()
            .ok_or(PeerRuntimeError::UnknownConnection(connection))?;
        upload.payload_bytes = payload_bytes;
        upload.payload_rate = payload_rate;
        Ok(())
    }

    pub(crate) fn set_metadata_extension(
        &mut self,
        connection: ConnectionId,
        supported: bool,
    ) -> Result<(), PeerRuntimeError> {
        self.connection_mut(connection)?.supports_ut_metadata = Some(supported);
        Ok(())
    }

    pub(crate) fn begin_disconnect(
        &mut self,
        connection: ConnectionId,
        reason: Option<PeerFailure>,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        if peer.lifecycle != PeerConnectionLifecycle::Disconnecting {
            peer.lifecycle = PeerConnectionLifecycle::Disconnecting;
            peer.lifecycle_changed_at = now;
        }
        if peer.close_reason.is_none() {
            peer.close_reason = reason;
        }
        Ok(())
    }

    pub(crate) fn remove(
        &mut self,
        connection: ConnectionId,
    ) -> Result<PeerConnectionObservation, PeerRuntimeError> {
        let peer = self
            .connections
            .get(&connection)
            .ok_or(PeerRuntimeError::UnknownConnection(connection))?;
        if peer.lifecycle != PeerConnectionLifecycle::Disconnecting {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: peer.lifecycle,
                to: PeerConnectionLifecycle::Disconnecting,
            });
        }
        let removed = self
            .connections
            .remove(&connection)
            .expect("connection exists after validation");
        if let Some(peer_id) = removed.peer_id
            && self
                .peer_ids
                .get(&peer_id)
                .is_some_and(|entry| entry.connection == connection)
        {
            self.peer_ids.remove(&peer_id);
        }
        Ok(removed)
    }

    pub(crate) fn observation(
        &self,
        connection: ConnectionId,
    ) -> Option<&PeerConnectionObservation> {
        self.connections.get(&connection)
    }

    pub(crate) fn snapshot(&self) -> Vec<PeerConnectionObservation> {
        self.connections.values().cloned().collect()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn peer_id_count(&self) -> usize {
        self.peer_ids.len()
    }

    fn insert(&mut self, peer: PeerConnectionObservation) -> Result<(), PeerRuntimeError> {
        let connection = peer.connection_id;
        if self.connections.insert(connection, peer).is_some() {
            return Err(PeerRuntimeError::DuplicateConnection(connection));
        }
        Ok(())
    }

    fn transition(
        &mut self,
        connection: ConnectionId,
        to: PeerConnectionLifecycle,
        now: Duration,
        allowed: &[PeerConnectionLifecycle],
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        if !allowed.contains(&peer.lifecycle) {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: peer.lifecycle,
                to,
            });
        }
        peer.lifecycle = to;
        peer.lifecycle_changed_at = now;
        Ok(())
    }

    fn connection_mut(
        &mut self,
        connection: ConnectionId,
    ) -> Result<&mut PeerConnectionObservation, PeerRuntimeError> {
        self.connections
            .get_mut(&connection)
            .ok_or(PeerRuntimeError::UnknownConnection(connection))
    }

    fn reject(
        &mut self,
        connection: ConnectionId,
        failure: PeerFailure,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        self.begin_disconnect(connection, Some(failure), now)
    }
}

fn preferred_direction(
    local_peer_id: [u8; 20],
    remote_peer_id: [u8; 20],
) -> PeerConnectionDirection {
    if local_peer_id > remote_peer_id {
        PeerConnectionDirection::Outgoing
    } else {
        PeerConnectionDirection::Incoming
    }
}

pub(crate) fn connection_id(attempt: DialAttempt) -> ConnectionId {
    attempt.connection_id()
}

#[cfg(test)]
mod tests {
    use super::{
        IncomingPeerStart, PeerAdmissionOutcome, PeerAdmissionRejection, PeerConnectionDirection,
        PeerConnectionLifecycle, PeerConnectionRole, PeerRuntime, PeerRuntimeError, PeerTransport,
        connection_id,
    };
    use crate::peer::{
        PeerEndpoint, PeerObservation, PeerRegistry, PeerRegistryConfig, PeerSelectionContext,
        PeerSelector, PeerSource,
    };
    use crate::swarm::ConnectionId;
    use rstorrent_protocol::peer_wire::Handshake;
    use std::time::Duration;

    fn attempt() -> crate::peer::DialAttempt {
        attempt_with(1, "127.0.0.1:6881")
    }

    fn attempt_with(connection: u64, address: &str) -> crate::peer::DialAttempt {
        let endpoint = PeerEndpoint::new(address.parse().expect("address")).expect("endpoint");
        let mut registry = PeerRegistry::new(PeerRegistryConfig::default()).expect("registry");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("observe");
        let context = PeerSelectionContext {
            now: Duration::ZERO,
        };
        let candidate = PeerSelector.select(&registry, context).expect("candidate");
        registry
            .begin_dial_with_connection_id(
                candidate,
                context,
                ConnectionId::new(connection).expect("connection ID"),
            )
            .expect("attempt")
    }

    fn begin_outgoing(runtime: &mut PeerRuntime, connection: u64, address: &str) -> ConnectionId {
        let attempt = attempt_with(connection, address);
        let id = connection_id(attempt);
        runtime
            .begin_outgoing(attempt, PeerConnectionRole::Content, Duration::ZERO)
            .expect("begin outgoing");
        runtime
            .transport_connected(id, PeerTransport::Tcp, Duration::ZERO)
            .expect("transport connected");
        id
    }

    fn begin_incoming(
        runtime: &mut PeerRuntime,
        connection: u64,
        address: &str,
        peer_id: [u8; 20],
    ) -> ConnectionId {
        let id = ConnectionId::new(connection).expect("connection ID");
        runtime
            .begin_incoming(
                id,
                IncomingPeerStart {
                    record_id: attempt().record_id(),
                    endpoint: address.parse().expect("endpoint"),
                    local_endpoint: "127.0.0.1:6881".parse().expect("local endpoint"),
                    transport: PeerTransport::Tcp,
                    role: PeerConnectionRole::Content,
                    peer_id,
                    supports_extensions: false,
                    mse_method: None,
                },
                Duration::ZERO,
            )
            .expect("begin incoming");
        id
    }

    #[test]
    fn outgoing_generation_retains_identity_until_disconnect_cleanup() {
        let attempt = attempt();
        let connection = connection_id(attempt);
        let mut runtime = PeerRuntime::default();
        runtime
            .begin_outgoing(attempt, PeerConnectionRole::Metadata, Duration::ZERO)
            .expect("begin");
        runtime
            .transport_connected(connection, PeerTransport::Tcp, Duration::from_millis(5))
            .expect("transport");
        let handshake = Handshake {
            peer_id: *b"-LTTEST-000000000000",
            reserved: [0; 8],
        };
        runtime
            .handshake_completed(
                connection,
                &handshake,
                *b"-RS0001-LOCALPEER001",
                Duration::from_millis(8),
            )
            .expect("handshake");
        runtime
            .set_role(connection, PeerConnectionRole::Content)
            .expect("handoff");

        let peer = runtime.observation(connection).expect("active row");
        assert_eq!(peer.connection_id, connection);
        assert_eq!(peer.lifecycle, PeerConnectionLifecycle::Connected);
        assert_eq!(peer.role, PeerConnectionRole::Content);
        assert_eq!(peer.peer_id, Some(handshake.peer_id));

        runtime
            .begin_disconnect(connection, None, Duration::from_millis(10))
            .expect("disconnecting");
        assert_eq!(
            runtime
                .observation(connection)
                .expect("disconnecting row")
                .lifecycle,
            PeerConnectionLifecycle::Disconnecting
        );
        runtime.remove(connection).expect("remove after cleanup");
        assert!(runtime.is_empty());
    }

    #[test]
    fn incoming_generation_begins_at_protocol_handshake_with_routed_identity() {
        let connection = ConnectionId::new(42).expect("connection");
        let mut runtime = PeerRuntime::default();
        runtime
            .begin_incoming(
                connection,
                IncomingPeerStart {
                    record_id: attempt().record_id(),
                    endpoint: "127.0.0.1:51413".parse().expect("endpoint"),
                    local_endpoint: "127.0.0.1:6881".parse().expect("local endpoint"),
                    transport: PeerTransport::Utp,
                    role: PeerConnectionRole::Metadata,
                    peer_id: *b"-LTTEST-000000000000",
                    supports_extensions: true,
                    mse_method: None,
                },
                Duration::from_secs(1),
            )
            .expect("incoming intake");
        let peer = runtime.observation(connection).expect("active row");
        assert_eq!(peer.direction, PeerConnectionDirection::Incoming);
        assert_eq!(peer.transport, PeerTransport::Utp);
        assert_eq!(peer.lifecycle, PeerConnectionLifecycle::ProtocolHandshaking);
        assert!(peer.record_id.is_some());
        assert_eq!(peer.peer_id, Some(*b"-LTTEST-000000000000"));
    }

    #[test]
    fn stale_or_out_of_order_transitions_are_rejected() {
        let attempt = attempt();
        let connection = connection_id(attempt);
        let mut runtime = PeerRuntime::default();
        runtime
            .begin_outgoing(attempt, PeerConnectionRole::Metadata, Duration::ZERO)
            .expect("begin");
        assert!(matches!(
            runtime.remove(connection),
            Err(PeerRuntimeError::InvalidTransition { .. })
        ));
        let unknown = ConnectionId::new(connection.get() + 1).expect("unknown");
        assert!(matches!(
            runtime.transport_connected(unknown, PeerTransport::Tcp, Duration::ZERO),
            Err(PeerRuntimeError::UnknownConnection(actual)) if actual == unknown
        ));
    }

    #[test]
    fn self_connection_is_rejected_before_admission() {
        let local = *b"-RS0001-LOCALPEER001";
        let mut runtime = PeerRuntime::default();
        let connection = begin_outgoing(&mut runtime, 1, "127.0.0.1:6881");
        let outcome = runtime
            .handshake_completed(
                connection,
                &Handshake {
                    peer_id: local,
                    reserved: [0; 8],
                },
                local,
                Duration::from_millis(1),
            )
            .expect("decision");
        assert_eq!(
            outcome,
            PeerAdmissionOutcome::Rejected(PeerAdmissionRejection::SelfConnection)
        );
        let peer = runtime.observation(connection).expect("candidate");
        assert_eq!(peer.lifecycle, PeerConnectionLifecycle::Disconnecting);
        assert_eq!(
            peer.close_reason,
            Some(crate::peer::PeerFailure::SelfConnection)
        );
        assert_eq!(runtime.peer_id_count(), 0);
    }

    #[test]
    fn same_direction_duplicate_keeps_first_generation() {
        let local = [b'a'; 20];
        let remote = [b'z'; 20];
        let mut runtime = PeerRuntime::default();
        let first = begin_outgoing(&mut runtime, 1, "127.0.0.1:6881");
        let second = begin_outgoing(&mut runtime, 2, "127.0.0.1:6882");
        for (connection, expected) in [
            (first, PeerAdmissionOutcome::Admitted { evicted: None }),
            (
                second,
                PeerAdmissionOutcome::Rejected(PeerAdmissionRejection::DuplicatePeerId {
                    winner: first,
                }),
            ),
        ] {
            assert_eq!(
                runtime
                    .handshake_completed(
                        connection,
                        &Handshake {
                            peer_id: remote,
                            reserved: [0; 8],
                        },
                        local,
                        Duration::from_millis(connection.get()),
                    )
                    .expect("decision"),
                expected
            );
        }
        assert_eq!(runtime.peer_id_count(), 1);
        assert_eq!(
            runtime.observation(first).expect("winner").lifecycle,
            PeerConnectionLifecycle::Connected
        );
        assert_eq!(
            runtime.observation(second).expect("loser").close_reason,
            Some(crate::peer::PeerFailure::DuplicatePeerId)
        );
    }

    #[test]
    fn crossed_connection_rule_selects_the_same_physical_direction() {
        let remote = [b'm'; 20];
        for (local, winner_direction) in [
            ([b'z'; 20], PeerConnectionDirection::Outgoing),
            ([b'a'; 20], PeerConnectionDirection::Incoming),
        ] {
            let mut runtime = PeerRuntime::default();
            let first = if winner_direction == PeerConnectionDirection::Outgoing {
                begin_incoming(&mut runtime, 1, "127.0.0.1:51413", remote)
            } else {
                begin_outgoing(&mut runtime, 1, "127.0.0.1:51413")
            };
            let first_outcome = match winner_direction {
                PeerConnectionDirection::Outgoing => {
                    runtime.incoming_handshake_completed(first, local, Duration::from_millis(1))
                }
                PeerConnectionDirection::Incoming => runtime.handshake_completed(
                    first,
                    &Handshake {
                        peer_id: remote,
                        reserved: [0; 8],
                    },
                    local,
                    Duration::from_millis(1),
                ),
            }
            .expect("first admission");
            assert_eq!(
                first_outcome,
                PeerAdmissionOutcome::Admitted { evicted: None }
            );

            let winner = if winner_direction == PeerConnectionDirection::Outgoing {
                begin_outgoing(&mut runtime, 2, "[::1]:51413")
            } else {
                begin_incoming(&mut runtime, 2, "[::1]:51413", remote)
            };
            let winner_outcome = match winner_direction {
                PeerConnectionDirection::Outgoing => runtime.handshake_completed(
                    winner,
                    &Handshake {
                        peer_id: remote,
                        reserved: [0; 8],
                    },
                    local,
                    Duration::from_millis(2),
                ),
                PeerConnectionDirection::Incoming => {
                    runtime.incoming_handshake_completed(winner, local, Duration::from_millis(2))
                }
            }
            .expect("winner admission");
            assert_eq!(
                winner_outcome,
                PeerAdmissionOutcome::Admitted {
                    evicted: Some(first)
                }
            );
            assert_eq!(
                runtime.observation(first).expect("old loser").close_reason,
                Some(crate::peer::PeerFailure::DuplicatePeerId)
            );
            assert_eq!(
                runtime.observation(winner).expect("winner").direction,
                winner_direction
            );
            runtime.remove(first).expect("stale loser cleanup");
            assert_eq!(runtime.peer_id_count(), 1);
            runtime
                .begin_disconnect(winner, None, Duration::from_millis(3))
                .expect("winner disconnect");
            runtime.remove(winner).expect("winner cleanup");
            assert_eq!(runtime.peer_id_count(), 0);
        }
    }
}

//! Runtime-independent active peer connection lifecycle and observation.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use rstorrent_protocol::peer_wire::Handshake;

use crate::peer::{DialAttempt, PeerFailure, PeerRecordId, PeerSources};
use crate::swarm::ConnectionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerConnectionDirection {
    Incoming,
    Outgoing,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeerConnectionObservation {
    pub connection_id: ConnectionId,
    pub record_id: Option<PeerRecordId>,
    pub endpoint: SocketAddr,
    pub sources: PeerSources,
    pub direction: PeerConnectionDirection,
    pub transport: PeerTransport,
    pub lifecycle: PeerConnectionLifecycle,
    pub role: PeerConnectionRole,
    pub started_at: Duration,
    pub lifecycle_changed_at: Duration,
    pub peer_id: Option<[u8; 20]>,
    pub supports_extensions: Option<bool>,
    pub content: Option<PeerContentActivity>,
    pub close_reason: Option<PeerFailure>,
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
}

impl PeerRuntime {
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
            sources: PeerSources::default(),
            direction: PeerConnectionDirection::Outgoing,
            transport: PeerTransport::Tcp,
            lifecycle: PeerConnectionLifecycle::TransportConnecting,
            role,
            started_at: now,
            lifecycle_changed_at: now,
            peer_id: None,
            supports_extensions: None,
            content: None,
            close_reason: None,
        })?;
        Ok(connection)
    }

    #[cfg(test)]
    pub(crate) fn begin_incoming(
        &mut self,
        connection: ConnectionId,
        endpoint: SocketAddr,
        transport: PeerTransport,
        role: PeerConnectionRole,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        self.insert(PeerConnectionObservation {
            connection_id: connection,
            record_id: None,
            endpoint,
            sources: PeerSources::from_source(crate::peer::PeerSource::Incoming),
            direction: PeerConnectionDirection::Incoming,
            transport,
            lifecycle: PeerConnectionLifecycle::ProtocolHandshaking,
            role,
            started_at: now,
            lifecycle_changed_at: now,
            peer_id: None,
            supports_extensions: None,
            content: None,
            close_reason: None,
        })
    }

    pub(crate) fn transport_connected(
        &mut self,
        connection: ConnectionId,
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
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
        now: Duration,
    ) -> Result<(), PeerRuntimeError> {
        let peer = self.connection_mut(connection)?;
        if !matches!(
            peer.lifecycle,
            PeerConnectionLifecycle::TransportConnecting
                | PeerConnectionLifecycle::ProtocolHandshaking
        ) {
            return Err(PeerRuntimeError::InvalidTransition {
                connection,
                from: peer.lifecycle,
                to: PeerConnectionLifecycle::Connected,
            });
        }
        peer.lifecycle = PeerConnectionLifecycle::Connected;
        peer.lifecycle_changed_at = now;
        peer.peer_id = Some(handshake.peer_id);
        peer.supports_extensions = Some(handshake.supports_extensions());
        Ok(())
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
        Ok(self
            .connections
            .remove(&connection)
            .expect("connection exists after validation"))
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
}

pub(crate) fn connection_id(attempt: DialAttempt) -> ConnectionId {
    ConnectionId::new(attempt.id().get()).expect("dial attempt identifiers are nonzero")
}

#[cfg(test)]
mod tests {
    use super::{
        PeerConnectionDirection, PeerConnectionLifecycle, PeerConnectionRole, PeerRuntime,
        PeerRuntimeError, PeerTransport, connection_id,
    };
    use crate::peer::{
        PeerEndpoint, PeerObservation, PeerRegistry, PeerRegistryConfig, PeerSelectionContext,
        PeerSelector, PeerSource,
    };
    use crate::swarm::ConnectionId;
    use rstorrent_protocol::peer_wire::Handshake;
    use std::time::Duration;

    fn attempt() -> crate::peer::DialAttempt {
        let endpoint =
            PeerEndpoint::new("127.0.0.1:6881".parse().expect("address")).expect("endpoint");
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
        registry.begin_dial(candidate, context).expect("attempt")
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
            .transport_connected(connection, Duration::from_millis(5))
            .expect("transport");
        let handshake = Handshake {
            peer_id: *b"-LTTEST-000000000000",
            reserved: [0; 8],
        };
        runtime
            .handshake_completed(connection, &handshake, Duration::from_millis(8))
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
    fn incoming_generation_begins_at_protocol_handshake_without_identity() {
        let connection = ConnectionId::new(42).expect("connection");
        let mut runtime = PeerRuntime::default();
        runtime
            .begin_incoming(
                connection,
                "127.0.0.1:51413".parse().expect("endpoint"),
                PeerTransport::Utp,
                PeerConnectionRole::Metadata,
                Duration::from_secs(1),
            )
            .expect("incoming intake");
        let peer = runtime.observation(connection).expect("active row");
        assert_eq!(peer.direction, PeerConnectionDirection::Incoming);
        assert_eq!(peer.transport, PeerTransport::Utp);
        assert_eq!(peer.lifecycle, PeerConnectionLifecycle::ProtocolHandshaking);
        assert_eq!(peer.record_id, None);
        assert_eq!(peer.peer_id, None);
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
            runtime.transport_connected(unknown, Duration::ZERO),
            Err(PeerRuntimeError::UnknownConnection(actual)) if actual == unknown
        ));
    }
}

//! Task-free BEP 11 admission, provenance, cadence, and outbound diff state.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use rstorrent_protocol::extension::{
    ExtensionError, ExtensionHandshake, ExtensionMap, PexContact, PexEndpoint, PexFlags, PexIp,
    PexMessage, encode_pex_message, parse_pex_message,
};

use crate::network::NetworkPolicy;
use crate::peer::{PeerEndpoint, PeerObservation, PeerRegistry, PeerRegistryError, PeerSource};
use crate::swarm::ConnectionId;

pub(crate) const PEX_INTERVAL: Duration = Duration::from_secs(60);
pub(crate) const PEX_RATE_STRIKES_BEFORE_CLOSE: u8 = 3;
pub(crate) const MAX_PEX_CONTACTS_PER_SOURCE: usize = 50;
pub(crate) const MAX_PEX_CONTACTS_PER_TORRENT: usize = 200;
pub(crate) const MAX_PEX_TIMELINE_EVENTS: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PexSourceCadence {
    last_accepted: Option<Duration>,
    strikes: u8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PexSourceState {
    cadence: PexSourceCadence,
    contacts: BTreeMap<IpAddr, PeerEndpoint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PexReceiveDisposition {
    Applied {
        added: usize,
        dropped: usize,
        filtered: usize,
        truncated: usize,
    },
    PrivacyBlocked,
    RateLimited {
        strikes: u8,
        close: bool,
    },
}

pub(crate) struct PexReceiveContext<'a> {
    pub(crate) source_endpoint: SocketAddr,
    pub(crate) now: Duration,
    pub(crate) verified_public: bool,
    pub(crate) network_policy: NetworkPolicy,
    pub(crate) self_endpoints: &'a [SocketAddr],
}

#[derive(Debug)]
pub enum PexError {
    Extension(ExtensionError),
    Registry(PeerRegistryError),
}

impl fmt::Display for PexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Extension(error) => write!(formatter, "PEX wire message: {error}"),
            Self::Registry(error) => write!(formatter, "PEX peer registry: {error}"),
        }
    }
}

impl Error for PexError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Extension(error) => Some(error),
            Self::Registry(error) => Some(error),
        }
    }
}

impl From<ExtensionError> for PexError {
    fn from(error: ExtensionError) -> Self {
        Self::Extension(error)
    }
}

impl From<PeerRegistryError> for PexError {
    fn from(error: PeerRegistryError) -> Self {
        Self::Registry(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PexTimelineKind {
    Added(PexFlags),
    Dropped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PexTimelineEvent {
    sequence: u64,
    endpoint: PeerEndpoint,
    kind: PexTimelineKind,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PexCursor {
    next_sequence: u64,
    initial_sent: bool,
    last_sent: Option<Duration>,
}

#[derive(Debug)]
pub(crate) struct PexState {
    sources: BTreeMap<ConnectionId, PexSourceState>,
    endpoint_sources: BTreeMap<PeerEndpoint, BTreeSet<ConnectionId>>,
    live: BTreeMap<PeerEndpoint, PexFlags>,
    timeline: VecDeque<PexTimelineEvent>,
    next_sequence: u64,
    cursors: BTreeMap<ConnectionId, PexCursor>,
    extensions: BTreeMap<ConnectionId, ExtensionMap>,
}

impl Default for PexState {
    fn default() -> Self {
        Self {
            sources: BTreeMap::new(),
            endpoint_sources: BTreeMap::new(),
            live: BTreeMap::new(),
            timeline: VecDeque::new(),
            next_sequence: 1,
            cursors: BTreeMap::new(),
            extensions: BTreeMap::new(),
        }
    }
}

impl PexState {
    pub(crate) fn receive(
        &mut self,
        source: ConnectionId,
        payload: &[u8],
        context: PexReceiveContext<'_>,
        registry: &mut PeerRegistry,
    ) -> Result<PexReceiveDisposition, PexError> {
        let PexReceiveContext {
            source_endpoint,
            now,
            verified_public,
            network_policy,
            self_endpoints,
        } = context;
        if !verified_public {
            return Ok(PexReceiveDisposition::PrivacyBlocked);
        }
        {
            let source_state = self.sources.entry(source).or_default();
            if source_state
                .cadence
                .last_accepted
                .is_some_and(|last| now.saturating_sub(last) < PEX_INTERVAL)
            {
                source_state.cadence.strikes = source_state.cadence.strikes.saturating_add(1);
                return Ok(PexReceiveDisposition::RateLimited {
                    strikes: source_state.cadence.strikes,
                    close: source_state.cadence.strikes >= PEX_RATE_STRIKES_BEFORE_CLOSE,
                });
            }
        }
        let message = parse_pex_message(payload)?;
        {
            let cadence = &mut self.sources.entry(source).or_default().cadence;
            cadence.last_accepted = Some(now);
            cadence.strikes = 0;
        }
        let truncated = message
            .additions_truncated
            .saturating_add(message.drops_truncated);
        let mut filtered = 0;
        let mut dropped = 0;
        for wire_endpoint in message.dropped {
            let endpoint = socket_endpoint(wire_endpoint);
            let existing = self
                .sources
                .get(&source)
                .and_then(|state| state.contacts.get(&endpoint.ip()).copied());
            let Some(existing) = existing else {
                filtered += 1;
                continue;
            };
            if existing.address() != endpoint {
                filtered += 1;
                continue;
            }
            self.sources
                .get_mut(&source)
                .expect("cadence creates PEX source state")
                .contacts
                .remove(&endpoint.ip());
            self.remove_endpoint_source(existing, source, registry);
            dropped += 1;
        }

        let mut added = 0;
        for contact in message.added {
            let address = socket_endpoint(contact.endpoint);
            if !pex_address_allowed(address, source_endpoint, network_policy, self_endpoints)
                || self.sources.get(&source).is_some_and(|state| {
                    state.contacts.contains_key(&address.ip())
                        || state.contacts.len() >= MAX_PEX_CONTACTS_PER_SOURCE
                })
            {
                filtered += 1;
                continue;
            }
            let endpoint = PeerEndpoint::new(address)?;
            if !self.endpoint_sources.contains_key(&endpoint)
                && self.endpoint_sources.len() >= MAX_PEX_CONTACTS_PER_TORRENT
            {
                filtered += 1;
                continue;
            }
            registry.observe(
                PeerObservation::dialable(endpoint, PeerSource::PeerExchange),
                now,
            )?;
            self.sources
                .get_mut(&source)
                .expect("cadence creates PEX source state")
                .contacts
                .insert(address.ip(), endpoint);
            self.endpoint_sources
                .entry(endpoint)
                .or_default()
                .insert(source);
            added += 1;
        }
        Ok(PexReceiveDisposition::Applied {
            added,
            dropped,
            filtered,
            truncated,
        })
    }

    pub(crate) fn remove_source(
        &mut self,
        source: ConnectionId,
        registry: &mut PeerRegistry,
    ) -> usize {
        self.cursors.remove(&source);
        self.extensions.remove(&source);
        let Some(state) = self.sources.remove(&source) else {
            return 0;
        };
        let removed = state.contacts.len();
        for endpoint in state.contacts.into_values() {
            self.remove_endpoint_source(endpoint, source, registry);
        }
        removed
    }

    pub(crate) fn purge(&mut self, registry: &mut PeerRegistry) -> usize {
        let removed = registry.remove_source(PeerSource::PeerExchange);
        self.sources.clear();
        self.endpoint_sources.clear();
        self.cursors.clear();
        self.extensions.clear();
        self.live.clear();
        self.timeline.clear();
        removed
    }

    fn remove_endpoint_source(
        &mut self,
        endpoint: PeerEndpoint,
        source: ConnectionId,
        registry: &mut PeerRegistry,
    ) {
        let remove_last = self
            .endpoint_sources
            .get_mut(&endpoint)
            .is_some_and(|sources| {
                sources.remove(&source);
                sources.is_empty()
            });
        if remove_last {
            self.endpoint_sources.remove(&endpoint);
            registry.remove_endpoint_source(endpoint, PeerSource::PeerExchange);
        }
    }

    pub(crate) fn enable_outbound(&mut self, connection: ConnectionId) {
        self.cursors.entry(connection).or_insert(PexCursor {
            next_sequence: self.next_sequence,
            ..PexCursor::default()
        });
    }

    pub(crate) fn apply_extension_handshake(
        &mut self,
        connection: ConnectionId,
        handshake: ExtensionHandshake,
    ) -> ExtensionMap {
        let map = self.extensions.entry(connection).or_default();
        map.apply(handshake);
        let map = *map;
        if map.pex_id().is_some() {
            self.enable_outbound(connection);
        } else {
            self.disable_outbound(connection);
        }
        map
    }

    pub(crate) fn install_extension_map(&mut self, connection: ConnectionId, map: ExtensionMap) {
        self.extensions.insert(connection, map);
        if map.pex_id().is_some() {
            self.enable_outbound(connection);
        }
    }

    pub(crate) fn extension_map(&self, connection: ConnectionId) -> ExtensionMap {
        self.extensions
            .get(&connection)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn disable_outbound(&mut self, connection: ConnectionId) {
        self.cursors.remove(&connection);
    }

    pub(crate) fn peer_established(&mut self, endpoint: PeerEndpoint, flags: PexFlags) {
        if self.live.insert(endpoint, flags).is_none() {
            self.push_event(endpoint, PexTimelineKind::Added(flags));
        }
    }

    pub(crate) fn peer_dropped(&mut self, endpoint: PeerEndpoint) {
        if self.live.remove(&endpoint).is_some() {
            self.push_event(endpoint, PexTimelineKind::Dropped);
        }
    }

    pub(crate) fn next_outbound(
        &mut self,
        connection: ConnectionId,
        receiving_peer: PeerEndpoint,
        now: Duration,
    ) -> Result<Option<Vec<u8>>, PexError> {
        let Some(cursor) = self.cursors.get_mut(&connection) else {
            return Ok(None);
        };
        if cursor
            .last_sent
            .is_some_and(|last| now.saturating_sub(last) < PEX_INTERVAL)
        {
            return Ok(None);
        }
        let oldest = self
            .timeline
            .front()
            .map_or(self.next_sequence, |event| event.sequence);
        let lagged = cursor.next_sequence < oldest;
        let message = if !cursor.initial_sent || lagged {
            let added = self
                .live
                .iter()
                .filter(|(endpoint, _)| **endpoint != receiving_peer)
                .take(rstorrent_protocol::extension::MAX_PEX_ADDITIONS)
                .map(|(endpoint, flags)| PexContact {
                    endpoint: wire_endpoint(endpoint.address()),
                    flags: *flags,
                })
                .collect::<Vec<_>>();
            cursor.next_sequence = self.next_sequence;
            cursor.initial_sent = true;
            PexMessage {
                added,
                ..PexMessage::default()
            }
        } else {
            let mut net = BTreeMap::<PeerEndpoint, PexTimelineKind>::new();
            let mut next_sequence = cursor.next_sequence;
            for event in self
                .timeline
                .iter()
                .filter(|event| event.sequence >= cursor.next_sequence)
            {
                if event.endpoint == receiving_peer {
                    next_sequence = event.sequence.saturating_add(1);
                    continue;
                }
                match (net.get(&event.endpoint), event.kind) {
                    (Some(PexTimelineKind::Added(_)), PexTimelineKind::Dropped)
                    | (Some(PexTimelineKind::Dropped), PexTimelineKind::Added(_)) => {
                        net.remove(&event.endpoint);
                    }
                    _ => {
                        let additions = net
                            .values()
                            .filter(|kind| matches!(kind, PexTimelineKind::Added(_)))
                            .count();
                        let drops = net
                            .values()
                            .filter(|kind| matches!(kind, PexTimelineKind::Dropped))
                            .count();
                        if matches!(event.kind, PexTimelineKind::Added(_))
                            && additions == rstorrent_protocol::extension::MAX_PEX_ADDITIONS
                            || matches!(event.kind, PexTimelineKind::Dropped)
                                && drops == rstorrent_protocol::extension::MAX_PEX_DROPS
                        {
                            break;
                        }
                        net.insert(event.endpoint, event.kind);
                    }
                }
                next_sequence = event.sequence.saturating_add(1);
            }
            cursor.next_sequence = next_sequence;
            let mut message = PexMessage::default();
            for (endpoint, kind) in net {
                match kind {
                    PexTimelineKind::Added(flags)
                        if message.added.len()
                            < rstorrent_protocol::extension::MAX_PEX_ADDITIONS =>
                    {
                        message.added.push(PexContact {
                            endpoint: wire_endpoint(endpoint.address()),
                            flags,
                        });
                    }
                    PexTimelineKind::Dropped
                        if message.dropped.len() < rstorrent_protocol::extension::MAX_PEX_DROPS =>
                    {
                        message.dropped.push(wire_endpoint(endpoint.address()));
                    }
                    _ => {}
                }
            }
            message
        };
        if message.added.is_empty() && message.dropped.is_empty() {
            return Ok(None);
        }
        cursor.last_sent = Some(now);
        Ok(Some(encode_pex_message(&message)?))
    }

    fn push_event(&mut self, endpoint: PeerEndpoint, kind: PexTimelineKind) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.timeline.push_back(PexTimelineEvent {
            sequence,
            endpoint,
            kind,
        });
        if self.timeline.len() > MAX_PEX_TIMELINE_EVENTS {
            self.timeline.pop_front();
        }
    }

    #[cfg(test)]
    fn source_contacts(&self, source: ConnectionId) -> usize {
        self.sources
            .get(&source)
            .map_or(0, |state| state.contacts.len())
    }
}

fn pex_address_allowed(
    candidate: SocketAddr,
    source: SocketAddr,
    policy: NetworkPolicy,
    self_endpoints: &[SocketAddr],
) -> bool {
    if self_endpoints.contains(&candidate)
        || candidate.ip() == source.ip()
        || !policy.allows(candidate)
    {
        return false;
    }
    !is_local_address(candidate.ip()) || local_source_allows(source.ip(), candidate.ip())
}

fn local_source_allows(source: IpAddr, candidate: IpAddr) -> bool {
    if source.is_loopback() && candidate.is_loopback() {
        return true;
    }
    match (source, candidate) {
        (IpAddr::V4(source), IpAddr::V4(candidate)) => {
            is_local_v4(source) && is_local_v4(candidate)
        }
        (IpAddr::V6(source), IpAddr::V6(candidate)) => {
            is_local_v6(source) && is_local_v6(candidate)
        }
        _ => false,
    }
}

fn is_local_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_loopback() || is_local_v4(address),
        IpAddr::V6(address) => address.is_loopback() || is_local_v6(address),
    }
}

fn is_local_v4(address: Ipv4Addr) -> bool {
    address.is_private() || address.is_link_local()
}

fn is_local_v6(address: Ipv6Addr) -> bool {
    address.is_unique_local() || address.is_unicast_link_local()
}

fn socket_endpoint(endpoint: PexEndpoint) -> SocketAddr {
    match endpoint.ip {
        PexIp::V4(address) => SocketAddr::from((Ipv4Addr::from(address), endpoint.port)),
        PexIp::V6(address) => SocketAddr::from((Ipv6Addr::from(address), endpoint.port)),
    }
}

fn wire_endpoint(endpoint: SocketAddr) -> PexEndpoint {
    match endpoint {
        SocketAddr::V4(endpoint) => PexEndpoint::v4(endpoint.ip().octets(), endpoint.port()),
        SocketAddr::V6(endpoint) => PexEndpoint::v6(endpoint.ip().octets(), endpoint.port()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::{PeerRegistryConfig, PeerSources};

    fn connection(value: u64) -> ConnectionId {
        ConnectionId::new(value).expect("connection")
    }

    fn registry() -> PeerRegistry {
        PeerRegistry::new(PeerRegistryConfig::default()).expect("registry")
    }

    fn payload(added: &[SocketAddr], dropped: &[SocketAddr]) -> Vec<u8> {
        encode_pex_message(&PexMessage {
            added: added
                .iter()
                .map(|endpoint| PexContact {
                    endpoint: wire_endpoint(*endpoint),
                    flags: PexFlags::default(),
                })
                .collect(),
            dropped: dropped.iter().copied().map(wire_endpoint).collect(),
            ..PexMessage::default()
        })
        .expect("PEX payload")
    }

    #[test]
    fn inbound_enforces_privacy_cadence_source_and_drop_provenance() {
        let mut state = PexState::default();
        let mut registry = registry();
        let source = connection(1);
        let remote = "198.51.100.1:5000".parse().expect("remote");
        let peer = "203.0.113.9:6881".parse().expect("peer");
        assert_eq!(
            state
                .receive(
                    source,
                    &payload(&[peer], &[]),
                    PexReceiveContext {
                        source_endpoint: remote,
                        now: Duration::ZERO,
                        verified_public: false,
                        network_policy: NetworkPolicy::Online,
                        self_endpoints: &[],
                    },
                    &mut registry,
                )
                .expect("private block"),
            PexReceiveDisposition::PrivacyBlocked
        );
        assert!(matches!(
            state
                .receive(
                    source,
                    &payload(&[peer], &[]),
                    PexReceiveContext {
                        source_endpoint: remote,
                        now: Duration::ZERO,
                        verified_public: true,
                        network_policy: NetworkPolicy::Online,
                        self_endpoints: &[],
                    },
                    &mut registry,
                )
                .expect("first"),
            PexReceiveDisposition::Applied { added: 1, .. }
        ));
        for strike in 1..=3 {
            assert_eq!(
                state
                    .receive(
                        source,
                        &payload(&["203.0.113.10:6881".parse().expect("second")], &[]),
                        PexReceiveContext {
                            source_endpoint: remote,
                            now: Duration::from_secs(strike.into()),
                            verified_public: true,
                            network_policy: NetworkPolicy::Online,
                            self_endpoints: &[],
                        },
                        &mut registry,
                    )
                    .expect("rate limit"),
                PexReceiveDisposition::RateLimited {
                    strikes: strike,
                    close: strike == 3,
                }
            );
        }
        state
            .receive(
                source,
                &payload(&[], &[peer]),
                PexReceiveContext {
                    source_endpoint: remote,
                    now: PEX_INTERVAL,
                    verified_public: true,
                    network_policy: NetworkPolicy::Online,
                    self_endpoints: &[],
                },
                &mut registry,
            )
            .expect("drop");
        assert!(
            registry
                .find_endpoint(PeerEndpoint::new(peer).expect("endpoint"))
                .is_none()
        );
    }

    #[test]
    fn inbound_filters_alternate_ports_self_and_ineligible_local_contacts() {
        let mut state = PexState::default();
        let mut registry = registry();
        let contacts = [
            "203.0.113.9:6881".parse().expect("first"),
            "203.0.113.9:6882".parse().expect("alternate"),
            "127.0.0.2:6881".parse().expect("loopback"),
            "198.51.100.4:6881".parse().expect("self"),
        ];
        let disposition = state
            .receive(
                connection(1),
                &payload(&contacts, &[]),
                PexReceiveContext {
                    source_endpoint: "198.51.100.4:5000".parse().expect("source"),
                    now: Duration::ZERO,
                    verified_public: true,
                    network_policy: NetworkPolicy::Online,
                    self_endpoints: &[],
                },
                &mut registry,
            )
            .expect("receive");
        assert_eq!(
            disposition,
            PexReceiveDisposition::Applied {
                added: 1,
                dropped: 0,
                filtered: 2,
                truncated: 0,
            }
        );
        assert_eq!(state.source_contacts(connection(1)), 1);
    }

    #[test]
    fn independent_registry_source_survives_pex_disconnect_and_private_purge() {
        let mut state = PexState::default();
        let mut registry = registry();
        let peer = "203.0.113.9:6881".parse().expect("peer");
        let endpoint = PeerEndpoint::new(peer).expect("endpoint");
        registry
            .observe(
                PeerObservation::dialable(endpoint, PeerSource::Tracker),
                Duration::ZERO,
            )
            .expect("tracker");
        state
            .receive(
                connection(1),
                &payload(&[peer], &[]),
                PexReceiveContext {
                    source_endpoint: "198.51.100.4:5000".parse().expect("source"),
                    now: Duration::ZERO,
                    verified_public: true,
                    network_policy: NetworkPolicy::Online,
                    self_endpoints: &[],
                },
                &mut registry,
            )
            .expect("PEX");
        state.remove_source(connection(1), &mut registry);
        let record = registry
            .find_endpoint(endpoint)
            .expect("tracker record remains");
        assert_eq!(
            record.sources(),
            PeerSources::from_source(PeerSource::Tracker)
        );
        assert_eq!(state.purge(&mut registry), 0);
    }

    #[test]
    fn source_and_torrent_contact_high_waters_are_exact() {
        let mut state = PexState::default();
        let mut registry = registry();
        for source_index in 0..5_u64 {
            let contacts = (0..60_u8)
                .map(|index| {
                    SocketAddr::from((Ipv4Addr::new(20 + source_index as u8, 1, index, 1), 6881))
                })
                .collect::<Vec<_>>();
            let remote = SocketAddr::from((Ipv4Addr::new(100, 64, source_index as u8, 1), 5000));
            for (batch, now) in [
                (&contacts[..50], Duration::ZERO),
                (&contacts[50..], PEX_INTERVAL),
            ] {
                state
                    .receive(
                        connection(source_index + 1),
                        &payload(batch, &[]),
                        PexReceiveContext {
                            source_endpoint: remote,
                            now,
                            verified_public: true,
                            network_policy: NetworkPolicy::Online,
                            self_endpoints: &[],
                        },
                        &mut registry,
                    )
                    .expect("bounded source");
            }
            assert!(state.source_contacts(connection(source_index + 1)) <= 50);
        }
        assert_eq!(state.endpoint_sources.len(), MAX_PEX_CONTACTS_PER_TORRENT);
        assert_eq!(registry.len(), MAX_PEX_CONTACTS_PER_TORRENT);
    }

    #[test]
    fn outbound_snapshot_diff_cadence_transient_elision_and_cursor_reset_are_bounded() {
        let mut state = PexState::default();
        let receiver = PeerEndpoint::new("198.51.100.1:6881".parse().expect("receiver"))
            .expect("receiver endpoint");
        let first =
            PeerEndpoint::new("203.0.113.1:6881".parse().expect("first")).expect("first endpoint");
        state.peer_established(first, PexFlags::from_bits(PexFlags::OUTGOING));
        state.enable_outbound(connection(1));
        let initial = state
            .next_outbound(connection(1), receiver, Duration::ZERO)
            .expect("initial")
            .expect("snapshot");
        assert_eq!(parse_pex_message(&initial).expect("parse").added.len(), 1);
        state.peer_dropped(first);
        let transient = PeerEndpoint::new("203.0.113.2:6881".parse().expect("transient"))
            .expect("transient endpoint");
        state.peer_established(transient, PexFlags::default());
        state.peer_dropped(transient);
        assert!(
            state
                .next_outbound(connection(1), receiver, Duration::from_secs(59))
                .expect("cadence")
                .is_none()
        );
        let diff = state
            .next_outbound(connection(1), receiver, PEX_INTERVAL)
            .expect("diff")
            .expect("drop");
        let diff = parse_pex_message(&diff).expect("parse diff");
        assert_eq!(diff.dropped, vec![wire_endpoint(first.address())]);
        assert!(diff.added.is_empty());

        for index in 0..60_u8 {
            let endpoint =
                PeerEndpoint::new(SocketAddr::from((Ipv4Addr::new(40, 0, index, 1), 6881)))
                    .expect("batched endpoint");
            state.peer_established(endpoint, PexFlags::default());
        }
        let first_batch = state
            .next_outbound(connection(1), receiver, PEX_INTERVAL * 2)
            .expect("first batch")
            .expect("first payload");
        assert_eq!(
            parse_pex_message(&first_batch)
                .expect("first parse")
                .added
                .len(),
            50
        );
        let second_batch = state
            .next_outbound(connection(1), receiver, PEX_INTERVAL * 3)
            .expect("second batch")
            .expect("second payload");
        assert_eq!(
            parse_pex_message(&second_batch)
                .expect("second parse")
                .added
                .len(),
            10
        );

        for index in 0..=MAX_PEX_TIMELINE_EVENTS {
            let endpoint = PeerEndpoint::new(SocketAddr::from((
                Ipv4Addr::new(10, (index / 255) as u8, index as u8, 1),
                6881,
            )))
            .expect("timeline endpoint");
            state.peer_established(endpoint, PexFlags::default());
        }
        assert_eq!(state.timeline.len(), MAX_PEX_TIMELINE_EVENTS);
    }
}

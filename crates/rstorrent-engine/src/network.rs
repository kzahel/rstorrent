use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";

const NETWORK_PREREQUISITE_ALLOWED: u64 = 1;
const NETWORK_PREREQUISITE_MAX_GENERATION: u64 = u64::MAX >> 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ApplicationNetworkPrerequisite {
    #[default]
    Allowed,
    WaitingForUnmeteredNetwork,
}

impl ApplicationNetworkPrerequisite {
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::WaitingForUnmeteredNetwork => "waiting_for_unmetered_network",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkPrerequisiteSnapshot {
    pub generation: u64,
    pub prerequisite: ApplicationNetworkPrerequisite,
}

impl NetworkPrerequisiteSnapshot {
    #[must_use]
    pub const fn is_allowed(self) -> bool {
        self.prerequisite.is_allowed()
    }
}

#[derive(Clone, Debug)]
pub struct NetworkPrerequisiteHandle {
    state: Arc<AtomicU64>,
    generation: Arc<Mutex<NetworkPrerequisiteGeneration>>,
    updates: watch::Sender<NetworkPrerequisiteSnapshot>,
}

#[derive(Debug)]
struct NetworkPrerequisiteGeneration {
    snapshot: NetworkPrerequisiteSnapshot,
    cancellation: CancellationToken,
}

impl NetworkPrerequisiteHandle {
    #[must_use]
    pub fn new(initial: ApplicationNetworkPrerequisite) -> Self {
        let snapshot = NetworkPrerequisiteSnapshot {
            generation: 1,
            prerequisite: initial,
        };
        let (updates, _) = watch::channel(snapshot);
        let cancellation = CancellationToken::new();
        if !snapshot.is_allowed() {
            cancellation.cancel();
        }
        Self {
            state: Arc::new(AtomicU64::new(encode_network_prerequisite(snapshot))),
            generation: Arc::new(Mutex::new(NetworkPrerequisiteGeneration {
                snapshot,
                cancellation,
            })),
            updates,
        }
    }

    #[must_use]
    pub fn load(&self) -> NetworkPrerequisiteSnapshot {
        decode_network_prerequisite(self.state.load(Ordering::Acquire))
    }

    pub fn replace(
        &self,
        prerequisite: ApplicationNetworkPrerequisite,
    ) -> Result<NetworkPrerequisiteSnapshot, NetworkPrerequisiteError> {
        let mut active = self
            .generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let current = decode_network_prerequisite(self.state.load(Ordering::Acquire));
        debug_assert_eq!(active.snapshot, current);
        if current.prerequisite == prerequisite {
            return Ok(current);
        }
        let generation = current
            .generation
            .checked_add(1)
            .filter(|generation| *generation <= NETWORK_PREREQUISITE_MAX_GENERATION)
            .ok_or(NetworkPrerequisiteError::GenerationExhausted)?;
        let next = NetworkPrerequisiteSnapshot {
            generation,
            prerequisite,
        };
        let cancellation = CancellationToken::new();
        if next.is_allowed() {
            *active = NetworkPrerequisiteGeneration {
                snapshot: next,
                cancellation,
            };
            self.state
                .store(encode_network_prerequisite(next), Ordering::Release);
        } else {
            self.state
                .store(encode_network_prerequisite(next), Ordering::Release);
            active.cancellation.cancel();
            cancellation.cancel();
            *active = NetworkPrerequisiteGeneration {
                snapshot: next,
                cancellation,
            };
        }
        self.updates.send_replace(next);
        Ok(next)
    }

    pub fn close(&self) -> Result<NetworkPrerequisiteSnapshot, NetworkPrerequisiteError> {
        self.replace(ApplicationNetworkPrerequisite::WaitingForUnmeteredNetwork)
    }

    pub fn allow(&self) -> Result<NetworkPrerequisiteSnapshot, NetworkPrerequisiteError> {
        self.replace(ApplicationNetworkPrerequisite::Allowed)
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<NetworkPrerequisiteSnapshot> {
        self.updates.subscribe()
    }

    /// Returns a child cancellation domain only when `snapshot` is the live
    /// allowed generation. Stale, blocked, and superseded generations receive
    /// an already-cancelled token.
    #[must_use]
    pub fn cancellation_token(&self, snapshot: NetworkPrerequisiteSnapshot) -> CancellationToken {
        let active = self
            .generation
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.snapshot == snapshot && snapshot.is_allowed() {
            active.cancellation.child_token()
        } else {
            let cancellation = CancellationToken::new();
            cancellation.cancel();
            cancellation
        }
    }

    #[cfg(test)]
    fn with_snapshot_for_test(snapshot: NetworkPrerequisiteSnapshot) -> Self {
        let (updates, _) = watch::channel(snapshot);
        let cancellation = CancellationToken::new();
        if !snapshot.is_allowed() {
            cancellation.cancel();
        }
        Self {
            state: Arc::new(AtomicU64::new(encode_network_prerequisite(snapshot))),
            generation: Arc::new(Mutex::new(NetworkPrerequisiteGeneration {
                snapshot,
                cancellation,
            })),
            updates,
        }
    }
}

impl Default for NetworkPrerequisiteHandle {
    fn default() -> Self {
        Self::new(ApplicationNetworkPrerequisite::Allowed)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPrerequisiteError {
    GenerationExhausted,
}

impl fmt::Display for NetworkPrerequisiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GenerationExhausted => {
                formatter.write_str("network prerequisite generation exhausted")
            }
        }
    }
}

impl std::error::Error for NetworkPrerequisiteError {}

const fn encode_network_prerequisite(snapshot: NetworkPrerequisiteSnapshot) -> u64 {
    (snapshot.generation << 1) | (snapshot.prerequisite.is_allowed() as u64)
}

const fn decode_network_prerequisite(encoded: u64) -> NetworkPrerequisiteSnapshot {
    NetworkPrerequisiteSnapshot {
        generation: encoded >> 1,
        prerequisite: if encoded & NETWORK_PREREQUISITE_ALLOWED != 0 {
            ApplicationNetworkPrerequisite::Allowed
        } else {
            ApplicationNetworkPrerequisite::WaitingForUnmeteredNetwork
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AddressFamily {
    Ipv4,
    Ipv6,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PeerTransportPolicy {
    #[default]
    TcpOnly,
    PreferUtp,
}

impl AddressFamily {
    #[must_use]
    pub const fn of(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(_) => Self::Ipv4,
            IpAddr::V6(_) => Self::Ipv6,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4 => "IPv4",
            Self::Ipv6 => "IPv6",
        }
    }
}

impl fmt::Display for AddressFamily {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AddressFamilyPolicy {
    ipv6_enabled: bool,
}

impl AddressFamilyPolicy {
    #[must_use]
    pub const fn ipv4_only() -> Self {
        Self {
            ipv6_enabled: false,
        }
    }

    #[must_use]
    pub const fn dual_stack() -> Self {
        Self { ipv6_enabled: true }
    }

    #[must_use]
    pub const fn ipv6_enabled(self) -> bool {
        self.ipv6_enabled
    }

    #[must_use]
    pub const fn permits(self, address: IpAddr) -> bool {
        matches!(address, IpAddr::V4(_)) || self.ipv6_enabled
    }
}

impl Default for AddressFamilyPolicy {
    fn default() -> Self {
        Self::dual_stack()
    }
}

#[derive(Clone, Debug)]
pub struct AddressFamilyPolicyHandle {
    ipv6_enabled: Arc<AtomicBool>,
}

impl AddressFamilyPolicyHandle {
    #[must_use]
    pub fn new(policy: AddressFamilyPolicy) -> Self {
        Self {
            ipv6_enabled: Arc::new(AtomicBool::new(policy.ipv6_enabled())),
        }
    }

    #[must_use]
    pub fn load(&self) -> AddressFamilyPolicy {
        if self.ipv6_enabled.load(Ordering::Acquire) {
            AddressFamilyPolicy::dual_stack()
        } else {
            AddressFamilyPolicy::ipv4_only()
        }
    }

    pub fn replace(&self, policy: AddressFamilyPolicy) -> AddressFamilyPolicy {
        if self
            .ipv6_enabled
            .swap(policy.ipv6_enabled(), Ordering::AcqRel)
        {
            AddressFamilyPolicy::dual_stack()
        } else {
            AddressFamilyPolicy::ipv4_only()
        }
    }
}

impl Default for AddressFamilyPolicyHandle {
    fn default() -> Self {
        Self::new(AddressFamilyPolicy::default())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum PeerEncryptionPolicy {
    Disabled,
    #[default]
    Allow,
    Prefer,
    Required,
}

impl PeerEncryptionPolicy {
    #[must_use]
    pub const fn accepts_incoming_mse(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    #[must_use]
    pub const fn prefers_rc4_when_selecting(self) -> bool {
        matches!(self, Self::Prefer | Self::Required)
    }
}

#[derive(Clone, Debug)]
pub struct PeerEncryptionPolicyHandle {
    value: Arc<AtomicU8>,
}

impl PeerEncryptionPolicyHandle {
    #[must_use]
    pub fn new(policy: PeerEncryptionPolicy) -> Self {
        Self {
            value: Arc::new(AtomicU8::new(policy as u8)),
        }
    }

    #[must_use]
    pub fn load(&self) -> PeerEncryptionPolicy {
        match self.value.load(Ordering::Acquire) {
            0 => PeerEncryptionPolicy::Disabled,
            1 => PeerEncryptionPolicy::Allow,
            2 => PeerEncryptionPolicy::Prefer,
            3 => PeerEncryptionPolicy::Required,
            _ => unreachable!("encryption policy handle stores only closed enum values"),
        }
    }

    pub fn replace(&self, policy: PeerEncryptionPolicy) -> PeerEncryptionPolicy {
        match self.value.swap(policy as u8, Ordering::AcqRel) {
            0 => PeerEncryptionPolicy::Disabled,
            1 => PeerEncryptionPolicy::Allow,
            2 => PeerEncryptionPolicy::Prefer,
            3 => PeerEncryptionPolicy::Required,
            _ => unreachable!("encryption policy handle stores only closed enum values"),
        }
    }
}

impl Default for PeerEncryptionPolicyHandle {
    fn default() -> Self {
        Self::new(PeerEncryptionPolicy::default())
    }
}

/// Shared live session policy for BEP 11 participation.
///
/// `NetworkConfig::peer_exchange` remains the immutable capability/profile
/// gate. This handle is the reversible product setting sampled by current and
/// future torrent owners.
#[derive(Clone, Debug)]
pub struct PeerExchangePolicyHandle {
    enabled: Arc<AtomicBool>,
    updates: watch::Sender<bool>,
}

impl PeerExchangePolicyHandle {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        let (updates, _) = watch::channel(enabled);
        Self {
            enabled: Arc::new(AtomicBool::new(enabled)),
            updates,
        }
    }

    #[must_use]
    pub fn load(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn replace(&self, enabled: bool) -> bool {
        let previous = self.enabled.swap(enabled, Ordering::AcqRel);
        if previous != enabled {
            self.updates.send_replace(enabled);
        }
        previous
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.updates.subscribe()
    }
}

impl Default for PeerExchangePolicyHandle {
    fn default() -> Self {
        Self::new(true)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPolicy {
    Offline,
    LoopbackOnly,
    Online,
}

impl NetworkPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::LoopbackOnly => "loopback_only",
            Self::Online => "online",
        }
    }

    pub fn allows(self, address: SocketAddr) -> bool {
        if !is_valid_outbound_address(address) {
            return false;
        }
        match self {
            Self::Offline => false,
            Self::LoopbackOnly => address.ip().is_loopback(),
            Self::Online => true,
        }
    }

    pub const fn permits_dns(self) -> bool {
        !matches!(self, Self::Offline)
    }
}

impl fmt::Display for NetworkPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NetworkConfig {
    pub policy: NetworkPolicy,
    pub address_families: AddressFamilyPolicy,
    /// Whether public torrents advertise, accept, and emit BEP 11 peer exchange.
    pub peer_exchange: bool,
    /// Restrict initiated MSE handshakes to RC4 payload encryption.
    pub mse_rc4_only: bool,
    /// Total lifetime of one initiated transport and BitTorrent handshake.
    pub peer_connect_timeout: Duration,
    /// Maximum time spent preferring uTP before falling back to TCP.
    pub utp_fallback_timeout: Duration,
    /// Maximum outgoing BitTorrent/MSE handshake time within the attempt.
    pub outgoing_handshake_timeout: Duration,
    /// Timeout for established peer reads and writes.
    pub peer_io_timeout: Duration,
    pub peer_id: [u8; 20],
    pub encryption: PeerEncryptionPolicy,
}

impl NetworkConfig {
    pub const fn new(
        policy: NetworkPolicy,
        peer_connect_timeout: Duration,
        peer_io_timeout: Duration,
    ) -> Self {
        Self {
            policy,
            address_families: AddressFamilyPolicy::dual_stack(),
            peer_exchange: true,
            mse_rc4_only: false,
            peer_connect_timeout,
            utp_fallback_timeout: Duration::from_secs(3),
            outgoing_handshake_timeout: Duration::from_secs(10),
            peer_io_timeout,
            peer_id: DEFAULT_PEER_ID,
            encryption: PeerEncryptionPolicy::Allow,
        }
    }

    pub const fn with_peer_id(mut self, peer_id: [u8; 20]) -> Self {
        self.peer_id = peer_id;
        self
    }

    pub const fn with_address_families(mut self, address_families: AddressFamilyPolicy) -> Self {
        self.address_families = address_families;
        self
    }

    pub const fn with_peer_exchange(mut self, peer_exchange: bool) -> Self {
        self.peer_exchange = peer_exchange;
        self
    }

    pub const fn with_mse_rc4_only(mut self, mse_rc4_only: bool) -> Self {
        self.mse_rc4_only = mse_rc4_only;
        self
    }

    pub const fn with_utp_fallback_timeout(mut self, timeout: Duration) -> Self {
        self.utp_fallback_timeout = timeout;
        self
    }

    pub const fn with_outgoing_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.outgoing_handshake_timeout = timeout;
        self
    }

    pub const fn with_encryption(mut self, encryption: PeerEncryptionPolicy) -> Self {
        self.encryption = encryption;
        self
    }
}

pub(crate) fn is_valid_outbound_address(address: SocketAddr) -> bool {
    let invalid_ip = address.ip().is_unspecified()
        || address.ip().is_multicast()
        || address.ip() == IpAddr::V4(Ipv4Addr::BROADCAST);
    let unscoped_link_local = match address {
        SocketAddr::V6(address) => address.ip().is_unicast_link_local() && address.scope_id() == 0,
        SocketAddr::V4(_) => false,
    };
    address.port() != 0 && !invalid_ip && !unscoped_link_local
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

    use super::{
        AddressFamily, AddressFamilyPolicy, ApplicationNetworkPrerequisite, NetworkConfig,
        NetworkPolicy, NetworkPrerequisiteError, NetworkPrerequisiteHandle,
        NetworkPrerequisiteSnapshot, PeerEncryptionPolicy, PeerEncryptionPolicyHandle,
        PeerExchangePolicyHandle, is_valid_outbound_address,
    };

    #[tokio::test]
    async fn peer_exchange_policy_is_latest_value_and_idempotent() {
        let handle = PeerExchangePolicyHandle::default();
        let mut updates = handle.subscribe();
        assert!(handle.load());
        assert!(handle.replace(false));
        updates.changed().await.expect("disable update");
        assert!(!*updates.borrow_and_update());
        assert!(!handle.load());
        assert!(!handle.replace(false));
        assert!(!updates.has_changed().expect("sender remains available"));
        assert!(!handle.replace(true));
        updates.changed().await.expect("enable update");
        assert!(*updates.borrow_and_update());
    }

    #[tokio::test]
    async fn network_prerequisite_is_nonzero_ordered_and_latest_value() {
        let handle = NetworkPrerequisiteHandle::default();
        let mut updates = handle.subscribe();
        assert_eq!(handle.load().generation, 1);
        assert!(handle.load().is_allowed());
        let first = handle.cancellation_token(handle.load());
        assert!(!first.is_cancelled());

        let closed = handle.close().expect("close available generation");
        assert_eq!(closed.generation, 2);
        assert!(!closed.is_allowed());
        assert!(first.is_cancelled());
        assert!(handle.cancellation_token(closed).is_cancelled());
        updates.changed().await.expect("close update");
        assert_eq!(*updates.borrow_and_update(), closed);

        assert_eq!(handle.close().expect("duplicate close"), closed);
        assert!(!updates.has_changed().expect("sender remains available"));

        let allowed = handle.allow().expect("allow newer generation");
        assert_eq!(allowed.generation, 3);
        assert!(allowed.is_allowed());
        assert_eq!(handle.load(), allowed);
        let third = handle.cancellation_token(allowed);
        assert!(!third.is_cancelled());
        assert!(handle.cancellation_token(closed).is_cancelled());
        handle.close().expect("close third generation");
        assert!(third.is_cancelled());
    }

    #[test]
    fn prerequisite_values_are_closed_and_stable() {
        assert_eq!(ApplicationNetworkPrerequisite::Allowed.as_str(), "allowed");
        assert_eq!(
            ApplicationNetworkPrerequisite::WaitingForUnmeteredNetwork.as_str(),
            "waiting_for_unmetered_network"
        );
    }

    #[test]
    fn prerequisite_generation_refuses_overflow_without_changing_state() {
        let maximum = NetworkPrerequisiteSnapshot {
            generation: super::NETWORK_PREREQUISITE_MAX_GENERATION,
            prerequisite: ApplicationNetworkPrerequisite::Allowed,
        };
        let handle = NetworkPrerequisiteHandle::with_snapshot_for_test(maximum);
        assert_eq!(
            handle.close(),
            Err(NetworkPrerequisiteError::GenerationExhausted)
        );
        assert_eq!(handle.load(), maximum);
    }

    #[test]
    fn comparison_toggles_are_explicit_and_default_on_product_behavior() {
        let defaults = NetworkConfig::new(
            NetworkPolicy::Online,
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(1),
        );
        assert!(defaults.peer_exchange);
        assert!(!defaults.mse_rc4_only);
        assert_eq!(
            defaults.utp_fallback_timeout,
            std::time::Duration::from_secs(3)
        );
        assert_eq!(
            defaults.outgoing_handshake_timeout,
            std::time::Duration::from_secs(10)
        );
        let matched = defaults
            .with_peer_exchange(false)
            .with_mse_rc4_only(true)
            .with_utp_fallback_timeout(std::time::Duration::from_millis(250))
            .with_outgoing_handshake_timeout(std::time::Duration::from_millis(500));
        assert!(!matched.peer_exchange);
        assert!(matched.mse_rc4_only);
        assert_eq!(
            matched.utp_fallback_timeout,
            std::time::Duration::from_millis(250)
        );
        assert_eq!(
            matched.outgoing_handshake_timeout,
            std::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn address_family_policy_always_retains_ipv4() {
        let ipv4 = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let ipv6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
        assert_eq!(AddressFamily::of(ipv4), AddressFamily::Ipv4);
        assert_eq!(AddressFamily::of(ipv6), AddressFamily::Ipv6);
        assert!(AddressFamilyPolicy::ipv4_only().permits(ipv4));
        assert!(!AddressFamilyPolicy::ipv4_only().permits(ipv6));
        assert!(AddressFamilyPolicy::dual_stack().permits(ipv4));
        assert!(AddressFamilyPolicy::dual_stack().permits(ipv6));
    }

    #[test]
    fn encryption_policy_handle_replaces_only_future_samples() {
        let policy = PeerEncryptionPolicyHandle::new(PeerEncryptionPolicy::Allow);
        let captured = policy.load();
        assert_eq!(
            policy.replace(PeerEncryptionPolicy::Required),
            PeerEncryptionPolicy::Allow
        );
        assert_eq!(captured, PeerEncryptionPolicy::Allow);
        assert_eq!(policy.load(), PeerEncryptionPolicy::Required);
    }

    #[test]
    fn encryption_policy_separates_compatibility_from_method_preference() {
        assert!(!PeerEncryptionPolicy::Disabled.accepts_incoming_mse());
        assert!(PeerEncryptionPolicy::Allow.accepts_incoming_mse());
        assert!(PeerEncryptionPolicy::Prefer.accepts_incoming_mse());
        assert!(PeerEncryptionPolicy::Required.accepts_incoming_mse());

        assert!(!PeerEncryptionPolicy::Disabled.prefers_rc4_when_selecting());
        assert!(!PeerEncryptionPolicy::Allow.prefers_rc4_when_selecting());
        assert!(PeerEncryptionPolicy::Prefer.prefers_rc4_when_selecting());
        assert!(PeerEncryptionPolicy::Required.prefers_rc4_when_selecting());
    }

    #[test]
    fn policies_cover_valid_loopback_private_and_public_destinations() {
        let loopback_v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
        let loopback_v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 1));
        let private = SocketAddr::from((Ipv4Addr::new(192, 168, 1, 1), 1));
        let public = SocketAddr::from((Ipv4Addr::new(192, 0, 2, 1), 1));
        let scoped_link_local = SocketAddr::V6(SocketAddrV6::new(
            "fe80::1".parse().expect("link-local IPv6"),
            1,
            0,
            2,
        ));

        for address in [loopback_v4, loopback_v6] {
            assert!(NetworkPolicy::LoopbackOnly.allows(address));
            assert!(NetworkPolicy::Online.allows(address));
            assert!(!NetworkPolicy::Offline.allows(address));
        }
        for address in [private, public, scoped_link_local] {
            assert!(!NetworkPolicy::LoopbackOnly.allows(address));
            assert!(NetworkPolicy::Online.allows(address));
            assert!(!NetworkPolicy::Offline.allows(address));
        }
    }

    #[test]
    fn every_policy_rejects_invalid_socket_destinations() {
        let invalid = [
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 1)),
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 1)),
            SocketAddr::from((Ipv4Addr::new(224, 0, 0, 1), 1)),
            SocketAddr::from(("ff02::1".parse::<Ipv6Addr>().expect("multicast IPv6"), 1)),
            SocketAddr::from((Ipv4Addr::BROADCAST, 1)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, 0)),
            SocketAddr::from(("fe80::1".parse::<Ipv6Addr>().expect("link-local IPv6"), 1)),
        ];

        for address in invalid {
            assert!(!is_valid_outbound_address(address), "{address}");
            assert!(!NetworkPolicy::Offline.allows(address), "{address}");
            assert!(!NetworkPolicy::LoopbackOnly.allows(address), "{address}");
            assert!(!NetworkPolicy::Online.allows(address), "{address}");
        }
    }
}

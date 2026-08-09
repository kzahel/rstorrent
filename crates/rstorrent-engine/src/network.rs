use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

pub const DEFAULT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum AddressFamily {
    Ipv4,
    Ipv6,
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
    pub peer_connect_timeout: Duration,
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
            peer_connect_timeout,
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
        AddressFamily, AddressFamilyPolicy, NetworkPolicy, PeerEncryptionPolicy,
        PeerEncryptionPolicyHandle, is_valid_outbound_address,
    };

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

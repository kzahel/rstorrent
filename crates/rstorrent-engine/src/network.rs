use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

pub const DEFAULT_PEER_ID: [u8; 20] = *b"-RS0001-000000000000";

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
    pub peer_connect_timeout: Duration,
    pub peer_io_timeout: Duration,
    pub peer_id: [u8; 20],
}

impl NetworkConfig {
    pub const fn new(
        policy: NetworkPolicy,
        peer_connect_timeout: Duration,
        peer_io_timeout: Duration,
    ) -> Self {
        Self {
            policy,
            peer_connect_timeout,
            peer_io_timeout,
            peer_id: DEFAULT_PEER_ID,
        }
    }

    pub const fn with_peer_id(mut self, peer_id: [u8; 20]) -> Self {
        self.peer_id = peer_id;
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
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};

    use super::{NetworkPolicy, is_valid_outbound_address};

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

//! Coordinated application-generation TCP and UDP socket allocation.

use std::error::Error;
use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use tokio::net::{TcpListener, TcpSocket, UdpSocket};

use crate::incoming::{IncomingPeerError, IncomingTcpBootstrap, select_local_network_ipv4};
use crate::network::{AddressFamily, AddressFamilyPolicy};
use crate::peer_budget::DEFAULT_LISTEN_BACKLOG;

pub const MAX_LISTEN_PORT_RETRIES: u8 = 10;
pub const MIN_PREFERRED_LISTEN_PORT: u16 = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionSocketTransport {
    Tcp,
    Udp,
}

impl fmt::Display for SessionSocketTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        })
    }
}

#[derive(Debug)]
pub enum SessionSocketError {
    InvalidPreferredPort(u16),
    InvalidFixedPort(u16),
    InvalidUdpFallbackAddress,
    GlobalIpv6Address(io::Error),
    IneligibleGlobalIpv6Address(Ipv6Addr),
    LocalNetworkAddress(IncomingPeerError),
    Bind {
        family: AddressFamily,
        transport: SessionSocketTransport,
        port: u16,
        source: io::Error,
    },
    LocalAddress {
        transport: SessionSocketTransport,
        source: io::Error,
    },
}

impl SessionSocketError {
    pub fn io_error(&self) -> Option<&io::Error> {
        match self {
            Self::GlobalIpv6Address(source)
            | Self::Bind { source, .. }
            | Self::LocalAddress { source, .. } => Some(source),
            Self::InvalidPreferredPort(_)
            | Self::InvalidFixedPort(_)
            | Self::InvalidUdpFallbackAddress
            | Self::IneligibleGlobalIpv6Address(_)
            | Self::LocalNetworkAddress(_) => None,
        }
    }
}

impl fmt::Display for SessionSocketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPreferredPort(port) => write!(
                formatter,
                "preferred listen port {port} is outside {MIN_PREFERRED_LISTEN_PORT}..=65535"
            ),
            Self::InvalidFixedPort(port) => write!(
                formatter,
                "fixed listen port {port} is outside {MIN_PREFERRED_LISTEN_PORT}..=65535"
            ),
            Self::InvalidUdpFallbackAddress => {
                formatter.write_str("UDP fallback address must be IPv4, non-multicast, and port 0")
            }
            Self::GlobalIpv6Address(error) => {
                write!(formatter, "select global-unicast IPv6 address: {error}")
            }
            Self::IneligibleGlobalIpv6Address(address) => {
                write!(
                    formatter,
                    "IPv6 address {address} is not eligible global unicast"
                )
            }
            Self::LocalNetworkAddress(error) => {
                write!(formatter, "select local-network listen address: {error}")
            }
            Self::Bind {
                family,
                transport,
                port,
                source,
            } => write!(
                formatter,
                "bind session {family} {transport} port {port}: {source}"
            ),
            Self::LocalAddress { transport, source } => {
                write!(
                    formatter,
                    "read session {transport} local address: {source}"
                )
            }
        }
    }
}

impl Error for SessionSocketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::LocalNetworkAddress(error) => Some(error),
            Self::GlobalIpv6Address(error) => Some(error),
            Self::Bind { source, .. } | Self::LocalAddress { source, .. } => Some(source),
            Self::InvalidPreferredPort(_)
            | Self::InvalidFixedPort(_)
            | Self::InvalidUdpFallbackAddress => None,
            Self::IneligibleGlobalIpv6Address(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SessionSocketConfig {
    pub tcp: IncomingTcpBootstrap,
    pub preferred_port: u16,
    pub udp_fallback_address: SocketAddr,
    address_families: AddressFamilyPolicy,
    local_network_address_override: Option<Ipv4Addr>,
    global_ipv6_address_override: Option<Ipv6Addr>,
}

impl SessionSocketConfig {
    pub fn new(
        tcp: IncomingTcpBootstrap,
        preferred_port: u16,
        udp_fallback_address: SocketAddr,
    ) -> Self {
        Self {
            tcp,
            preferred_port,
            udp_fallback_address,
            address_families: AddressFamilyPolicy::ipv4_only(),
            local_network_address_override: None,
            global_ipv6_address_override: None,
        }
    }

    #[must_use]
    pub const fn with_address_families(mut self, address_families: AddressFamilyPolicy) -> Self {
        self.address_families = address_families;
        self
    }

    #[doc(hidden)]
    pub fn with_local_network_address_for_testing(mut self, address: Ipv4Addr) -> Self {
        self.local_network_address_override = Some(address);
        self
    }

    #[doc(hidden)]
    pub fn with_global_ipv6_address_for_testing(mut self, address: Ipv6Addr) -> Self {
        self.global_ipv6_address_override = Some(address);
        self
    }

    fn validate(&self) -> Result<(), SessionSocketError> {
        if self.preferred_port < MIN_PREFERRED_LISTEN_PORT {
            return Err(SessionSocketError::InvalidPreferredPort(
                self.preferred_port,
            ));
        }
        if !self.udp_fallback_address.is_ipv4()
            || self.udp_fallback_address.port() != 0
            || self.udp_fallback_address.ip().is_multicast()
        {
            return Err(SessionSocketError::InvalidUdpFallbackAddress);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SessionSocketSet {
    ipv4: SessionSocketFamilyState,
    ipv6: SessionSocketFamilyState,
}

#[derive(Debug)]
pub enum SessionSocketFamilyState {
    Disabled,
    Unavailable(SessionSocketError),
    Bound(SessionSocketFamilySet),
}

impl SessionSocketFamilyState {
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        matches!(self, Self::Bound(_))
    }

    #[must_use]
    pub fn bound(&self) -> Option<&SessionSocketFamilySet> {
        match self {
            Self::Bound(sockets) => Some(sockets),
            Self::Disabled | Self::Unavailable(_) => None,
        }
    }

    pub fn into_bound(self) -> Option<SessionSocketFamilySet> {
        match self {
            Self::Bound(sockets) => Some(sockets),
            Self::Disabled | Self::Unavailable(_) => None,
        }
    }

    #[must_use]
    pub fn error(&self) -> Option<&SessionSocketError> {
        match self {
            Self::Unavailable(error) => Some(error),
            Self::Disabled | Self::Bound(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct SessionSocketFamilySet {
    family: AddressFamily,
    tcp_listener: Option<TcpListener>,
    udp_socket: UdpSocket,
    tcp_address: Option<SocketAddr>,
    tcp_peer_address: Option<SocketAddr>,
    udp_address: SocketAddr,
}

impl SessionSocketSet {
    pub async fn bind(config: SessionSocketConfig) -> Result<Self, SessionSocketError> {
        config.validate()?;
        let ipv4_result = bind_ipv4(config).await;
        let ipv6 = if config.address_families.ipv6_enabled() {
            match bind_ipv6(config).await {
                Ok(sockets) => SessionSocketFamilyState::Bound(sockets),
                Err(error) => SessionSocketFamilyState::Unavailable(error),
            }
        } else {
            SessionSocketFamilyState::Disabled
        };
        let ipv4 = match ipv4_result {
            Ok(sockets) => SessionSocketFamilyState::Bound(sockets),
            Err(error) if config.address_families.ipv6_enabled() => {
                SessionSocketFamilyState::Unavailable(error)
            }
            Err(error) => return Err(error),
        };
        Ok(Self { ipv4, ipv6 })
    }

    #[must_use]
    pub const fn ipv4(&self) -> &SessionSocketFamilyState {
        &self.ipv4
    }

    #[must_use]
    pub const fn ipv6(&self) -> &SessionSocketFamilyState {
        &self.ipv6
    }

    pub fn into_families(self) -> (SessionSocketFamilyState, SessionSocketFamilyState) {
        (self.ipv4, self.ipv6)
    }

    pub fn tcp_address(&self) -> Option<SocketAddr> {
        self.ipv4
            .bound()
            .and_then(SessionSocketFamilySet::tcp_address)
    }

    pub fn tcp_peer_address(&self) -> Option<SocketAddr> {
        self.ipv4
            .bound()
            .and_then(SessionSocketFamilySet::tcp_peer_address)
    }

    pub fn udp_address(&self) -> SocketAddr {
        self.ipv4
            .bound()
            .expect("legacy IPv4 socket access requires a bound IPv4 family")
            .udp_address()
    }

    pub fn ports_match(&self) -> bool {
        self.ipv4
            .bound()
            .is_some_and(SessionSocketFamilySet::ports_match)
    }

    pub fn into_parts(self) -> (Option<TcpListener>, UdpSocket) {
        self.ipv4
            .into_bound()
            .expect("legacy IPv4 socket access requires a bound IPv4 family")
            .into_parts()
    }
}

impl SessionSocketFamilySet {
    pub async fn bind(
        config: SessionSocketConfig,
        family: AddressFamily,
    ) -> Result<Self, SessionSocketError> {
        config.validate()?;
        match family {
            AddressFamily::Ipv4 => bind_ipv4(config).await,
            AddressFamily::Ipv6 => bind_ipv6(config).await,
        }
    }

    #[must_use]
    pub const fn family(&self) -> AddressFamily {
        self.family
    }

    pub fn tcp_address(&self) -> Option<SocketAddr> {
        self.tcp_address
    }

    /// A concrete local endpoint when routing can identify one, otherwise the
    /// observed bind endpoint. This is bookkeeping for advertisement and port
    /// mapping; the listener may be bound more broadly.
    pub fn tcp_peer_address(&self) -> Option<SocketAddr> {
        self.tcp_peer_address
    }

    pub fn udp_address(&self) -> SocketAddr {
        self.udp_address
    }

    pub fn ports_match(&self) -> bool {
        self.tcp_address
            .is_some_and(|tcp| tcp.port() == self.udp_address.port())
    }

    pub fn into_parts(self) -> (Option<TcpListener>, UdpSocket) {
        (self.tcp_listener, self.udp_socket)
    }
}

async fn bind_ipv4(
    config: SessionSocketConfig,
) -> Result<SessionSocketFamilySet, SessionSocketError> {
    let Some((address, fixed_port)) = tcp_bind_intent(config).await? else {
        let udp_socket = bind_udp(config.udp_fallback_address, 0).await?;
        let udp_address = local_address(&udp_socket, SessionSocketTransport::Udp)?;
        return Ok(SessionSocketFamilySet {
            family: AddressFamily::Ipv4,
            tcp_listener: None,
            udp_socket,
            tcp_address: None,
            tcp_peer_address: None,
            udp_address,
        });
    };

    let (tcp_listener, udp_socket) = if let Some(port) = fixed_port {
        bind_fixed(IpAddr::V4(address), port).await?
    } else {
        bind_automatic(IpAddr::V4(address), config.preferred_port).await?
    };
    let tcp_address = local_address(&tcp_listener, SessionSocketTransport::Tcp)?;
    let tcp_peer_address = if matches!(
        config.tcp,
        IncomingTcpBootstrap::AutomaticLocalNetwork | IncomingTcpBootstrap::FixedLocalNetwork(_)
    ) {
        select_local_network_ipv4(config.local_network_address_override)
            .await
            .ok()
            .map(|address| SocketAddr::from((address, tcp_address.port())))
            .or(Some(tcp_address))
    } else {
        Some(tcp_address)
    };
    let udp_address = local_address(&udp_socket, SessionSocketTransport::Udp)?;
    Ok(SessionSocketFamilySet {
        family: AddressFamily::Ipv4,
        tcp_listener: Some(tcp_listener),
        udp_socket,
        tcp_address: Some(tcp_address),
        tcp_peer_address,
        udp_address,
    })
}

async fn bind_ipv6(
    config: SessionSocketConfig,
) -> Result<SessionSocketFamilySet, SessionSocketError> {
    let (address, fixed_port, tcp_enabled) = match config.tcp {
        IncomingTcpBootstrap::Disabled => (
            select_global_ipv6_for_bind(config.global_ipv6_address_override).await?,
            None,
            false,
        ),
        IncomingTcpBootstrap::AutomaticLoopback => (Ipv6Addr::LOCALHOST, None, true),
        IncomingTcpBootstrap::FixedLoopback(port) => (Ipv6Addr::LOCALHOST, Some(port), true),
        IncomingTcpBootstrap::AutomaticLocalNetwork => (
            select_global_ipv6_for_bind(config.global_ipv6_address_override).await?,
            None,
            true,
        ),
        IncomingTcpBootstrap::FixedLocalNetwork(port) => (
            select_global_ipv6_for_bind(config.global_ipv6_address_override).await?,
            Some(port),
            true,
        ),
    };
    if !tcp_enabled {
        let udp_socket = bind_udp(SocketAddr::from((address, 0)), 0).await?;
        let udp_address = local_address(&udp_socket, SessionSocketTransport::Udp)?;
        return Ok(SessionSocketFamilySet {
            family: AddressFamily::Ipv6,
            tcp_listener: None,
            udp_socket,
            tcp_address: None,
            tcp_peer_address: None,
            udp_address,
        });
    }
    let (tcp_listener, udp_socket) = if let Some(port) = fixed_port {
        bind_fixed(IpAddr::V6(address), port).await?
    } else {
        bind_automatic(IpAddr::V6(address), config.preferred_port).await?
    };
    let tcp_address = local_address(&tcp_listener, SessionSocketTransport::Tcp)?;
    let udp_address = local_address(&udp_socket, SessionSocketTransport::Udp)?;
    Ok(SessionSocketFamilySet {
        family: AddressFamily::Ipv6,
        tcp_listener: Some(tcp_listener),
        udp_socket,
        tcp_address: Some(tcp_address),
        tcp_peer_address: Some(tcp_address),
        udp_address,
    })
}

/// Selects the concrete source address the kernel would use for a global IPv6
/// route without transmitting a datagram.
pub async fn select_global_ipv6() -> Result<Ipv6Addr, SessionSocketError> {
    select_global_ipv6_for_bind(None).await
}

async fn select_global_ipv6_for_bind(
    address_override: Option<Ipv6Addr>,
) -> Result<Ipv6Addr, SessionSocketError> {
    let address = if let Some(address) = address_override {
        address
    } else {
        let probe = UdpSocket::bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)))
            .await
            .map_err(SessionSocketError::GlobalIpv6Address)?;
        probe_ipv6_source(
            &probe,
            SocketAddr::from((Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1), 1)),
        )
        .await?
    };
    if !eligible_global_ipv6(address) {
        return Err(SessionSocketError::IneligibleGlobalIpv6Address(address));
    }
    Ok(address)
}

async fn probe_ipv6_source(
    probe: &UdpSocket,
    target: SocketAddr,
) -> Result<Ipv6Addr, SessionSocketError> {
    probe
        .connect(target)
        .await
        .map_err(SessionSocketError::GlobalIpv6Address)?;
    match probe
        .local_addr()
        .map_err(SessionSocketError::GlobalIpv6Address)?
    {
        SocketAddr::V6(address) => Ok(*address.ip()),
        SocketAddr::V4(_) => Err(SessionSocketError::GlobalIpv6Address(io::Error::other(
            "IPv6 route probe returned an IPv4 source",
        ))),
    }
}

pub fn eligible_global_ipv6(address: Ipv6Addr) -> bool {
    let octets = address.octets();
    let global_unicast = octets[0] & 0xe0 == 0x20;
    let documentation = octets[..4] == [0x20, 0x01, 0x0d, 0xb8];
    let teredo = octets[..4] == [0x20, 0x01, 0x00, 0x00];
    let six_to_four = octets[..2] == [0x20, 0x02];
    global_unicast && !documentation && !teredo && !six_to_four
}

async fn tcp_bind_intent(
    config: SessionSocketConfig,
) -> Result<Option<(Ipv4Addr, Option<u16>)>, SessionSocketError> {
    match config.tcp {
        IncomingTcpBootstrap::Disabled => Ok(None),
        IncomingTcpBootstrap::AutomaticLoopback => Ok(Some((Ipv4Addr::LOCALHOST, None))),
        IncomingTcpBootstrap::FixedLoopback(port) => Ok(Some((Ipv4Addr::LOCALHOST, Some(port)))),
        IncomingTcpBootstrap::AutomaticLocalNetwork => Ok(Some((Ipv4Addr::UNSPECIFIED, None))),
        IncomingTcpBootstrap::FixedLocalNetwork(port) => {
            Ok(Some((Ipv4Addr::UNSPECIFIED, Some(port))))
        }
    }
}

async fn bind_fixed(
    address: IpAddr,
    port: u16,
) -> Result<(TcpListener, UdpSocket), SessionSocketError> {
    if port < MIN_PREFERRED_LISTEN_PORT {
        return Err(SessionSocketError::InvalidFixedPort(port));
    }
    let endpoint = SocketAddr::new(address, port);
    let tcp = bind_tcp(endpoint, port)?;
    let udp = bind_udp(endpoint, port).await?;
    Ok((tcp, udp))
}

async fn bind_automatic(
    address: IpAddr,
    preferred_port: u16,
) -> Result<(TcpListener, UdpSocket), SessionSocketError> {
    let mut retries = MAX_LISTEN_PORT_RETRIES;
    let mut tcp_port = preferred_port;
    let tcp = loop {
        match bind_tcp(SocketAddr::new(address, tcp_port), tcp_port) {
            Ok(listener) => break listener,
            Err(SessionSocketError::Bind { source, .. })
                if source.kind() == io::ErrorKind::AddrInUse
                    && retry_successor(tcp_port, &mut retries).is_some() =>
            {
                tcp_port = tcp_port
                    .checked_add(1)
                    .expect("retry successor checked automatic TCP increment");
            }
            Err(SessionSocketError::Bind { source, .. })
                if source.kind() == io::ErrorKind::AddrInUse =>
            {
                break bind_tcp(SocketAddr::new(address, 0), 0)?;
            }
            Err(error) => return Err(error),
        }
    };
    let tcp_address = local_address(&tcp, SessionSocketTransport::Tcp)?;
    let mut udp_port = tcp_address.port();
    let udp = loop {
        match bind_udp(SocketAddr::new(address, udp_port), udp_port).await {
            Ok(socket) => break socket,
            Err(SessionSocketError::Bind { source, .. })
                if source.kind() == io::ErrorKind::AddrInUse
                    && retry_successor(udp_port, &mut retries).is_some() =>
            {
                udp_port = udp_port
                    .checked_add(1)
                    .expect("retry successor checked automatic UDP increment");
            }
            Err(SessionSocketError::Bind { source, .. })
                if source.kind() == io::ErrorKind::AddrInUse =>
            {
                break bind_udp(SocketAddr::new(address, 0), 0).await?;
            }
            Err(error) => return Err(error),
        }
    };
    Ok((tcp, udp))
}

fn next_port(port: u16) -> Option<u16> {
    port.checked_add(1)
}

fn retry_successor(port: u16, retries: &mut u8) -> Option<u16> {
    if *retries == 0 {
        return None;
    }
    let successor = next_port(port)?;
    *retries -= 1;
    Some(successor)
}

fn bind_tcp(endpoint: SocketAddr, reported_port: u16) -> Result<TcpListener, SessionSocketError> {
    let family = AddressFamily::of(endpoint.ip());
    let socket = match family {
        AddressFamily::Ipv4 => TcpSocket::new_v4(),
        AddressFamily::Ipv6 => TcpSocket::new_v6(),
    }
    .map_err(|source| SessionSocketError::Bind {
        family,
        transport: SessionSocketTransport::Tcp,
        port: reported_port,
        source,
    })?;
    socket
        .bind(endpoint)
        .map_err(|source| SessionSocketError::Bind {
            family,
            transport: SessionSocketTransport::Tcp,
            port: reported_port,
            source,
        })?;
    socket
        .listen(DEFAULT_LISTEN_BACKLOG)
        .map_err(|source| SessionSocketError::Bind {
            family,
            transport: SessionSocketTransport::Tcp,
            port: reported_port,
            source,
        })
}

async fn bind_udp(
    endpoint: SocketAddr,
    reported_port: u16,
) -> Result<UdpSocket, SessionSocketError> {
    let family = AddressFamily::of(endpoint.ip());
    UdpSocket::bind(endpoint)
        .await
        .map_err(|source| SessionSocketError::Bind {
            family,
            transport: SessionSocketTransport::Udp,
            port: reported_port,
            source,
        })
}

trait LocalAddress {
    fn read_local_address(&self) -> io::Result<SocketAddr>;
}

impl LocalAddress for TcpListener {
    fn read_local_address(&self) -> io::Result<SocketAddr> {
        self.local_addr()
    }
}

impl LocalAddress for UdpSocket {
    fn read_local_address(&self) -> io::Result<SocketAddr> {
        self.local_addr()
    }
}

fn local_address(
    socket: &impl LocalAddress,
    transport: SessionSocketTransport,
) -> Result<SocketAddr, SessionSocketError> {
    socket
        .read_local_address()
        .map_err(|source| SessionSocketError::LocalAddress { transport, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU16, Ordering};
    use tokio::net::TcpStream;
    use tokio::time::{Duration, timeout};

    static NEXT_TEST_PORT_RANGE: AtomicU16 = AtomicU16::new(20_000);

    fn config(tcp: IncomingTcpBootstrap, preferred_port: u16) -> SessionSocketConfig {
        SessionSocketConfig::new(
            tcp,
            preferred_port,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
    }

    fn dual_config(tcp: IncomingTcpBootstrap, preferred_port: u16) -> SessionSocketConfig {
        config(tcp, preferred_port).with_address_families(AddressFamilyPolicy::dual_stack())
    }

    async fn available_port() -> u16 {
        loop {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            if UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await.is_ok() {
                return port;
            }
        }
    }

    async fn available_dual_stack_port() -> u16 {
        loop {
            let ipv4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            let port = ipv4.local_addr().unwrap().port();
            let Ok(ipv4_udp) = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await else {
                continue;
            };
            let Ok(ipv6) = TcpListener::bind((Ipv6Addr::LOCALHOST, port)).await else {
                continue;
            };
            let Ok(ipv6_udp) = UdpSocket::bind((Ipv6Addr::LOCALHOST, port)).await else {
                continue;
            };
            drop((ipv4, ipv4_udp, ipv6, ipv6_udp));
            return port;
        }
    }

    async fn consecutive_tcp_blockers(count: usize) -> (u16, Vec<TcpListener>) {
        let start = NEXT_TEST_PORT_RANGE.fetch_add(100, Ordering::Relaxed);
        for base in (start..start + 90).step_by(count + 1) {
            let mut blockers = Vec::with_capacity(count);
            for offset in 0..count {
                let Ok(offset) = u16::try_from(offset) else {
                    break;
                };
                let Some(port) = base.checked_add(offset) else {
                    break;
                };
                match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
                    Ok(listener) => blockers.push(listener),
                    Err(_) => break,
                }
            }
            if blockers.len() == count {
                return (base, blockers);
            }
        }
        panic!("could not allocate {count} consecutive TCP blockers");
    }

    async fn consecutive_udp_blockers(count: usize) -> (u16, Vec<UdpSocket>) {
        let start = NEXT_TEST_PORT_RANGE.fetch_add(100, Ordering::Relaxed);
        for base in (start..start + 90).step_by(count + 1) {
            let mut blockers = Vec::with_capacity(count);
            for offset in 0..count {
                let Ok(offset) = u16::try_from(offset) else {
                    break;
                };
                let Some(port) = base.checked_add(offset) else {
                    break;
                };
                match UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await {
                    Ok(socket) => blockers.push(socket),
                    Err(_) => break,
                }
            }
            if blockers.len() == count
                && TcpListener::bind((Ipv4Addr::LOCALHOST, base)).await.is_ok()
            {
                return (base, blockers);
            }
        }
        panic!("could not allocate {count} consecutive UDP blockers");
    }

    #[test]
    fn automatic_successor_never_wraps() {
        assert_eq!(next_port(1_024), Some(1_025));
        assert_eq!(next_port(65_535), None);
        let mut shared = MAX_LISTEN_PORT_RETRIES;
        assert_eq!(retry_successor(6_881, &mut shared), Some(6_882));
        assert_eq!(shared, 9);
        for port in 6_882..=6_890 {
            assert_eq!(retry_successor(port, &mut shared), port.checked_add(1));
        }
        assert_eq!(shared, 0);
        assert_eq!(retry_successor(6_891, &mut shared), None);
        let mut overflow_retries = MAX_LISTEN_PORT_RETRIES;
        assert_eq!(retry_successor(65_535, &mut overflow_retries), None);
        assert_eq!(overflow_retries, MAX_LISTEN_PORT_RETRIES);
    }

    #[test]
    fn global_ipv6_eligibility_rejects_non_native_or_reserved_addresses() {
        let rejected = [
            "::",
            "::1",
            "fe80::1",
            "fec0::1",
            "fd00::1",
            "ff02::1",
            "::ffff:192.0.2.1",
            "::192.0.2.1",
            "2001:db8::1",
            "2001::1",
            "2002::1",
        ];
        for text in rejected {
            let address = text.parse::<Ipv6Addr>().unwrap();
            assert!(!eligible_global_ipv6(address), "accepted {address}");
        }
        assert!(eligible_global_ipv6(
            "2606:4700:4700::1111".parse().unwrap()
        ));
    }

    #[tokio::test]
    async fn ipv6_route_probe_connects_without_sending_a_datagram() {
        let receiver = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).await.unwrap();
        let probe = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).await.unwrap();
        let source = probe_ipv6_source(&probe, receiver.local_addr().unwrap())
            .await
            .unwrap();
        assert_eq!(source, Ipv6Addr::LOCALHOST);
        let mut byte = [0_u8; 1];
        assert!(
            timeout(Duration::from_millis(50), receiver.recv_from(&mut byte))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn automatic_binds_tcp_and_udp_to_the_preferred_port() {
        let port = available_port().await;
        let sockets = SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, port))
            .await
            .unwrap();
        assert_eq!(sockets.tcp_address().unwrap().port(), port);
        assert_eq!(sockets.udp_address().port(), port);
        assert!(sockets.ports_match());
    }

    #[tokio::test]
    async fn dual_stack_allocation_attempts_the_preferred_port_per_family() {
        let port = available_dual_stack_port().await;
        let sockets =
            SessionSocketSet::bind(dual_config(IncomingTcpBootstrap::AutomaticLoopback, port))
                .await
                .unwrap();
        let ipv4 = sockets.ipv4().bound().expect("IPv4 family binds");
        assert_eq!(ipv4.tcp_address().unwrap().port(), port);
        assert_eq!(ipv4.udp_address().port(), port);
        let ipv6 = sockets.ipv6().bound().expect("IPv6 family binds");
        assert_eq!(ipv6.tcp_address().unwrap().port(), port);
        assert_eq!(ipv6.udp_address().port(), port);
    }

    #[tokio::test]
    async fn ipv6_family_failure_retains_the_serving_ipv4_pair() {
        let port = available_dual_stack_port().await;
        let blocker = UdpSocket::bind((Ipv6Addr::LOCALHOST, port)).await.unwrap();
        let sockets = SessionSocketSet::bind(dual_config(
            IncomingTcpBootstrap::FixedLoopback(port),
            6_881,
        ))
        .await
        .unwrap();
        let ipv4 = sockets.ipv4().bound().expect("IPv4 family binds");
        assert_eq!(ipv4.tcp_address().unwrap().port(), port);
        assert_eq!(ipv4.udp_address().port(), port);
        assert!(matches!(
            sockets.ipv6(),
            SessionSocketFamilyState::Unavailable(SessionSocketError::Bind {
                family: AddressFamily::Ipv6,
                transport: SessionSocketTransport::Udp,
                port: failed_port,
                source,
            }) if *failed_port == port && source.kind() == io::ErrorKind::AddrInUse
        ));
        TcpStream::connect(ipv4.tcp_address().unwrap())
            .await
            .expect("IPv4 sibling remains accepting");
        drop(blocker);
    }

    #[tokio::test]
    async fn ipv4_family_failure_retains_the_serving_ipv6_pair() {
        let port = available_dual_stack_port().await;
        let blocker = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        let sockets = SessionSocketSet::bind(dual_config(
            IncomingTcpBootstrap::FixedLoopback(port),
            6_881,
        ))
        .await
        .unwrap();
        assert!(matches!(
            sockets.ipv4(),
            SessionSocketFamilyState::Unavailable(SessionSocketError::Bind {
                family: AddressFamily::Ipv4,
                transport: SessionSocketTransport::Udp,
                port: failed_port,
                source,
            }) if *failed_port == port && source.kind() == io::ErrorKind::AddrInUse
        ));
        let ipv6 = sockets.ipv6().bound().expect("IPv6 sibling remains bound");
        assert_eq!(ipv6.tcp_address().unwrap().port(), port);
        assert_eq!(ipv6.udp_address().port(), port);
        TcpStream::connect(ipv6.tcp_address().unwrap())
            .await
            .expect("IPv6 sibling remains accepting");
        drop(blocker);
    }

    #[tokio::test]
    async fn ipv4_only_policy_constructs_no_ipv6_socket() {
        let port = available_port().await;
        let sockets = SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, port))
            .await
            .unwrap();
        assert!(matches!(sockets.ipv6(), SessionSocketFamilyState::Disabled));
    }

    #[tokio::test]
    async fn ordinary_listener_binds_all_ipv4_interfaces() {
        let port = available_port().await;
        let sockets = SessionSocketSet::bind(
            config(IncomingTcpBootstrap::AutomaticLocalNetwork, port)
                .with_local_network_address_for_testing(Ipv4Addr::new(192, 0, 2, 10)),
        )
        .await
        .unwrap();
        assert_eq!(sockets.tcp_address().unwrap().ip(), Ipv4Addr::UNSPECIFIED);
        assert_eq!(sockets.udp_address().ip(), Ipv4Addr::UNSPECIFIED);
        assert_eq!(
            sockets.tcp_peer_address(),
            Some(SocketAddr::from((Ipv4Addr::new(192, 0, 2, 10), port)))
        );
    }

    #[tokio::test]
    async fn automatic_tcp_conflict_advances_both_transports() {
        let (port, blockers) = consecutive_tcp_blockers(1).await;
        let sockets = SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, port))
            .await
            .unwrap();
        assert_eq!(sockets.tcp_address().unwrap().port(), port + 1);
        assert_eq!(sockets.udp_address().port(), port + 1);
        drop(blockers);
    }

    #[tokio::test]
    async fn automatic_udp_only_conflict_can_diverge() {
        let (port, blockers) = consecutive_udp_blockers(1).await;
        let sockets = SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, port))
            .await
            .unwrap();
        assert_eq!(sockets.tcp_address().unwrap().port(), port);
        assert_eq!(sockets.udp_address().port(), port + 1);
        assert!(!sockets.ports_match());
        drop(blockers);
    }

    #[tokio::test]
    async fn automatic_exhaustion_uses_system_ports() {
        let count = usize::from(MAX_LISTEN_PORT_RETRIES) + 1;
        let (tcp_port, tcp_blockers) = consecutive_tcp_blockers(count).await;
        let sockets =
            SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, tcp_port))
                .await
                .unwrap();
        let actual_tcp = sockets.tcp_address().unwrap().port();
        assert!(actual_tcp != 0);
        assert!(!(tcp_port..=tcp_port + u16::from(MAX_LISTEN_PORT_RETRIES)).contains(&actual_tcp));
        drop(sockets);
        drop(tcp_blockers);

        let (udp_port, udp_blockers) = consecutive_udp_blockers(count).await;
        let sockets =
            SessionSocketSet::bind(config(IncomingTcpBootstrap::AutomaticLoopback, udp_port))
                .await
                .unwrap();
        assert_eq!(sockets.tcp_address().unwrap().port(), udp_port);
        let actual_udp = sockets.udp_address().port();
        assert!(actual_udp != 0);
        assert!(!(udp_port..=udp_port + u16::from(MAX_LISTEN_PORT_RETRIES)).contains(&actual_udp));
        drop(udp_blockers);
    }

    #[tokio::test]
    async fn fixed_udp_conflict_fails_atomically_without_leaking_tcp() {
        let (port, blockers) = consecutive_udp_blockers(1).await;
        let error =
            SessionSocketSet::bind(config(IncomingTcpBootstrap::FixedLoopback(port), 6_881))
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            SessionSocketError::Bind {
                family: AddressFamily::Ipv4,
                transport: SessionSocketTransport::Udp,
                port: failed_port,
                ref source,
            } if failed_port == port && source.kind() == io::ErrorKind::AddrInUse
        ));
        TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .expect("failed coordinated construction must drop TCP");
        drop(blockers);
    }

    #[tokio::test]
    async fn disabled_listener_still_allocates_ephemeral_udp() {
        let sockets = SessionSocketSet::bind(config(IncomingTcpBootstrap::Disabled, 6_881))
            .await
            .unwrap();
        assert_eq!(sockets.tcp_address(), None);
        assert_ne!(sockets.udp_address().port(), 0);
        assert!(!sockets.ports_match());
    }
}

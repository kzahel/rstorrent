//! Coordinated application-generation TCP and UDP socket allocation.

use std::error::Error;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::{TcpListener, TcpSocket, UdpSocket};

use crate::incoming::{IncomingPeerError, IncomingTcpBootstrap, select_local_network_ipv4};
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
    LocalNetworkAddress(IncomingPeerError),
    Bind {
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
            Self::Bind { source, .. } | Self::LocalAddress { source, .. } => Some(source),
            Self::InvalidPreferredPort(_)
            | Self::InvalidFixedPort(_)
            | Self::InvalidUdpFallbackAddress
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
            Self::LocalNetworkAddress(error) => {
                write!(formatter, "select local-network listen address: {error}")
            }
            Self::Bind {
                transport,
                port,
                source,
            } => write!(formatter, "bind session {transport} port {port}: {source}"),
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
            Self::Bind { source, .. } | Self::LocalAddress { source, .. } => Some(source),
            Self::InvalidPreferredPort(_)
            | Self::InvalidFixedPort(_)
            | Self::InvalidUdpFallbackAddress => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SessionSocketConfig {
    pub tcp: IncomingTcpBootstrap,
    pub preferred_port: u16,
    pub udp_fallback_address: SocketAddr,
    local_network_address_override: Option<Ipv4Addr>,
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
            local_network_address_override: None,
        }
    }

    #[doc(hidden)]
    pub fn with_local_network_address_for_testing(mut self, address: Ipv4Addr) -> Self {
        self.local_network_address_override = Some(address);
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
    tcp_listener: Option<TcpListener>,
    udp_socket: UdpSocket,
    tcp_address: Option<SocketAddr>,
    tcp_peer_address: Option<SocketAddr>,
    udp_address: SocketAddr,
}

impl SessionSocketSet {
    pub async fn bind(config: SessionSocketConfig) -> Result<Self, SessionSocketError> {
        config.validate()?;
        let Some((address, fixed_port)) = tcp_bind_intent(config).await? else {
            let udp_socket = bind_udp(config.udp_fallback_address, 0).await?;
            let udp_address = local_address(&udp_socket, SessionSocketTransport::Udp)?;
            return Ok(Self {
                tcp_listener: None,
                udp_socket,
                tcp_address: None,
                tcp_peer_address: None,
                udp_address,
            });
        };

        let (tcp_listener, udp_socket) = if let Some(port) = fixed_port {
            bind_fixed(address, port).await?
        } else {
            bind_automatic(address, config.preferred_port).await?
        };
        let tcp_address = local_address(&tcp_listener, SessionSocketTransport::Tcp)?;
        let tcp_peer_address = if matches!(
            config.tcp,
            IncomingTcpBootstrap::AutomaticLocalNetwork
                | IncomingTcpBootstrap::FixedLocalNetwork(_)
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
        Ok(Self {
            tcp_listener: Some(tcp_listener),
            udp_socket,
            tcp_address: Some(tcp_address),
            tcp_peer_address,
            udp_address,
        })
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
    address: Ipv4Addr,
    port: u16,
) -> Result<(TcpListener, UdpSocket), SessionSocketError> {
    if port < MIN_PREFERRED_LISTEN_PORT {
        return Err(SessionSocketError::InvalidFixedPort(port));
    }
    let endpoint = SocketAddr::from((address, port));
    let tcp = bind_tcp(endpoint, port)?;
    let udp = bind_udp(endpoint, port).await?;
    Ok((tcp, udp))
}

async fn bind_automatic(
    address: Ipv4Addr,
    preferred_port: u16,
) -> Result<(TcpListener, UdpSocket), SessionSocketError> {
    let mut retries = MAX_LISTEN_PORT_RETRIES;
    let mut tcp_port = preferred_port;
    let tcp = loop {
        match bind_tcp(SocketAddr::from((address, tcp_port)), tcp_port) {
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
                break bind_tcp(SocketAddr::from((address, 0)), 0)?;
            }
            Err(error) => return Err(error),
        }
    };
    let tcp_address = local_address(&tcp, SessionSocketTransport::Tcp)?;
    let mut udp_port = tcp_address.port();
    let udp = loop {
        match bind_udp(SocketAddr::from((address, udp_port)), udp_port).await {
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
                break bind_udp(SocketAddr::from((address, 0)), 0).await?;
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
    let socket = TcpSocket::new_v4().map_err(|source| SessionSocketError::Bind {
        transport: SessionSocketTransport::Tcp,
        port: reported_port,
        source,
    })?;
    socket
        .bind(endpoint)
        .map_err(|source| SessionSocketError::Bind {
            transport: SessionSocketTransport::Tcp,
            port: reported_port,
            source,
        })?;
    socket
        .listen(DEFAULT_LISTEN_BACKLOG)
        .map_err(|source| SessionSocketError::Bind {
            transport: SessionSocketTransport::Tcp,
            port: reported_port,
            source,
        })
}

async fn bind_udp(
    endpoint: SocketAddr,
    reported_port: u16,
) -> Result<UdpSocket, SessionSocketError> {
    UdpSocket::bind(endpoint)
        .await
        .map_err(|source| SessionSocketError::Bind {
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

    static NEXT_TEST_PORT_RANGE: AtomicU16 = AtomicU16::new(20_000);

    fn config(tcp: IncomingTcpBootstrap, preferred_port: u16) -> SessionSocketConfig {
        SessionSocketConfig::new(
            tcp,
            preferred_port,
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        )
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

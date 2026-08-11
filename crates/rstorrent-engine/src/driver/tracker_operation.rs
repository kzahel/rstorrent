//! Task-free tracker transport execution shared by the direct and session owners.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rstorrent_protocol::udp_tracker::{
    AnnounceEvent, AnnounceRequest, MAX_ANNOUNCE_RESPONSE_LENGTH, TrackerAddressFamily,
    TransactionId, UdpTrackerError, encode_announce_request, encode_connect_request,
    parse_announce_response, parse_connect_response,
};
use tokio::net::{UdpSocket, lookup_host};
use tokio::time::{Instant as TokioInstant, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use super::{DownloadActivityEvent, DownloadControl, DownloadError};
use crate::http_tracker::{
    HttpTrackerAnnounce, HttpTrackerClients, HttpTrackerResponse, TrackerRetryDirective,
    announce_http_tracker_with_address_families,
};
use crate::metrics::ByteMetric;
use crate::network::{AddressFamilyPolicy, NetworkConfig, NetworkPolicy};
use crate::tracker::{TrackerConnectionFamily, TrackerEndpoint};

const NETWORK_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(10);
const UDP_TRACKER_RETRANSMIT_AFTER: Duration = Duration::from_secs(15);
const UDP_TRACKER_COMPLETION_TIMEOUT: Duration = Duration::from_secs(30);
const UDP_TRACKER_TOKEN_LIFETIME: Duration = Duration::from_secs(60);
const MAX_UDP_TRACKER_TOKENS: usize = 64;
const MAX_RESOLVED_ADDRESSES: usize = 32;
const UDP_TRACKER_RECEIVE_LENGTH: usize = MAX_ANNOUNCE_RESPONSE_LENGTH + 1;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackerAnnounceInput {
    pub(crate) info_hash: [u8; 20],
    pub(crate) peer_id: [u8; 20],
    pub(crate) key: u32,
    pub(crate) downloaded: u64,
    pub(crate) left: u64,
    pub(crate) uploaded: u64,
    pub(crate) event: AnnounceEvent,
    pub(crate) num_want: i32,
    pub(crate) port: u16,
    pub(crate) ipv6_port: u16,
    pub(crate) support_crypto: bool,
}

#[derive(Debug)]
pub(crate) struct TrackerAnnounceOutcome {
    pub(crate) interval: Duration,
    pub(crate) seeders: Option<u32>,
    pub(crate) leechers: Option<u32>,
    pub(crate) connection_family: Option<TrackerConnectionFamily>,
    pub(crate) peers: Vec<SocketAddr>,
    pub(crate) tracker_id: Option<Vec<u8>>,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug)]
pub(crate) enum TrackerOperationFailure {
    Cancelled,
    Transport(String),
    Declared {
        reason: String,
        retry: Option<TrackerRetryDirective>,
    },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackerOperationSources {
    pub(crate) ipv4: Option<IpAddr>,
    pub(crate) ipv6: Option<IpAddr>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerTiming {
    pub(crate) retransmit_after: Duration,
    pub(crate) completion_timeout: Duration,
}

impl UdpTrackerTiming {
    pub(crate) const PRODUCTION: Self = Self {
        retransmit_after: UDP_TRACKER_RETRANSMIT_AFTER,
        completion_timeout: UDP_TRACKER_COMPLETION_TIMEOUT,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerAnnounce {
    pub(crate) info_hash: [u8; 20],
    pub(crate) peer_id: [u8; 20],
    pub(crate) key: u32,
    pub(crate) downloaded: u64,
    pub(crate) left: u64,
    pub(crate) uploaded: u64,
    pub(crate) event: AnnounceEvent,
    pub(crate) num_want: i32,
    pub(crate) port: u16,
    pub(crate) ipv6_port: u16,
}

impl From<TrackerAnnounceInput> for UdpTrackerAnnounce {
    fn from(input: TrackerAnnounceInput) -> Self {
        Self {
            info_hash: input.info_hash,
            peer_id: input.peer_id,
            key: input.key,
            downloaded: input.downloaded,
            left: input.left,
            uploaded: input.uploaded,
            event: input.event,
            num_want: input.num_want,
            port: input.port,
            ipv6_port: input.ipv6_port,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct UdpTrackerExchange<'a> {
    pub(crate) timing: UdpTrackerTiming,
    pub(crate) control: &'a DownloadControl,
    pub(crate) tracker_label: &'a str,
    pub(crate) source_ipv4: Option<IpAddr>,
    pub(crate) source_ipv6: Option<IpAddr>,
}

#[derive(Clone, Copy, Debug)]
struct UdpTrackerToken {
    connection_id: u64,
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub(crate) struct UdpTrackerTokenCache {
    tokens: BTreeMap<SocketAddr, UdpTrackerToken>,
}

impl UdpTrackerTokenCache {
    pub(crate) fn get(&mut self, address: SocketAddr, now: Instant) -> Option<u64> {
        self.prune(now);
        self.tokens
            .get(&address)
            .filter(|token| token.expires_at > now)
            .map(|token| token.connection_id)
    }

    pub(crate) fn insert(&mut self, address: SocketAddr, connection_id: u64, now: Instant) {
        self.prune(now);
        if self.tokens.len() == MAX_UDP_TRACKER_TOKENS && !self.tokens.contains_key(&address) {
            let first = self.tokens.keys().next().copied();
            if let Some(first) = first {
                self.tokens.remove(&first);
            }
        }
        self.tokens.insert(
            address,
            UdpTrackerToken {
                connection_id,
                expires_at: now + UDP_TRACKER_TOKEN_LIFETIME,
            },
        );
    }

    fn remove(&mut self, address: SocketAddr) {
        self.tokens.remove(&address);
    }

    fn prune(&mut self, now: Instant) {
        self.tokens.retain(|_, token| token.expires_at > now);
    }
}

#[derive(Debug)]
pub(crate) struct UdpTrackerAnnounceResult {
    pub(crate) response: rstorrent_protocol::udp_tracker::AnnounceResponse,
    pub(crate) connection_family: TrackerConnectionFamily,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tracker_operation(
    endpoint: TrackerEndpoint,
    tracker_url: &str,
    tracker_label: &str,
    network: NetworkConfig,
    sources: TrackerOperationSources,
    http_clients: Option<Arc<HttpTrackerClients>>,
    announce: TrackerAnnounceInput,
    http_timeout: Duration,
    token_cache: &mut UdpTrackerTokenCache,
    tracker_id: Option<Vec<u8>>,
    control: &DownloadControl,
    cancellation: &CancellationToken,
) -> Result<TrackerAnnounceOutcome, TrackerOperationFailure> {
    let operation = async {
        match endpoint {
            TrackerEndpoint::Udp(url) => announce_udp_tracker(
                &url,
                network.policy,
                network.address_families,
                token_cache,
                announce.into(),
                UdpTrackerExchange {
                    timing: UdpTrackerTiming::PRODUCTION,
                    control,
                    tracker_label,
                    source_ipv4: sources.ipv4,
                    source_ipv6: sources.ipv6,
                },
            )
            .await
            .map(|result| {
                let response = result.response;
                TrackerAnnounceOutcome {
                    interval: Duration::from_secs(u64::from(response.interval)),
                    seeders: Some(response.seeders),
                    leechers: Some(response.leechers),
                    connection_family: Some(result.connection_family),
                    peers: response
                        .peers
                        .into_iter()
                        .map(compact_peer_address)
                        .collect(),
                    tracker_id: None,
                    warnings: Vec::new(),
                }
            })
            .map_err(|error| TrackerOperationFailure::Transport(error.to_string())),
            TrackerEndpoint::Http { .. } => {
                let Some(clients) = http_clients else {
                    return Err(TrackerOperationFailure::Transport(
                        "HTTP tracker client construction is unavailable".to_owned(),
                    ));
                };
                let request = HttpTrackerAnnounce {
                    info_hash: announce.info_hash,
                    peer_id: announce.peer_id,
                    port: announce.port,
                    ipv6_port: announce.ipv6_port,
                    uploaded: announce.uploaded,
                    downloaded: announce.downloaded,
                    left: announce.left,
                    event: announce.event,
                    key: announce.key,
                    num_want: u32::try_from(announce.num_want).unwrap_or(0),
                    support_crypto: announce.support_crypto,
                    tracker_id,
                };
                match announce_http_tracker_with_address_families(
                    &clients,
                    tracker_url,
                    network.policy,
                    network.address_families,
                    false,
                    &request,
                    http_timeout,
                )
                .await
                {
                    Ok(HttpTrackerResponse::Success(success)) => {
                        let mut warnings = success.diagnostics;
                        if let Some(warning) = success.warning {
                            warnings.insert(0, warning);
                        }
                        Ok(TrackerAnnounceOutcome {
                            interval: success.interval,
                            seeders: success.seeders,
                            leechers: success.leechers,
                            connection_family: success.connection_family,
                            peers: success
                                .peers
                                .into_iter()
                                .filter_map(|peer| match peer {
                                    crate::http_tracker::TrackerPeer::Address(address) => {
                                        Some(address)
                                    }
                                    crate::http_tracker::TrackerPeer::Hostname { .. } => None,
                                })
                                .collect(),
                            tracker_id: success.tracker_id,
                            warnings,
                        })
                    }
                    Ok(HttpTrackerResponse::Failure { reason, retry }) => {
                        Err(TrackerOperationFailure::Declared { reason, retry })
                    }
                    Err(error) => Err(TrackerOperationFailure::Transport(error.to_string())),
                }
            }
        }
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(TrackerOperationFailure::Cancelled),
        result = operation => result,
    }
}

pub(crate) fn random_nonzero_u32() -> Result<u32, DownloadError> {
    let mut bytes = [0; 4];
    getrandom::fill(&mut bytes).map_err(DownloadError::Entropy)?;
    Ok(u32::from_ne_bytes(bytes).max(1))
}

pub(crate) fn redacted_tracker_label(value: &str) -> String {
    let Ok(url) = url::Url::parse(value) else {
        return "tracker".to_owned();
    };
    let Some(host) = url.host_str() else {
        return "tracker".to_owned();
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

pub(crate) fn compact_peer_address(
    peer: rstorrent_protocol::udp_tracker::CompactPeer,
) -> SocketAddr {
    match peer {
        rstorrent_protocol::udp_tracker::CompactPeer::Ipv4 { address, port } => {
            SocketAddr::from((Ipv4Addr::from(address), port))
        }
        rstorrent_protocol::udp_tracker::CompactPeer::Ipv6 { address, port } => {
            SocketAddr::from((Ipv6Addr::from(address), port))
        }
    }
}

pub(crate) async fn resolve_host(
    host: &str,
    port: u16,
    operation: &'static str,
) -> Result<Vec<SocketAddr>, DownloadError> {
    timeout(NETWORK_RESOLUTION_TIMEOUT, lookup_host((host, port)))
        .await
        .map_err(|_| DownloadError::NetworkTimedOut {
            operation,
            timeout: NETWORK_RESOLUTION_TIMEOUT,
        })?
        .map(|addresses| addresses.take(MAX_RESOLVED_ADDRESSES).collect())
        .map_err(|source| DownloadError::Io { operation, source })
}

pub(crate) async fn announce_udp_tracker(
    tracker: &rstorrent_protocol::magnet::UdpTrackerUrl,
    network_policy: NetworkPolicy,
    address_families: AddressFamilyPolicy,
    token_cache: &mut UdpTrackerTokenCache,
    announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<UdpTrackerAnnounceResult, DownloadError> {
    if !network_policy.permits_dns() {
        return Err(DownloadError::NetworkDisabled);
    }
    let addresses = resolve_host(&tracker.host, tracker.port, "resolve UDP tracker").await?;
    let mut last_error = None;
    let mut found_allowed = false;
    for address in addresses {
        if !address_families.permits(address.ip()) || !network_policy.allows(address) {
            continue;
        }
        found_allowed = true;
        match announce_udp_tracker_address(address, token_cache, announce, exchange).await {
            Ok(response) => return Ok(response),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or({
        if found_allowed {
            DownloadError::NoUsablePeer
        } else {
            DownloadError::NoUsableTrackerAddress
        }
    }))
}

pub(crate) async fn announce_udp_tracker_address(
    address: SocketAddr,
    token_cache: &mut UdpTrackerTokenCache,
    mut announce: UdpTrackerAnnounce,
    exchange: UdpTrackerExchange<'_>,
) -> Result<UdpTrackerAnnounceResult, DownloadError> {
    let bind_address = match address {
        SocketAddr::V4(_) => exchange
            .source_ipv4
            .filter(IpAddr::is_ipv4)
            .map_or(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)), |source| {
                SocketAddr::new(source, 0)
            }),
        SocketAddr::V6(_) => {
            announce.port = announce.ipv6_port;
            exchange
                .source_ipv6
                .filter(IpAddr::is_ipv6)
                .map_or(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)), |source| {
                    SocketAddr::new(source, 0)
                })
        }
    };
    let socket = UdpSocket::bind(bind_address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "bind UDP tracker socket",
            source,
        })?;
    socket
        .connect(address)
        .await
        .map_err(|source| DownloadError::Io {
            operation: "connect UDP tracker socket",
            source,
        })?;

    let connection_id = match token_cache.get(address, Instant::now()) {
        Some(connection_id) => connection_id,
        None => {
            let connect_transaction = TransactionId::new(random_nonzero_u32()?);
            let connect_request = encode_connect_request(connect_transaction);
            let connection_id = exchange_udp_tracker_packet(
                &socket,
                &connect_request,
                "connect response",
                exchange.timing,
                exchange.control,
                exchange.tracker_label,
                |bytes| parse_connect_response(bytes, connect_transaction),
            )
            .await?;
            token_cache.insert(address, connection_id, Instant::now());
            connection_id
        }
    };

    let announce_transaction = TransactionId::new(random_nonzero_u32()?);
    let request = encode_announce_request(AnnounceRequest {
        connection_id,
        transaction_id: announce_transaction,
        info_hash: announce.info_hash,
        peer_id: announce.peer_id,
        downloaded: announce.downloaded,
        left: announce.left,
        uploaded: announce.uploaded,
        event: announce.event,
        ip_address: 0,
        key: announce.key,
        num_want: announce.num_want,
        port: announce.port,
    });
    let family = match address {
        SocketAddr::V4(_) => TrackerAddressFamily::Ipv4,
        SocketAddr::V6(_) => TrackerAddressFamily::Ipv6,
    };
    let result = exchange_udp_tracker_packet(
        &socket,
        &request,
        "announce response",
        exchange.timing,
        exchange.control,
        exchange.tracker_label,
        |bytes| parse_announce_response(bytes, announce_transaction, family),
    )
    .await;
    if result.is_err() {
        token_cache.remove(address);
    }
    result.map(|response| UdpTrackerAnnounceResult {
        response,
        connection_family: match family {
            TrackerAddressFamily::Ipv4 => TrackerConnectionFamily::Ipv4,
            TrackerAddressFamily::Ipv6 => TrackerConnectionFamily::Ipv6,
        },
    })
}

async fn send_udp_tracker_packet(
    socket: &UdpSocket,
    packet: &[u8],
    operation: &'static str,
    timeout_duration: Duration,
) -> Result<(), DownloadError> {
    let sent = socket.send(packet);
    let sent = timeout(timeout_duration, sent)
        .await
        .map_err(|_| DownloadError::NetworkTimedOut {
            operation,
            timeout: timeout_duration,
        })?
        .map_err(|source| DownloadError::Io { operation, source })?;
    if sent != packet.len() {
        return Err(DownloadError::Io {
            operation,
            source: io::Error::new(io::ErrorKind::WriteZero, "short UDP tracker send"),
        });
    }
    Ok(())
}

async fn exchange_udp_tracker_packet<T>(
    socket: &UdpSocket,
    packet: &[u8],
    operation: &'static str,
    timing: UdpTrackerTiming,
    control: &DownloadControl,
    tracker_label: &str,
    parse: impl Fn(&[u8]) -> Result<T, UdpTrackerError>,
) -> Result<T, DownloadError> {
    send_udp_tracker_packet(
        socket,
        packet,
        "send UDP tracker request",
        timing.completion_timeout,
    )
    .await?;
    control.record_bytes(ByteMetric::TrackerSent, packet.len());
    let started = TokioInstant::now();
    let retransmit_at = started + timing.retransmit_after;
    let deadline = started + timing.completion_timeout;
    let mut retransmitted = false;
    let mut buffer = [0; UDP_TRACKER_RECEIVE_LENGTH];
    loop {
        let next_deadline = if retransmitted {
            deadline
        } else {
            retransmit_at.min(deadline)
        };
        let received = match timeout_at(next_deadline, socket.recv(&mut buffer)).await {
            Ok(result) => result.map_err(|source| DownloadError::Io {
                operation: "receive UDP tracker response",
                source,
            })?,
            Err(_) if !retransmitted && next_deadline < deadline => {
                send_udp_tracker_packet(
                    socket,
                    packet,
                    "retransmit UDP tracker request",
                    timing.completion_timeout,
                )
                .await?;
                control.record_bytes(ByteMetric::TrackerSent, packet.len());
                retransmitted = true;
                control.emit(DownloadActivityEvent::TrackerUdpRetransmitted {
                    tracker: tracker_label.to_owned(),
                    operation,
                });
                continue;
            }
            Err(_) => {
                return Err(DownloadError::UdpTrackerTimedOut {
                    operation,
                    timeout: timing.completion_timeout,
                });
            }
        };
        control.record_bytes(ByteMetric::TrackerReceived, received);
        if received > MAX_ANNOUNCE_RESPONSE_LENGTH {
            return Err(DownloadError::UdpTrackerResponseTooLarge {
                maximum: MAX_ANNOUNCE_RESPONSE_LENGTH,
            });
        }
        if received < 8 {
            continue;
        }
        match parse(&buffer[..received]) {
            Err(UdpTrackerError::UnexpectedTransaction { .. }) => {}
            result => return result.map_err(DownloadError::UdpTracker),
        }
    }
}

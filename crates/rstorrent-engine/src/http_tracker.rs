use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use async_compression::tokio::bufread::GzipDecoder;
use base64::Engine as _;
use futures_util::{StreamExt, TryStreamExt};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{ACCEPT_ENCODING, AUTHORIZATION, CONTENT_ENCODING, CONTENT_LENGTH, LOCATION};
use rstorrent_protocol::bencode::{
    DictionaryEntry, Limits, Node, Value, parse_with_limits_permissive_dictionaries,
};
use rstorrent_protocol::magnet::{MAX_HOST_LENGTH, MAX_TRACKER_URL_LENGTH};
use rstorrent_protocol::udp_tracker::AnnounceEvent;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::net::lookup_host;

use crate::network::NetworkPolicy;
use crate::peer::PeerEndpoint;

pub(crate) const MAX_HTTP_TRACKER_TARGET_LENGTH: usize = 4 * 1024;
pub(crate) const MAX_HTTP_TRACKER_BODY_LENGTH: usize = 1024 * 1024;
pub(crate) const MAX_HTTP_TRACKER_PEERS: usize = 200;
pub(crate) const MAX_HTTP_TRACKER_HOSTNAMES: usize = 16;
pub(crate) const MAX_HTTP_TRACKER_CONTEXT_LENGTH: usize = 256;
pub(crate) const MAX_HTTP_TRACKER_REDIRECTS: usize = 5;
pub(crate) const MAX_HTTP_TRACKER_RESOLVED_ADDRESSES: usize = 16;
pub(crate) const MAX_HTTP_TRACKER_HOSTNAME_RESOLUTIONS: usize = 4;
pub(crate) const HTTP_TRACKER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const HTTP_TRACKER_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const DEFAULT_HTTP_TRACKER_INTERVAL: Duration = Duration::from_secs(30 * 60);
pub(crate) const MAX_HTTP_TRACKER_RETRY: Duration = Duration::from_secs(24 * 60 * 60);

const TRACKER_BENCODE_LIMITS: Limits = Limits {
    max_input_length: MAX_HTTP_TRACKER_BODY_LENGTH,
    max_string_length: MAX_HTTP_TRACKER_BODY_LENGTH,
    max_decoded_items: 4_096,
    max_depth: 8,
    max_collection_entries: 512,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpTrackerAnnounce {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: AnnounceEvent,
    pub key: u32,
    pub num_want: u32,
    pub tracker_id: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpBasicAuth {
    pub username: Vec<u8>,
    pub password: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpTrackerRequestTarget {
    pub url: String,
    pub auth: Option<HttpBasicAuth>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TrackerPeer {
    Address(SocketAddr),
    Hostname { host: String, port: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TrackerRetryDirective {
    After(Duration),
    Never,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HttpTrackerSuccess {
    pub interval: Duration,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub peers: Vec<TrackerPeer>,
    pub warning: Option<String>,
    pub tracker_id: Option<Vec<u8>>,
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HttpTrackerResponse {
    Success(HttpTrackerSuccess),
    Failure {
        reason: String,
        retry: Option<TrackerRetryDirective>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HttpTrackerError {
    InvalidUrl,
    UrlTooLong { length: usize, maximum: usize },
    RequestTargetTooLong { length: usize, maximum: usize },
    InvalidUserInfo,
    TrackerIdTooLong { length: usize, maximum: usize },
    InvalidBencode(String),
    RootNotDictionary,
    InvalidField(&'static str),
    MalformedPeers(&'static str),
    NetworkDisabled,
    ResolutionFailed,
    NoPermittedAddress,
    Client(String),
    Redirect(String),
    HttpStatus(u16),
    InvalidContentEncoding,
    EncodedBodyTooLong { maximum: usize },
    DecodedBodyTooLong { maximum: usize },
    InvalidCompression,
    Timeout,
}

impl fmt::Display for HttpTrackerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => write!(formatter, "invalid HTTP tracker URL"),
            Self::UrlTooLong { length, maximum } => {
                write!(formatter, "tracker URL length {length} exceeds {maximum}")
            }
            Self::RequestTargetTooLong { length, maximum } => write!(
                formatter,
                "tracker request target length {length} exceeds {maximum}"
            ),
            Self::InvalidUserInfo => write!(formatter, "invalid HTTP tracker URL userinfo"),
            Self::TrackerIdTooLong { length, maximum } => {
                write!(formatter, "tracker ID length {length} exceeds {maximum}")
            }
            Self::InvalidBencode(detail) => {
                write!(formatter, "invalid HTTP tracker bencode: {detail}")
            }
            Self::RootNotDictionary => {
                write!(formatter, "HTTP tracker response root is not a dictionary")
            }
            Self::InvalidField(field) => {
                write!(formatter, "HTTP tracker response has invalid {field}")
            }
            Self::MalformedPeers(field) => {
                write!(formatter, "HTTP tracker response has malformed {field}")
            }
            Self::NetworkDisabled => write!(formatter, "network policy is offline"),
            Self::ResolutionFailed => write!(formatter, "HTTP tracker name resolution failed"),
            Self::NoPermittedAddress => {
                write!(formatter, "HTTP tracker has no policy-permitted address")
            }
            Self::Client(detail) => write!(formatter, "HTTP tracker request failed: {detail}"),
            Self::Redirect(detail) => write!(formatter, "HTTP tracker redirect failed: {detail}"),
            Self::HttpStatus(status) => {
                write!(formatter, "HTTP tracker returned status {status}")
            }
            Self::InvalidContentEncoding => {
                write!(
                    formatter,
                    "HTTP tracker used an unsupported content encoding"
                )
            }
            Self::EncodedBodyTooLong { maximum } => {
                write!(
                    formatter,
                    "encoded HTTP tracker body exceeds {maximum} bytes"
                )
            }
            Self::DecodedBodyTooLong { maximum } => {
                write!(
                    formatter,
                    "decoded HTTP tracker body exceeds {maximum} bytes"
                )
            }
            Self::InvalidCompression => write!(formatter, "HTTP tracker gzip body is invalid"),
            Self::Timeout => write!(formatter, "HTTP tracker operation timed out"),
        }
    }
}

impl Error for HttpTrackerError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AddressFamily {
    Ipv4,
    Ipv6,
}

impl AddressFamily {
    fn matches(self, address: SocketAddr) -> bool {
        match self {
            Self::Ipv4 => address.is_ipv4(),
            Self::Ipv6 => address.is_ipv6(),
        }
    }
}

#[derive(Clone, Debug)]
struct TrackerResolver {
    policy: NetworkPolicy,
    family: AddressFamily,
}

impl Resolve for TrackerResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let policy = self.policy;
        let family = self.family;
        Box::pin(async move {
            let resolved = lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)?;
            let addresses = resolved
                .filter(|address| {
                    family.matches(*address) && policy.allows(SocketAddr::new(address.ip(), 1))
                })
                .take(MAX_HTTP_TRACKER_RESOLVED_ADDRESSES)
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    "no permitted tracker address in selected family",
                )) as Box<dyn Error + Send + Sync>);
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct HttpTrackerClients {
    ipv4: reqwest::Client,
    ipv6: reqwest::Client,
}

impl HttpTrackerClients {
    pub(crate) fn new(policy: NetworkPolicy) -> Result<Self, HttpTrackerError> {
        Ok(Self {
            ipv4: build_client(policy, AddressFamily::Ipv4)?,
            ipv6: build_client(policy, AddressFamily::Ipv6)?,
        })
    }

    fn get(&self, family: AddressFamily) -> &reqwest::Client {
        match family {
            AddressFamily::Ipv4 => &self.ipv4,
            AddressFamily::Ipv6 => &self.ipv6,
        }
    }
}

fn build_client(
    policy: NetworkPolicy,
    family: AddressFamily,
) -> Result<reqwest::Client, HttpTrackerError> {
    reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .connect_timeout(HTTP_TRACKER_CONNECT_TIMEOUT)
        .timeout(HTTP_TRACKER_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("RSTorrent/0.1")
        .tls_danger_accept_invalid_certs(true)
        .tls_danger_accept_invalid_hostnames(true)
        .dns_resolver(TrackerResolver { policy, family })
        .build()
        .map_err(redacted_reqwest_error)
}

pub(crate) async fn announce_http_tracker(
    clients: &HttpTrackerClients,
    base_url: &str,
    policy: NetworkPolicy,
    prefer_ipv6: bool,
    announce: &HttpTrackerAnnounce,
    timeout: Duration,
) -> Result<HttpTrackerResponse, HttpTrackerError> {
    if policy == NetworkPolicy::Offline {
        return Err(HttpTrackerError::NetworkDisabled);
    }
    let _ = build_announce_target(base_url, announce)?;
    tokio::time::timeout(
        timeout,
        announce_http_tracker_inner(clients, base_url, announce, policy, prefer_ipv6),
    )
    .await
    .map_err(|_| HttpTrackerError::Timeout)?
}

async fn announce_http_tracker_inner(
    clients: &HttpTrackerClients,
    base_url: &str,
    announce: &HttpTrackerAnnounce,
    policy: NetworkPolicy,
    prefer_ipv6: bool,
) -> Result<HttpTrackerResponse, HttpTrackerError> {
    let base = url::Url::parse(base_url).map_err(|_| HttpTrackerError::InvalidUrl)?;
    let families = resolve_url_families(&base, policy, prefer_ipv6).await?;
    let mut last_error = None;
    for family in families {
        let mut family_announce = announce.clone();
        if family == AddressFamily::Ipv6 {
            family_announce.port = 1;
        }
        let target = build_announce_target(base_url, &family_announce)?;
        let url = url::Url::parse(&target.url).map_err(|_| HttpTrackerError::InvalidUrl)?;
        match request_family(clients.get(family), url, target.auth, policy, family).await {
            Ok(HttpTrackerResponse::Success(mut success)) => {
                success.peers = resolve_peer_addresses(success.peers, policy).await;
                return Ok(HttpTrackerResponse::Success(success));
            }
            Ok(response) => return Ok(response),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or(HttpTrackerError::NoPermittedAddress))
}

async fn resolve_url_families(
    url: &url::Url,
    policy: NetworkPolicy,
    prefer_ipv6: bool,
) -> Result<Vec<AddressFamily>, HttpTrackerError> {
    let host = url.host().ok_or(HttpTrackerError::InvalidUrl)?;
    let port = url
        .port_or_known_default()
        .ok_or(HttpTrackerError::InvalidUrl)?;
    let mut has_v4 = false;
    let mut has_v6 = false;
    match host {
        url::Host::Ipv4(address) => {
            has_v4 = policy.allows(SocketAddr::new(IpAddr::V4(address), port));
        }
        url::Host::Ipv6(address) => {
            has_v6 = policy.allows(SocketAddr::new(IpAddr::V6(address), port));
        }
        url::Host::Domain(host) => {
            let resolved = lookup_host((host, port))
                .await
                .map_err(|_| HttpTrackerError::ResolutionFailed)?;
            let mut v4 = 0_usize;
            let mut v6 = 0_usize;
            for address in resolved.filter(|address| policy.allows(*address)) {
                if address.is_ipv4() && v4 < MAX_HTTP_TRACKER_RESOLVED_ADDRESSES {
                    has_v4 = true;
                    v4 += 1;
                } else if address.is_ipv6() && v6 < MAX_HTTP_TRACKER_RESOLVED_ADDRESSES {
                    has_v6 = true;
                    v6 += 1;
                }
            }
        }
    }
    let mut families = Vec::with_capacity(2);
    for family in if prefer_ipv6 {
        [AddressFamily::Ipv6, AddressFamily::Ipv4]
    } else {
        [AddressFamily::Ipv4, AddressFamily::Ipv6]
    } {
        if matches!(family, AddressFamily::Ipv4) && has_v4
            || matches!(family, AddressFamily::Ipv6) && has_v6
        {
            families.push(family);
        }
    }
    if families.is_empty() {
        return Err(HttpTrackerError::NoPermittedAddress);
    }
    Ok(families)
}

async fn request_family(
    client: &reqwest::Client,
    mut url: url::Url,
    mut auth: Option<HttpBasicAuth>,
    policy: NetworkPolicy,
    family: AddressFamily,
) -> Result<HttpTrackerResponse, HttpTrackerError> {
    let mut visited = HashSet::new();
    for redirect_count in 0..=MAX_HTTP_TRACKER_REDIRECTS {
        if !visited.insert(url.to_string()) {
            return Err(HttpTrackerError::Redirect("redirect loop".to_owned()));
        }
        ensure_url_family(&url, policy, family).await?;
        let mut request = client.get(url.clone()).header(ACCEPT_ENCODING, "gzip");
        if let Some(auth) = auth.as_ref() {
            let mut credentials = Vec::with_capacity(auth.username.len() + auth.password.len() + 1);
            credentials.extend_from_slice(&auth.username);
            credentials.push(b':');
            credentials.extend_from_slice(&auth.password);
            let value = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(credentials)
            );
            request = request.header(AUTHORIZATION, value);
        }
        let response = request.send().await.map_err(redacted_reqwest_error)?;
        if let Some(remote) = response.remote_addr()
            && (!family.matches(remote) || !policy.allows(remote))
        {
            return Err(HttpTrackerError::NoPermittedAddress);
        }
        if response.status().is_redirection() {
            if redirect_count == MAX_HTTP_TRACKER_REDIRECTS {
                return Err(HttpTrackerError::Redirect(
                    "redirect limit exceeded".to_owned(),
                ));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .ok_or_else(|| HttpTrackerError::Redirect("missing Location header".to_owned()))?
                .to_str()
                .map_err(|_| HttpTrackerError::Redirect("invalid Location header".to_owned()))?;
            let next = url
                .join(location)
                .map_err(|_| HttpTrackerError::Redirect("invalid redirect target".to_owned()))?;
            validate_redirect(&url, &next)?;
            if !same_origin(&url, &next) {
                auth = None;
            }
            url = next;
            continue;
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(HttpTrackerError::HttpStatus(response.status().as_u16()));
        }
        let encoding = response_content_encoding(&response)?;
        let body = read_response_body(response).await?;
        let decoded = decode_response_body(body, encoding).await?;
        return parse_tracker_response(&decoded);
    }
    Err(HttpTrackerError::Redirect(
        "redirect limit exceeded".to_owned(),
    ))
}

async fn ensure_url_family(
    url: &url::Url,
    policy: NetworkPolicy,
    family: AddressFamily,
) -> Result<(), HttpTrackerError> {
    let families = resolve_url_families(url, policy, family == AddressFamily::Ipv6).await?;
    if families.contains(&family) {
        Ok(())
    } else {
        Err(HttpTrackerError::Redirect(
            "redirect changed address family".to_owned(),
        ))
    }
}

fn validate_redirect(previous: &url::Url, next: &url::Url) -> Result<(), HttpTrackerError> {
    match (previous.scheme(), next.scheme()) {
        ("http", "http" | "https") | ("https", "https") => {}
        ("https", "http") => {
            return Err(HttpTrackerError::Redirect(
                "HTTPS downgrade is forbidden".to_owned(),
            ));
        }
        _ => {
            return Err(HttpTrackerError::Redirect(
                "unsupported redirect scheme".to_owned(),
            ));
        }
    }
    if next.host().is_none()
        || next.fragment().is_some()
        || !next.username().is_empty()
        || next.password().is_some()
    {
        return Err(HttpTrackerError::Redirect(
            "invalid redirect target".to_owned(),
        ));
    }
    Ok(())
}

fn same_origin(first: &url::Url, second: &url::Url) -> bool {
    first.scheme() == second.scheme()
        && first.host() == second.host()
        && first.port_or_known_default() == second.port_or_known_default()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContentEncoding {
    Identity,
    Gzip,
}

fn response_content_encoding(
    response: &reqwest::Response,
) -> Result<ContentEncoding, HttpTrackerError> {
    let values = response.headers().get_all(CONTENT_ENCODING);
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Ok(ContentEncoding::Identity);
    };
    if values.next().is_some() {
        return Err(HttpTrackerError::InvalidContentEncoding);
    }
    let value = value
        .to_str()
        .map_err(|_| HttpTrackerError::InvalidContentEncoding)?
        .trim();
    if value.eq_ignore_ascii_case("identity") {
        Ok(ContentEncoding::Identity)
    } else if value.eq_ignore_ascii_case("gzip") || value.eq_ignore_ascii_case("x-gzip") {
        Ok(ContentEncoding::Gzip)
    } else {
        Err(HttpTrackerError::InvalidContentEncoding)
    }
}

async fn read_response_body(response: reqwest::Response) -> Result<Vec<u8>, HttpTrackerError> {
    if response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_HTTP_TRACKER_BODY_LENGTH as u64)
    {
        return Err(HttpTrackerError::EncodedBodyTooLong {
            maximum: MAX_HTTP_TRACKER_BODY_LENGTH,
        });
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.try_next().await.map_err(redacted_reqwest_error)? {
        let remaining = (MAX_HTTP_TRACKER_BODY_LENGTH + 1).saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if body.len() > MAX_HTTP_TRACKER_BODY_LENGTH || chunk.len() > remaining {
            return Err(HttpTrackerError::EncodedBodyTooLong {
                maximum: MAX_HTTP_TRACKER_BODY_LENGTH,
            });
        }
    }
    Ok(body)
}

async fn decode_response_body(
    body: Vec<u8>,
    encoding: ContentEncoding,
) -> Result<Vec<u8>, HttpTrackerError> {
    if encoding == ContentEncoding::Identity {
        return Ok(body);
    }
    let reader = BufReader::new(body.as_slice());
    let decoder = GzipDecoder::new(reader);
    let mut limited = decoder.take((MAX_HTTP_TRACKER_BODY_LENGTH + 1) as u64);
    let mut decoded = Vec::new();
    limited
        .read_to_end(&mut decoded)
        .await
        .map_err(|_| HttpTrackerError::InvalidCompression)?;
    if decoded.len() > MAX_HTTP_TRACKER_BODY_LENGTH {
        return Err(HttpTrackerError::DecodedBodyTooLong {
            maximum: MAX_HTTP_TRACKER_BODY_LENGTH,
        });
    }
    let decoder = limited.into_inner();
    let reader = decoder.into_inner();
    if !reader.buffer().is_empty() || !reader.get_ref().is_empty() {
        return Err(HttpTrackerError::InvalidCompression);
    }
    Ok(decoded)
}

async fn resolve_peer_addresses(
    peers: Vec<TrackerPeer>,
    policy: NetworkPolicy,
) -> Vec<TrackerPeer> {
    let hostnames = peers
        .iter()
        .filter_map(|peer| match peer {
            TrackerPeer::Hostname { host, port } => Some((host.clone(), *port)),
            TrackerPeer::Address(_) => None,
        })
        .collect::<Vec<_>>();
    let hostname_requests = futures_util::stream::iter(hostnames)
        .map(|(host, port)| async move {
            let addresses = lookup_host((host.as_str(), port))
                .await
                .map(|resolved| {
                    resolved
                        .filter(|address| policy.allows(*address))
                        .take(MAX_HTTP_TRACKER_RESOLVED_ADDRESSES * 2)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            ((host, port), addresses)
        })
        .buffered(MAX_HTTP_TRACKER_HOSTNAME_RESOLUTIONS)
        .collect::<Vec<_>>()
        .await;
    let resolved = hostname_requests
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for peer in peers {
        let addresses = match peer {
            TrackerPeer::Address(address) => vec![address],
            TrackerPeer::Hostname { host, port } => {
                resolved.get(&(host, port)).cloned().unwrap_or_default()
            }
        };
        for address in addresses {
            if output.len() == MAX_HTTP_TRACKER_PEERS {
                return output;
            }
            if policy.allows(address) && seen.insert(address) {
                output.push(TrackerPeer::Address(address));
            }
        }
    }
    output
}

fn redacted_reqwest_error(error: reqwest::Error) -> HttpTrackerError {
    HttpTrackerError::Client(error.without_url().to_string())
}

pub(crate) fn build_announce_target(
    base: &str,
    announce: &HttpTrackerAnnounce,
) -> Result<HttpTrackerRequestTarget, HttpTrackerError> {
    if base.len() > MAX_TRACKER_URL_LENGTH {
        return Err(HttpTrackerError::UrlTooLong {
            length: base.len(),
            maximum: MAX_TRACKER_URL_LENGTH,
        });
    }
    if base.is_empty()
        || !base.is_ascii()
        || base
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || base.contains('#')
    {
        return Err(HttpTrackerError::InvalidUrl);
    }
    let (_, remainder) = base.split_once("://").ok_or(HttpTrackerError::InvalidUrl)?;
    let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
    let host_port = remainder[..authority_end]
        .rsplit_once('@')
        .map_or(&remainder[..authority_end], |(_, host_port)| host_port);
    if host_port.is_empty() {
        return Err(HttpTrackerError::InvalidUrl);
    }
    let parsed = url::Url::parse(base).map_err(|_| HttpTrackerError::InvalidUrl)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || parsed.fragment().is_some()
    {
        return Err(HttpTrackerError::InvalidUrl);
    }
    let (mut target, auth) = strip_userinfo(base)?;
    if !target.contains('?') {
        target.push('?');
    } else if !target.ends_with(['?', '&']) {
        target.push('&');
    }
    append_parameter(&mut target, "info_hash", &announce.info_hash);
    append_parameter(&mut target, "peer_id", &announce.peer_id);
    append_decimal(&mut target, "port", u64::from(announce.port));
    append_decimal(&mut target, "uploaded", announce.uploaded);
    append_decimal(&mut target, "downloaded", announce.downloaded);
    append_decimal(&mut target, "left", announce.left);
    target.push_str("&compact=1&no_peer_id=1");
    append_decimal(&mut target, "key", u64::from(announce.key));
    append_decimal(&mut target, "numwant", u64::from(announce.num_want));
    match announce.event {
        AnnounceEvent::None => {}
        AnnounceEvent::Completed => target.push_str("&event=completed"),
        AnnounceEvent::Started => target.push_str("&event=started"),
        AnnounceEvent::Stopped => target.push_str("&event=stopped"),
    }
    if let Some(tracker_id) = announce.tracker_id.as_deref() {
        if tracker_id.len() > MAX_HTTP_TRACKER_CONTEXT_LENGTH {
            return Err(HttpTrackerError::TrackerIdTooLong {
                length: tracker_id.len(),
                maximum: MAX_HTTP_TRACKER_CONTEXT_LENGTH,
            });
        }
        append_parameter(&mut target, "trackerid", tracker_id);
    }
    if target.len() > MAX_HTTP_TRACKER_TARGET_LENGTH {
        return Err(HttpTrackerError::RequestTargetTooLong {
            length: target.len(),
            maximum: MAX_HTTP_TRACKER_TARGET_LENGTH,
        });
    }
    Ok(HttpTrackerRequestTarget { url: target, auth })
}

fn strip_userinfo(base: &str) -> Result<(String, Option<HttpBasicAuth>), HttpTrackerError> {
    let (_, after_scheme) = base.split_once("://").ok_or(HttpTrackerError::InvalidUrl)?;
    let authority_end = after_scheme.find(['/', '?']).unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    let Some(at) = authority.rfind('@') else {
        return Ok((base.to_owned(), None));
    };
    let userinfo = &authority[..at];
    let (username, password) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    let username = decode_userinfo(username)?;
    let password = decode_userinfo(password)?;
    if username.len() > MAX_HTTP_TRACKER_CONTEXT_LENGTH
        || password.len() > MAX_HTTP_TRACKER_CONTEXT_LENGTH
        || username
            .iter()
            .chain(&password)
            .any(|byte| byte.is_ascii_control())
    {
        return Err(HttpTrackerError::InvalidUserInfo);
    }
    let scheme_end = base
        .find("://")
        .expect("validated URL retains its scheme separator")
        + 3;
    let mut target = String::with_capacity(base.len());
    target.push_str(&base[..scheme_end]);
    target.push_str(&authority[at + 1..]);
    target.push_str(&after_scheme[authority_end..]);
    Ok((target, Some(HttpBasicAuth { username, password })))
}

fn decode_userinfo(value: &str) -> Result<Vec<u8>, HttpTrackerError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        if bytes[position] == b'%' {
            let high = bytes
                .get(position + 1)
                .copied()
                .and_then(hex_value)
                .ok_or(HttpTrackerError::InvalidUserInfo)?;
            let low = bytes
                .get(position + 2)
                .copied()
                .and_then(hex_value)
                .ok_or(HttpTrackerError::InvalidUserInfo)?;
            decoded.push((high << 4) | low);
            position += 3;
        } else {
            decoded.push(bytes[position]);
            position += 1;
        }
    }
    Ok(decoded)
}

fn append_parameter(target: &mut String, name: &str, value: &[u8]) {
    if !target.ends_with(['?', '&']) {
        target.push('&');
    }
    target.push_str(name);
    target.push('=');
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in value {
        target.push('%');
        target.push(char::from(HEX[usize::from(byte >> 4)]));
        target.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn append_decimal(target: &mut String, name: &str, value: u64) {
    target.push('&');
    target.push_str(name);
    target.push('=');
    target.push_str(&value.to_string());
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_tracker_response(body: &[u8]) -> Result<HttpTrackerResponse, HttpTrackerError> {
    let root = parse_with_limits_permissive_dictionaries(body, TRACKER_BENCODE_LIMITS)
        .map_err(|error| HttpTrackerError::InvalidBencode(error.to_string()))?;
    let Value::Dictionary(entries) = root.value else {
        return Err(HttpTrackerError::RootNotDictionary);
    };
    if let Some(reason) = field(&entries, b"failure reason") {
        let Value::Bytes(reason) = reason.value else {
            return Err(HttpTrackerError::InvalidField("failure reason"));
        };
        return Ok(HttpTrackerResponse::Failure {
            reason: bounded_lossy(reason),
            retry: field(&entries, b"retry in").and_then(parse_retry_directive),
        });
    }

    let interval = parse_interval(field(&entries, b"interval"), DEFAULT_HTTP_TRACKER_INTERVAL)?;
    let min_interval = parse_interval(field(&entries, b"min interval"), Duration::ZERO)?;
    let interval = interval.max(min_interval);
    let seeders = optional_u32(field(&entries, b"complete"));
    let leechers = optional_u32(field(&entries, b"incomplete"));
    let warning = optional_bounded_bytes(field(&entries, b"warning message"));
    let mut diagnostics = Vec::new();
    let tracker_id = match field(&entries, b"tracker id") {
        Some(Node {
            value: Value::Bytes(value),
            ..
        }) if value.len() <= MAX_HTTP_TRACKER_CONTEXT_LENGTH => Some(value.to_vec()),
        Some(Node {
            value: Value::Bytes(_),
            ..
        }) => {
            diagnostics.push("tracker ID exceeded 256 bytes and was ignored".to_owned());
            None
        }
        Some(_) => {
            diagnostics.push("non-string tracker ID was ignored".to_owned());
            None
        }
        None => None,
    };
    let mut peers = Vec::new();
    let mut addresses = HashSet::new();
    let mut hostnames = HashSet::new();
    if let Some(value) = field(&entries, b"peers") {
        match &value.value {
            Value::Bytes(compact) => parse_compact_peers(
                compact,
                6,
                "peers",
                &mut peers,
                &mut addresses,
                &mut diagnostics,
            )?,
            Value::List(entries) => parse_noncompact_peers(
                entries,
                &mut peers,
                &mut addresses,
                &mut hostnames,
                &mut diagnostics,
            )?,
            _ => return Err(HttpTrackerError::MalformedPeers("peers")),
        }
    }
    if let Some(value) = field(&entries, b"peers6") {
        let Value::Bytes(compact) = &value.value else {
            return Err(HttpTrackerError::MalformedPeers("peers6"));
        };
        parse_compact_peers(
            compact,
            18,
            "peers6",
            &mut peers,
            &mut addresses,
            &mut diagnostics,
        )?;
    }
    Ok(HttpTrackerResponse::Success(HttpTrackerSuccess {
        interval,
        seeders,
        leechers,
        peers,
        warning,
        tracker_id,
        diagnostics,
    }))
}

fn field<'a>(entries: &'a [DictionaryEntry<'a>], key: &[u8]) -> Option<&'a Node<'a>> {
    entries
        .binary_search_by(|entry| entry.key.cmp(key))
        .ok()
        .map(|index| &entries[index].value)
}

fn parse_interval(
    node: Option<&Node<'_>>,
    default: Duration,
) -> Result<Duration, HttpTrackerError> {
    let Some(node) = node else {
        return Ok(default);
    };
    let Value::Integer(value) = node.value else {
        return Err(HttpTrackerError::InvalidField("interval"));
    };
    let seconds = u64::try_from(value).map_err(|_| HttpTrackerError::InvalidField("interval"))?;
    Ok(Duration::from_secs(seconds))
}

fn optional_u32(node: Option<&Node<'_>>) -> Option<u32> {
    match node.map(|node| &node.value) {
        Some(Value::Integer(value)) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn optional_bounded_bytes(node: Option<&Node<'_>>) -> Option<String> {
    match node.map(|node| &node.value) {
        Some(Value::Bytes(value)) => Some(bounded_lossy(value)),
        _ => None,
    }
}

fn bounded_lossy(value: &[u8]) -> String {
    String::from_utf8_lossy(&value[..value.len().min(MAX_HTTP_TRACKER_CONTEXT_LENGTH)]).into_owned()
}

fn parse_retry_directive(node: &Node<'_>) -> Option<TrackerRetryDirective> {
    let minutes = match &node.value {
        Value::Bytes(b"never") => return Some(TrackerRetryDirective::Never),
        Value::Bytes(value) => parse_positive_decimal(value)?,
        Value::Integer(value) => u64::try_from(*value).ok().filter(|value| *value != 0)?,
        _ => return None,
    };
    Some(TrackerRetryDirective::After(
        Duration::from_secs(minutes.saturating_mul(60)).min(MAX_HTTP_TRACKER_RETRY),
    ))
}

fn parse_positive_decimal(value: &[u8]) -> Option<u64> {
    if value.is_empty() || value.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let mut parsed = 0_u64;
    for digit in value {
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(u64::from(digit - b'0'));
    }
    (parsed != 0).then_some(parsed)
}

fn parse_compact_peers(
    compact: &[u8],
    stride: usize,
    field: &'static str,
    peers: &mut Vec<TrackerPeer>,
    addresses: &mut HashSet<SocketAddr>,
    diagnostics: &mut Vec<String>,
) -> Result<(), HttpTrackerError> {
    if !compact.is_empty() && compact.len() < stride {
        return Err(HttpTrackerError::MalformedPeers(field));
    }
    if compact.len() % stride != 0 {
        diagnostics.push(format!("{field} had a short trailing compact suffix"));
    }
    for entry in compact.chunks_exact(stride) {
        if peers.len() == MAX_HTTP_TRACKER_PEERS {
            break;
        }
        let address = if stride == 6 {
            SocketAddr::from((
                Ipv4Addr::new(entry[0], entry[1], entry[2], entry[3]),
                port(&entry[4..]),
            ))
        } else {
            let mut octets = [0; 16];
            octets.copy_from_slice(&entry[..16]);
            SocketAddr::from((Ipv6Addr::from(octets), port(&entry[16..])))
        };
        push_address(address, peers, addresses);
    }
    Ok(())
}

fn parse_noncompact_peers(
    entries: &[Node<'_>],
    peers: &mut Vec<TrackerPeer>,
    addresses: &mut HashSet<SocketAddr>,
    hostnames: &mut HashSet<(String, u16)>,
    diagnostics: &mut Vec<String>,
) -> Result<(), HttpTrackerError> {
    let mut structurally_valid = 0_usize;
    for entry in entries {
        let Value::Dictionary(fields) = &entry.value else {
            continue;
        };
        let Some(Node {
            value: Value::Bytes(ip),
            ..
        }) = field(fields, b"ip")
        else {
            continue;
        };
        let Some(Node {
            value: Value::Integer(port),
            ..
        }) = field(fields, b"port")
        else {
            continue;
        };
        let Ok(port) = u16::try_from(*port) else {
            continue;
        };
        if port == 0 {
            continue;
        }
        let Ok(ip) = std::str::from_utf8(ip) else {
            continue;
        };
        if let Ok(ip) = ip.parse::<IpAddr>() {
            structurally_valid += 1;
            if peers.len() < MAX_HTTP_TRACKER_PEERS {
                push_address(SocketAddr::new(ip, port), peers, addresses);
            }
            continue;
        }
        let Some(host) = normalize_hostname(ip) else {
            continue;
        };
        structurally_valid += 1;
        if hostnames.len() == MAX_HTTP_TRACKER_HOSTNAMES || peers.len() == MAX_HTTP_TRACKER_PEERS {
            diagnostics.push("noncompact peer hostname limit reached".to_owned());
            continue;
        }
        if hostnames.insert((host.clone(), port)) {
            peers.push(TrackerPeer::Hostname { host, port });
        }
    }
    if !entries.is_empty() && structurally_valid == 0 {
        return Err(HttpTrackerError::MalformedPeers("peers"));
    }
    Ok(())
}

fn push_address(
    address: SocketAddr,
    peers: &mut Vec<TrackerPeer>,
    addresses: &mut HashSet<SocketAddr>,
) {
    if PeerEndpoint::new(address).is_ok() && addresses.insert(address) {
        peers.push(TrackerPeer::Address(address));
    }
}

fn port(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn normalize_hostname(host: &str) -> Option<String> {
    if host.is_empty()
        || host.len() > MAX_HOST_LENGTH
        || !host.is_ascii()
        || host.ends_with('.')
        || host
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    for label in host.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return None;
        }
    }
    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt as _;
    use tokio::net::{TcpListener, TcpStream};

    async fn read_request(stream: &mut TcpStream) -> String {
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1024];
            let length = stream.read(&mut chunk).await.expect("read request");
            assert_ne!(length, 0, "request ended before headers");
            request.extend_from_slice(&chunk[..length]);
            assert!(request.len() <= 16 * 1024, "request headers are bounded");
        }
        String::from_utf8(request).expect("ASCII request")
    }

    async fn serve_once(listener: TcpListener, response: Vec<u8>) -> String {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let request = read_request(&mut stream).await;
        stream.write_all(&response).await.expect("write response");
        stream.shutdown().await.expect("close response");
        request
    }

    async fn serve_tls_once(listener: TcpListener, response: Vec<u8>) -> String {
        use tokio_rustls::TlsAcceptor;
        use tokio_rustls::rustls::ServerConfig;
        use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

        const CERT: &str = concat!(
            "MIIDETCCAfmgAwIBAgIUaGe1xp9QWaOq2j9MBVoYJQHxQjUwDQYJKoZIhvcNAQELBQAwGDEW",
            "MBQGA1UEAwwNd3JvbmcuZXhhbXBsZTAeFw0yNjA4MDUxODMxMDNaFw0zNjA4MDIxODMxMDNa",
            "MBgxFjAUBgNVBAMMDXdyb25nLmV4YW1wbGUwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEK",
            "AoIBAQDfQV9T9lOopJtynnl+v0lzgusiA5zeYE20tSlD04EtdJ8SakjrEC1cbfGQN9azRSNA",
            "oVKdazKOQpGib+cwbXd4snw/CE2qKVZ6j5grp8QZvSqm8gjHwp3WwlfVbmOJPXLsSjvCU36j",
            "qI5s5VWfWiPAxDYklfUn4hz4aR5oabP+poMgsXxq411UEclqr+s6fv1TVPO95hT9CeTyNXtN",
            "1T1Wq1tVMLm3ULI7oGVLhdZJGEyLLdricnIda+YOZEeoUslfzuV3rQMQJyDRWdajNdpbPJL/",
            "nBzVTAaGpe+O+JIj0hP1jdDNzdODTJ6b0JfrngB9mDYKGw4tnaLz8i0Tcpc/AgMBAAGjUzBR",
            "MB0GA1UdDgQWBBQ5BQU9ptEzvoQqvmi6fnGKh3GdMDAfBgNVHSMEGDAWgBQ5BQU9ptEzvoQq",
            "vmi6fnGKh3GdMDAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQBfpwTOEGer",
            "XsgIPC5kVDTdQI0d5XmVuAbxCWQZCsRbrn65hW3E+uaWr2W7SMUAczYpWi3W/Q+YWXz/F19",
            "3IBVLUL7zohyQP06vYIsQUAgKpWDwPMHTtzLKAy2wtn50Y83CR/FMEXQpSFsFxZEO7rupEAL",
            "E3oA/jzaApcIuqMOJbGFcrsuRy3HVS6g+T1wabFZGts9XdXmHoXc6zXL8fxUVvdI4Sdl39co",
            "nd7TayWPpdy+pD//z0qsoTkHEuKwd9dgiIU2bg7kh7AilXQM9ncze4IxkZyQfM87YAb3bMG",
            "Nwrd/DjbA2yWPWUmGV4+laMESgYDrXRFemNrNgif1v4eM5"
        );
        const KEY: &str = concat!(
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDfQV9T9lOopJtynnl+v0lz",
            "gusiA5zeYE20tSlD04EtdJ8SakjrEC1cbfGQN9azRSNAoVKdazKOQpGib+cwbXd4snw/CE2q",
            "KVZ6j5grp8QZvSqm8gjHwp3WwlfVbmOJPXLsSjvCU36jqI5s5VWfWiPAxDYklfUn4hz4aR5",
            "oabP+poMgsXxq411UEclqr+s6fv1TVPO95hT9CeTyNXtN1T1Wq1tVMLm3ULI7oGVLhdZJGE",
            "yLLdricnIda+YOZEeoUslfzuV3rQMQJyDRWdajNdpbPJL/nBzVTAaGpe+O+JIj0hP1jdDNzd",
            "ODTJ6b0JfrngB9mDYKGw4tnaLz8i0Tcpc/AgMBAAECggEASQLPgp1diZrfbVIPSJiVFE4d",
            "yF9nF0BmWTEfwBs0tSFc/kA8/YaqVv5rj+7662CyYSoA4xNSEr0JdJZlBGzgM9wnDtQP1hSz",
            "v9wi9y/jzUkUYElp/q4SQVAIOnfh3Fl4snaqaWg1057FiS5M3JK1e46PaFKUPIlRURnLhHkB",
            "EMdWNcCozrzihkLoJ5rTBQSGdmMFThGoF5MaK0MN4AxU1o0rWbI8GVnua4Cm4FuR3Eaqqsb",
            "b0mUl1JWykLo7FoO60tWCiD6mv5bhKNtkFpMMHBSWj9W9jX0n5pprsnfSYh0I8WHQKRBwqF",
            "KAxz3RDpo7LKS9RM1eqmos4FsW+L68IQKBgQD6TB/LeQa6kk45T5lJ6coABm1x66bves1L/",
            "8/PSE8XV3gBobSs4jG00bd9Mh1ulmEzd1oey62F5Dky/z4o6Fs/uj03dZjynsc0I9D3oykKB",
            "w7XkKdb7F5UYCVxAR818/bKZM1gbWqkWoo6pRA9bj8pNzvrRhbb8Rj959ZJtxOf4QKBgQDk",
            "V4Yg0u3pCtvlEp0x8+v4WbiQIrqgaJ6UwAAwpt4XS2yW6YU2x813McUXsqf3CiYCUurhxtZP",
            "q50llvXFZXKpzO8fekR5q7vgSFu9UrplCsDqkTcWqyGaGNuFBfr8TzSyc2zKkhrpWLn36sy/",
            "jxMgprk+uaQV2+CVdyHBQuWbHwKBgBLugRUl0VF5UXtaPvDtQv8ffVW5ikXg1vhhn/lAseL",
            "FFemhroXJEhNoLWXFzZ4Yt79pzqI3q6dN7NmjnrL/aC94ybqRJYFsawrRjrO8XpVIlWHOqi",
            "n0xenB3/MdL5woGMmUOEiL3h4STxRCeej7lsFqURjpkz8NjGNgDsBCnbRhAoGBALA0bje0LY",
            "0xKQE7fPyIO2bZbZgkhJm2QfGNvFfO3QFi3bgTGg5s3rwFNw+TeRQky7HtZH2337d5OfpA5",
            "QVfxL0NfNVwl5jAkml/zPNq/JVuV/Jq/vTKOFLerb+YHtdHE+ZFNgWX+5ZoNpH+qeOEuADx",
            "R3AE9386vrL4TJ8DTYWH AoGAb7l64MdxbuMZnzRpYMOg4n8aUQOD9QxX9WKtVwbcCfBskW",
            "p44v/NxFnLaJjqdg9zioAL8hL1PO1z0ya5mvAYkCV6ti80T8JZi144iV7yaciKXorCcvNqX",
            "ljY3cEmel5PhjJTLCn1+cVe/seZJwrrd5zP5Jlb+jvZxUQIzUmkAXw="
        );
        let key = KEY.replace(' ', "");
        let cert = base64::engine::general_purpose::STANDARD
            .decode(CERT)
            .expect("certificate DER");
        let key = base64::engine::general_purpose::STANDARD
            .decode(key)
            .expect("key DER");
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert)],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .expect("TLS server config");
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let (stream, _) = listener.accept().await.expect("accept TLS request");
        let mut stream = acceptor.accept(stream).await.expect("TLS handshake");
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1024];
            let length = stream.read(&mut chunk).await.expect("read TLS request");
            assert_ne!(length, 0, "TLS request ended before headers");
            request.extend_from_slice(&chunk[..length]);
        }
        stream
            .write_all(&response)
            .await
            .expect("write TLS response");
        stream.shutdown().await.expect("close TLS response");
        String::from_utf8(request).expect("ASCII TLS request")
    }

    fn http_response(headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len());
        for (name, value) in headers {
            response.push_str(name);
            response.push_str(": ");
            response.push_str(value);
            response.push_str("\r\n");
        }
        response.push_str("Connection: close\r\n\r\n");
        let mut response = response.into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn announce(event: AnnounceEvent) -> HttpTrackerAnnounce {
        HttpTrackerAnnounce {
            info_hash: std::array::from_fn(|index| u8::try_from(index).expect("hash byte")),
            peer_id: std::array::from_fn(|index| u8::try_from(index + 20).expect("peer ID byte")),
            port: 1,
            uploaded: 2,
            downloaded: 3,
            left: 4,
            event,
            key: 5,
            num_want: 200,
            tracker_id: None,
        }
    }

    fn bytes(value: &[u8]) -> Vec<u8> {
        let mut encoded = value.len().to_string().into_bytes();
        encoded.push(b':');
        encoded.extend_from_slice(value);
        encoded
    }

    fn dictionary(fields: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut encoded = vec![b'd'];
        for (key, value) in fields {
            encoded.extend(bytes(key));
            encoded.extend(value);
        }
        encoded.push(b'e');
        encoded
    }

    #[test]
    fn request_preserves_query_encodes_binary_and_extracts_basic_auth() {
        let mut request = announce(AnnounceEvent::Started);
        request.tracker_id = Some(vec![0, b'&', 255]);
        let target = build_announce_target(
            "https://us%65r:p%40ss@[::1]:8443/announce?pass=abc&x=1",
            &request,
        )
        .expect("request target");

        assert_eq!(
            target.auth,
            Some(HttpBasicAuth {
                username: b"user".to_vec(),
                password: b"p@ss".to_vec(),
            })
        );
        assert!(!target.url.contains("user"));
        assert!(target.url.starts_with(
            "https://[::1]:8443/announce?pass=abc&x=1&info_hash=%00%01%02%03%04%05%06%07%08%09%0A%0B%0C%0D%0E%0F%10%11%12%13"
        ));
        assert!(
            target
                .url
                .contains("&peer_id=%14%15%16%17%18%19%1A%1B%1C%1D%1E%1F%20%21%22%23%24%25%26%27")
        );
        assert!(
            target
                .url
                .contains("&port=1&uploaded=2&downloaded=3&left=4")
        );
        assert!(
            target
                .url
                .contains("&compact=1&no_peer_id=1&key=5&numwant=200")
        );
        assert!(target.url.contains("&event=started"));
        assert!(target.url.ends_with("&trackerid=%00%26%FF"));
    }

    #[test]
    fn request_handles_empty_queries_and_omits_update_event() {
        for (base, expected) in [
            (
                "http://tracker/announce",
                "http://tracker/announce?info_hash=",
            ),
            (
                "http://tracker/announce?",
                "http://tracker/announce?info_hash=",
            ),
            (
                "http://tracker/announce?a=1&",
                "http://tracker/announce?a=1&info_hash=",
            ),
        ] {
            let target = build_announce_target(base, &announce(AnnounceEvent::None))
                .expect("request target");
            assert!(target.url.starts_with(expected), "{}", target.url);
            assert!(!target.url.contains("&event="));
        }
    }

    #[test]
    fn request_rejects_bad_urls_credentials_and_bounds() {
        for base in [
            "udp://tracker:80",
            "http:///announce",
            "http://tracker/a#fragment",
            "http://tracker/a b",
        ] {
            assert!(
                build_announce_target(base, &announce(AnnounceEvent::None)).is_err(),
                "accepted {base}"
            );
        }
        assert_eq!(
            build_announce_target(
                &format!("http://{}:p@tracker/announce", "u".repeat(257)),
                &announce(AnnounceEvent::None),
            ),
            Err(HttpTrackerError::InvalidUserInfo)
        );
        let mut request = announce(AnnounceEvent::None);
        request.tracker_id = Some(vec![0; 257]);
        assert!(matches!(
            build_announce_target("http://tracker/announce", &request),
            Err(HttpTrackerError::TrackerIdTooLong { .. })
        ));
        assert!(matches!(
            build_announce_target(
                &format!("http://tracker/{}", "x".repeat(MAX_TRACKER_URL_LENGTH)),
                &announce(AnnounceEvent::None),
            ),
            Err(HttpTrackerError::UrlTooLong { .. })
        ));
    }

    #[test]
    fn parses_out_of_order_compact_ipv4_ipv6_and_optional_fields() {
        let mut compact4 = Vec::new();
        compact4.extend([127, 0, 0, 1, 0x1a, 0xe1]);
        compact4.extend([127, 0, 0, 1, 0x1a, 0xe1]);
        compact4.push(0xff);
        let mut compact6 = Ipv6Addr::LOCALHOST.octets().to_vec();
        compact6.extend(6882_u16.to_be_bytes());
        let body = dictionary(&[
            (b"warning message", bytes(b"old tracker")),
            (b"peers6", bytes(&compact6)),
            (b"tracker id", bytes(b"opaque")),
            (b"peers", bytes(&compact4)),
            (b"min interval", b"i900e".to_vec()),
            (b"interval", b"i60e".to_vec()),
            (b"incomplete", b"i5e".to_vec()),
            (b"complete", b"i7e".to_vec()),
        ]);

        let HttpTrackerResponse::Success(success) =
            parse_tracker_response(&body).expect("tracker response")
        else {
            panic!("expected success");
        };
        assert_eq!(success.interval, Duration::from_secs(900));
        assert_eq!(success.seeders, Some(7));
        assert_eq!(success.leechers, Some(5));
        assert_eq!(success.warning.as_deref(), Some("old tracker"));
        assert_eq!(success.tracker_id.as_deref(), Some(b"opaque".as_slice()));
        assert_eq!(
            success.peers,
            [
                TrackerPeer::Address("127.0.0.1:6881".parse().expect("IPv4 peer")),
                TrackerPeer::Address("[::1]:6882".parse().expect("IPv6 peer")),
            ]
        );
        assert_eq!(
            success.diagnostics,
            ["peers had a short trailing compact suffix"]
        );
    }

    #[test]
    fn accepts_noncompact_numeric_and_hostname_peers_and_skips_bad_entries() {
        let good_v4 = dictionary(&[(b"ip", bytes(b"127.0.0.1")), (b"port", b"i6881e".to_vec())]);
        let good_host = dictionary(&[
            (b"port", b"i6882e".to_vec()),
            (b"ip", bytes(b"Peer.Example")),
            (b"peer id", bytes(b"ignored")),
        ]);
        let bad = dictionary(&[(b"ip", bytes(b"bad host"))]);
        let mut list = vec![b'l'];
        list.extend(good_v4);
        list.extend(bad);
        list.extend(good_host);
        list.push(b'e');
        let body = dictionary(&[(b"peers", list)]);

        let HttpTrackerResponse::Success(success) =
            parse_tracker_response(&body).expect("noncompact response")
        else {
            panic!("expected success");
        };
        assert_eq!(success.interval, DEFAULT_HTTP_TRACKER_INTERVAL);
        assert_eq!(
            success.peers,
            [
                TrackerPeer::Address("127.0.0.1:6881".parse().expect("IPv4 peer")),
                TrackerPeer::Hostname {
                    host: "peer.example".to_owned(),
                    port: 6882,
                },
            ]
        );
    }

    #[test]
    fn failure_reason_supports_bep31_minutes_and_never() {
        for (retry, expected) in [
            (
                bytes(b"5"),
                Some(TrackerRetryDirective::After(Duration::from_secs(300))),
            ),
            (
                b"i7e".to_vec(),
                Some(TrackerRetryDirective::After(Duration::from_secs(420))),
            ),
            (bytes(b"never"), Some(TrackerRetryDirective::Never)),
            (bytes(b"0"), None),
        ] {
            let body = dictionary(&[
                (b"retry in", retry),
                (b"failure reason", bytes(b"temporarily unavailable")),
            ]);
            assert_eq!(
                parse_tracker_response(&body).expect("declared failure"),
                HttpTrackerResponse::Failure {
                    reason: "temporarily unavailable".to_owned(),
                    retry: expected,
                }
            );
        }
        let body = dictionary(&[
            (b"failure reason", bytes(b"no")),
            (b"retry in", bytes(b"999999999999999999999999")),
        ]);
        let HttpTrackerResponse::Failure { retry, .. } =
            parse_tracker_response(&body).expect("bounded retry")
        else {
            panic!("expected failure");
        };
        assert_eq!(
            retry,
            Some(TrackerRetryDirective::After(MAX_HTTP_TRACKER_RETRY))
        );
    }

    #[test]
    fn missing_peers_and_interval_is_a_zero_peer_success() {
        let HttpTrackerResponse::Success(success) =
            parse_tracker_response(b"de").expect("empty response dictionary")
        else {
            panic!("expected success");
        };
        assert_eq!(success.interval, DEFAULT_HTTP_TRACKER_INTERVAL);
        assert!(success.peers.is_empty());
        assert_eq!(success.seeders, None);
        assert_eq!(success.leechers, None);
    }

    #[test]
    fn rejects_duplicate_trailing_negative_and_wholly_malformed_peers() {
        for body in [
            b"d8:intervali1e8:intervali2ee".as_slice(),
            b"degarbage".as_slice(),
            b"d8:intervali-1ee".as_slice(),
            b"d5:peers5:abcdee".as_slice(),
            b"d5:peersli1eee".as_slice(),
        ] {
            assert!(parse_tracker_response(body).is_err(), "{body:?}");
        }
    }

    #[test]
    fn bounds_tracker_id_hostnames_and_total_peers() {
        let mut list = vec![b'l'];
        for index in 0..20 {
            list.extend(dictionary(&[
                (b"ip", bytes(format!("peer-{index}.example").as_bytes())),
                (b"port", b"i6881e".to_vec()),
            ]));
        }
        list.push(b'e');
        let body = dictionary(&[(b"tracker id", bytes(&vec![b'x'; 257])), (b"peers", list)]);
        let HttpTrackerResponse::Success(success) =
            parse_tracker_response(&body).expect("bounded response")
        else {
            panic!("expected success");
        };
        assert_eq!(success.tracker_id, None);
        assert_eq!(success.peers.len(), MAX_HTTP_TRACKER_HOSTNAMES);
        assert!(success.diagnostics.len() >= 2);

        let mut compact = Vec::new();
        for index in 0..250_u16 {
            compact.extend([
                127,
                0,
                u8::try_from(index / 255).expect("octet"),
                u8::try_from(index % 255).expect("octet"),
            ]);
            compact.extend((index + 1).to_be_bytes());
        }
        let body = dictionary(&[(b"peers", bytes(&compact))]);
        let HttpTrackerResponse::Success(success) =
            parse_tracker_response(&body).expect("bounded compact response")
        else {
            panic!("expected success");
        };
        assert_eq!(success.peers.len(), MAX_HTTP_TRACKER_PEERS);
    }

    #[tokio::test]
    async fn runtime_fetches_plain_and_chunked_x_gzip_responses() {
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        let plain = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("plain listener");
        let plain_url = format!("http://{}/announce?pass=abc", plain.local_addr().unwrap());
        let plain_task = tokio::spawn(serve_once(
            plain,
            http_response(&[("Content-Type", "text/plain")], b"de"),
        ));
        let response = announce_http_tracker(
            &clients,
            &plain_url,
            NetworkPolicy::LoopbackOnly,
            false,
            &announce(AnnounceEvent::Started),
            Duration::from_secs(2),
        )
        .await
        .expect("plain announce");
        assert!(matches!(response, HttpTrackerResponse::Success(_)));
        let request = plain_task.await.expect("plain server");
        assert!(request.starts_with("GET /announce?pass=abc&info_hash="));
        assert!(request.contains("&port=1&uploaded=2&downloaded=3&left=4"));
        assert!(request.contains("accept-encoding: gzip\r\n"));

        let gzip = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("gzip listener");
        let gzip_url = format!("http://{}/announce", gzip.local_addr().unwrap());
        let compressed = [
            0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x4b, 0x49, 0x05, 0x00,
            0x8b, 0x29, 0x90, 0x7d, 0x02, 0x00, 0x00, 0x00,
        ];
        let mut chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: x-gzip\r\nConnection: close\r\n\r\n".to_vec();
        chunked.extend(format!("{:x}\r\n", compressed.len()).as_bytes());
        chunked.extend(compressed);
        chunked.extend_from_slice(b"\r\n0\r\n\r\n");
        let gzip_task = tokio::spawn(serve_once(gzip, chunked));
        let response = announce_http_tracker(
            &clients,
            &gzip_url,
            NetworkPolicy::LoopbackOnly,
            false,
            &announce(AnnounceEvent::None),
            Duration::from_secs(2),
        )
        .await
        .expect("gzip announce");
        assert!(matches!(response, HttpTrackerResponse::Success(_)));
        gzip_task.await.expect("gzip server");
    }

    #[tokio::test]
    async fn gzip_decoder_rejects_truncation_concatenation_and_decoded_overflow() {
        async fn gzip(body: &[u8]) -> Vec<u8> {
            let reader = BufReader::new(body);
            let mut encoder = async_compression::tokio::bufread::GzipEncoder::new(reader);
            let mut compressed = Vec::new();
            encoder
                .read_to_end(&mut compressed)
                .await
                .expect("encode gzip fixture");
            compressed
        }

        let valid = gzip(b"de").await;
        assert_eq!(
            decode_response_body(valid.clone(), ContentEncoding::Gzip)
                .await
                .expect("valid gzip"),
            b"de"
        );

        let truncated = valid[..valid.len() - 1].to_vec();
        assert_eq!(
            decode_response_body(truncated, ContentEncoding::Gzip).await,
            Err(HttpTrackerError::InvalidCompression)
        );

        let concatenated = [valid.as_slice(), valid.as_slice()].concat();
        assert_eq!(
            decode_response_body(concatenated, ContentEncoding::Gzip).await,
            Err(HttpTrackerError::InvalidCompression)
        );

        let expanded = vec![0_u8; MAX_HTTP_TRACKER_BODY_LENGTH + 1];
        let bomb = gzip(&expanded).await;
        assert!(bomb.len() < MAX_HTTP_TRACKER_BODY_LENGTH);
        assert_eq!(
            decode_response_body(bomb, ContentEncoding::Gzip).await,
            Err(HttpTrackerError::DecodedBodyTooLong {
                maximum: MAX_HTTP_TRACKER_BODY_LENGTH,
            })
        );
    }

    #[tokio::test]
    async fn runtime_https_accepts_untrusted_hostname_mismatched_certificate() {
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("TLS listener");
        let url = format!("https://{}/announce", listener.local_addr().unwrap());
        let server = tokio::spawn(serve_tls_once(listener, http_response(&[], b"de")));
        let response = announce_http_tracker(
            &clients,
            &url,
            NetworkPolicy::LoopbackOnly,
            false,
            &announce(AnnounceEvent::Started),
            Duration::from_secs(5),
        )
        .await
        .expect("unauthenticated HTTPS announce");
        assert!(matches!(response, HttpTrackerResponse::Success(_)));
        let request = server.await.expect("TLS server");
        assert!(request.starts_with("GET /announce?info_hash="));
    }

    #[tokio::test]
    async fn runtime_redirects_strip_cross_origin_basic_auth_and_reject_downgrade() {
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        let destination = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("destination listener");
        let destination_url = format!("http://{}/result", destination.local_addr().unwrap());
        let destination_task = tokio::spawn(serve_once(destination, http_response(&[], b"de")));
        let origin = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("origin listener");
        let origin_url = format!("http://user:pass@{}/announce", origin.local_addr().unwrap());
        let redirect = format!(
            "HTTP/1.1 302 Found\r\nLocation: {destination_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        let origin_task = tokio::spawn(serve_once(origin, redirect));
        announce_http_tracker(
            &clients,
            &origin_url,
            NetworkPolicy::LoopbackOnly,
            false,
            &announce(AnnounceEvent::None),
            Duration::from_secs(2),
        )
        .await
        .expect("redirect announce");
        let origin_request = origin_task.await.expect("origin server");
        let destination_request = destination_task.await.expect("destination server");
        assert!(origin_request.contains("authorization: Basic dXNlcjpwYXNz\r\n"));
        assert!(!destination_request.contains("authorization:"));

        assert!(matches!(
            validate_redirect(
                &url::Url::parse("https://tracker/announce").unwrap(),
                &url::Url::parse("http://tracker/announce").unwrap(),
            ),
            Err(HttpTrackerError::Redirect(_))
        ));
        assert!(matches!(
            validate_redirect(
                &url::Url::parse("http://tracker/announce").unwrap(),
                &url::Url::parse("http://user:pass@tracker/announce").unwrap(),
            ),
            Err(HttpTrackerError::Redirect(_))
        ));
    }

    #[tokio::test]
    async fn runtime_ipv6_request_forces_outbound_only_port() {
        let listener = match TcpListener::bind("[::1]:0").await {
            Ok(listener) => listener,
            Err(_) => return,
        };
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        let url = format!(
            "http://[::1]:{}/announce",
            listener.local_addr().unwrap().port()
        );
        let task = tokio::spawn(serve_once(listener, http_response(&[], b"de")));
        let mut request = announce(AnnounceEvent::Started);
        request.port = 6881;
        announce_http_tracker(
            &clients,
            &url,
            NetworkPolicy::LoopbackOnly,
            false,
            &request,
            Duration::from_secs(2),
        )
        .await
        .expect("IPv6 announce");
        let request = task.await.expect("IPv6 server");
        assert!(request.contains("&port=1&"), "{request}");
    }

    #[tokio::test]
    async fn runtime_aaaa_only_tracker_accepts_only_peers6() {
        let listener = match TcpListener::bind("[::1]:0").await {
            Ok(listener) => listener,
            Err(_) => return,
        };
        let port = listener.local_addr().expect("IPv6 tracker address").port();
        let resolved = match lookup_host(("ip6-localhost", port)).await {
            Ok(resolved) => resolved.collect::<Vec<_>>(),
            Err(_) => return,
        };
        if resolved.is_empty() {
            return;
        }
        assert!(
            resolved.iter().all(SocketAddr::is_ipv6),
            "ip6-localhost must be an AAAA-only controlled name"
        );

        let mut compact = Ipv6Addr::LOCALHOST.octets().to_vec();
        compact.extend_from_slice(&49_002_u16.to_be_bytes());
        let body = dictionary(&[(b"peers6", bytes(&compact))]);
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        let url = format!("http://ip6-localhost:{port}/announce");
        let task = tokio::spawn(serve_once(listener, http_response(&[], &body)));
        let response = announce_http_tracker(
            &clients,
            &url,
            NetworkPolicy::LoopbackOnly,
            false,
            &announce(AnnounceEvent::Started),
            Duration::from_secs(2),
        )
        .await
        .expect("AAAA-only announce");
        let HttpTrackerResponse::Success(success) = response else {
            panic!("expected tracker success");
        };
        assert_eq!(
            success.peers,
            [TrackerPeer::Address(
                "[::1]:49002".parse().expect("IPv6 peer")
            )]
        );
        let request = task.await.expect("AAAA-only server");
        assert!(request.contains("&port=1&"), "{request}");
    }

    #[tokio::test]
    async fn runtime_rejects_status_encoding_and_declared_length_without_reading_body() {
        let clients = HttpTrackerClients::new(NetworkPolicy::LoopbackOnly).expect("HTTP clients");
        for (response, expected) in [
            (
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                "status",
            ),
            (
                http_response(&[("Content-Encoding", "br")], b"de"),
                "encoding",
            ),
            (
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_HTTP_TRACKER_BODY_LENGTH + 1
                )
                .into_bytes(),
                "exceeds",
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
            let url = format!("http://{}/announce", listener.local_addr().unwrap());
            let task = tokio::spawn(serve_once(listener, response));
            let error = announce_http_tracker(
                &clients,
                &url,
                NetworkPolicy::LoopbackOnly,
                false,
                &announce(AnnounceEvent::None),
                Duration::from_secs(2),
            )
            .await
            .expect_err("response should fail");
            assert!(error.to_string().contains(expected), "{error}");
            task.await.expect("server");
        }
    }
}

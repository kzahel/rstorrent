use std::error::Error;
use std::fmt;

pub const MAX_MAGNET_LENGTH: usize = 16 * 1024;
pub const MAX_MAGNET_PARAMETERS: usize = 128;
pub const MAX_PEER_HINTS: usize = 32;
pub const MAX_TRACKERS: usize = 32;
pub const MAX_HOST_LENGTH: usize = 253;
pub const MAX_TRACKER_URL_LENGTH: usize = 2 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Magnet {
    pub info_hash: [u8; 20],
    pub peer_hints: Vec<PeerHint>,
    pub trackers: Vec<TrackerUrl>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackerUrlTransport {
    Udp,
    Http,
    Https,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackerUrl {
    url: String,
    identity: String,
    transport: TrackerUrlTransport,
    udp_endpoint: Option<UdpTrackerUrl>,
}

impl TrackerUrl {
    pub fn from_magnet_url(value: &str) -> Option<Self> {
        if value.len() > MAX_TRACKER_URL_LENGTH
            || value.is_empty()
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
            || value.contains('#')
        {
            return None;
        }
        let (scheme, _) = value.split_once("://")?;
        if scheme.eq_ignore_ascii_case("udp") {
            let endpoint = parse_udp_tracker_url(value)?;
            return Some(Self {
                url: value.to_owned(),
                identity: udp_tracker_url(&endpoint),
                transport: TrackerUrlTransport::Udp,
                udp_endpoint: Some(endpoint),
            });
        }
        let transport = if scheme.eq_ignore_ascii_case("http") {
            TrackerUrlTransport::Http
        } else if scheme.eq_ignore_ascii_case("https") {
            TrackerUrlTransport::Https
        } else {
            return None;
        };
        let identity = http_tracker_identity(value, transport)?;
        Some(Self {
            url: value.to_owned(),
            identity,
            transport,
            udp_endpoint: None,
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub const fn transport(&self) -> TrackerUrlTransport {
        self.transport
    }

    pub fn udp_endpoint(&self) -> Option<&UdpTrackerUrl> {
        self.udp_endpoint.as_ref()
    }

    fn same_identity(&self, other: &Self) -> bool {
        self.transport == other.transport && self.identity == other.identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PeerHint {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UdpTrackerUrl {
    pub host: String,
    pub port: u16,
}

impl UdpTrackerUrl {
    /// Parse only the UDP authority needed by the runtime from an already
    /// bounded metainfo tracker URL. Unlike magnet intake, the outer source
    /// byte/work profile owns URL length and passkey-bearing paths are valid.
    pub fn from_metainfo_url(value: &str) -> Option<Self> {
        if !value.is_ascii() || value.contains(['#', '@']) {
            return None;
        }
        let (scheme, remainder) = value.split_once("://")?;
        if !scheme.eq_ignore_ascii_case("udp") || remainder.is_empty() {
            return None;
        }
        let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let endpoint = parse_peer_hint(&remainder[..authority_end])?;
        Some(Self {
            host: endpoint.host,
            port: endpoint.port,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MagnetError {
    TooLong { length: usize, maximum: usize },
    InvalidScheme,
    TooManyParameters { maximum: usize },
    InvalidPercentEscape,
    InvalidUtf8,
    MissingInfoHash,
    InvalidInfoHash,
    ConflictingInfoHashes,
    UnsupportedV2,
    TooManyPeerHints { maximum: usize },
    TooManyTrackers { maximum: usize },
}

impl fmt::Display for MagnetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { length, maximum } => {
                write!(formatter, "magnet length {length} exceeds limit {maximum}")
            }
            Self::InvalidScheme => write!(formatter, "input is not a magnet URI with a query"),
            Self::TooManyParameters { maximum } => {
                write!(formatter, "magnet has more than {maximum} query parameters")
            }
            Self::InvalidPercentEscape => write!(formatter, "magnet contains an invalid escape"),
            Self::InvalidUtf8 => write!(formatter, "magnet contains non-UTF-8 query text"),
            Self::MissingInfoHash => write!(formatter, "magnet has no v1 btih identity"),
            Self::InvalidInfoHash => write!(formatter, "magnet has an invalid v1 btih identity"),
            Self::ConflictingInfoHashes => {
                write!(formatter, "magnet has conflicting v1 btih identities")
            }
            Self::UnsupportedV2 => {
                write!(formatter, "v2 and hybrid magnet identities are unsupported")
            }
            Self::TooManyPeerHints { maximum } => {
                write!(formatter, "magnet has more than {maximum} valid peer hints")
            }
            Self::TooManyTrackers { maximum } => {
                write!(formatter, "magnet has more than {maximum} valid trackers")
            }
        }
    }
}

impl Error for MagnetError {}

impl Magnet {
    pub fn parse(uri: &str) -> Result<Self, MagnetError> {
        if uri.len() > MAX_MAGNET_LENGTH {
            return Err(MagnetError::TooLong {
                length: uri.len(),
                maximum: MAX_MAGNET_LENGTH,
            });
        }
        let (scheme, query) = uri.split_once('?').ok_or(MagnetError::InvalidScheme)?;
        if !scheme.eq_ignore_ascii_case("magnet:") {
            return Err(MagnetError::InvalidScheme);
        }

        let parameters: Vec<_> = if query.is_empty() {
            Vec::new()
        } else {
            query.split('&').collect()
        };
        if parameters.len() > MAX_MAGNET_PARAMETERS {
            return Err(MagnetError::TooManyParameters {
                maximum: MAX_MAGNET_PARAMETERS,
            });
        }

        let mut info_hash = None;
        let mut has_v2_identity = false;
        let mut peer_hints = Vec::new();
        let mut trackers = Vec::new();
        for parameter in parameters {
            let (encoded_name, encoded_value) =
                parameter.split_once('=').unwrap_or((parameter, ""));
            let name = percent_decode(encoded_name)?;
            let value = percent_decode(encoded_value)?;
            if name.eq_ignore_ascii_case("xt") {
                if starts_with_ignore_ascii_case(&value, "urn:btmh:") {
                    has_v2_identity = true;
                    continue;
                }
                if starts_with_ignore_ascii_case(&value, "urn:btih:") {
                    let hash = parse_btih(&value[b"urn:btih:".len()..])?;
                    if info_hash.is_some_and(|existing| existing != hash) {
                        return Err(MagnetError::ConflictingInfoHashes);
                    }
                    info_hash = Some(hash);
                }
            } else if name.eq_ignore_ascii_case("x.pe")
                && let Some(hint) = parse_peer_hint(&value)
                && !peer_hints.contains(&hint)
            {
                if peer_hints.len() == MAX_PEER_HINTS {
                    return Err(MagnetError::TooManyPeerHints {
                        maximum: MAX_PEER_HINTS,
                    });
                }
                peer_hints.push(hint);
            } else if name.eq_ignore_ascii_case("tr")
                && let Some(tracker) = TrackerUrl::from_magnet_url(&value)
                && !trackers
                    .iter()
                    .any(|existing: &TrackerUrl| existing.same_identity(&tracker))
            {
                if trackers.len() == MAX_TRACKERS {
                    return Err(MagnetError::TooManyTrackers {
                        maximum: MAX_TRACKERS,
                    });
                }
                trackers.push(tracker);
            }
        }

        if has_v2_identity {
            return Err(MagnetError::UnsupportedV2);
        }
        Ok(Self {
            info_hash: info_hash.ok_or(MagnetError::MissingInfoHash)?,
            peer_hints,
            trackers,
        })
    }
}

fn percent_decode(input: &str) -> Result<String, MagnetError> {
    let mut decoded = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        match bytes[position] {
            b'%' => {
                let high = bytes
                    .get(position + 1)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(MagnetError::InvalidPercentEscape)?;
                let low = bytes
                    .get(position + 2)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(MagnetError::InvalidPercentEscape)?;
                decoded.push((high << 4) | low);
                position += 3;
            }
            b'+' => {
                decoded.push(b' ');
                position += 1;
            }
            byte => {
                decoded.push(byte);
                position += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| MagnetError::InvalidUtf8)
}

fn parse_btih(value: &str) -> Result<[u8; 20], MagnetError> {
    match value.len() {
        40 => {
            let mut hash = [0; 20];
            for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
                let high = hex_value(pair[0]).ok_or(MagnetError::InvalidInfoHash)?;
                let low = hex_value(pair[1]).ok_or(MagnetError::InvalidInfoHash)?;
                hash[index] = (high << 4) | low;
            }
            Ok(hash)
        }
        32 => decode_base32(value),
        _ => Err(MagnetError::InvalidInfoHash),
    }
}

fn decode_base32(value: &str) -> Result<[u8; 20], MagnetError> {
    let mut hash = [0; 20];
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    let mut output = 0;
    for byte in value.bytes() {
        let digit = match byte.to_ascii_uppercase() {
            b'A'..=b'Z' => byte.to_ascii_uppercase() - b'A',
            b'2'..=b'7' => byte - b'2' + 26,
            _ => return Err(MagnetError::InvalidInfoHash),
        };
        accumulator = (accumulator << 5) | u32::from(digit);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            hash[output] = (accumulator >> bits) as u8;
            output += 1;
            accumulator &= (1_u32 << bits) - 1;
        }
    }
    if output != hash.len() || bits != 0 {
        return Err(MagnetError::InvalidInfoHash);
    }
    Ok(hash)
}

fn parse_peer_hint(value: &str) -> Option<PeerHint> {
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        if host.is_empty() || port.contains(':') {
            return None;
        }
        if !valid_ipv6(host) {
            return None;
        }
        (host.to_ascii_lowercase(), parse_port(port)?)
    } else {
        let (host, port) = value.rsplit_once(':')?;
        if host.is_empty() || host.contains(':') || value.contains('@') || value.contains('/') {
            return None;
        }
        (normalize_host(host)?, parse_port(port)?)
    };
    Some(PeerHint { host, port })
}

fn parse_udp_tracker_url(value: &str) -> Option<UdpTrackerUrl> {
    if value.len() > MAX_TRACKER_URL_LENGTH || !value.is_ascii() {
        return None;
    }
    let (scheme, remainder) = value.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("udp")
        || remainder.is_empty()
        || remainder.contains(['?', '#', '@'])
    {
        return None;
    }
    let (authority, path) = remainder
        .split_once('/')
        .map_or((remainder, ""), |(authority, path)| (authority, path));
    if !matches!(path, "" | "announce") {
        return None;
    }
    let endpoint = parse_peer_hint(authority)?;
    Some(UdpTrackerUrl {
        host: endpoint.host,
        port: endpoint.port,
    })
}

fn udp_tracker_url(tracker: &UdpTrackerUrl) -> String {
    if tracker.host.contains(':') {
        format!("udp://[{}]:{}", tracker.host, tracker.port)
    } else {
        format!("udp://{}:{}", tracker.host, tracker.port)
    }
}

fn http_tracker_identity(value: &str, transport: TrackerUrlTransport) -> Option<String> {
    if value.contains('\\') {
        return None;
    }
    let (_, remainder) = value.split_once("://")?;
    let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    let suffix = &remainder[authority_end..];
    if authority.is_empty() {
        return None;
    }
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(userinfo, host_port)| {
            (Some(userinfo), host_port)
        });
    if userinfo.is_some_and(|userinfo| userinfo.contains('@')) || host_port.is_empty() {
        return None;
    }
    let (host, port, ipv6) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let close = bracketed.find(']')?;
        let host = &bracketed[..close];
        let trailing = &bracketed[close + 1..];
        let port = if trailing.is_empty() {
            None
        } else {
            Some(parse_port(trailing.strip_prefix(':')?)?)
        };
        if !valid_ipv6(host) {
            return None;
        }
        (host.to_ascii_lowercase(), port, true)
    } else if host_port.contains(':') {
        let (host, port) = host_port.rsplit_once(':')?;
        if host.contains(':') {
            return None;
        }
        (normalize_host(host)?, Some(parse_port(port)?), false)
    } else {
        (normalize_host(host_port)?, None, false)
    };
    let scheme = match transport {
        TrackerUrlTransport::Http => "http",
        TrackerUrlTransport::Https => "https",
        TrackerUrlTransport::Udp => return None,
    };
    let default_port = matches!(
        (transport, port),
        (TrackerUrlTransport::Http, Some(80)) | (TrackerUrlTransport::Https, Some(443))
    );
    let mut identity = String::with_capacity(value.len() + 1);
    identity.push_str(scheme);
    identity.push_str("://");
    if let Some(userinfo) = userinfo {
        identity.push_str(userinfo);
        identity.push('@');
    }
    if ipv6 {
        identity.push('[');
    }
    identity.push_str(&host);
    if ipv6 {
        identity.push(']');
    }
    if let Some(port) = port.filter(|_| !default_port) {
        identity.push(':');
        identity.push_str(&port.to_string());
    }
    if suffix.is_empty() {
        identity.push('/');
    } else if suffix.starts_with('?') {
        identity.push('/');
        identity.push_str(suffix);
    } else {
        identity.push_str(suffix);
    }
    Some(identity)
}

fn normalize_host(host: &str) -> Option<String> {
    if host.len() > MAX_HOST_LENGTH || !host.is_ascii() {
        return None;
    }
    if valid_ipv4(host) {
        return Some(host.to_owned());
    }
    if host
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    if host.ends_with('.') {
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

fn valid_ipv4(host: &str) -> bool {
    let mut count = 0;
    for component in host.split('.') {
        count += 1;
        if component.is_empty()
            || component.len() > 3
            || component.len() > 1 && component.starts_with('0')
            || !component.bytes().all(|byte| byte.is_ascii_digit())
            || component.parse::<u16>().map_or(true, |value| value > 255)
        {
            return false;
        }
    }
    count == 4
}

fn valid_ipv6(host: &str) -> bool {
    if !host.is_ascii()
        || host.contains('%')
        || host.starts_with(':') && !host.starts_with("::")
        || host.ends_with(':') && !host.ends_with("::")
    {
        return false;
    }
    let mut compressed = host.split("::");
    let left = compressed.next().unwrap_or_default();
    let right = compressed.next();
    if compressed.next().is_some() {
        return false;
    }

    let Some(left_groups) = ipv6_group_count(left, right.is_none()) else {
        return false;
    };
    let right_groups = match right {
        Some(value) => {
            let Some(count) = ipv6_group_count(value, true) else {
                return false;
            };
            count
        }
        None => 0,
    };
    match right {
        Some(_) => left_groups + right_groups < 8,
        None => left_groups == 8,
    }
}

fn ipv6_group_count(value: &str, allow_ipv4_tail: bool) -> Option<usize> {
    if value.is_empty() {
        return Some(0);
    }
    let groups: Vec<_> = value.split(':').collect();
    let mut count = 0;
    for (index, group) in groups.iter().enumerate() {
        if group.contains('.') {
            if !allow_ipv4_tail || index + 1 != groups.len() || !valid_ipv4(group) {
                return None;
            }
            count += 2;
        } else {
            if group.is_empty()
                || group.len() > 4
                || !group.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return None;
            }
            count += 1;
        }
    }
    Some(count)
}

fn parse_port(port: &str) -> Option<u16> {
    let port = port.parse::<u16>().ok()?;
    (port != 0).then_some(port)
}

fn starts_with_ignore_ascii_case(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HOST_LENGTH, MAX_MAGNET_LENGTH, MAX_MAGNET_PARAMETERS, MAX_PEER_HINTS,
        MAX_TRACKER_URL_LENGTH, MAX_TRACKERS, Magnet, MagnetError, PeerHint, TrackerUrlTransport,
    };

    const HEX_HASH: &str = "0123456789abcdef0123456789abcdef01234567";
    const BASE32_HASH: &str = "AERUKZ4JVPG66AJDIVTYTK6N54ASGRLH";

    #[test]
    fn accepts_hex_base32_repetition_and_bounded_peer_forms() {
        let hex = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{HEX_HASH}&xt=URN:BTIH:{HEX_HASH}&\
             x.pe=127.0.0.1:6881&x.pe=seed.EXAMPLE:80&\
             x.pe=%5B2001%3Adb8%3A%3A1%5D%3A443&x.pe=127.0.0.1:6881"
        ))
        .expect("valid magnet");
        let base32 =
            Magnet::parse(&format!("MAGNET:?xt=urn:btih:{BASE32_HASH}")).expect("base32 identity");

        assert_eq!(hex.info_hash, base32.info_hash);
        assert_eq!(
            hex.peer_hints,
            [
                PeerHint {
                    host: "127.0.0.1".to_owned(),
                    port: 6881
                },
                PeerHint {
                    host: "seed.example".to_owned(),
                    port: 80
                },
                PeerHint {
                    host: "2001:db8::1".to_owned(),
                    port: 443
                }
            ]
        );
        assert!(hex.trackers.is_empty());
    }

    #[test]
    fn rejects_bad_identity_scheme_escapes_and_v2() {
        for input in [
            "",
            "https:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            "magnet:",
        ] {
            assert!(Magnet::parse(input).is_err(), "{input}");
        }
        assert_eq!(
            Magnet::parse("magnet:?dn=x"),
            Err(MagnetError::MissingInfoHash)
        );
        assert_eq!(
            Magnet::parse("magnet:?xt=urn%ZZbtih"),
            Err(MagnetError::InvalidPercentEscape)
        );
        assert_eq!(
            Magnet::parse("magnet:?xt=urn:btih:xyz"),
            Err(MagnetError::InvalidInfoHash)
        );
        assert_eq!(
            Magnet::parse(
                "magnet:?xt=urn:btmh:1220aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ),
            Err(MagnetError::UnsupportedV2)
        );
        assert_eq!(
            Magnet::parse(&format!(
                "magnet:?xt=urn:btih:{HEX_HASH}&xt=urn:btmh:1220aa"
            )),
            Err(MagnetError::UnsupportedV2)
        );
        assert_eq!(
            Magnet::parse(&format!(
                "magnet:?xt=urn:btih:{HEX_HASH}&\
                 xt=urn:btih:1123456789abcdef0123456789abcdef01234567"
            )),
            Err(MagnetError::ConflictingInfoHashes)
        );
    }

    #[test]
    fn ignores_malformed_hints_without_weakening_valid_host_rules() {
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{HEX_HASH}&x.pe=:1&x.pe=host:0&\
             x.pe=999.1.1.1:80&x.pe=2001:db8::1:80&x.pe=%5Bbad%5D:2&\
             x.pe=user@host:1&x.pe=good-host:65535"
        ))
        .expect("identity remains valid");

        assert_eq!(
            magnet.peer_hints,
            [PeerHint {
                host: "good-host".to_owned(),
                port: 65535
            }]
        );
    }

    #[test]
    fn enforces_uri_parameter_host_and_valid_hint_bounds() {
        let oversized = "x".repeat(MAX_MAGNET_LENGTH + 1);
        assert!(matches!(
            Magnet::parse(&oversized),
            Err(MagnetError::TooLong { .. })
        ));

        let parameters = std::iter::repeat_n("dn=x", MAX_MAGNET_PARAMETERS + 1)
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(
            Magnet::parse(&format!("magnet:?{parameters}")),
            Err(MagnetError::TooManyParameters {
                maximum: MAX_MAGNET_PARAMETERS
            })
        );

        let long_host = "a".repeat(MAX_HOST_LENGTH + 1);
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{HEX_HASH}&x.pe={long_host}:1"
        ))
        .expect("bad hint is ignored");
        assert!(magnet.peer_hints.is_empty());
        assert!(magnet.trackers.is_empty());

        let hints = (0..=MAX_PEER_HINTS)
            .map(|index| format!("x.pe=host-{index}:1"))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(
            Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX_HASH}&{hints}")),
            Err(MagnetError::TooManyPeerHints {
                maximum: MAX_PEER_HINTS
            })
        );
    }

    #[test]
    fn accepts_deduplicates_and_bounds_supported_trackers() {
        let magnet = Magnet::parse(&format!(
            "magnet:?xt=urn:btih:{HEX_HASH}&\
             tr=udp%3A%2F%2FTracker.Example%3A6969&\
             tr=UDP%3A%2F%2Ftracker.example%3A6969%2Fannounce&\
             tr=udp%3A%2F%2F%5B%3A%3A1%5D%3A80%2F&\
             tr=https%3A%2F%2Ftracker.example%2Fannounce&\
             tr=HTTPS%3A%2F%2FTRACKER.EXAMPLE%3A443%2Fannounce&\
             tr=http%3A%2F%2Fuser%3Apass%40tracker.example%2Fa%3Fx%3D1"
        ))
        .expect("tracker magnet");
        assert_eq!(
            magnet
                .trackers
                .iter()
                .map(|tracker| (tracker.url(), tracker.transport()))
                .collect::<Vec<_>>(),
            [
                ("udp://Tracker.Example:6969", TrackerUrlTransport::Udp),
                ("udp://[::1]:80/", TrackerUrlTransport::Udp),
                (
                    "https://tracker.example/announce",
                    TrackerUrlTransport::Https
                ),
                (
                    "http://user:pass@tracker.example/a?x=1",
                    TrackerUrlTransport::Http
                ),
            ]
        );
        assert_eq!(
            magnet.trackers[0]
                .udp_endpoint()
                .expect("UDP endpoint")
                .host,
            "tracker.example"
        );

        let invalid = [
            "udp://tracker.example",
            "udp://tracker.example:0",
            "udp://user@tracker.example:80",
            "udp://tracker.example:80/path",
            "udp://tracker.example:80?query",
            "udp://2001:db8::1:80",
            "udp://999.1.1.1:80",
            "http://tracker.example:bad/announce",
            "https://[2001:db8::1/announce",
            "http://tracker.example\\redirect",
            "wss://tracker.example",
        ]
        .into_iter()
        .map(|tracker| format!("tr={tracker}"))
        .collect::<Vec<_>>()
        .join("&");
        let magnet = Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX_HASH}&{invalid}"))
            .expect("invalid trackers are ignored");
        assert!(magnet.trackers.is_empty());

        let oversized = "a".repeat(MAX_TRACKER_URL_LENGTH + 1);
        let magnet = Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX_HASH}&tr={oversized}"))
            .expect("oversized tracker is ignored");
        assert!(magnet.trackers.is_empty());

        let trackers = (0..=MAX_TRACKERS)
            .map(|index| format!("tr=udp://tracker-{index}.example:80"))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(
            Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX_HASH}&{trackers}")),
            Err(MagnetError::TooManyTrackers {
                maximum: MAX_TRACKERS
            })
        );

        let trackers = (0..MAX_TRACKERS)
            .map(|index| format!("tr=udp://tracker-{index}.example:80"))
            .chain(std::iter::once("tr=udp://invalid".to_owned()))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(
            Magnet::parse(&format!("magnet:?xt=urn:btih:{HEX_HASH}&{trackers}"))
                .expect("invalid tracker does not consume capacity")
                .trackers
                .len(),
            MAX_TRACKERS
        );
    }
}

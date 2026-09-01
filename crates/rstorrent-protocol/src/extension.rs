//! Bounded BEP 10 recognized-extension negotiation and BEP 11 wire values.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use crate::bencode::{
    DictionaryEntry, Limits, Node, ParseError, Value, parse_with_limits_permissive_dictionaries,
};

pub const UT_METADATA_LOCAL_ID: u8 = 1;
pub const UT_PEX_LOCAL_ID: u8 = 2;
pub const MAX_PEX_PAYLOAD_LENGTH: usize = 16 * 1024;
pub const MAX_PEX_ADDITIONS: usize = 50;
pub const MAX_PEX_DROPS: usize = 50;
pub const MAX_TRUSTED_METADATA_HANDSHAKE_LENGTH: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExtensionUpdate {
    #[default]
    Unchanged,
    Disabled,
    Enabled(u8),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionHandshake {
    pub metadata: ExtensionUpdate,
    pub pex: ExtensionUpdate,
    pub metadata_size: Option<usize>,
    pub listen_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionMap {
    metadata_id: Option<u8>,
    pex_id: Option<u8>,
    listen_port: Option<u16>,
}

impl ExtensionMap {
    pub fn apply(&mut self, handshake: ExtensionHandshake) {
        apply_update(&mut self.metadata_id, handshake.metadata);
        apply_update(&mut self.pex_id, handshake.pex);
        if let Some(port) = handshake.listen_port {
            self.listen_port = Some(port);
        }
    }

    pub const fn metadata_id(self) -> Option<u8> {
        self.metadata_id
    }

    pub const fn pex_id(self) -> Option<u8> {
        self.pex_id
    }

    pub const fn listen_port(self) -> Option<u16> {
        self.listen_port
    }
}

fn apply_update(target: &mut Option<u8>, update: ExtensionUpdate) {
    match update {
        ExtensionUpdate::Unchanged => {}
        ExtensionUpdate::Disabled => *target = None,
        ExtensionUpdate::Enabled(id) => *target = Some(id),
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionAdvertisement {
    pub metadata_id: Option<u8>,
    pub pex_id: Option<u8>,
    pub metadata_size: Option<usize>,
    pub listen_port: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PexFlags(u8);

impl PexFlags {
    pub const ENCRYPTION: u8 = 0x01;
    pub const SEED: u8 = 0x02;
    pub const UTP: u8 = 0x04;
    pub const HOLE_PUNCH: u8 = 0x08;
    pub const OUTGOING: u8 = 0x10;

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PexIp {
    V4([u8; 4]),
    V6([u8; 16]),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PexEndpoint {
    pub ip: PexIp,
    pub port: u16,
}

impl PexEndpoint {
    pub const fn new(ip: PexIp, port: u16) -> Self {
        match ip {
            PexIp::V6(bytes)
                if bytes[0] == 0
                    && bytes[1] == 0
                    && bytes[2] == 0
                    && bytes[3] == 0
                    && bytes[4] == 0
                    && bytes[5] == 0
                    && bytes[6] == 0
                    && bytes[7] == 0
                    && bytes[8] == 0
                    && bytes[9] == 0
                    && bytes[10] == 0xff
                    && bytes[11] == 0xff =>
            {
                Self {
                    ip: PexIp::V4([bytes[12], bytes[13], bytes[14], bytes[15]]),
                    port,
                }
            }
            _ => Self { ip, port },
        }
    }

    pub const fn v4(ip: [u8; 4], port: u16) -> Self {
        Self::new(PexIp::V4(ip), port)
    }

    pub const fn v6(ip: [u8; 16], port: u16) -> Self {
        Self::new(PexIp::V6(ip), port)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PexContact {
    pub endpoint: PexEndpoint,
    pub flags: PexFlags,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PexMessage {
    pub added: Vec<PexContact>,
    pub dropped: Vec<PexEndpoint>,
    pub additions_truncated: usize,
    pub drops_truncated: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionError {
    Bencode(ParseError),
    RootIsNotDictionary,
    InvalidField(&'static str),
    ConflictingIds,
    EmptyPex,
    InvalidCompactStride(&'static str),
    InvalidFlagsLength(&'static str),
    ConflictingContact(PexIp),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bencode(error) => write!(formatter, "invalid extension bencode: {error}"),
            Self::RootIsNotDictionary => formatter.write_str("extension root is not a dictionary"),
            Self::InvalidField(field) => write!(formatter, "invalid extension field {field}"),
            Self::ConflictingIds => formatter.write_str("recognized extensions share an ID"),
            Self::EmptyPex => formatter.write_str("PEX message contains no contacts"),
            Self::InvalidCompactStride(field) => {
                write!(formatter, "PEX field {field} has an invalid compact stride")
            }
            Self::InvalidFlagsLength(field) => {
                write!(formatter, "PEX field {field} has a mismatched flags length")
            }
            Self::ConflictingContact(address) => {
                write!(
                    formatter,
                    "PEX adds and drops address {address:?} in one message"
                )
            }
        }
    }
}

impl Error for ExtensionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bencode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ParseError> for ExtensionError {
    fn from(error: ParseError) -> Self {
        Self::Bencode(error)
    }
}

pub fn parse_extension_handshake(payload: &[u8]) -> Result<ExtensionHandshake, ExtensionError> {
    let root = parse_with_limits_permissive_dictionaries(payload, extension_limits())?;
    let entries = dictionary(&root).ok_or(ExtensionError::RootIsNotDictionary)?;
    let mapping = field(entries, b"m")
        .map(|node| dictionary(node).ok_or(ExtensionError::InvalidField("m")))
        .transpose()?;
    let metadata = extension_update(mapping, b"ut_metadata", "m.ut_metadata")?;
    let pex = extension_update(mapping, b"ut_pex", "m.ut_pex")?;
    if matches!((metadata, pex), (ExtensionUpdate::Enabled(lhs), ExtensionUpdate::Enabled(rhs)) if lhs == rhs)
    {
        return Err(ExtensionError::ConflictingIds);
    }
    let metadata_size = field(entries, b"metadata_size")
        .map(|node| {
            let value = integer(node, "metadata_size")?;
            let size = usize::try_from(value)
                .ok()
                .filter(|size| *size != 0)
                .ok_or(ExtensionError::InvalidField("metadata_size"))?;
            Ok::<_, ExtensionError>((size <= MAX_TRUSTED_METADATA_HANDSHAKE_LENGTH).then_some(size))
        })
        .transpose()?
        .flatten();
    let listen_port = field(entries, b"p")
        .map(|node| {
            let value = integer(node, "p")?;
            u16::try_from(value)
                .ok()
                .filter(|port| *port != 0)
                .ok_or(ExtensionError::InvalidField("p"))
        })
        .transpose()?;
    Ok(ExtensionHandshake {
        metadata,
        pex,
        metadata_size,
        listen_port,
    })
}

pub fn encode_extension_handshake(
    advertisement: ExtensionAdvertisement,
) -> Result<Vec<u8>, ExtensionError> {
    if advertisement.metadata_id == Some(0) || advertisement.pex_id == Some(0) {
        return Err(ExtensionError::InvalidField("local extension ID"));
    }
    if advertisement.metadata_id.is_some() && advertisement.metadata_id == advertisement.pex_id {
        return Err(ExtensionError::ConflictingIds);
    }
    if advertisement.metadata_size.is_some_and(|size| size == 0) {
        return Err(ExtensionError::InvalidField("metadata_size"));
    }
    let mut encoded = Vec::new();
    encoded.push(b'd');
    if advertisement.metadata_id.is_some() || advertisement.pex_id.is_some() {
        encoded.extend_from_slice(b"1:md");
        if let Some(id) = advertisement.metadata_id {
            encoded.extend_from_slice(b"11:ut_metadatai");
            push_integer(&mut encoded, u64::from(id));
            encoded.push(b'e');
        }
        if let Some(id) = advertisement.pex_id {
            encoded.extend_from_slice(b"6:ut_pexi");
            push_integer(&mut encoded, u64::from(id));
            encoded.push(b'e');
        }
        encoded.push(b'e');
    }
    if let Some(size) = advertisement.metadata_size {
        encoded.extend_from_slice(b"13:metadata_sizei");
        push_integer(&mut encoded, size as u64);
        encoded.push(b'e');
    }
    if let Some(port) = advertisement.listen_port {
        encoded.extend_from_slice(b"1:pi");
        push_integer(&mut encoded, u64::from(port));
        encoded.push(b'e');
    }
    encoded.push(b'e');
    Ok(encoded)
}

/// Encodes a repeated BEP 10 handshake update, including zero-valued
/// disable entries that cannot be represented by `ExtensionAdvertisement`.
pub fn encode_extension_handshake_update(
    update: ExtensionHandshake,
) -> Result<Vec<u8>, ExtensionError> {
    let metadata_id = match update.metadata {
        ExtensionUpdate::Enabled(id) if id != 0 => Some(id),
        ExtensionUpdate::Enabled(_) => {
            return Err(ExtensionError::InvalidField("local extension ID"));
        }
        ExtensionUpdate::Unchanged | ExtensionUpdate::Disabled => None,
    };
    let pex_id = match update.pex {
        ExtensionUpdate::Enabled(id) if id != 0 => Some(id),
        ExtensionUpdate::Enabled(_) => {
            return Err(ExtensionError::InvalidField("local extension ID"));
        }
        ExtensionUpdate::Unchanged | ExtensionUpdate::Disabled => None,
    };
    if metadata_id.is_some() && metadata_id == pex_id {
        return Err(ExtensionError::ConflictingIds);
    }
    if update.metadata_size.is_some_and(|size| size == 0) {
        return Err(ExtensionError::InvalidField("metadata_size"));
    }

    let mut encoded = Vec::new();
    encoded.push(b'd');
    if !matches!(update.metadata, ExtensionUpdate::Unchanged)
        || !matches!(update.pex, ExtensionUpdate::Unchanged)
    {
        encoded.extend_from_slice(b"1:md");
        match update.metadata {
            ExtensionUpdate::Unchanged => {}
            ExtensionUpdate::Disabled => encoded.extend_from_slice(b"11:ut_metadatai0e"),
            ExtensionUpdate::Enabled(id) => {
                encoded.extend_from_slice(b"11:ut_metadatai");
                push_integer(&mut encoded, u64::from(id));
                encoded.push(b'e');
            }
        }
        match update.pex {
            ExtensionUpdate::Unchanged => {}
            ExtensionUpdate::Disabled => encoded.extend_from_slice(b"6:ut_pexi0e"),
            ExtensionUpdate::Enabled(id) => {
                encoded.extend_from_slice(b"6:ut_pexi");
                push_integer(&mut encoded, u64::from(id));
                encoded.push(b'e');
            }
        }
        encoded.push(b'e');
    }
    if let Some(size) = update.metadata_size {
        encoded.extend_from_slice(b"13:metadata_sizei");
        push_integer(&mut encoded, size as u64);
        encoded.push(b'e');
    }
    if let Some(port) = update.listen_port {
        encoded.extend_from_slice(b"1:pi");
        push_integer(&mut encoded, u64::from(port));
        encoded.push(b'e');
    }
    encoded.push(b'e');
    Ok(encoded)
}

pub fn parse_pex_message(payload: &[u8]) -> Result<PexMessage, ExtensionError> {
    let root = parse_with_limits_permissive_dictionaries(payload, pex_limits())?;
    let entries = dictionary(&root).ok_or(ExtensionError::RootIsNotDictionary)?;
    let added4 = bytes_field(entries, b"added", "added")?;
    let flags4 = bytes_field(entries, b"added.f", "added.f")?;
    let added6 = bytes_field(entries, b"added6", "added6")?;
    let flags6 = bytes_field(entries, b"added6.f", "added6.f")?;
    let dropped4 = bytes_field(entries, b"dropped", "dropped")?;
    let dropped6 = bytes_field(entries, b"dropped6", "dropped6")?;
    validate_stride(added4, 6, "added")?;
    validate_stride(added6, 18, "added6")?;
    validate_stride(dropped4, 6, "dropped")?;
    validate_stride(dropped6, 18, "dropped6")?;
    validate_flags(flags4, added4.len() / 6, "added.f")?;
    validate_flags(flags6, added6.len() / 18, "added6.f")?;
    let mut added = Vec::new();
    let mut added_ips = BTreeSet::new();
    decode_added4(added4, flags4, &mut added, &mut added_ips);
    decode_added6(added6, flags6, &mut added, &mut added_ips);
    let total_additions = added.len();
    added.truncate(MAX_PEX_ADDITIONS);
    let mut dropped = Vec::new();
    let mut dropped_ips = BTreeSet::new();
    decode_dropped4(dropped4, &mut dropped, &mut dropped_ips);
    decode_dropped6(dropped6, &mut dropped, &mut dropped_ips);
    if let Some(conflict) = added_ips.intersection(&dropped_ips).next().copied() {
        return Err(ExtensionError::ConflictingContact(conflict));
    }
    let total_drops = dropped.len();
    dropped.truncate(MAX_PEX_DROPS);
    if total_additions == 0 && total_drops == 0 {
        return Err(ExtensionError::EmptyPex);
    }
    Ok(PexMessage {
        added,
        dropped,
        additions_truncated: total_additions.saturating_sub(MAX_PEX_ADDITIONS),
        drops_truncated: total_drops.saturating_sub(MAX_PEX_DROPS),
    })
}

pub fn encode_pex_message(message: &PexMessage) -> Result<Vec<u8>, ExtensionError> {
    if message.added.is_empty() && message.dropped.is_empty() {
        return Err(ExtensionError::EmptyPex);
    }
    if message.added.len() > MAX_PEX_ADDITIONS || message.dropped.len() > MAX_PEX_DROPS {
        return Err(ExtensionError::InvalidField("PEX contact count"));
    }
    let mut added4 = Vec::new();
    let mut added4_flags = Vec::new();
    let mut added6 = Vec::new();
    let mut added6_flags = Vec::new();
    let mut dropped4 = Vec::new();
    let mut dropped6 = Vec::new();
    for contact in &message.added {
        match contact.endpoint.ip {
            PexIp::V4(address) => {
                added4.extend_from_slice(&address);
                added4.extend_from_slice(&contact.endpoint.port.to_be_bytes());
                added4_flags.push(contact.flags.bits());
            }
            PexIp::V6(address) => {
                added6.extend_from_slice(&address);
                added6.extend_from_slice(&contact.endpoint.port.to_be_bytes());
                added6_flags.push(contact.flags.bits());
            }
        }
    }
    for endpoint in &message.dropped {
        match endpoint.ip {
            PexIp::V4(address) => {
                dropped4.extend_from_slice(&address);
                dropped4.extend_from_slice(&endpoint.port.to_be_bytes());
            }
            PexIp::V6(address) => {
                dropped6.extend_from_slice(&address);
                dropped6.extend_from_slice(&endpoint.port.to_be_bytes());
            }
        }
    }
    let mut encoded = vec![b'd'];
    append_bytes_field(&mut encoded, b"added", &added4);
    append_bytes_field(&mut encoded, b"added.f", &added4_flags);
    append_bytes_field(&mut encoded, b"added6", &added6);
    append_bytes_field(&mut encoded, b"added6.f", &added6_flags);
    append_bytes_field(&mut encoded, b"dropped", &dropped4);
    append_bytes_field(&mut encoded, b"dropped6", &dropped6);
    encoded.push(b'e');
    if encoded.len() > MAX_PEX_PAYLOAD_LENGTH {
        return Err(ExtensionError::InvalidField("PEX payload length"));
    }
    Ok(encoded)
}

fn extension_update(
    mapping: Option<&[DictionaryEntry<'_>]>,
    name: &[u8],
    label: &'static str,
) -> Result<ExtensionUpdate, ExtensionError> {
    let Some(node) = mapping.and_then(|entries| field(entries, name)) else {
        return Ok(ExtensionUpdate::Unchanged);
    };
    match integer(node, label)? {
        0 => Ok(ExtensionUpdate::Disabled),
        id @ 1..=255 => Ok(ExtensionUpdate::Enabled(id as u8)),
        _ => Err(ExtensionError::InvalidField(label)),
    }
}

fn dictionary<'a>(node: &'a Node<'a>) -> Option<&'a [DictionaryEntry<'a>]> {
    match &node.value {
        Value::Dictionary(entries) => Some(entries),
        _ => None,
    }
}

fn field<'a>(entries: &'a [DictionaryEntry<'a>], key: &[u8]) -> Option<&'a Node<'a>> {
    entries
        .binary_search_by_key(&key, |entry| entry.key)
        .ok()
        .map(|index| &entries[index].value)
}

fn integer(node: &Node<'_>, field: &'static str) -> Result<i64, ExtensionError> {
    match node.value {
        Value::Integer(value) => Ok(value),
        _ => Err(ExtensionError::InvalidField(field)),
    }
}

fn bytes_field<'a>(
    entries: &'a [DictionaryEntry<'a>],
    key: &[u8],
    label: &'static str,
) -> Result<&'a [u8], ExtensionError> {
    match field(entries, key) {
        None => Ok(&[]),
        Some(Node {
            value: Value::Bytes(bytes),
            ..
        }) => Ok(bytes),
        Some(_) => Err(ExtensionError::InvalidField(label)),
    }
}

fn validate_stride(bytes: &[u8], stride: usize, field: &'static str) -> Result<(), ExtensionError> {
    if !bytes.len().is_multiple_of(stride) {
        return Err(ExtensionError::InvalidCompactStride(field));
    }
    Ok(())
}

fn validate_flags(
    flags: &[u8],
    contacts: usize,
    field: &'static str,
) -> Result<(), ExtensionError> {
    if !flags.is_empty() && flags.len() != contacts {
        return Err(ExtensionError::InvalidFlagsLength(field));
    }
    Ok(())
}

fn decode_added4(
    bytes: &[u8],
    flags: &[u8],
    target: &mut Vec<PexContact>,
    ips: &mut BTreeSet<PexIp>,
) {
    for (position, chunk) in bytes.chunks_exact(6).enumerate() {
        let address = PexIp::V4([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if ips.insert(address) {
            target.push(PexContact {
                endpoint: PexEndpoint::new(address, u16::from_be_bytes([chunk[4], chunk[5]])),
                flags: PexFlags::from_bits(flags.get(position).copied().unwrap_or(0)),
            });
        }
    }
}

fn decode_added6(
    bytes: &[u8],
    flags: &[u8],
    target: &mut Vec<PexContact>,
    ips: &mut BTreeSet<PexIp>,
) {
    for (position, chunk) in bytes.chunks_exact(18).enumerate() {
        let mut octets = [0; 16];
        octets.copy_from_slice(&chunk[..16]);
        let endpoint = PexEndpoint::new(
            PexIp::V6(octets),
            u16::from_be_bytes([chunk[16], chunk[17]]),
        );
        if ips.insert(endpoint.ip) {
            target.push(PexContact {
                endpoint,
                flags: PexFlags::from_bits(flags.get(position).copied().unwrap_or(0)),
            });
        }
    }
}

fn decode_dropped4(bytes: &[u8], target: &mut Vec<PexEndpoint>, ips: &mut BTreeSet<PexIp>) {
    for chunk in bytes.chunks_exact(6) {
        let endpoint = PexEndpoint::v4(
            [chunk[0], chunk[1], chunk[2], chunk[3]],
            u16::from_be_bytes([chunk[4], chunk[5]]),
        );
        if ips.insert(endpoint.ip) {
            target.push(endpoint);
        }
    }
}

fn decode_dropped6(bytes: &[u8], target: &mut Vec<PexEndpoint>, ips: &mut BTreeSet<PexIp>) {
    for chunk in bytes.chunks_exact(18) {
        let mut octets = [0; 16];
        octets.copy_from_slice(&chunk[..16]);
        let endpoint = PexEndpoint::new(
            PexIp::V6(octets),
            u16::from_be_bytes([chunk[16], chunk[17]]),
        );
        if ips.insert(endpoint.ip) {
            target.push(endpoint);
        }
    }
}

fn append_bytes_field(encoded: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    if value.is_empty() {
        return;
    }
    push_integer(encoded, key.len() as u64);
    encoded.push(b':');
    encoded.extend_from_slice(key);
    push_integer(encoded, value.len() as u64);
    encoded.push(b':');
    encoded.extend_from_slice(value);
}

fn push_integer(encoded: &mut Vec<u8>, value: u64) {
    encoded.extend_from_slice(value.to_string().as_bytes());
}

fn extension_limits() -> Limits {
    Limits {
        max_input_length: 16 * 1024,
        max_string_length: 8 * 1024,
        max_decoded_items: 128,
        max_depth: 4,
        max_collection_entries: 64,
    }
}

fn pex_limits() -> Limits {
    Limits {
        max_input_length: MAX_PEX_PAYLOAD_LENGTH,
        max_string_length: MAX_PEX_PAYLOAD_LENGTH,
        max_decoded_items: 32,
        max_depth: 2,
        max_collection_entries: 12,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExtensionAdvertisement, ExtensionHandshake, ExtensionMap, ExtensionUpdate,
        MAX_PEX_ADDITIONS, MAX_PEX_PAYLOAD_LENGTH, PexContact, PexEndpoint, PexFlags, PexIp,
        PexMessage, encode_extension_handshake, encode_extension_handshake_update,
        encode_pex_message, parse_extension_handshake, parse_pex_message,
    };

    #[test]
    fn repeated_handshake_update_encodes_pex_disable_and_reenable() {
        let disabled = encode_extension_handshake_update(ExtensionHandshake {
            pex: ExtensionUpdate::Disabled,
            ..ExtensionHandshake::default()
        })
        .expect("disable update");
        assert_eq!(
            parse_extension_handshake(&disabled).expect("parse disable"),
            ExtensionHandshake {
                pex: ExtensionUpdate::Disabled,
                ..ExtensionHandshake::default()
            }
        );

        let enabled = encode_extension_handshake_update(ExtensionHandshake {
            pex: ExtensionUpdate::Enabled(2),
            ..ExtensionHandshake::default()
        })
        .expect("enable update");
        assert_eq!(
            parse_extension_handshake(&enabled)
                .expect("parse enable")
                .pex,
            ExtensionUpdate::Enabled(2)
        );
    }

    #[test]
    fn recognized_map_is_directional_additive_and_disable_by_name() {
        let first = parse_extension_handshake(
            b"d1:md11:ut_metadatai7e6:ut_pexi9ee1:pi6881e13:metadata_sizei40000ee",
        )
        .expect("initial handshake");
        let mut map = ExtensionMap::default();
        map.apply(first);
        assert_eq!(map.metadata_id(), Some(7));
        assert_eq!(map.pex_id(), Some(9));
        assert_eq!(map.listen_port(), Some(6881));
        map.apply(parse_extension_handshake(b"d1:md6:ut_pexi0eee").expect("disable PEX"));
        assert_eq!(map.metadata_id(), Some(7));
        assert_eq!(map.pex_id(), None);
        map.apply(parse_extension_handshake(b"d1:md3:fooi4eee").expect("unknown name"));
        assert_eq!(map.metadata_id(), Some(7));
        assert_eq!(map.listen_port(), Some(6881));
    }

    #[test]
    fn handshake_encoding_is_canonical_and_rejects_shared_ids() {
        let advertisement = ExtensionAdvertisement {
            metadata_id: Some(1),
            pex_id: Some(2),
            metadata_size: Some(40_000),
            listen_port: Some(6881),
        };
        let encoded = encode_extension_handshake(advertisement).expect("encode handshake");
        assert_eq!(
            parse_extension_handshake(&encoded).expect("parse"),
            super::ExtensionHandshake {
                metadata: ExtensionUpdate::Enabled(1),
                pex: ExtensionUpdate::Enabled(2),
                metadata_size: Some(40_000),
                listen_port: Some(6881),
            }
        );
        assert!(
            encode_extension_handshake(ExtensionAdvertisement {
                metadata_id: Some(1),
                pex_id: Some(1),
                ..ExtensionAdvertisement::default()
            })
            .is_err()
        );
    }

    #[test]
    fn pex_round_trip_covers_v4_v6_flags_and_drops() {
        let message = PexMessage {
            added: vec![
                PexContact {
                    endpoint: PexEndpoint::v4([198, 51, 100, 7], 6881),
                    flags: PexFlags::from_bits(PexFlags::SEED | PexFlags::OUTGOING),
                },
                PexContact {
                    endpoint: PexEndpoint::v6(
                        [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7],
                        6882,
                    ),
                    flags: PexFlags::from_bits(PexFlags::UTP),
                },
            ],
            dropped: vec![PexEndpoint::v4([203, 0, 113, 9], 6883)],
            ..PexMessage::default()
        };
        let encoded = encode_pex_message(&message).expect("encode PEX");
        assert_eq!(parse_pex_message(&encoded).expect("parse PEX"), message);
    }

    #[test]
    fn pex_rejects_malformed_atomic_inputs_and_normalizes_duplicates() {
        for invalid in [
            b"de".as_slice(),
            b"d5:added5:abcdee".as_slice(),
            b"d5:added6:\x01\x02\x03\x04\x1a\xe16:added.f0:e".as_slice(),
        ] {
            assert!(parse_pex_message(invalid).is_err(), "{invalid:?}");
        }
        let duplicate = b"d5:added12:\xc6\x33\x64\x01\x1a\xe1\xc6\x33\x64\x01\x1a\xe2e";
        let parsed = parse_pex_message(duplicate).expect("duplicate IP is ignored");
        assert_eq!(parsed.added.len(), 1);
        assert_eq!(parsed.added[0].endpoint.port, 6881);
    }

    #[test]
    fn pex_caps_contacts_after_validating_the_whole_payload() {
        let mut added = Vec::new();
        for index in 0..75_u8 {
            added.extend_from_slice(&[198, 51, index, 1, 0x1a, index]);
        }
        let mut payload = format!("d5:added{}:", added.len()).into_bytes();
        payload.extend_from_slice(&added);
        payload.push(b'e');
        let parsed = parse_pex_message(&payload).expect("bounded PEX");
        assert_eq!(parsed.added.len(), MAX_PEX_ADDITIONS);
        assert_eq!(parsed.additions_truncated, 25);
        let mut oversized = vec![b'd'; MAX_PEX_PAYLOAD_LENGTH + 1];
        oversized[0] = b'd';
        assert!(parse_pex_message(&oversized).is_err());
    }

    #[test]
    fn mapped_v6_and_v4_share_one_duplicate_ip() {
        let mapped = PexEndpoint::new(
            PexIp::V6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 198, 51, 100, 8]),
            6882,
        );
        let message = PexMessage {
            added: vec![
                PexContact {
                    endpoint: PexEndpoint::v4([198, 51, 100, 8], 6881),
                    flags: PexFlags::default(),
                },
                PexContact {
                    endpoint: mapped,
                    flags: PexFlags::default(),
                },
            ],
            ..PexMessage::default()
        };
        let parsed =
            parse_pex_message(&encode_pex_message(&message).expect("encode")).expect("parse");
        assert_eq!(parsed.added.len(), 1);
    }
}

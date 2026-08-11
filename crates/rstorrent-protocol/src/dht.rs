//! Runtime-independent Mainline DHT values, codecs, and bounded routing state.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

use crate::bencode::{self, DictionaryEntry, Limits, Node, Value};

pub const NODE_ID_LENGTH: usize = 20;
pub const K: usize = 8;
pub const ALPHA: usize = 3;
pub const MAX_DATAGRAM_SIZE: usize = 1024;
pub const MAX_DHT_DECODED_ITEMS: usize = 1024;
pub const MAX_TRANSACTION_LENGTH: usize = 8;
pub const MAX_RESPONSE_NODES: usize = 16;
pub const MAX_RESPONSE_PEERS: usize = 200;
pub const MAX_ROUTING_NODES: usize = K * 160;
pub const MAX_REPLACEMENTS_PER_BUCKET: usize = K;
pub const GOOD_NODE_AGE_SECONDS: u64 = 15 * 60;
pub const MAX_NODE_FAILURES: u8 = 2;

const CLIENT_VERSION: &[u8; 4] = b"RS01";
const BEP42_V4_MASK: [u8; 4] = [0x03, 0x0f, 0x3f, 0xff];
const BEP42_V6_MASK: [u8; 8] = [0x01, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0xff];

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeId(pub [u8; NODE_ID_LENGTH]);

impl NodeId {
    pub const ZERO: Self = Self([0; NODE_ID_LENGTH]);

    pub fn xor_distance(self, other: Self) -> [u8; NODE_ID_LENGTH] {
        let mut distance = [0; NODE_ID_LENGTH];
        for (output, (left, right)) in distance.iter_mut().zip(self.0.into_iter().zip(other.0)) {
            *output = left ^ right;
        }
        distance
    }

    pub fn compare_distance(left: Self, right: Self, target: Self) -> Ordering {
        left.xor_distance(target).cmp(&right.xor_distance(target))
    }

    /// Returns the number of most-significant bits shared with another ID.
    pub fn shared_prefix_bits(self, other: Self) -> u16 {
        let distance = self.xor_distance(other);
        let whole_bytes = distance.iter().take_while(|byte| **byte == 0).count();
        if whole_bytes == NODE_ID_LENGTH {
            return 160;
        }
        (whole_bytes * 8 + distance[whole_bytes].leading_zeros() as usize) as u16
    }

    fn bucket_index(self, other: Self) -> Option<usize> {
        let shared = usize::from(self.shared_prefix_bits(other));
        if shared < 160 {
            Some(159 - shared)
        } else {
            None
        }
    }
}

impl From<[u8; NODE_ID_LENGTH]> for NodeId {
    fn from(value: [u8; NODE_ID_LENGTH]) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeContact {
    pub id: NodeId,
    pub address: DhtEndpoint,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DhtIp {
    V4([u8; 4]),
    V6([u8; 16]),
}

impl DhtIp {
    pub const fn is_ipv4(self) -> bool {
        matches!(self, Self::V4(_))
    }

    pub const fn is_ipv6(self) -> bool {
        matches!(self, Self::V6(_))
    }

    pub const fn octets(self) -> DhtIpOctets {
        match self {
            Self::V4(bytes) => DhtIpOctets::V4(bytes),
            Self::V6(bytes) => DhtIpOctets::V6(bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DhtIpOctets {
    V4([u8; 4]),
    V6([u8; 16]),
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DhtEndpoint {
    pub ip: DhtIp,
    pub port: u16,
}

impl DhtEndpoint {
    pub const fn new(ip: DhtIp, port: u16) -> Self {
        Self { ip, port }
    }

    pub const fn is_ipv4(self) -> bool {
        self.ip.is_ipv4()
    }

    pub const fn is_ipv6(self) -> bool {
        self.ip.is_ipv6()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Want {
    Ipv4,
    Ipv6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    Ping,
    FindNode {
        target: NodeId,
        want: Vec<Want>,
    },
    GetPeers {
        info_hash: NodeId,
        want: Vec<Want>,
    },
    AnnouncePeer {
        info_hash: NodeId,
        port: u16,
        implied_port: bool,
        token: Vec<u8>,
    },
    Unknown(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryMessage {
    pub transaction: Vec<u8>,
    pub id: NodeId,
    pub query: Query,
    pub read_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseMessage {
    pub transaction: Vec<u8>,
    pub id: NodeId,
    pub nodes: Vec<NodeContact>,
    pub nodes6: Vec<NodeContact>,
    pub peers: Vec<DhtEndpoint>,
    pub token: Option<Vec<u8>>,
    pub observed_address: Option<DhtEndpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorMessage {
    pub transaction: Vec<u8>,
    pub code: i64,
    pub message: Vec<u8>,
    pub observed_address: Option<DhtEndpoint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Query(QueryMessage),
    Response(ResponseMessage),
    Error(ErrorMessage),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DhtCodecError {
    Bencode(bencode::ParseError),
    Missing(&'static str),
    Invalid(&'static str),
    InvalidCompactLength { field: &'static str, length: usize },
    TooManyValues { field: &'static str, maximum: usize },
    EncodedMessageTooLarge { length: usize, maximum: usize },
}

impl fmt::Display for DhtCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bencode(error) => write!(formatter, "invalid DHT bencode: {error}"),
            Self::Missing(field) => write!(formatter, "DHT message is missing {field}"),
            Self::Invalid(field) => write!(formatter, "DHT message has invalid {field}"),
            Self::InvalidCompactLength { field, length } => {
                write!(
                    formatter,
                    "DHT {field} compact value has invalid length {length}"
                )
            }
            Self::TooManyValues { field, maximum } => {
                write!(formatter, "DHT {field} exceeds limit {maximum}")
            }
            Self::EncodedMessageTooLarge { length, maximum } => write!(
                formatter,
                "encoded DHT message length {length} exceeds limit {maximum}"
            ),
        }
    }
}

impl Error for DhtCodecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Bencode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<bencode::ParseError> for DhtCodecError {
    fn from(value: bencode::ParseError) -> Self {
        Self::Bencode(value)
    }
}

pub fn decode_message(bytes: &[u8]) -> Result<Message, DhtCodecError> {
    let root = bencode::parse_with_limits_permissive_dictionaries(
        bytes,
        Limits {
            max_input_length: MAX_DATAGRAM_SIZE,
            max_string_length: MAX_DATAGRAM_SIZE,
            max_decoded_items: MAX_DHT_DECODED_ITEMS,
            max_depth: 8,
            max_collection_entries: 64,
        },
    )?;
    let top = dictionary(&root).ok_or(DhtCodecError::Invalid("root dictionary"))?;
    let transaction = required_bytes(top, b"t", "transaction ID")?;
    validate_transaction(transaction)?;
    let kind = required_bytes(top, b"y", "message type")?;
    if kind.len() != 1 {
        return Err(DhtCodecError::Invalid("message type"));
    }
    match kind[0] {
        b'q' => decode_query(top, transaction),
        b'r' => decode_response(top, transaction),
        b'e' => decode_error(top, transaction),
        _ => Err(DhtCodecError::Invalid("message type")),
    }
}

fn decode_query(top: &[DictionaryEntry<'_>], transaction: &[u8]) -> Result<Message, DhtCodecError> {
    let method = required_bytes(top, b"q", "query method")?;
    let arguments = field(top, b"a")
        .and_then(dictionary)
        .ok_or(DhtCodecError::Missing("query arguments"))?;
    let id = required_id(arguments, b"id", "query node ID")?;
    let want = decode_want(arguments)?;
    let query = match method {
        b"ping" => Query::Ping,
        b"find_node" => Query::FindNode {
            target: required_id(arguments, b"target", "find_node target")?,
            want,
        },
        b"get_peers" => Query::GetPeers {
            info_hash: required_id(arguments, b"info_hash", "get_peers info hash")?,
            want,
        },
        b"announce_peer" => {
            let info_hash = required_id(arguments, b"info_hash", "announce_peer info hash")?;
            let implied_port = optional_integer(arguments, b"implied_port")?.unwrap_or(0) != 0;
            let explicit_port = optional_integer(arguments, b"port")?
                .ok_or(DhtCodecError::Missing("announce_peer port"))?;
            let port = u16::try_from(explicit_port)
                .ok()
                .filter(|port| *port != 0)
                .ok_or(DhtCodecError::Invalid("announce_peer port"))?;
            let token = required_bytes(arguments, b"token", "announce_peer token")?;
            if token.is_empty() || token.len() > 64 {
                return Err(DhtCodecError::Invalid("announce_peer token"));
            }
            Query::AnnouncePeer {
                info_hash,
                port,
                implied_port,
                token: token.to_vec(),
            }
        }
        _ => Query::Unknown(method.to_vec()),
    };
    let read_only = optional_integer(top, b"ro")?.unwrap_or(0) != 0;
    Ok(Message::Query(QueryMessage {
        transaction: transaction.to_vec(),
        id,
        query,
        read_only,
    }))
}

fn decode_response(
    top: &[DictionaryEntry<'_>],
    transaction: &[u8],
) -> Result<Message, DhtCodecError> {
    let response = field(top, b"r")
        .and_then(dictionary)
        .ok_or(DhtCodecError::Missing("response dictionary"))?;
    let id = required_id(response, b"id", "response node ID")?;
    let nodes = optional_bytes(response, b"nodes")?
        .map(decode_nodes_v4)
        .transpose()?
        .unwrap_or_default();
    let nodes6 = optional_bytes(response, b"nodes6")?
        .map(decode_nodes_v6)
        .transpose()?
        .unwrap_or_default();
    let peers = decode_values(response)?;
    let token = optional_bytes(response, b"token")?.map(ToOwned::to_owned);
    if token.as_ref().is_some_and(|token| token.len() > 64) {
        return Err(DhtCodecError::Invalid("response token"));
    }
    let observed_address = optional_bytes(top, b"ip")?
        .map(|value| decode_compact_endpoint(value, "observed address"))
        .transpose()?;
    Ok(Message::Response(ResponseMessage {
        transaction: transaction.to_vec(),
        id,
        nodes,
        nodes6,
        peers,
        token,
        observed_address,
    }))
}

fn decode_error(top: &[DictionaryEntry<'_>], transaction: &[u8]) -> Result<Message, DhtCodecError> {
    let Value::List(values) = &field(top, b"e")
        .ok_or(DhtCodecError::Missing("error values"))?
        .value
    else {
        return Err(DhtCodecError::Invalid("error values"));
    };
    if values.len() != 2 {
        return Err(DhtCodecError::Invalid("error values"));
    }
    let Value::Integer(code) = values[0].value else {
        return Err(DhtCodecError::Invalid("error code"));
    };
    let Value::Bytes(message) = values[1].value else {
        return Err(DhtCodecError::Invalid("error message"));
    };
    if message.len() > 128 {
        return Err(DhtCodecError::Invalid("error message"));
    }
    let observed_address = optional_bytes(top, b"ip")?
        .map(|value| decode_compact_endpoint(value, "observed address"))
        .transpose()?;
    Ok(Message::Error(ErrorMessage {
        transaction: transaction.to_vec(),
        code,
        message: message.to_vec(),
        observed_address,
    }))
}

pub fn encode_query(
    transaction: &[u8],
    id: NodeId,
    query: &Query,
    read_only: bool,
) -> Result<Vec<u8>, DhtCodecError> {
    validate_transaction(transaction)?;
    let mut output = Vec::with_capacity(160);
    output.push(b'd');
    bytes_field_start(&mut output, b"a");
    output.push(b'd');
    bytes_field(&mut output, b"id", &id.0);
    match query {
        Query::Ping => {}
        Query::FindNode { target, want } => {
            bytes_field(&mut output, b"target", &target.0);
            encode_want(&mut output, want);
        }
        Query::GetPeers { info_hash, want } => {
            bytes_field(&mut output, b"info_hash", &info_hash.0);
            encode_want(&mut output, want);
        }
        Query::AnnouncePeer {
            info_hash,
            port,
            implied_port,
            token,
        } => {
            if *port == 0 || token.is_empty() || token.len() > 64 {
                return Err(DhtCodecError::Invalid("announce_peer arguments"));
            }
            if *implied_port {
                integer_field(&mut output, b"implied_port", 1);
            }
            bytes_field(&mut output, b"info_hash", &info_hash.0);
            integer_field(&mut output, b"port", i64::from(*port));
            bytes_field(&mut output, b"token", token);
        }
        Query::Unknown(_) => return Err(DhtCodecError::Invalid("outgoing query method")),
    }
    output.push(b'e');
    let method = match query {
        Query::Ping => b"ping".as_slice(),
        Query::FindNode { .. } => b"find_node".as_slice(),
        Query::GetPeers { .. } => b"get_peers".as_slice(),
        Query::AnnouncePeer { .. } => b"announce_peer".as_slice(),
        Query::Unknown(_) => unreachable!(),
    };
    bytes_field(&mut output, b"q", method);
    if read_only {
        integer_field(&mut output, b"ro", 1);
    }
    bytes_field(&mut output, b"t", transaction);
    bytes_field(&mut output, b"v", CLIENT_VERSION);
    bytes_field(&mut output, b"y", b"q");
    output.push(b'e');
    finish_encode(output)
}

pub fn encode_response(
    transaction: &[u8],
    id: NodeId,
    nodes: &[NodeContact],
    peers: &[DhtEndpoint],
    token: Option<&[u8]>,
    observed_address: DhtEndpoint,
) -> Result<Vec<u8>, DhtCodecError> {
    validate_transaction(transaction)?;
    if nodes.len() > MAX_RESPONSE_NODES {
        return Err(DhtCodecError::TooManyValues {
            field: "response nodes",
            maximum: MAX_RESPONSE_NODES,
        });
    }
    if peers.len() > MAX_RESPONSE_PEERS {
        return Err(DhtCodecError::TooManyValues {
            field: "response peers",
            maximum: MAX_RESPONSE_PEERS,
        });
    }
    if token.is_some_and(|token| token.is_empty() || token.len() > 64) {
        return Err(DhtCodecError::Invalid("response token"));
    }
    let mut output = Vec::with_capacity(384);
    output.push(b'd');
    bytes_field(
        &mut output,
        b"ip",
        &encode_compact_endpoint(observed_address),
    );
    bytes_field_start(&mut output, b"r");
    output.push(b'd');
    bytes_field(&mut output, b"id", &id.0);
    let nodes4 = nodes
        .iter()
        .copied()
        .filter(|node| node.address.is_ipv4())
        .collect::<Vec<_>>();
    if !nodes4.is_empty() {
        bytes_field(&mut output, b"nodes", &encode_nodes(&nodes4));
    }
    let nodes6 = nodes
        .iter()
        .copied()
        .filter(|node| node.address.is_ipv6())
        .collect::<Vec<_>>();
    if !nodes6.is_empty() {
        bytes_field(&mut output, b"nodes6", &encode_nodes(&nodes6));
    }
    if let Some(token) = token {
        bytes_field(&mut output, b"token", token);
    }
    if !peers.is_empty() {
        bytes_field_start(&mut output, b"values");
        output.push(b'l');
        for peer in peers {
            bytes_value(&mut output, &encode_compact_endpoint(*peer));
        }
        output.push(b'e');
    }
    output.push(b'e');
    bytes_field(&mut output, b"t", transaction);
    bytes_field(&mut output, b"v", CLIENT_VERSION);
    bytes_field(&mut output, b"y", b"r");
    output.push(b'e');
    finish_encode(output)
}

pub fn encode_error(
    transaction: &[u8],
    code: i64,
    message: &[u8],
    observed_address: DhtEndpoint,
) -> Result<Vec<u8>, DhtCodecError> {
    validate_transaction(transaction)?;
    if message.is_empty() || message.len() > 128 {
        return Err(DhtCodecError::Invalid("error message"));
    }
    let mut output = Vec::with_capacity(128);
    output.push(b'd');
    bytes_field_start(&mut output, b"e");
    output.push(b'l');
    integer_value(&mut output, code);
    bytes_value(&mut output, message);
    output.push(b'e');
    bytes_field(
        &mut output,
        b"ip",
        &encode_compact_endpoint(observed_address),
    );
    bytes_field(&mut output, b"t", transaction);
    bytes_field(&mut output, b"v", CLIENT_VERSION);
    bytes_field(&mut output, b"y", b"e");
    output.push(b'e');
    finish_encode(output)
}

fn finish_encode(output: Vec<u8>) -> Result<Vec<u8>, DhtCodecError> {
    if output.len() > MAX_DATAGRAM_SIZE {
        return Err(DhtCodecError::EncodedMessageTooLarge {
            length: output.len(),
            maximum: MAX_DATAGRAM_SIZE,
        });
    }
    Ok(output)
}

fn validate_transaction(transaction: &[u8]) -> Result<(), DhtCodecError> {
    if transaction.is_empty() || transaction.len() > MAX_TRANSACTION_LENGTH {
        return Err(DhtCodecError::Invalid("transaction ID"));
    }
    Ok(())
}

fn dictionary<'a>(node: &'a Node<'a>) -> Option<&'a [DictionaryEntry<'a>]> {
    match &node.value {
        Value::Dictionary(entries) => Some(entries),
        _ => None,
    }
}

fn field<'a>(entries: &'a [DictionaryEntry<'a>], key: &[u8]) -> Option<&'a Node<'a>> {
    entries
        .binary_search_by(|entry| entry.key.cmp(key))
        .ok()
        .map(|index| &entries[index].value)
}

fn required_bytes<'a>(
    entries: &'a [DictionaryEntry<'a>],
    key: &[u8],
    name: &'static str,
) -> Result<&'a [u8], DhtCodecError> {
    optional_bytes(entries, key)?.ok_or(DhtCodecError::Missing(name))
}

fn optional_bytes<'a>(
    entries: &'a [DictionaryEntry<'a>],
    key: &[u8],
) -> Result<Option<&'a [u8]>, DhtCodecError> {
    let Some(node) = field(entries, key) else {
        return Ok(None);
    };
    match &node.value {
        Value::Bytes(bytes) => Ok(Some(bytes)),
        _ => Err(DhtCodecError::Invalid("byte string field")),
    }
}

fn optional_integer(
    entries: &[DictionaryEntry<'_>],
    key: &[u8],
) -> Result<Option<i64>, DhtCodecError> {
    let Some(node) = field(entries, key) else {
        return Ok(None);
    };
    match node.value {
        Value::Integer(value) => Ok(Some(value)),
        _ => Err(DhtCodecError::Invalid("integer field")),
    }
}

fn required_id(
    entries: &[DictionaryEntry<'_>],
    key: &[u8],
    name: &'static str,
) -> Result<NodeId, DhtCodecError> {
    let value = required_bytes(entries, key, name)?;
    let bytes = value.try_into().map_err(|_| DhtCodecError::Invalid(name))?;
    Ok(NodeId(bytes))
}

fn decode_want(entries: &[DictionaryEntry<'_>]) -> Result<Vec<Want>, DhtCodecError> {
    let Some(node) = field(entries, b"want") else {
        return Ok(Vec::new());
    };
    let Value::List(values) = &node.value else {
        return Err(DhtCodecError::Invalid("want"));
    };
    if values.len() > 8 {
        return Err(DhtCodecError::TooManyValues {
            field: "want",
            maximum: 8,
        });
    }
    let mut wants = Vec::new();
    for value in values {
        let Value::Bytes(value) = value.value else {
            return Err(DhtCodecError::Invalid("want entry"));
        };
        let want = match value {
            b"n4" => Some(Want::Ipv4),
            b"n6" => Some(Want::Ipv6),
            _ => None,
        };
        if let Some(want) = want
            && !wants.contains(&want)
        {
            wants.push(want);
        }
    }
    Ok(wants)
}

fn decode_values(entries: &[DictionaryEntry<'_>]) -> Result<Vec<DhtEndpoint>, DhtCodecError> {
    let Some(node) = field(entries, b"values") else {
        return Ok(Vec::new());
    };
    let Value::List(values) = &node.value else {
        return Err(DhtCodecError::Invalid("values"));
    };
    if values.len() > MAX_RESPONSE_PEERS {
        return Err(DhtCodecError::TooManyValues {
            field: "values",
            maximum: MAX_RESPONSE_PEERS,
        });
    }
    let mut peers = Vec::new();
    for value in values {
        let Value::Bytes(value) = value.value else {
            return Err(DhtCodecError::Invalid("values entry"));
        };
        if value.len() > 18 && value.len() % 6 == 0 {
            for peer in value.chunks_exact(6) {
                push_unique_peer(&mut peers, decode_compact_endpoint(peer, "values entry")?)?;
            }
        } else {
            push_unique_peer(&mut peers, decode_compact_endpoint(value, "values entry")?)?;
        }
    }
    Ok(peers)
}

fn push_unique_peer(peers: &mut Vec<DhtEndpoint>, peer: DhtEndpoint) -> Result<(), DhtCodecError> {
    if !peers.contains(&peer) {
        if peers.len() == MAX_RESPONSE_PEERS {
            return Err(DhtCodecError::TooManyValues {
                field: "values",
                maximum: MAX_RESPONSE_PEERS,
            });
        }
        peers.push(peer);
    }
    Ok(())
}

fn decode_nodes_v4(value: &[u8]) -> Result<Vec<NodeContact>, DhtCodecError> {
    if !value.len().is_multiple_of(26) {
        return Err(DhtCodecError::InvalidCompactLength {
            field: "nodes",
            length: value.len(),
        });
    }
    if value.len() / 26 > MAX_RESPONSE_NODES {
        return Err(DhtCodecError::TooManyValues {
            field: "nodes",
            maximum: MAX_RESPONSE_NODES,
        });
    }
    value
        .chunks_exact(26)
        .map(|value| {
            let id = NodeId(value[..20].try_into().expect("20-byte node ID"));
            let address = decode_compact_endpoint(&value[20..], "nodes")?;
            Ok(NodeContact { id, address })
        })
        .collect()
}

fn decode_nodes_v6(value: &[u8]) -> Result<Vec<NodeContact>, DhtCodecError> {
    if !value.len().is_multiple_of(38) {
        return Err(DhtCodecError::InvalidCompactLength {
            field: "nodes6",
            length: value.len(),
        });
    }
    if value.len() / 38 > MAX_RESPONSE_NODES {
        return Err(DhtCodecError::TooManyValues {
            field: "nodes6",
            maximum: MAX_RESPONSE_NODES,
        });
    }
    value
        .chunks_exact(38)
        .map(|value| {
            let id = NodeId(value[..20].try_into().expect("20-byte node ID"));
            let address = decode_compact_endpoint(&value[20..], "nodes6")?;
            Ok(NodeContact { id, address })
        })
        .collect()
}

pub fn encode_compact_endpoint(address: DhtEndpoint) -> Vec<u8> {
    let mut output = Vec::with_capacity(if address.is_ipv4() { 6 } else { 18 });
    match address.ip {
        DhtIp::V4(ip) => output.extend_from_slice(&ip),
        DhtIp::V6(ip) => output.extend_from_slice(&ip),
    }
    output.extend_from_slice(&address.port.to_be_bytes());
    output
}

pub fn decode_compact_endpoint(
    value: &[u8],
    field: &'static str,
) -> Result<DhtEndpoint, DhtCodecError> {
    let address = match value.len() {
        6 => DhtEndpoint::new(
            DhtIp::V4([value[0], value[1], value[2], value[3]]),
            u16::from_be_bytes([value[4], value[5]]),
        ),
        18 => {
            let ip = <[u8; 16]>::try_from(&value[..16]).expect("16-byte IPv6");
            DhtEndpoint::new(DhtIp::V6(ip), u16::from_be_bytes([value[16], value[17]]))
        }
        length => return Err(DhtCodecError::InvalidCompactLength { field, length }),
    };
    if !is_valid_contact(address) {
        return Err(DhtCodecError::Invalid(field));
    }
    Ok(address)
}

fn encode_nodes(nodes: &[NodeContact]) -> Vec<u8> {
    let mut output = Vec::new();
    for node in nodes {
        output.extend_from_slice(&node.id.0);
        output.extend_from_slice(&encode_compact_endpoint(node.address));
    }
    output
}

fn encode_want(output: &mut Vec<u8>, wants: &[Want]) {
    if wants.is_empty() {
        return;
    }
    bytes_field_start(output, b"want");
    output.push(b'l');
    if wants.contains(&Want::Ipv4) {
        bytes_value(output, b"n4");
    }
    if wants.contains(&Want::Ipv6) {
        bytes_value(output, b"n6");
    }
    output.push(b'e');
}

fn bytes_field(output: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    bytes_value(output, key);
    bytes_value(output, value);
}

fn bytes_field_start(output: &mut Vec<u8>, key: &[u8]) {
    bytes_value(output, key);
}

fn integer_field(output: &mut Vec<u8>, key: &[u8], value: i64) {
    bytes_value(output, key);
    integer_value(output, value);
}

fn bytes_value(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(value.len().to_string().as_bytes());
    output.push(b':');
    output.extend_from_slice(value);
}

fn integer_value(output: &mut Vec<u8>, value: i64) {
    output.push(b'i');
    output.extend_from_slice(value.to_string().as_bytes());
    output.push(b'e');
}

fn is_valid_contact(address: DhtEndpoint) -> bool {
    if address.port == 0 {
        return false;
    }
    match address.ip {
        DhtIp::V4(bytes) => {
            bytes != [0; 4] && bytes != [255; 4] && !(224..=239).contains(&bytes[0])
        }
        DhtIp::V6(bytes) => bytes != [0; 16] && bytes[0] != 0xff,
    }
}

/// Generate a BEP 42 node ID using caller-provided random bytes.
///
/// `random[19]` supplies the full `r` byte. The remaining bytes are retained
/// except for the constrained 21-bit prefix.
pub fn generate_bep42_id(address: DhtIp, mut random: [u8; 20]) -> NodeId {
    let r = random[19];
    let crc = bep42_crc(address, r);
    random[0] = (crc >> 24) as u8;
    random[1] = (crc >> 16) as u8;
    random[2] = ((crc >> 8) as u8 & 0xf8) | (random[2] & 0x07);
    NodeId(random)
}

pub fn verify_bep42_id(id: NodeId, address: DhtIp) -> bool {
    if is_local_address(address) {
        return true;
    }
    let crc = bep42_crc(address, id.0[19]);
    id.0[0] == (crc >> 24) as u8
        && id.0[1] == (crc >> 16) as u8
        && id.0[2] & 0xf8 == (crc >> 8) as u8 & 0xf8
}

fn bep42_crc(address: DhtIp, r: u8) -> u32 {
    let mut bytes = match address {
        DhtIp::V4(address) => address.to_vec(),
        DhtIp::V6(address) => address[..8].to_vec(),
    };
    let mask: &[u8] = if bytes.len() == 4 {
        &BEP42_V4_MASK
    } else {
        &BEP42_V6_MASK
    };
    for (byte, mask) in bytes.iter_mut().zip(mask) {
        *byte &= mask;
    }
    *bytes.first_mut().expect("IP has bytes") |= (r & 0x07) << 5;
    crc32c(&bytes)
}

fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82f6_3b78_u32 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn is_local_address(address: DhtIp) -> bool {
    match address {
        DhtIp::V4(address) => {
            address[0] == 127
                || address[0] == 10
                || (address[0] == 172 && (16..=31).contains(&address[1]))
                || (address[0] == 192 && address[1] == 168)
                || (address[0] == 169 && address[1] == 254)
                || address == [0; 4]
        }
        DhtIp::V6(address) => {
            address == [0; 16]
                || address == {
                    let mut loopback = [0; 16];
                    loopback[15] = 1;
                    loopback
                }
                || (address[0] == 0xfe && address[1] & 0xc0 == 0x80)
                || address[0] & 0xfe == 0xfc
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingNodeState {
    Good,
    Questionable,
    Bad,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingNode {
    pub contact: NodeContact,
    pub last_response_seconds: u64,
    pub last_query_seconds: Option<u64>,
    pub failures: u8,
}

impl RoutingNode {
    pub fn state(self, now_seconds: u64) -> RoutingNodeState {
        if self.failures >= MAX_NODE_FAILURES {
            RoutingNodeState::Bad
        } else if now_seconds.saturating_sub(self.last_response_seconds) <= GOOD_NODE_AGE_SECONDS {
            RoutingNodeState::Good
        } else {
            RoutingNodeState::Questionable
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Bucket {
    live: Vec<RoutingNode>,
    replacements: Vec<RoutingNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingAdmission {
    Added,
    Refreshed,
    Replaced(NodeContact),
    ReplacementCached,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingBucketInspection {
    pub bucket_index: u16,
    pub good_nodes: u8,
    pub questionable_nodes: u8,
    pub replacement_candidates: u8,
    pub oldest_live_response_age_seconds: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutingTableInspection {
    pub routing_nodes: u16,
    pub occupied_buckets: u16,
    pub deepest_shared_prefix_bits: Option<u16>,
    pub buckets: Vec<RoutingBucketInspection>,
}

/// Fixed-distance K-buckets with independently bounded replacement caches.
///
/// Only `record_response` admits a node to the live table. `heard_about`
/// retains an untrusted candidate in the replacement cache until it proves
/// its endpoint with a correlated response.
#[derive(Clone, Debug)]
pub struct RoutingTable {
    local_id: NodeId,
    buckets: Vec<Bucket>,
}

impl RoutingTable {
    pub fn new(local_id: NodeId) -> Self {
        Self {
            local_id,
            buckets: vec![Bucket::default(); 160],
        }
    }

    pub fn local_id(&self) -> NodeId {
        self.local_id
    }

    pub fn len(&self) -> usize {
        self.buckets.iter().map(|bucket| bucket.live.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Captures endpoint-free routing occupancy at one monotonic instant.
    pub fn inspection(&self, now_seconds: u64) -> RoutingTableInspection {
        let mut routing_nodes = 0_u16;
        let mut occupied_buckets = 0_u16;
        let mut deepest_shared_prefix_bits = None;
        let buckets = self
            .buckets
            .iter()
            .enumerate()
            .map(|(bucket_index, bucket)| {
                let mut good_nodes = 0_u8;
                let mut questionable_nodes = 0_u8;
                let mut oldest_live_response_age_seconds = None;
                for node in &bucket.live {
                    match node.state(now_seconds) {
                        RoutingNodeState::Good => good_nodes = good_nodes.saturating_add(1),
                        RoutingNodeState::Questionable => {
                            questionable_nodes = questionable_nodes.saturating_add(1);
                        }
                        RoutingNodeState::Bad => {}
                    }
                    let age = now_seconds.saturating_sub(node.last_response_seconds);
                    oldest_live_response_age_seconds = Some(
                        oldest_live_response_age_seconds.map_or(age, |oldest: u64| oldest.max(age)),
                    );
                }
                let live = u16::from(good_nodes) + u16::from(questionable_nodes);
                if live != 0 {
                    routing_nodes = routing_nodes.saturating_add(live);
                    occupied_buckets = occupied_buckets.saturating_add(1);
                    let depth = 159_u16.saturating_sub(bucket_index as u16);
                    deepest_shared_prefix_bits = Some(
                        deepest_shared_prefix_bits.map_or(depth, |deepest: u16| deepest.max(depth)),
                    );
                }
                RoutingBucketInspection {
                    bucket_index: bucket_index as u16,
                    good_nodes,
                    questionable_nodes,
                    replacement_candidates: bucket.replacements.len() as u8,
                    oldest_live_response_age_seconds,
                }
            })
            .collect();
        RoutingTableInspection {
            routing_nodes,
            occupied_buckets,
            deepest_shared_prefix_bits,
            buckets,
        }
    }

    pub fn record_response(&mut self, contact: NodeContact, now_seconds: u64) -> RoutingAdmission {
        let Some(index) = self.accepted_bucket(contact) else {
            return RoutingAdmission::Rejected;
        };
        if let Some(existing) = self.find_by_endpoint(contact.address)
            && existing != contact.id
        {
            return RoutingAdmission::Rejected;
        }
        let bucket = &mut self.buckets[index];
        if let Some(node) = bucket
            .live
            .iter_mut()
            .find(|node| node.contact.id == contact.id)
        {
            if node.contact.address != contact.address {
                return RoutingAdmission::Rejected;
            }
            node.last_response_seconds = now_seconds;
            node.failures = 0;
            return RoutingAdmission::Refreshed;
        }
        bucket.replacements.retain(|node| {
            node.contact.id != contact.id && node.contact.address != contact.address
        });
        let replacement = RoutingNode {
            contact,
            last_response_seconds: now_seconds,
            last_query_seconds: None,
            failures: 0,
        };
        if bucket.live.len() < K {
            bucket.live.push(replacement);
            return RoutingAdmission::Added;
        }
        if let Some(index) = bucket
            .live
            .iter()
            .enumerate()
            .filter(|(_, node)| node.state(now_seconds) != RoutingNodeState::Good)
            .max_by_key(|(_, node)| {
                (
                    node.failures,
                    now_seconds.saturating_sub(node.last_response_seconds),
                )
            })
            .map(|(index, _)| index)
        {
            let evicted = bucket.live.swap_remove(index).contact;
            bucket.live.push(replacement);
            return RoutingAdmission::Replaced(evicted);
        }
        cache_replacement(bucket, replacement);
        RoutingAdmission::ReplacementCached
    }

    pub fn heard_about(&mut self, contact: NodeContact, now_seconds: u64) -> RoutingAdmission {
        let Some(index) = self.accepted_bucket(contact) else {
            return RoutingAdmission::Rejected;
        };
        if self.find_by_endpoint(contact.address).is_some()
            || self
                .buckets
                .iter()
                .flat_map(|bucket| bucket.live.iter().chain(&bucket.replacements))
                .any(|node| node.contact.id == contact.id)
        {
            return RoutingAdmission::Rejected;
        }
        cache_replacement(
            &mut self.buckets[index],
            RoutingNode {
                contact,
                last_response_seconds: now_seconds.saturating_sub(GOOD_NODE_AGE_SECONDS + 1),
                last_query_seconds: None,
                failures: 0,
            },
        );
        RoutingAdmission::ReplacementCached
    }

    pub fn record_failure(&mut self, contact: NodeContact) -> bool {
        let Some(index) = self.local_id.bucket_index(contact.id) else {
            return false;
        };
        let bucket = &mut self.buckets[index];
        let Some(node_index) = bucket.live.iter().position(|node| node.contact == contact) else {
            return false;
        };
        let node = &mut bucket.live[node_index];
        node.failures = node.failures.saturating_add(1);
        if node.failures < MAX_NODE_FAILURES {
            return true;
        }
        bucket.live.swap_remove(node_index);
        if let Some((replacement_index, _)) = bucket
            .replacements
            .iter()
            .enumerate()
            .max_by_key(|(_, node)| node.last_response_seconds)
        {
            bucket
                .live
                .push(bucket.replacements.swap_remove(replacement_index));
        }
        true
    }

    pub fn closest(&self, target: NodeId, maximum: usize, now_seconds: u64) -> Vec<NodeContact> {
        let maximum = maximum.min(K);
        let mut nodes = self
            .buckets
            .iter()
            .flat_map(|bucket| &bucket.live)
            .filter(|node| node.state(now_seconds) != RoutingNodeState::Bad)
            .map(|node| node.contact)
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| NodeId::compare_distance(left.id, right.id, target));
        nodes.truncate(maximum);
        nodes
    }

    pub fn responsive_sample(
        &self,
        family_v6: bool,
        maximum: usize,
        now_seconds: u64,
    ) -> Vec<NodeContact> {
        let mut nodes = self
            .buckets
            .iter()
            .flat_map(|bucket| &bucket.live)
            .filter(|node| {
                node.contact.address.is_ipv6() == family_v6
                    && node.state(now_seconds) == RoutingNodeState::Good
            })
            .copied()
            .collect::<Vec<_>>();
        nodes.sort_by_key(|node| std::cmp::Reverse(node.last_response_seconds));
        nodes
            .into_iter()
            .map(|node| node.contact)
            .take(maximum)
            .collect()
    }

    fn accepted_bucket(&self, contact: NodeContact) -> Option<usize> {
        if contact.id == NodeId::ZERO
            || !is_valid_contact(contact.address)
            || !verify_bep42_id(contact.id, contact.address.ip)
        {
            return None;
        }
        let index = self.local_id.bucket_index(contact.id)?;
        let prefix_duplicate = self.buckets[index]
            .live
            .iter()
            .chain(&self.buckets[index].replacements)
            .any(|node| {
                node.contact.id != contact.id
                    && !is_local_address(contact.address.ip)
                    && same_network_prefix(node.contact.address.ip, contact.address.ip)
            });
        (!prefix_duplicate).then_some(index)
    }

    fn find_by_endpoint(&self, address: DhtEndpoint) -> Option<NodeId> {
        self.buckets
            .iter()
            .flat_map(|bucket| bucket.live.iter().chain(&bucket.replacements))
            .find(|node| node.contact.address == address)
            .map(|node| node.contact.id)
    }
}

fn cache_replacement(bucket: &mut Bucket, node: RoutingNode) {
    if bucket.replacements.len() == MAX_REPLACEMENTS_PER_BUCKET {
        let oldest = bucket
            .replacements
            .iter()
            .enumerate()
            .min_by_key(|(_, node)| node.last_response_seconds)
            .map(|(index, _)| index)
            .expect("full replacement bucket has an oldest entry");
        bucket.replacements.swap_remove(oldest);
    }
    bucket.replacements.push(node);
}

fn same_network_prefix(left: DhtIp, right: DhtIp) -> bool {
    match (left, right) {
        (DhtIp::V4(left), DhtIp::V4(right)) => left[..3] == right[..3],
        (DhtIp::V6(left), DhtIp::V6(right)) => left[..8] == right[..8],
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u8) -> NodeId {
        let mut id = [0; 20];
        id[19] = value;
        NodeId(id)
    }

    #[test]
    fn bep42_matches_published_ipv4_vectors() {
        let vectors = [
            ([124, 31, 75, 21], 1_u8, "5fbfbf", 0x01),
            ([21, 75, 31, 124], 86, "5a3ce8", 0x56),
            ([65, 23, 51, 170], 22, "a5d430", 0x16),
            ([84, 124, 73, 14], 65, "1b0320", 0x41),
            ([43, 213, 53, 83], 90, "e56f68", 0x5a),
        ];
        for (address, r, prefix, last) in vectors {
            let mut random = [0x55; 20];
            random[19] = r;
            let generated = generate_bep42_id(DhtIp::V4(address), random);
            assert_eq!(
                hex(&generated.0[..3]) & 0xfffff8,
                parse_hex(prefix) & 0xfffff8
            );
            assert_eq!(generated.0[19], last);
            assert!(verify_bep42_id(generated, DhtIp::V4(address)));
        }
    }

    #[test]
    fn bep42_ipv6_mask_matches_pinned_libtorrent() {
        assert_eq!(
            BEP42_V6_MASK,
            [0x01, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0xff]
        );
        let address = DhtIp::V6([
            0x20, 1, 0x0d, 0xb8, 0xab, 0xcd, 0xef, 1, 2, 3, 4, 5, 6, 7, 8, 9,
        ]);
        let mut random = [0x55; 20];
        random[19] = 6;
        let generated = generate_bep42_id(address, random);
        assert!(verify_bep42_id(generated, address));
    }

    #[test]
    fn query_and_response_round_trip_with_bep32_values() {
        let query = Query::GetPeers {
            info_hash: id(9),
            want: vec![Want::Ipv4, Want::Ipv6],
        };
        let encoded = encode_query(b"ab", id(1), &query, false).expect("encode query");
        assert_eq!(
            decode_message(&encoded).expect("decode query"),
            Message::Query(QueryMessage {
                transaction: b"ab".to_vec(),
                id: id(1),
                query,
                read_only: false,
            })
        );

        let nodes = [
            NodeContact {
                id: id(2),
                address: ep4(6881),
            },
            NodeContact {
                id: id(3),
                address: DhtEndpoint::new(
                    DhtIp::V6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    6882,
                ),
            },
        ];
        let peers = [
            ep4(51413),
            DhtEndpoint::new(
                DhtIp::V6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]),
                51414,
            ),
        ];
        let observed = ep4(40000);
        let encoded = encode_response(b"ab", id(4), &nodes, &peers, Some(b"token"), observed)
            .expect("encode response");
        assert_eq!(
            decode_message(&encoded).expect("decode response"),
            Message::Response(ResponseMessage {
                transaction: b"ab".to_vec(),
                id: id(4),
                nodes: vec![nodes[0]],
                nodes6: vec![nodes[1]],
                peers: peers.to_vec(),
                token: Some(b"token".to_vec()),
                observed_address: Some(observed),
            })
        );
    }

    #[test]
    fn decoder_rejects_oversize_and_wrong_compact_lengths() {
        let oversized = vec![b'x'; MAX_DATAGRAM_SIZE + 1];
        assert!(matches!(
            decode_message(&oversized),
            Err(DhtCodecError::Bencode(
                bencode::ParseError::InputTooLarge { .. }
            ))
        ));
        let malformed = b"d1:rd2:id20:012345678901234567895:nodes1:xe1:t2:aa1:y1:re";
        assert!(matches!(
            decode_message(malformed),
            Err(DhtCodecError::InvalidCompactLength {
                field: "nodes",
                length: 1
            })
        ));
    }

    #[test]
    fn decoder_accepts_libtorrent_wire_dictionary_order() {
        let response = b"d1:rd2:id20:01234567890123456789e1:t2:aa1:y1:r1:v4:LT01e";
        assert!(matches!(
            decode_message(response),
            Ok(Message::Response(ResponseMessage { transaction, .. })) if transaction == b"aa"
        ));
    }

    #[test]
    fn routing_admits_only_correlated_responses_and_promotes_replacements() {
        let now = 10_000;
        let mut table = RoutingTable::new(NodeId::ZERO);
        let contacts = (1_u16..=10)
            .map(|port| NodeContact {
                id: NodeId({
                    let mut bytes = [0; 20];
                    bytes[0] = 0x80;
                    bytes[18..].copy_from_slice(&port.to_be_bytes());
                    bytes
                }),
                address: ep4(port),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            table.heard_about(contacts[0], now),
            RoutingAdmission::ReplacementCached
        );
        assert!(table.is_empty());
        for contact in &contacts[..8] {
            assert!(matches!(
                table.record_response(*contact, now),
                RoutingAdmission::Added
            ));
        }
        assert_eq!(table.len(), K);
        assert_eq!(
            table.record_response(contacts[8], now),
            RoutingAdmission::ReplacementCached
        );
        assert!(table.record_failure(contacts[0]));
        assert!(table.record_failure(contacts[0]));
        assert_eq!(table.len(), K);
        assert!(table.closest(contacts[8].id, K, now).contains(&contacts[8]));
    }

    #[test]
    fn shared_prefix_and_routing_inspection_preserve_all_fixed_buckets() {
        let local = NodeId::ZERO;
        let at_depth = |depth: u16, suffix: u16| {
            let mut bytes = [0_u8; NODE_ID_LENGTH];
            let byte = usize::from(depth / 8);
            let bit = depth % 8;
            bytes[byte] = 0x80 >> bit;
            bytes[18..].copy_from_slice(&suffix.to_be_bytes());
            NodeId(bytes)
        };
        assert_eq!(local.shared_prefix_bits(local), 160);
        assert_eq!(local.bucket_index(local), None);
        assert_eq!(local.shared_prefix_bits(at_depth(0, 1)), 0);
        assert_eq!(local.shared_prefix_bits(at_depth(17, 2)), 17);
        assert_eq!(local.shared_prefix_bits(at_depth(24, 3)), 24);
        assert_eq!(local.shared_prefix_bits(at_depth(159, 1)), 159);

        let now = 10_000;
        let mut table = RoutingTable::new(local);
        let depth_zero = NodeContact {
            id: at_depth(0, 1),
            address: ep4(6101),
        };
        let depth_seventeen = NodeContact {
            id: at_depth(17, 2),
            address: ep4(6102),
        };
        let depth_one_fifty_nine = NodeContact {
            id: at_depth(159, 1),
            address: ep4(6103),
        };
        assert_eq!(
            table.record_response(depth_zero, now),
            RoutingAdmission::Added
        );
        assert_eq!(
            table.record_response(depth_seventeen, now - GOOD_NODE_AGE_SECONDS - 1),
            RoutingAdmission::Added
        );
        assert_eq!(
            table.record_response(depth_one_fifty_nine, now - 42),
            RoutingAdmission::Added
        );
        let replacement = NodeContact {
            id: at_depth(24, 4),
            address: ep4(6104),
        };
        assert_eq!(
            table.heard_about(replacement, now),
            RoutingAdmission::ReplacementCached
        );

        let inspection = table.inspection(now);
        assert_eq!(inspection.buckets.len(), 160);
        assert_eq!(inspection.routing_nodes, 3);
        assert_eq!(inspection.occupied_buckets, 3);
        assert_eq!(inspection.deepest_shared_prefix_bits, Some(159));
        assert_eq!(inspection.buckets[159].good_nodes, 1);
        assert_eq!(inspection.buckets[142].questionable_nodes, 1);
        assert_eq!(
            inspection.buckets[142].oldest_live_response_age_seconds,
            Some(GOOD_NODE_AGE_SECONDS + 1)
        );
        assert_eq!(inspection.buckets[135].replacement_candidates, 1);
        assert_eq!(inspection.buckets[0].good_nodes, 1);
        assert_eq!(inspection.buckets[0].bucket_index, 0);
        assert_eq!(inspection.buckets[159].bucket_index, 159);
    }

    #[test]
    fn endpoint_and_id_conflicts_fail_closed() {
        let now = 1;
        let mut table = RoutingTable::new(id(1));
        let original = NodeContact {
            id: id(2),
            address: ep4(6000),
        };
        assert_eq!(
            table.record_response(original, now),
            RoutingAdmission::Added
        );
        assert_eq!(
            table.record_response(
                NodeContact {
                    id: id(3),
                    ..original
                },
                now
            ),
            RoutingAdmission::Rejected
        );
        assert_eq!(
            table.record_response(
                NodeContact {
                    address: ep4(6001),
                    ..original
                },
                now
            ),
            RoutingAdmission::Rejected
        );
    }

    fn hex(bytes: &[u8]) -> u32 {
        bytes
            .iter()
            .fold(0, |value, byte| (value << 8) | u32::from(*byte))
    }

    fn ep4(port: u16) -> DhtEndpoint {
        DhtEndpoint::new(DhtIp::V4([127, 0, 0, 1]), port)
    }

    fn parse_hex(value: &str) -> u32 {
        u32::from_str_radix(value, 16).expect("hex")
    }
}

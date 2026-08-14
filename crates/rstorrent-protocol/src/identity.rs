//! Runtime-independent torrent protocol identities.

use std::fmt;

/// The protocol version that gives an info hash its meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProtocolVersion {
    V1,
    V2,
}

impl ProtocolVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

/// A full BEP 3 SHA-1 info hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct V1InfoHash([u8; 20]);

impl V1InfoHash {
    pub const fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 20] {
        self.0
    }
}

impl From<[u8; 20]> for V1InfoHash {
    fn from(value: [u8; 20]) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for V1InfoHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_lower_hex(formatter, &self.0)
    }
}

/// A full BEP 52 SHA-256 info hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct V2InfoHash([u8; 32]);

impl V2InfoHash {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn swarm_key(self) -> SwarmKey {
        let mut truncated = [0_u8; 20];
        truncated.copy_from_slice(&self.0[..20]);
        SwarmKey::V2Truncated(truncated)
    }
}

impl From<[u8; 32]> for V2InfoHash {
    fn from(value: [u8; 32]) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for V2InfoHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_lower_hex(formatter, &self.0)
    }
}

/// One full, protocol-tagged torrent identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FullInfoHash {
    V1(V1InfoHash),
    V2(V2InfoHash),
}

impl FullInfoHash {
    pub const fn protocol(self) -> ProtocolVersion {
        match self {
            Self::V1(_) => ProtocolVersion::V1,
            Self::V2(_) => ProtocolVersion::V2,
        }
    }

    pub fn swarm_key(self) -> SwarmKey {
        match self {
            Self::V1(hash) => SwarmKey::V1(hash),
            Self::V2(hash) => hash.swarm_key(),
        }
    }

    pub const fn as_bytes(&self) -> &[u8] {
        match self {
            Self::V1(hash) => hash.as_bytes(),
            Self::V2(hash) => hash.as_bytes(),
        }
    }
}

impl From<V1InfoHash> for FullInfoHash {
    fn from(value: V1InfoHash) -> Self {
        Self::V1(value)
    }
}

impl From<V2InfoHash> for FullInfoHash {
    fn from(value: V2InfoHash) -> Self {
        Self::V2(value)
    }
}

impl From<[u8; 20]> for FullInfoHash {
    fn from(value: [u8; 20]) -> Self {
        Self::V1(V1InfoHash::new(value))
    }
}

/// The explicitly selected 20-byte identity carried by existing wire protocols.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SwarmKey {
    V1(V1InfoHash),
    V2Truncated([u8; 20]),
}

impl SwarmKey {
    pub const fn protocol(self) -> ProtocolVersion {
        match self {
            Self::V1(_) => ProtocolVersion::V1,
            Self::V2Truncated(_) => ProtocolVersion::V2,
        }
    }

    pub const fn as_bytes(&self) -> &[u8; 20] {
        match self {
            Self::V1(hash) => hash.as_bytes(),
            Self::V2Truncated(bytes) => bytes,
        }
    }

    pub const fn into_bytes(self) -> [u8; 20] {
        match self {
            Self::V1(hash) => hash.into_bytes(),
            Self::V2Truncated(bytes) => bytes,
        }
    }
}

/// A nonempty set of the full identities known for one torrent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InfoHashes {
    v1: Option<V1InfoHash>,
    v2: Option<V2InfoHash>,
}

impl InfoHashes {
    pub fn new(v1: Option<V1InfoHash>, v2: Option<V2InfoHash>) -> Result<Self, InfoHashesError> {
        if v1.is_none() && v2.is_none() {
            return Err(InfoHashesError::MissingIdentity);
        }
        Ok(Self { v1, v2 })
    }

    pub const fn v1(hash: V1InfoHash) -> Self {
        Self {
            v1: Some(hash),
            v2: None,
        }
    }

    pub const fn v2(hash: V2InfoHash) -> Self {
        Self {
            v1: None,
            v2: Some(hash),
        }
    }

    pub const fn hybrid(v1: V1InfoHash, v2: V2InfoHash) -> Self {
        Self {
            v1: Some(v1),
            v2: Some(v2),
        }
    }

    pub const fn v1_hash(self) -> Option<V1InfoHash> {
        self.v1
    }

    pub const fn v2_hash(self) -> Option<V2InfoHash> {
        self.v2
    }

    pub const fn identity_count(self) -> usize {
        self.v1.is_some() as usize + self.v2.is_some() as usize
    }

    pub const fn is_hybrid(self) -> bool {
        self.v1.is_some() && self.v2.is_some()
    }

    pub fn contains(self, identity: FullInfoHash) -> bool {
        match identity {
            FullInfoHash::V1(hash) => match self.v1 {
                Some(existing) => existing.into_bytes() == hash.into_bytes(),
                None => false,
            },
            FullInfoHash::V2(hash) => match self.v2 {
                Some(existing) => existing.into_bytes() == hash.into_bytes(),
                None => false,
            },
        }
    }

    pub fn for_each(self, mut visit: impl FnMut(FullInfoHash)) {
        if let Some(hash) = self.v1 {
            visit(FullInfoHash::V1(hash));
        }
        if let Some(hash) = self.v2 {
            visit(FullInfoHash::V2(hash));
        }
    }
}

impl From<FullInfoHash> for InfoHashes {
    fn from(value: FullInfoHash) -> Self {
        match value {
            FullInfoHash::V1(hash) => Self::v1(hash),
            FullInfoHash::V2(hash) => Self::v2(hash),
        }
    }
}

impl From<[u8; 20]> for InfoHashes {
    fn from(value: [u8; 20]) -> Self {
        Self::v1(V1InfoHash::new(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoHashesError {
    MissingIdentity,
}

impl fmt::Display for InfoHashesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingIdentity => formatter.write_str("at least one info hash is required"),
        }
    }
}

impl std::error::Error for InfoHashesError {}

fn write_lower_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_set_must_not_be_empty() {
        assert_eq!(
            InfoHashes::new(None, None),
            Err(InfoHashesError::MissingIdentity)
        );
    }

    #[test]
    fn zero_hashes_are_present_values() {
        let hashes = InfoHashes::hybrid(V1InfoHash::new([0; 20]), V2InfoHash::new([0; 32]));
        assert_eq!(hashes.identity_count(), 2);
        assert!(hashes.is_hybrid());
        assert_eq!(hashes.v1_hash(), Some(V1InfoHash::new([0; 20])));
        assert_eq!(hashes.v2_hash(), Some(V2InfoHash::new([0; 32])));
    }

    #[test]
    fn v2_wire_projection_is_exact_and_tagged() {
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::try_from(index).unwrap();
        }
        let hash = V2InfoHash::new(bytes);
        assert_eq!(
            hash.swarm_key(),
            SwarmKey::V2Truncated([
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ])
        );
        assert_ne!(
            hash.swarm_key(),
            SwarmKey::V1(V1InfoHash::new([
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ]))
        );
    }

    #[test]
    fn hashes_format_as_lowercase_full_hex() {
        assert_eq!(
            V1InfoHash::new([0xab; 20]).to_string(),
            "abababababababababababababababababababab"
        );
        assert_eq!(V2InfoHash::new([0xcd; 32]).to_string(), "cd".repeat(32));
    }
}

use crate::{RemoteCryptoError, Result};

const BINDING_MAGIC: &[u8; 4] = b"RSB1";
const PROTOCOL_LABEL: &[u8] = b"rstorrent.remote";
const PROTOCOL_VERSION: &[u8] = &1_u16.to_be_bytes();
const CREDENTIAL_DOMAIN: &[u8] = b"rstorrent.remote.opaque.credential.v1";
const CLIENT_DOMAIN: &[u8] = b"rstorrent.remote.opaque.client.v1";
const SERVER_DOMAIN: &[u8] = b"rstorrent.remote.opaque.server.v1";
const CONTEXT_DOMAIN: &[u8] = b"rstorrent.remote.opaque.context.v1";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RelayId([u8; 32]);

impl RelayId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HostId([u8; 32]);

impl HostId {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Username(String);

impl Username {
    pub fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        let valid_length = (3..=32).contains(&bytes.len());
        let valid_edges = bytes
            .first()
            .zip(bytes.last())
            .is_some_and(|(first, last)| {
                first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric()
            });
        let valid_characters = bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');

        if !valid_length || !valid_edges || !valid_characters {
            return Err(RemoteCryptoError::InvalidUsername);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for Username {
    type Error = RemoteCryptoError;

    fn try_from(value: &str) -> Result<Self> {
        Self::parse(value)
    }
}

/// Identity and route values bound into registration, login, host pinning,
/// and record-key derivation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding {
    relay_id: RelayId,
    username: Username,
    host_id: HostId,
}

impl Binding {
    pub const fn new(relay_id: RelayId, username: Username, host_id: HostId) -> Self {
        Self {
            relay_id,
            username,
            host_id,
        }
    }

    pub const fn relay_id(&self) -> RelayId {
        self.relay_id
    }

    pub fn username(&self) -> &Username {
        &self.username
    }

    pub const fn host_id(&self) -> HostId {
        self.host_id
    }

    /// Canonical, unambiguous encoding used by every protocol domain.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(
            4 + 2 + PROTOCOL_LABEL.len() + 2 + PROTOCOL_VERSION.len() + 2 + 32 + 2 + 32 + 2 + 32,
        );
        output.extend_from_slice(BINDING_MAGIC);
        append_field(&mut output, PROTOCOL_LABEL);
        append_field(&mut output, PROTOCOL_VERSION);
        append_field(&mut output, self.relay_id.as_bytes());
        append_field(&mut output, self.username.as_str().as_bytes());
        append_field(&mut output, self.host_id.as_bytes());
        output
    }

    pub(crate) fn credential_identifier(&self) -> Vec<u8> {
        domain_value(CREDENTIAL_DOMAIN, self)
    }

    pub(crate) fn client_identifier(&self) -> Vec<u8> {
        domain_value(CLIENT_DOMAIN, self)
    }

    pub(crate) fn server_identifier(&self) -> Vec<u8> {
        domain_value(SERVER_DOMAIN, self)
    }

    pub(crate) fn context(&self) -> Vec<u8> {
        domain_value(CONTEXT_DOMAIN, self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostPin {
    host_id: HostId,
    server_public_key: [u8; 32],
}

impl HostPin {
    pub const fn new(host_id: HostId, server_public_key: [u8; 32]) -> Self {
        Self {
            host_id,
            server_public_key,
        }
    }

    pub const fn host_id(&self) -> HostId {
        self.host_id
    }

    pub const fn server_public_key(&self) -> &[u8; 32] {
        &self.server_public_key
    }
}

fn append_field(output: &mut Vec<u8>, value: &[u8]) {
    let length = u16::try_from(value.len()).expect("binding fields are statically bounded");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

fn domain_value(domain: &[u8], binding: &Binding) -> Vec<u8> {
    let canonical = binding.canonical_bytes();
    let mut output = Vec::with_capacity(2 + domain.len() + 2 + canonical.len());
    append_field(&mut output, domain);
    append_field(&mut output, &canonical);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> Binding {
        Binding::new(
            RelayId::new([1; 32]),
            Username::parse("alice-2").unwrap(),
            HostId::new([2; 32]),
        )
    }

    #[test]
    fn username_policy_is_exact() {
        for valid in ["abc", "alice-2", "a0b", "a234567890123456789012345678901b"] {
            assert!(Username::parse(valid).is_ok(), "{valid}");
        }
        for invalid in [
            "ab",
            "-abc",
            "abc-",
            "Alice",
            "a_b",
            "a.b",
            "a2345678901234567890123456789012b",
        ] {
            assert_eq!(
                Username::parse(invalid),
                Err(RemoteCryptoError::InvalidUsername),
                "{invalid}"
            );
        }
    }

    #[test]
    fn canonical_binding_and_domains_are_distinct() {
        let first = binding();
        let second = Binding::new(
            RelayId::new([1; 32]),
            Username::parse("alice-3").unwrap(),
            HostId::new([2; 32]),
        );
        assert_ne!(first.canonical_bytes(), second.canonical_bytes());
        assert_ne!(first.credential_identifier(), first.client_identifier());
        assert_ne!(first.client_identifier(), first.server_identifier());
        assert_ne!(first.server_identifier(), first.context());
    }
}

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{AeadInPlace, generic_array::GenericArray},
};
use hkdf::Hkdf;
use sha2::{Digest, Sha512};
use zeroize::Zeroizing;

use crate::{Binding, RemoteCryptoError, Result};

pub const APP_CLIENT_MAX_PLAINTEXT: usize = 64 * 1024;
pub const APP_SERVER_MAX_PLAINTEXT: usize = 16 * 1024 * 1024 + 4 * 1024;
pub const RECORD_OVERHEAD: usize = 16 + 16;

const HEADER_LEN: usize = 16;
const TAG_LEN: usize = 16;
const MAGIC: &[u8; 4] = b"RSR1";
const CLOSE_FLAG: u8 = 0x01;
const SEQUENCE_LIMIT: u64 = 1_u64 << 32;

const C2H_KEY_LABEL: &[u8] = b"rstorrent.remote.record.c2h.key.v1";
const H2C_KEY_LABEL: &[u8] = b"rstorrent.remote.record.h2c.key.v1";
const C2H_NONCE_LABEL: &[u8] = b"rstorrent.remote.record.c2h.nonce.v1";
const H2C_NONCE_LABEL: &[u8] = b"rstorrent.remote.record.h2c.nonce.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Client,
    Host,
}

#[derive(Debug, Eq, PartialEq)]
pub struct OpenedRecord {
    pub plaintext: Vec<u8>,
    pub is_close: bool,
}

/// A paired, directional record layer derived from one authenticated OPAQUE
/// session. It is independent of WebSocket or application frame types.
pub struct SecureChannel {
    outbound: RecordSealer,
    inbound: RecordOpener,
}

impl SecureChannel {
    pub(crate) fn derive(role: Role, session_key: &[u8], binding: &Binding) -> Result<Self> {
        let canonical = binding.canonical_bytes();
        let salt = Sha512::digest(canonical);
        let hkdf = Hkdf::<Sha512>::new(Some(&salt), session_key);

        let c2h_key = derive_array::<32>(&hkdf, C2H_KEY_LABEL)?;
        let h2c_key = derive_array::<32>(&hkdf, H2C_KEY_LABEL)?;
        let c2h_nonce = derive_array::<4>(&hkdf, C2H_NONCE_LABEL)?;
        let h2c_nonce = derive_array::<4>(&hkdf, H2C_NONCE_LABEL)?;

        let channel = match role {
            Role::Client => Self {
                outbound: RecordSealer::new(0, c2h_key, c2h_nonce, APP_CLIENT_MAX_PLAINTEXT),
                inbound: RecordOpener::new(1, h2c_key, h2c_nonce, APP_SERVER_MAX_PLAINTEXT),
            },
            Role::Host => Self {
                outbound: RecordSealer::new(1, h2c_key, h2c_nonce, APP_SERVER_MAX_PLAINTEXT),
                inbound: RecordOpener::new(0, c2h_key, c2h_nonce, APP_CLIENT_MAX_PLAINTEXT),
            },
        };
        Ok(channel)
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        self.outbound.seal(plaintext, false)
    }

    pub fn seal_close(&mut self) -> Result<Vec<u8>> {
        let record = self.outbound.seal(&[], true)?;
        self.inbound.erase();
        Ok(record)
    }

    pub fn open(&mut self, record: &[u8]) -> Result<OpenedRecord> {
        let result = self.inbound.open(record);
        let terminates_circuit = match &result {
            Ok(opened) => opened.is_close,
            Err(_) => true,
        };
        if terminates_circuit {
            self.outbound.erase();
        }
        result
    }

    pub fn outbound_is_closed(&self) -> bool {
        self.outbound.terminal
    }

    pub fn inbound_is_closed(&self) -> bool {
        self.inbound.terminal
    }
}

struct RecordSealer {
    direction: u8,
    key: Zeroizing<[u8; 32]>,
    nonce_prefix: Zeroizing<[u8; 4]>,
    next_sequence: u64,
    max_plaintext: usize,
    terminal: bool,
}

impl RecordSealer {
    fn new(direction: u8, key: [u8; 32], nonce_prefix: [u8; 4], max_plaintext: usize) -> Self {
        Self {
            direction,
            key: Zeroizing::new(key),
            nonce_prefix: Zeroizing::new(nonce_prefix),
            next_sequence: 0,
            max_plaintext,
            terminal: false,
        }
    }

    fn seal(&mut self, plaintext: &[u8], close: bool) -> Result<Vec<u8>> {
        if self.terminal {
            return Err(RemoteCryptoError::ChannelClosed);
        }
        if plaintext.len() > self.max_plaintext || (close && !plaintext.is_empty()) {
            return Err(RemoteCryptoError::RecordTooLarge);
        }
        if self.next_sequence >= SEQUENCE_LIMIT {
            self.erase();
            return Err(RemoteCryptoError::RecordSequenceExhausted);
        }

        let flags = if close { CLOSE_FLAG } else { 0 };
        let header = make_header(self.direction, flags, self.next_sequence);
        let nonce = make_nonce(&self.nonce_prefix, self.next_sequence);
        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&*self.key));
        let mut ciphertext = plaintext.to_vec();
        let tag = match cipher.encrypt_in_place_detached(
            GenericArray::from_slice(&nonce),
            &header,
            &mut ciphertext,
        ) {
            Ok(tag) => tag,
            Err(_) => {
                self.erase();
                return Err(RemoteCryptoError::InvalidRecord);
            }
        };

        let mut record = Vec::with_capacity(RECORD_OVERHEAD + ciphertext.len());
        record.extend_from_slice(&header);
        record.extend_from_slice(&ciphertext);
        record.extend_from_slice(&tag);
        self.next_sequence += 1;
        if close {
            self.erase();
        }
        Ok(record)
    }

    fn erase(&mut self) {
        use zeroize::Zeroize;

        self.key.zeroize();
        self.nonce_prefix.zeroize();
        self.terminal = true;
    }
}

struct RecordOpener {
    direction: u8,
    key: Zeroizing<[u8; 32]>,
    nonce_prefix: Zeroizing<[u8; 4]>,
    next_sequence: u64,
    max_plaintext: usize,
    terminal: bool,
}

impl RecordOpener {
    fn new(direction: u8, key: [u8; 32], nonce_prefix: [u8; 4], max_plaintext: usize) -> Self {
        Self {
            direction,
            key: Zeroizing::new(key),
            nonce_prefix: Zeroizing::new(nonce_prefix),
            next_sequence: 0,
            max_plaintext,
            terminal: false,
        }
    }

    fn open(&mut self, record: &[u8]) -> Result<OpenedRecord> {
        if self.terminal {
            return Err(RemoteCryptoError::ChannelClosed);
        }
        let result = self.open_inner(record);
        if result.is_err() {
            self.erase();
        }
        result
    }

    fn open_inner(&mut self, record: &[u8]) -> Result<OpenedRecord> {
        if self.next_sequence >= SEQUENCE_LIMIT {
            return Err(RemoteCryptoError::RecordSequenceExhausted);
        }
        if !(RECORD_OVERHEAD..=self.max_plaintext + RECORD_OVERHEAD).contains(&record.len()) {
            return Err(RemoteCryptoError::RecordTooLarge);
        }

        let header: &[u8; HEADER_LEN] = record[..HEADER_LEN]
            .try_into()
            .map_err(|_| RemoteCryptoError::InvalidRecord)?;
        if &header[..4] != MAGIC
            || header[4] != self.direction
            || header[5] & !CLOSE_FLAG != 0
            || header[6] != 0
            || header[7] != 0
        {
            return Err(RemoteCryptoError::InvalidRecord);
        }
        let sequence = u64::from_be_bytes(
            header[8..16]
                .try_into()
                .map_err(|_| RemoteCryptoError::InvalidRecord)?,
        );
        if sequence != self.next_sequence {
            return Err(RemoteCryptoError::InvalidRecord);
        }

        let close = header[5] == CLOSE_FLAG;
        let ciphertext_end = record.len() - TAG_LEN;
        if close && ciphertext_end != HEADER_LEN {
            return Err(RemoteCryptoError::InvalidRecord);
        }
        let mut plaintext = record[HEADER_LEN..ciphertext_end].to_vec();
        let tag = GenericArray::from_slice(&record[ciphertext_end..]);
        let nonce = make_nonce(&self.nonce_prefix, sequence);
        let cipher = ChaCha20Poly1305::new(GenericArray::from_slice(&*self.key));
        cipher
            .decrypt_in_place_detached(
                GenericArray::from_slice(&nonce),
                header,
                &mut plaintext,
                tag,
            )
            .map_err(|_| RemoteCryptoError::InvalidRecord)?;

        self.next_sequence += 1;
        if close {
            self.erase();
        }
        Ok(OpenedRecord {
            plaintext,
            is_close: close,
        })
    }

    fn erase(&mut self) {
        use zeroize::Zeroize;

        self.key.zeroize();
        self.nonce_prefix.zeroize();
        self.terminal = true;
    }
}

fn derive_array<const N: usize>(hkdf: &Hkdf<Sha512>, label: &[u8]) -> Result<[u8; N]> {
    let mut output = [0_u8; N];
    hkdf.expand(label, &mut output)
        .map_err(|_| RemoteCryptoError::KeyDerivationFailed)?;
    Ok(output)
}

fn make_header(direction: u8, flags: u8, sequence: u64) -> [u8; HEADER_LEN] {
    let mut header = [0_u8; HEADER_LEN];
    header[..4].copy_from_slice(MAGIC);
    header[4] = direction;
    header[5] = flags;
    header[8..16].copy_from_slice(&sequence.to_be_bytes());
    header
}

fn make_nonce(prefix: &[u8; 4], sequence: u64) -> [u8; 12] {
    let mut nonce = [0_u8; 12];
    nonce[..4].copy_from_slice(prefix);
    nonce[4..].copy_from_slice(&sequence.to_be_bytes());
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostId, RelayId, Username};

    fn channels() -> (SecureChannel, SecureChannel) {
        let binding = Binding::new(
            RelayId::new([3; 32]),
            Username::parse("alice").unwrap(),
            HostId::new([4; 32]),
        );
        (
            SecureChannel::derive(Role::Client, &[5; 64], &binding).unwrap(),
            SecureChannel::derive(Role::Host, &[5; 64], &binding).unwrap(),
        )
    }

    #[test]
    fn bidirectional_records_and_close_are_exact() {
        let (mut client, mut host) = channels();
        let c2h = client.seal(b"command").unwrap();
        assert_eq!(host.open(&c2h).unwrap().plaintext, b"command");
        let h2c = host.seal(b"snapshot").unwrap();
        assert_eq!(client.open(&h2c).unwrap().plaintext, b"snapshot");

        let close = client.seal_close().unwrap();
        assert_eq!(close.len(), RECORD_OVERHEAD);
        assert!(host.open(&close).unwrap().is_close);
        assert_eq!(client.seal(b"late"), Err(RemoteCryptoError::ChannelClosed));
        assert_eq!(host.open(&close), Err(RemoteCryptoError::ChannelClosed));
    }

    #[test]
    fn tamper_and_sequence_errors_poison_the_direction() {
        let (mut client, mut host) = channels();
        let first = client.seal(b"one").unwrap();
        let second = client.seal(b"two").unwrap();

        assert_eq!(host.open(&second), Err(RemoteCryptoError::InvalidRecord));
        assert_eq!(host.open(&first), Err(RemoteCryptoError::ChannelClosed));

        let (mut client, mut host) = channels();
        let mut tampered = client.seal(b"one").unwrap();
        tampered[HEADER_LEN] ^= 1;
        assert_eq!(host.open(&tampered), Err(RemoteCryptoError::InvalidRecord));
        assert_eq!(host.open(&tampered), Err(RemoteCryptoError::ChannelClosed));
    }

    #[test]
    fn reflection_and_malformed_headers_fail_closed() {
        let (mut client, _) = channels();
        let reflected = client.seal(b"one").unwrap();
        assert_eq!(
            client.open(&reflected),
            Err(RemoteCryptoError::InvalidRecord)
        );

        for index in [0, 5, 6] {
            let (mut client, mut host) = channels();
            let mut invalid = client.seal(b"one").unwrap();
            invalid[index] ^= if index == 5 { 2 } else { 1 };
            assert_eq!(host.open(&invalid), Err(RemoteCryptoError::InvalidRecord));
            assert!(host.inbound_is_closed());
        }

        let (_, mut host) = channels();
        assert_eq!(
            host.open(&[0; RECORD_OVERHEAD - 1]),
            Err(RemoteCryptoError::RecordTooLarge)
        );
    }

    #[test]
    fn asymmetric_bounds_are_enforced() {
        let (mut client, mut host) = channels();
        assert_eq!(
            client.seal(&vec![0; APP_CLIENT_MAX_PLAINTEXT + 1]),
            Err(RemoteCryptoError::RecordTooLarge)
        );
        assert!(host.seal(&vec![0; APP_CLIENT_MAX_PLAINTEXT + 1]).is_ok());

        let oversized = vec![0; APP_CLIENT_MAX_PLAINTEXT + RECORD_OVERHEAD + 1];
        assert_eq!(
            host.open(&oversized),
            Err(RemoteCryptoError::RecordTooLarge)
        );
    }

    #[test]
    fn sequence_exhaustion_is_terminal() {
        let (mut client, _) = channels();
        client.outbound.next_sequence = SEQUENCE_LIMIT;
        assert_eq!(
            client.seal(b"nope"),
            Err(RemoteCryptoError::RecordSequenceExhausted)
        );
        assert_eq!(client.seal(b"nope"), Err(RemoteCryptoError::ChannelClosed));
    }
}

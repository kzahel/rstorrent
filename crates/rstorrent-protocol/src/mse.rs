//! Message Stream Encryption / Protocol Encryption primitives.
//!
//! This module contains only deterministic protocol values and cryptographic
//! transforms. Socket, clock, entropy-source, and task ownership remain in the
//! engine.

use core::fmt;

use crypto_bigint::{Encoding, U192, U768, const_monty_params, modular::ConstMontyForm};
use sha1::{Digest, Sha1};

mod handshake;

pub use handshake::{
    MSE_HANDSHAKE_BUFFER_LEN, MSE_MAX_PADDING_LEN, MseAction, MseBytes, MseFeed, MseHandshake,
    MseHandshakeComplete, MseHandshakeError, MsePadding, MseResume, MseStep,
};

pub const DH_PUBLIC_KEY_LEN: usize = 96;
pub const DH_PRIVATE_EXPONENT_LEN: usize = 20;
pub const RC4_DROP_BYTES: usize = 1024;

pub const MSE_METHOD_PLAINTEXT: u32 = 0x01;
pub const MSE_METHOD_RC4: u32 = 0x02;
pub const MSE_KNOWN_METHODS: u32 = MSE_METHOD_PLAINTEXT | MSE_METHOD_RC4;

const DH_PRIME_HEX: &str = concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C2",
    "45E485B576625E7EC6F44C42E9A63A36210000000000090563",
);

const_monty_params!(DhModulus, U768, DH_PRIME_HEX);

type DhField = ConstMontyForm<DhModulus, { U768::LIMBS }>;

#[cfg(test)]
const DH_PRIME: U768 = U768::from_be_hex(DH_PRIME_HEX);
const DH_PRIME_MINUS_ONE: U768 = U768::from_be_hex(concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C2",
    "45E485B576625E7EC6F44C42E9A63A36210000000000090562",
));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MseRole {
    Initiator,
    Responder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MseMethod {
    PlaintextPayload,
    Rc4,
}

impl MseMethod {
    #[must_use]
    pub const fn wire_bit(self) -> u32 {
        match self {
            Self::PlaintextPayload => MSE_METHOD_PLAINTEXT,
            Self::Rc4 => MSE_METHOD_RC4,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MseMethodError {
    NoSupportedMethod,
    AmbiguousSelection,
    SelectedMethodNotOffered,
}

impl fmt::Display for MseMethodError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSupportedMethod => formatter.write_str("no supported MSE method"),
            Self::AmbiguousSelection => {
                formatter.write_str("MSE selection does not name exactly one known method")
            }
            Self::SelectedMethodNotOffered => formatter.write_str("MSE selection was not offered"),
        }
    }
}

/// Select one known method while ignoring extension bits.
pub fn select_method(
    offered: u32,
    allowed: u32,
    prefer_rc4: bool,
) -> Result<MseMethod, MseMethodError> {
    let intersection = offered & allowed & MSE_KNOWN_METHODS;
    if prefer_rc4 && intersection & MSE_METHOD_RC4 != 0 {
        return Ok(MseMethod::Rc4);
    }
    if intersection & MSE_METHOD_PLAINTEXT != 0 {
        return Ok(MseMethod::PlaintextPayload);
    }
    if intersection & MSE_METHOD_RC4 != 0 {
        return Ok(MseMethod::Rc4);
    }
    Err(MseMethodError::NoSupportedMethod)
}

/// Validate a responder selection while ignoring extension bits.
pub fn validate_selected_method(offered: u32, selected: u32) -> Result<MseMethod, MseMethodError> {
    let known = selected & MSE_KNOWN_METHODS;
    let method = match known {
        MSE_METHOD_PLAINTEXT => MseMethod::PlaintextPayload,
        MSE_METHOD_RC4 => MseMethod::Rc4,
        _ => return Err(MseMethodError::AmbiguousSelection),
    };
    if offered & method.wire_bit() == 0 {
        return Err(MseMethodError::SelectedMethodNotOffered);
    }
    Ok(method)
}

pub struct DhPrivateExponent([u8; DH_PRIVATE_EXPONENT_LEN]);

impl DhPrivateExponent {
    /// Convert exactly 20 uniform bytes to a uniform exactly-160-bit exponent.
    #[must_use]
    pub fn from_entropy(mut entropy: [u8; DH_PRIVATE_EXPONENT_LEN]) -> Self {
        entropy[0] |= 0x80;
        Self(entropy)
    }

    fn as_uint(&self) -> U192 {
        let mut encoded = [0_u8; 24];
        encoded[4..].copy_from_slice(&self.0);
        U192::from_be_bytes(encoded.into())
    }

    #[cfg(test)]
    fn as_bytes(&self) -> &[u8; DH_PRIVATE_EXPONENT_LEN] {
        &self.0
    }
}

impl fmt::Debug for DhPrivateExponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DhPrivateExponent([REDACTED])")
    }
}

impl Drop for DhPrivateExponent {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DhPublicKey([u8; DH_PUBLIC_KEY_LEN]);

impl DhPublicKey {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; DH_PUBLIC_KEY_LEN] {
        &self.0
    }

    #[must_use]
    pub fn into_bytes(self) -> [u8; DH_PUBLIC_KEY_LEN] {
        self.0
    }
}

impl fmt::Debug for DhPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DhPublicKey([REDACTED])")
    }
}

pub struct DhSharedSecret([u8; DH_PUBLIC_KEY_LEN]);

impl DhSharedSecret {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; DH_PUBLIC_KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for DhSharedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DhSharedSecret([REDACTED])")
    }
}

impl Drop for DhSharedSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DhError {
    InvalidRemotePublicKey,
}

impl fmt::Display for DhError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRemotePublicKey => formatter.write_str("invalid MSE DH public key"),
        }
    }
}

#[must_use]
pub fn compute_public_key(private: &DhPrivateExponent) -> DhPublicKey {
    let generator = DhField::new(&U768::from(2_u8));
    let public = generator
        .pow_bounded_exp(&private.as_uint(), 160)
        .retrieve();
    DhPublicKey(encode_u768(public))
}

pub fn compute_shared_secret(
    private: &DhPrivateExponent,
    remote_public: &[u8; DH_PUBLIC_KEY_LEN],
) -> Result<DhSharedSecret, DhError> {
    let remote = U768::from_be_bytes((*remote_public).into());
    if remote < U768::from(2_u8) || remote >= DH_PRIME_MINUS_ONE {
        return Err(DhError::InvalidRemotePublicKey);
    }
    let secret = DhField::new(&remote)
        .pow_bounded_exp(&private.as_uint(), 160)
        .retrieve();
    Ok(DhSharedSecret(encode_u768(secret)))
}

fn encode_u768(value: U768) -> [u8; DH_PUBLIC_KEY_LEN] {
    value.to_be_bytes().into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rc4Error {
    EmptyKey,
}

impl fmt::Display for Rc4Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey => formatter.write_str("RC4 key must not be empty"),
        }
    }
}

/// RC4 with the MSE-mandated 1024-byte initial keystream discard.
pub struct Rc4(Rc4Core);

impl Rc4 {
    pub fn new(key: &[u8]) -> Result<Self, Rc4Error> {
        let mut core = Rc4Core::new(key)?;
        core.apply(&mut [0_u8; RC4_DROP_BYTES]);
        Ok(Self(core))
    }

    pub fn apply(&mut self, bytes: &mut [u8]) {
        self.0.apply(bytes);
    }
}

struct Rc4Core {
    state: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4Core {
    fn new(key: &[u8]) -> Result<Self, Rc4Error> {
        if key.is_empty() {
            return Err(Rc4Error::EmptyKey);
        }
        let mut state = [0_u8; 256];
        for (value, slot) in (0_u8..=u8::MAX).zip(state.iter_mut()) {
            *slot = value;
        }
        let mut j = 0_u8;
        for i in 0..state.len() {
            j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
            state.swap(i, usize::from(j));
        }
        Ok(Self { state, i: 0, j: 0 })
    }

    fn apply(&mut self, bytes: &mut [u8]) {
        let mut i = usize::from(self.i);
        let mut j = usize::from(self.j);
        for byte in bytes {
            i = (i + 1) & 0xff;
            j = (j + usize::from(self.state[i])) & 0xff;
            self.state.swap(i, j);
            let index = (usize::from(self.state[i]) + usize::from(self.state[j])) & 0xff;
            *byte ^= self.state[index];
        }
        self.i = i as u8;
        self.j = j as u8;
    }
}

impl Drop for Rc4Core {
    fn drop(&mut self) {
        self.state.fill(0);
        self.i = 0;
        self.j = 0;
    }
}

pub struct MseCipherPair {
    send: Rc4,
    receive: Rc4,
}

impl MseCipherPair {
    #[must_use]
    pub fn new(role: MseRole, shared: &DhSharedSecret, skey: &[u8; 20]) -> Self {
        let mut key_a = encryption_key(b"keyA", shared, skey);
        let mut key_b = encryption_key(b"keyB", shared, skey);
        let (send_key, receive_key) = match role {
            MseRole::Initiator => (&key_a, &key_b),
            MseRole::Responder => (&key_b, &key_a),
        };
        let pair = Self {
            send: Rc4::new_nonempty(send_key),
            receive: Rc4::new_nonempty(receive_key),
        };
        key_a.fill(0);
        key_b.fill(0);
        pair
    }

    pub fn apply_send(&mut self, bytes: &mut [u8]) {
        self.send.apply(bytes);
    }

    pub fn apply_receive(&mut self, bytes: &mut [u8]) {
        self.receive.apply(bytes);
    }

    #[must_use]
    pub fn into_parts(self) -> (Rc4, Rc4) {
        (self.send, self.receive)
    }
}

impl Rc4 {
    fn new_nonempty(key: &[u8; 20]) -> Self {
        let mut core = Rc4Core::new(key).unwrap_or_else(|_| unreachable!());
        core.apply(&mut [0_u8; RC4_DROP_BYTES]);
        Self(core)
    }
}

#[must_use]
pub fn req1_hash(shared: &DhSharedSecret) -> [u8; 20] {
    hash_parts(&[b"req1", shared.as_bytes()])
}

#[must_use]
pub fn req2_hash(skey: &[u8; 20]) -> [u8; 20] {
    hash_parts(&[b"req2", skey])
}

#[must_use]
pub fn req3_hash(shared: &DhSharedSecret) -> [u8; 20] {
    hash_parts(&[b"req3", shared.as_bytes()])
}

#[must_use]
pub fn obfuscated_skey(shared: &DhSharedSecret, skey: &[u8; 20]) -> [u8; 20] {
    let req2 = req2_hash(skey);
    let req3 = req3_hash(shared);
    let mut obfuscated = [0_u8; 20];
    for index in 0..obfuscated.len() {
        obfuscated[index] = req2[index] ^ req3[index];
    }
    obfuscated
}

fn encryption_key(label: &[u8; 4], shared: &DhSharedSecret, skey: &[u8; 20]) -> [u8; 20] {
    hash_parts(&[label, shared.as_bytes(), skey])
}

fn hash_parts(parts: &[&[u8]]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use crypto_bigint::{U768, Wrapping};

    use super::*;

    #[test]
    fn method_selection_ignores_unknown_bits_but_requires_one_known_selection() {
        assert_eq!(
            select_method(0x8000_0003, MSE_KNOWN_METHODS, true),
            Ok(MseMethod::Rc4)
        );
        assert_eq!(
            select_method(0x8000_0003, MSE_KNOWN_METHODS, false),
            Ok(MseMethod::PlaintextPayload)
        );
        assert_eq!(
            validate_selected_method(MSE_KNOWN_METHODS, 0x8000_0002),
            Ok(MseMethod::Rc4)
        );
        assert_eq!(
            validate_selected_method(MSE_METHOD_PLAINTEXT, MSE_METHOD_RC4),
            Err(MseMethodError::SelectedMethodNotOffered)
        );
        assert_eq!(
            validate_selected_method(MSE_KNOWN_METHODS, MSE_KNOWN_METHODS),
            Err(MseMethodError::AmbiguousSelection)
        );
        assert_eq!(
            select_method(0x8000_0000, MSE_KNOWN_METHODS, true),
            Err(MseMethodError::NoSupportedMethod)
        );
    }

    #[test]
    fn private_exponent_consumes_fixed_entropy_and_sets_the_high_bit() {
        let minimum = DhPrivateExponent::from_entropy([0_u8; 20]);
        assert_eq!(minimum.as_bytes()[0], 0x80);
        assert!(minimum.as_bytes()[1..].iter().all(|byte| *byte == 0));

        let maximum = DhPrivateExponent::from_entropy([0xff; 20]);
        assert!(maximum.as_bytes().iter().all(|byte| *byte == 0xff));
    }

    #[test]
    fn dh_prime_is_the_exact_96_byte_mse_value() {
        let encoded: [u8; 96] = DH_PRIME.to_be_bytes().into();
        assert_eq!(hex(&encoded), DH_PRIME_HEX.to_ascii_lowercase());
    }

    #[test]
    fn dh_round_trip_agrees_for_deterministic_pairs() {
        for seed in 0_u8..16 {
            let mut left_entropy = [seed; 20];
            let mut right_entropy = [seed.wrapping_add(0x40); 20];
            left_entropy[19] = seed.wrapping_mul(17);
            right_entropy[19] = seed.wrapping_mul(29);
            let left = DhPrivateExponent::from_entropy(left_entropy);
            let right = DhPrivateExponent::from_entropy(right_entropy);
            let left_public = compute_public_key(&left);
            let right_public = compute_public_key(&right);
            let left_secret =
                compute_shared_secret(&left, right_public.as_bytes()).expect("valid right key");
            let right_secret =
                compute_shared_secret(&right, left_public.as_bytes()).expect("valid left key");
            assert_eq!(left_secret.as_bytes(), right_secret.as_bytes());
        }
    }

    #[test]
    fn dh_rejects_degenerate_remote_keys_and_accepts_boundaries() {
        let private = DhPrivateExponent::from_entropy([0x23; 20]);
        let invalid = [
            U768::ZERO,
            U768::ONE,
            (Wrapping(DH_PRIME) - Wrapping(U768::ONE)).0,
            DH_PRIME,
            (Wrapping(DH_PRIME) + Wrapping(U768::ONE)).0,
            U768::MAX,
        ];
        for remote in invalid {
            assert!(matches!(
                compute_shared_secret(&private, &encode_u768(remote)),
                Err(DhError::InvalidRemotePublicKey)
            ));
        }

        for remote in [
            U768::from(2_u8),
            (Wrapping(DH_PRIME) - Wrapping(U768::from(2_u8))).0,
        ] {
            assert!(compute_shared_secret(&private, &encode_u768(remote)).is_ok());
        }
    }

    #[test]
    fn u768_export_preserves_exact_width_and_leading_zeroes() {
        for (value, expected_zeroes) in [
            (U768::ONE, 95),
            (U768::from(0x0102_u16), 94),
            (U768::from(0x0102_0304_0506_0708_u64), 88),
        ] {
            let encoded = encode_u768(value);
            assert!(encoded[..expected_zeroes].iter().all(|byte| *byte == 0));
            assert_ne!(encoded[expected_zeroes], 0);
        }
    }

    #[test]
    fn rfc_6229_vectors_cover_initial_output_and_mse_discard() {
        // Source: RFC 6229, section 2, 128-bit key 0x01..0x10.
        // The selected bytes are treated as RFC Code Components under the
        // Simplified BSD terms recorded in THIRD_PARTY_NOTICES.md.
        let key: [u8; 16] = core::array::from_fn(|index| (index + 1) as u8);
        let mut initial = [0_u8; 16];
        Rc4Core::new(&key)
            .expect("non-empty key")
            .apply(&mut initial);
        assert_eq!(initial, decode_hex_16("9ac7cc9a609d1ef7b2932899cde41b97"));

        let mut after_drop = [0_u8; 16];
        Rc4::new(&key)
            .expect("non-empty key")
            .apply(&mut after_drop);
        assert_eq!(
            after_drop,
            decode_hex_16("bdf0324e6083dcc6d3cedd3ca8c53c16")
        );
    }

    #[test]
    fn rc4_is_length_preserving_and_chunk_invariant() {
        let key = [0x5a; 20];
        let mut contiguous: Vec<u8> = (0..4097).map(|index| index as u8).collect();
        let original = contiguous.clone();
        let mut chunked = contiguous.clone();
        Rc4::new(&key)
            .expect("non-empty key")
            .apply(&mut contiguous);

        let mut cipher = Rc4::new(&key).expect("non-empty key");
        for chunk in chunked.chunks_mut(37) {
            cipher.apply(chunk);
        }
        assert_eq!(chunked, contiguous);
        assert_eq!(chunked.len(), original.len());

        Rc4::new(&key)
            .expect("non-empty key")
            .apply(&mut contiguous);
        assert_eq!(contiguous, original);
    }

    #[test]
    fn derivation_vectors_are_stable_and_directional_ciphers_agree() {
        let secret_bytes: [u8; 96] = core::array::from_fn(|index| index as u8);
        let shared = DhSharedSecret(secret_bytes);
        let skey: [u8; 20] = core::array::from_fn(|index| 0xa0 + index as u8);
        assert_eq!(
            req1_hash(&shared),
            decode_hex_20("ed37476cccf63496894d66db14b9806c8c9efe3e")
        );
        assert_eq!(
            req2_hash(&skey),
            decode_hex_20("c7bdc667f9b7bbbbfe5c7231f29e47264bbeef27")
        );
        assert_eq!(
            req3_hash(&shared),
            decode_hex_20("4986fc7b9cb37334e5ab5c3034812fdbc7cd8573")
        );
        assert_eq!(
            obfuscated_skey(&shared, &skey),
            decode_hex_20("8e3b3a1c6504c88f1bf72e01c61f68fd8c736a54")
        );
        assert_eq!(
            encryption_key(b"keyA", &shared, &skey),
            decode_hex_20("d42e913c68c979809c84d5a1f870f3a617ae6ace")
        );
        assert_eq!(
            encryption_key(b"keyB", &shared, &skey),
            decode_hex_20("c5b5e5cefe5c09890b318504825c0f9d94849af6")
        );

        let mut initiator = MseCipherPair::new(MseRole::Initiator, &shared, &skey);
        let mut responder = MseCipherPair::new(MseRole::Responder, &shared, &skey);
        let mut request = *b"initiator-to-responder";
        let request_plain = request;
        initiator.apply_send(&mut request);
        assert_ne!(request, request_plain);
        responder.apply_receive(&mut request);
        assert_eq!(request, request_plain);

        let mut response = *b"responder-to-initiator";
        let response_plain = response;
        responder.apply_send(&mut response);
        assert_ne!(response, response_plain);
        initiator.apply_receive(&mut response);
        assert_eq!(response, response_plain);
    }

    fn decode_hex_16(value: &str) -> [u8; 16] {
        decode_hex(value)
    }

    fn decode_hex_20(value: &str) -> [u8; 20] {
        decode_hex(value)
    }

    fn decode_hex<const N: usize>(value: &str) -> [u8; N] {
        assert_eq!(value.len(), N * 2);
        let mut output = [0_u8; N];
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = (nibble(value.as_bytes()[index * 2]) << 4)
                | nibble(value.as_bytes()[index * 2 + 1]);
        }
        output
    }

    fn nibble(value: u8) -> u8 {
        match value {
            b'0'..=b'9' => value - b'0',
            b'a'..=b'f' => value - b'a' + 10,
            _ => panic!("invalid test hex"),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        use core::fmt::Write as _;

        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(&mut output, "{byte:02x}").expect("write to string");
        }
        output
    }
}

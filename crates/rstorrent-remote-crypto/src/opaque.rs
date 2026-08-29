use argon2::{Algorithm, Argon2, Params, Version};
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialFinalizationLen,
    CredentialRequest, CredentialRequestLen, CredentialResponse, CredentialResponseLen,
    Identifiers, RegistrationRequest, RegistrationRequestLen, RegistrationResponse,
    RegistrationResponseLen, RegistrationUpload, RegistrationUploadLen, Ristretto255, ServerLogin,
    ServerLoginParameters, ServerRegistration, ServerRegistrationLen, ServerSetup, TripleDh,
    generic_array::{ArrayLength, GenericArray, typenum::Unsigned},
    ksf::Ksf,
};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use sha2::Sha512;
use zeroize::{Zeroize, Zeroizing};

use crate::{Binding, HostPin, OperationSeed, RemoteCryptoError, Result, Role, SecureChannel};

// SHA-512 OPRF seed (64) + Ristretto255 private key (32) + dummy public key
// used by opaque-ke's unknown-user path (32).
const SERVER_AUTHORITY_LEN: usize = 128;
const MIN_PASSPHRASE_LEN: usize = 12;
const MAX_PASSPHRASE_LEN: usize = 256;
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_PASSES: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const ARGON2_SALT: [u8; 16] = [0; 16];

struct Suite;

impl CipherSuite for Suite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, Sha512>;
    type Ksf = OpaqueKsf;
}

#[derive(Default)]
struct OpaqueKsf;

impl Ksf for OpaqueKsf {
    fn hash<L: ArrayLength<u8>>(
        &self,
        mut input: GenericArray<u8, L>,
    ) -> core::result::Result<GenericArray<u8, L>, opaque_ke::errors::InternalError> {
        let mut output = GenericArray::default();
        let result = argon2id(&input, &mut output, ARGON2_MEMORY_KIB, ARGON2_PASSES)
            .map_err(|_| opaque_ke::errors::InternalError::KsfError);
        input.zeroize();
        result.map(|()| output)
    }
}

fn argon2id(input: &[u8], output: &mut [u8], memory_kib: u32, passes: u32) -> Result<()> {
    let params = Params::new(memory_kib, passes, ARGON2_PARALLELISM, Some(output.len()))
        .map_err(|_| RemoteCryptoError::KeyDerivationFailed)?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(input, &ARGON2_SALT, output)
        .map_err(|_| RemoteCryptoError::KeyDerivationFailed)
}

/// Execute one bounded Argon2id candidate for the controlled browser matrix.
/// The feature is excluded from ordinary builds and the derived output is
/// wiped without crossing the caller boundary.
#[cfg(feature = "ksf-bench")]
pub fn exercise_argon2id_candidate(input: &[u8], memory_kib: u32, passes: u32) -> Result<()> {
    if input.len() != 64
        || !(32 * 1024..=128 * 1024).contains(&memory_kib)
        || !(1..=4).contains(&passes)
        || !memory_kib.is_multiple_of(32 * 1024)
    {
        return Err(RemoteCryptoError::KeyDerivationFailed);
    }
    let mut output = Zeroizing::new([0_u8; 64]);
    argon2id(input, &mut *output, memory_kib, passes)
}

/// Complete host OPAQUE authority. Its serialized secret bytes are wiped on
/// drop and cloning is deliberately unavailable.
pub struct ServerAuthority {
    encoded: Zeroizing<Vec<u8>>,
}

impl ServerAuthority {
    pub fn generate(seed: OperationSeed) -> Self {
        let mut rng = rng_from_seed(&seed);
        let setup = ServerSetup::<Suite>::new(&mut rng);
        Self {
            encoded: Zeroizing::new(setup.serialize().to_vec()),
        }
    }

    pub fn from_bytes(encoded: &[u8]) -> Result<Self> {
        if encoded.len() != SERVER_AUTHORITY_LEN {
            return Err(RemoteCryptoError::InvalidMessage);
        }
        ServerSetup::<Suite>::deserialize(encoded)
            .map_err(|_| RemoteCryptoError::InvalidMessage)?;
        Ok(Self {
            encoded: Zeroizing::new(encoded.to_vec()),
        })
    }

    /// Export the complete authority for caller-owned protected persistence.
    pub fn export(&self) -> SecretBytes {
        SecretBytes::new(self.encoded.to_vec())
    }

    pub fn public_key(&self) -> [u8; 32] {
        let setup = self
            .setup()
            .expect("validated authority bytes remain valid");
        setup
            .keypair()
            .public()
            .serialize()
            .as_slice()
            .try_into()
            .expect("Ristretto255 public keys are 32 bytes")
    }

    fn setup(&self) -> Result<ServerSetup<Suite>> {
        ServerSetup::<Suite>::deserialize(&self.encoded)
            .map_err(|_| RemoteCryptoError::InvalidMessage)
    }
}

/// Server-side OPAQUE password file, equivalent in sensitivity to a password
/// verifier. Its bytes are wiped on drop and cloning is deliberately absent.
pub struct PasswordFile {
    encoded: Zeroizing<Vec<u8>>,
}

impl PasswordFile {
    pub fn from_bytes(encoded: &[u8]) -> Result<Self> {
        require_length::<ServerRegistrationLen<Suite>>(encoded)?;
        ServerRegistration::<Suite>::deserialize(encoded)
            .map_err(|_| RemoteCryptoError::InvalidMessage)?;
        Ok(Self {
            encoded: Zeroizing::new(encoded.to_vec()),
        })
    }

    /// Export the password file for caller-owned protected persistence.
    pub fn export(&self) -> SecretBytes {
        SecretBytes::new(self.encoded.to_vec())
    }

    fn registration(&self) -> Result<ServerRegistration<Suite>> {
        ServerRegistration::<Suite>::deserialize(&self.encoded)
            .map_err(|_| RemoteCryptoError::InvalidMessage)
    }
}

/// Explicit secret-byte transfer for protected caller-owned persistence.
/// This type has no `Debug` or `Clone` implementation and wipes its bytes on
/// drop.
pub struct SecretBytes {
    encoded: Zeroizing<Vec<u8>>,
}

impl SecretBytes {
    fn new(encoded: Vec<u8>) -> Self {
        Self {
            encoded: Zeroizing::new(encoded),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.encoded
    }
}

pub struct ClientRegistrationStart {
    state: ClientRegistration<Suite>,
    request: Vec<u8>,
}

impl ClientRegistrationStart {
    pub fn request(&self) -> &[u8] {
        &self.request
    }
}

pub struct ClientRegistrationFinish {
    upload: Zeroizing<Vec<u8>>,
}

impl ClientRegistrationFinish {
    pub fn upload(&self) -> &[u8] {
        &self.upload
    }
}

pub struct ClientLoginStart {
    state: ClientLogin<Suite>,
    request: Vec<u8>,
}

impl ClientLoginStart {
    pub fn request(&self) -> &[u8] {
        &self.request
    }
}

pub struct ServerLoginStart {
    state: ServerLogin<Suite>,
    response: Vec<u8>,
    binding: Binding,
}

impl ServerLoginStart {
    pub fn response(&self) -> &[u8] {
        &self.response
    }
}

pub struct ClientLoginFinish {
    finalization: Vec<u8>,
    channel: SecureChannel,
    host_pin: HostPin,
}

impl ClientLoginFinish {
    pub fn finalization(&self) -> &[u8] {
        &self.finalization
    }

    pub fn into_parts(self) -> (Vec<u8>, SecureChannel, HostPin) {
        (self.finalization, self.channel, self.host_pin)
    }
}

pub fn start_client_registration(
    passphrase: &[u8],
    seed: OperationSeed,
) -> Result<ClientRegistrationStart> {
    validate_passphrase(passphrase)?;
    let mut rng = rng_from_seed(&seed);
    let result = ClientRegistration::<Suite>::start(&mut rng, passphrase)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    Ok(ClientRegistrationStart {
        request: result.message.serialize().to_vec(),
        state: result.state,
    })
}

pub fn start_server_registration(
    authority: &ServerAuthority,
    binding: &Binding,
    request: &[u8],
) -> Result<Vec<u8>> {
    require_length::<RegistrationRequestLen<Suite>>(request)?;
    let message = RegistrationRequest::<Suite>::deserialize(request)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let setup = authority.setup()?;
    let credential_identifier = binding.credential_identifier();
    let result = ServerRegistration::<Suite>::start(&setup, message, &credential_identifier)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    Ok(result.message.serialize().to_vec())
}

pub fn finish_client_registration(
    start: ClientRegistrationStart,
    passphrase: &[u8],
    binding: &Binding,
    response: &[u8],
    seed: OperationSeed,
) -> Result<ClientRegistrationFinish> {
    validate_passphrase(passphrase)?;
    require_length::<RegistrationResponseLen<Suite>>(response)?;
    let message = RegistrationResponse::<Suite>::deserialize(response)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let identifiers = identifiers(binding);
    let ksf = OpaqueKsf;
    let mut rng = rng_from_seed(&seed);
    let mut result = start
        .state
        .finish(
            &mut rng,
            passphrase,
            message,
            ClientRegistrationFinishParameters::new(identifiers.as_opaque(), Some(&ksf)),
        )
        .map_err(|_| RemoteCryptoError::AuthenticationFailed)?;
    result.export_key.zeroize();
    Ok(ClientRegistrationFinish {
        upload: Zeroizing::new(result.message.serialize().to_vec()),
    })
}

pub fn finish_server_registration(upload: &[u8]) -> Result<PasswordFile> {
    require_length::<RegistrationUploadLen<Suite>>(upload)?;
    let message = RegistrationUpload::<Suite>::deserialize(upload)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let registration = ServerRegistration::<Suite>::finish(message);
    PasswordFile::from_bytes(&registration.serialize())
}

pub fn start_client_login(passphrase: &[u8], seed: OperationSeed) -> Result<ClientLoginStart> {
    validate_passphrase(passphrase)?;
    let mut rng = rng_from_seed(&seed);
    let result = ClientLogin::<Suite>::start(&mut rng, passphrase)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    Ok(ClientLoginStart {
        request: result.message.serialize().to_vec(),
        state: result.state,
    })
}

pub fn start_server_login(
    authority: &ServerAuthority,
    password_file: Option<&PasswordFile>,
    binding: &Binding,
    request: &[u8],
    seed: OperationSeed,
) -> Result<ServerLoginStart> {
    require_length::<CredentialRequestLen<Suite>>(request)?;
    let message = CredentialRequest::<Suite>::deserialize(request)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let setup = authority.setup()?;
    let record = password_file.map(PasswordFile::registration).transpose()?;
    let values = identifiers(binding);
    let context = binding.context();
    let credential_identifier = binding.credential_identifier();
    let parameters = ServerLoginParameters {
        context: Some(&context),
        identifiers: values.as_opaque(),
    };
    let mut rng = rng_from_seed(&seed);
    let result = ServerLogin::<Suite>::start(
        &mut rng,
        &setup,
        record,
        message,
        &credential_identifier,
        parameters,
    )
    .map_err(|_| RemoteCryptoError::AuthenticationFailed)?;
    Ok(ServerLoginStart {
        response: result.message.serialize().to_vec(),
        state: result.state,
        binding: binding.clone(),
    })
}

pub fn finish_client_login(
    start: ClientLoginStart,
    passphrase: &[u8],
    binding: &Binding,
    expected_pin: Option<HostPin>,
    response: &[u8],
    seed: OperationSeed,
) -> Result<ClientLoginFinish> {
    validate_passphrase(passphrase)?;
    require_length::<CredentialResponseLen<Suite>>(response)?;
    let message = CredentialResponse::<Suite>::deserialize(response)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let values = identifiers(binding);
    let context = binding.context();
    let parameters =
        ClientLoginFinishParameters::new(Some(&context), values.as_opaque(), Some(&OpaqueKsf));
    let mut rng = rng_from_seed(&seed);
    let mut result = start
        .state
        .finish(&mut rng, passphrase, message, parameters)
        .map_err(|_| RemoteCryptoError::AuthenticationFailed)?;
    let public_key: [u8; 32] = result
        .server_s_pk
        .serialize()
        .as_slice()
        .try_into()
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let host_pin = HostPin::new(binding.host_id(), public_key);
    if expected_pin.is_some_and(|pin| pin != host_pin) {
        result.session_key.zeroize();
        result.export_key.zeroize();
        return Err(RemoteCryptoError::HostIdentityChanged);
    }
    let channel = SecureChannel::derive(Role::Client, &result.session_key, binding)?;
    result.session_key.zeroize();
    result.export_key.zeroize();
    Ok(ClientLoginFinish {
        finalization: result.message.serialize().to_vec(),
        channel,
        host_pin,
    })
}

pub fn finish_server_login(start: ServerLoginStart, finalization: &[u8]) -> Result<SecureChannel> {
    require_length::<CredentialFinalizationLen<Suite>>(finalization)?;
    let message = CredentialFinalization::<Suite>::deserialize(finalization)
        .map_err(|_| RemoteCryptoError::InvalidMessage)?;
    let values = identifiers(&start.binding);
    let context = start.binding.context();
    let parameters = ServerLoginParameters {
        context: Some(&context),
        identifiers: values.as_opaque(),
    };
    let mut result = start
        .state
        .finish(message, parameters)
        .map_err(|_| RemoteCryptoError::AuthenticationFailed)?;
    let channel = SecureChannel::derive(Role::Host, &result.session_key, &start.binding)?;
    result.session_key.zeroize();
    Ok(channel)
}

fn validate_passphrase(passphrase: &[u8]) -> Result<()> {
    if !(MIN_PASSPHRASE_LEN..=MAX_PASSPHRASE_LEN).contains(&passphrase.len())
        || core::str::from_utf8(passphrase).is_err()
    {
        return Err(RemoteCryptoError::InvalidPassphrase);
    }
    Ok(())
}

fn rng_from_seed(seed: &OperationSeed) -> ChaCha20Rng {
    ChaCha20Rng::from_seed(*seed.as_bytes())
}

fn require_length<L: Unsigned>(encoded: &[u8]) -> Result<()> {
    if encoded.len() != L::USIZE {
        return Err(RemoteCryptoError::InvalidMessage);
    }
    Ok(())
}

struct OwnedIdentifiers {
    client: Vec<u8>,
    server: Vec<u8>,
}

impl OwnedIdentifiers {
    fn as_opaque(&self) -> Identifiers<'_> {
        Identifiers {
            client: Some(&self.client),
            server: Some(&self.server),
        }
    }
}

fn identifiers(binding: &Binding) -> OwnedIdentifiers {
    OwnedIdentifiers {
        client: binding.client_identifier(),
        server: binding.server_identifier(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostId, RelayId, Username};
    use opaque_ke::generic_array::typenum::U64;

    const PASSWORD: &[u8] = b"correct horse battery staple";

    fn seed(byte: u8) -> OperationSeed {
        OperationSeed::new([byte; 32])
    }

    fn binding() -> Binding {
        Binding::new(
            RelayId::new([1; 32]),
            Username::parse("alice").unwrap(),
            HostId::new([2; 32]),
        )
    }

    fn register(authority: &ServerAuthority, binding: &Binding) -> PasswordFile {
        let client = start_client_registration(PASSWORD, seed(3)).unwrap();
        let response = start_server_registration(authority, binding, client.request()).unwrap();
        let client =
            finish_client_registration(client, PASSWORD, binding, &response, seed(4)).unwrap();
        finish_server_registration(client.upload()).unwrap()
    }

    fn login(
        authority: &ServerAuthority,
        password_file: Option<&PasswordFile>,
        binding: &Binding,
        password: &[u8],
        pin: Option<HostPin>,
    ) -> Result<(SecureChannel, SecureChannel, HostPin)> {
        let client = start_client_login(password, seed(5))?;
        let server =
            start_server_login(authority, password_file, binding, client.request(), seed(6))?;
        let client =
            finish_client_login(client, password, binding, pin, server.response(), seed(7))?;
        let (finalization, client_channel, actual_pin) = client.into_parts();
        let server_channel = finish_server_login(server, &finalization)?;
        Ok((client_channel, server_channel, actual_pin))
    }

    #[test]
    fn passphrase_policy_is_bounded_utf8() {
        assert_eq!(
            start_client_registration(b"too short", seed(0)).err(),
            Some(RemoteCryptoError::InvalidPassphrase)
        );
        assert_eq!(
            start_client_registration(&[0xff; 12], seed(0)).err(),
            Some(RemoteCryptoError::InvalidPassphrase)
        );
        assert_eq!(
            start_client_registration(&vec![b'a'; 257], seed(0)).err(),
            Some(RemoteCryptoError::InvalidPassphrase)
        );
    }

    #[test]
    fn selected_ksf_is_deterministic_and_never_identity() {
        let input = GenericArray::<u8, U64>::clone_from_slice(&[11; 64]);
        let first = OpaqueKsf.hash(input).unwrap();
        let second = OpaqueKsf.hash(input).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, input);

        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_PASSES,
            ARGON2_PARALLELISM,
            Some(U64::USIZE),
        )
        .unwrap();
        assert_eq!(params.m_cost(), 64 * 1024);
        assert_eq!(params.t_cost(), 3);
        assert_eq!(params.p_cost(), 1);
    }

    #[test]
    fn authority_and_password_file_serialization_are_exact() {
        let authority = ServerAuthority::generate(seed(8));
        let bytes = authority.export();
        assert_eq!(bytes.as_bytes().len(), SERVER_AUTHORITY_LEN);
        assert_eq!(
            ServerAuthority::from_bytes(bytes.as_bytes())
                .unwrap()
                .public_key(),
            authority.public_key()
        );
        assert_eq!(
            ServerAuthority::from_bytes(&bytes.as_bytes()[..SERVER_AUTHORITY_LEN - 1]).err(),
            Some(RemoteCryptoError::InvalidMessage)
        );
        let mut extended = bytes.as_bytes().to_vec();
        extended.push(0);
        assert_eq!(
            ServerAuthority::from_bytes(&extended).err(),
            Some(RemoteCryptoError::InvalidMessage)
        );

        let file = register(&authority, &binding());
        let bytes = file.export();
        assert_eq!(
            bytes.as_bytes().len(),
            ServerRegistrationLen::<Suite>::USIZE
        );
        assert!(PasswordFile::from_bytes(bytes.as_bytes()).is_ok());
        assert_eq!(
            PasswordFile::from_bytes(&bytes.as_bytes()[..bytes.as_bytes().len() - 1]).err(),
            Some(RemoteCryptoError::InvalidMessage)
        );

        let registration = start_client_registration(PASSWORD, seed(3)).unwrap();
        let mut request = registration.request().to_vec();
        request.push(0);
        assert_eq!(
            start_server_registration(&authority, &binding(), &request).err(),
            Some(RemoteCryptoError::InvalidMessage)
        );
    }

    #[test]
    fn registration_login_pinning_and_records_work_end_to_end() {
        let authority = ServerAuthority::generate(seed(8));
        let binding = binding();
        let file = register(&authority, &binding);
        let (mut client, mut host, login_pin) =
            login(&authority, Some(&file), &binding, PASSWORD, None).unwrap();
        assert_eq!(login_pin.server_public_key(), &authority.public_key());
        assert!(login(&authority, Some(&file), &binding, PASSWORD, Some(login_pin)).is_ok());

        let command = client.seal(br#"{"type":"command","id":"1"}"#).unwrap();
        assert_eq!(
            host.open(&command).unwrap().plaintext,
            br#"{"type":"command","id":"1"}"#
        );
        let snapshot = host.seal(br#"{"type":"snapshot"}"#).unwrap();
        assert_eq!(
            client.open(&snapshot).unwrap().plaintext,
            br#"{"type":"snapshot"}"#
        );
    }

    #[test]
    fn wrong_password_and_unknown_user_are_indistinguishable() {
        let authority = ServerAuthority::generate(seed(8));
        let binding = binding();
        let file = register(&authority, &binding);
        let wrong = login(
            &authority,
            Some(&file),
            &binding,
            b"incorrect horse battery staple",
            None,
        )
        .err();
        let unknown = login(&authority, None, &binding, PASSWORD, None).err();
        assert_eq!(wrong, Some(RemoteCryptoError::AuthenticationFailed));
        assert_eq!(unknown, Some(RemoteCryptoError::AuthenticationFailed));
    }

    #[test]
    fn route_and_host_substitution_fail_authentication() {
        let authority = ServerAuthority::generate(seed(8));
        let original = binding();
        let file = register(&authority, &original);
        let substituted = Binding::new(
            RelayId::new([9; 32]),
            original.username().clone(),
            original.host_id(),
        );
        assert_eq!(
            login(&authority, Some(&file), &substituted, PASSWORD, None).err(),
            Some(RemoteCryptoError::AuthenticationFailed)
        );
        let substituted = Binding::new(
            original.relay_id(),
            Username::parse("mallory").unwrap(),
            original.host_id(),
        );
        assert_eq!(
            login(&authority, Some(&file), &substituted, PASSWORD, None).err(),
            Some(RemoteCryptoError::AuthenticationFailed)
        );
        let substituted = Binding::new(
            original.relay_id(),
            original.username().clone(),
            HostId::new([9; 32]),
        );
        assert_eq!(
            login(&authority, Some(&file), &substituted, PASSWORD, None).err(),
            Some(RemoteCryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn changed_authority_trips_the_explicit_pin() {
        let authority = ServerAuthority::generate(seed(8));
        let changed = ServerAuthority::generate(seed(9));
        let binding = binding();
        let file = register(&authority, &binding);
        let (_, _, pin) = login(&authority, Some(&file), &binding, PASSWORD, None).unwrap();
        let changed_file = register(&changed, &binding);
        assert_eq!(
            login(&changed, Some(&changed_file), &binding, PASSWORD, Some(pin)).err(),
            Some(RemoteCryptoError::HostIdentityChanged)
        );
        assert!(login(&authority, Some(&file), &binding, PASSWORD, Some(pin)).is_ok());
    }

    #[test]
    fn clone_matrix_requires_authority_and_password_file_together() {
        let authority = ServerAuthority::generate(seed(8));
        let binding = binding();
        let file = register(&authority, &binding);
        let (_, _, pin) = login(&authority, Some(&file), &binding, PASSWORD, None).unwrap();
        let authority_export = authority.export();
        let file_export = file.export();
        let authority_clone = ServerAuthority::from_bytes(authority_export.as_bytes()).unwrap();
        let file_clone = PasswordFile::from_bytes(file_export.as_bytes()).unwrap();
        assert!(
            login(
                &authority_clone,
                Some(&file_clone),
                &binding,
                PASSWORD,
                Some(pin)
            )
            .is_ok()
        );

        let new_authority = ServerAuthority::generate(seed(10));
        assert_eq!(
            login(&new_authority, Some(&file), &binding, PASSWORD, None).err(),
            Some(RemoteCryptoError::AuthenticationFailed)
        );
        assert_eq!(
            login(&authority, None, &binding, PASSWORD, None).err(),
            Some(RemoteCryptoError::AuthenticationFailed)
        );
    }

    #[test]
    fn deterministic_operation_seeds_fix_messages_and_records() {
        let authority = ServerAuthority::generate(seed(8));
        let binding = binding();

        let first = start_client_registration(PASSWORD, seed(3)).unwrap();
        let first_request = first.request().to_vec();
        let first_response =
            start_server_registration(&authority, &binding, first.request()).unwrap();
        let first = finish_client_registration(first, PASSWORD, &binding, &first_response, seed(4))
            .unwrap();
        let first_upload = first.upload().to_vec();
        let file = finish_server_registration(first.upload()).unwrap();

        let second = start_client_registration(PASSWORD, seed(3)).unwrap();
        assert_eq!(second.request(), first_request);
        let second_response =
            start_server_registration(&authority, &binding, second.request()).unwrap();
        assert_eq!(second_response, first_response);
        let second =
            finish_client_registration(second, PASSWORD, &binding, &second_response, seed(4))
                .unwrap();
        assert_eq!(second.upload(), first_upload);

        let first_client = start_client_login(PASSWORD, seed(5)).unwrap();
        let first_login_request = first_client.request().to_vec();
        let first_server = start_server_login(
            &authority,
            Some(&file),
            &binding,
            first_client.request(),
            seed(6),
        )
        .unwrap();
        let first_login_response = first_server.response().to_vec();
        let first_client = finish_client_login(
            first_client,
            PASSWORD,
            &binding,
            None,
            first_server.response(),
            seed(7),
        )
        .unwrap();
        let (first_finalization, mut first_channel, _) = first_client.into_parts();
        let _ = finish_server_login(first_server, &first_finalization).unwrap();
        let first_record = first_channel.seal(b"deterministic").unwrap();

        let second_client = start_client_login(PASSWORD, seed(5)).unwrap();
        assert_eq!(second_client.request(), first_login_request);
        let second_server = start_server_login(
            &authority,
            Some(&file),
            &binding,
            second_client.request(),
            seed(6),
        )
        .unwrap();
        assert_eq!(second_server.response(), first_login_response);
        let second_client = finish_client_login(
            second_client,
            PASSWORD,
            &binding,
            None,
            second_server.response(),
            seed(7),
        )
        .unwrap();
        let (second_finalization, mut second_channel, _) = second_client.into_parts();
        assert_eq!(second_finalization, first_finalization);
        let _ = finish_server_login(second_server, &second_finalization).unwrap();
        assert_eq!(second_channel.seal(b"deterministic").unwrap(), first_record);
    }
}

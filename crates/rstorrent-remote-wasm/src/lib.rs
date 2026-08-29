#![forbid(unsafe_code)]
//! Narrow browser binding for the shared remote-access cryptographic core.

use rstorrent_remote_crypto::{
    AuthorizationChallenge, AuthorizationGeneration, Binding, ClientId, ClientLoginStart,
    ClientRegistrationStart, ClientResumeFinish, HostId, HostPin, OperationSeed, P256PublicKey,
    P256Signature, RelayId, ResumeClientStart, ResumeContext, ResumeServerChallenge, SecureChannel,
    Username, authorization_metadata_digest, authorization_transcript, finish_client_login,
    finish_client_registration, finish_client_resume, start_client_login,
    start_client_registration, start_client_resume,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = ClientRegistration)]
pub struct WasmClientRegistration {
    state: Option<ClientRegistrationStart>,
    request: Vec<u8>,
}

#[wasm_bindgen(js_class = ClientRegistration)]
impl WasmClientRegistration {
    #[wasm_bindgen(constructor)]
    pub fn new(passphrase: &[u8], entropy: &[u8]) -> core::result::Result<Self, JsError> {
        let state =
            start_client_registration(passphrase, operation_seed(entropy)?).map_err(js_error)?;
        let request = state.request().to_vec();
        Ok(Self {
            state: Some(state),
            request,
        })
    }

    pub fn request(&self) -> core::result::Result<Vec<u8>, JsError> {
        if self.state.is_none() {
            return Err(consumed_error());
        }
        Ok(self.request.clone())
    }

    pub fn finish(
        &mut self,
        passphrase: &[u8],
        relay_id: &[u8],
        username: &str,
        host_id: &[u8],
        response: &[u8],
        entropy: &[u8],
    ) -> core::result::Result<Vec<u8>, JsError> {
        let state = self.state.take().ok_or_else(consumed_error)?;
        self.request.clear();
        let binding = binding(relay_id, username, host_id)?;
        let result = finish_client_registration(
            state,
            passphrase,
            &binding,
            response,
            operation_seed(entropy)?,
        )
        .map_err(js_error)?;
        Ok(result.upload().to_vec())
    }
}

#[wasm_bindgen(js_name = ClientLogin)]
pub struct WasmClientLogin {
    state: Option<ClientLoginStart>,
    request: Vec<u8>,
}

#[wasm_bindgen(js_class = ClientLogin)]
impl WasmClientLogin {
    #[wasm_bindgen(constructor)]
    pub fn new(passphrase: &[u8], entropy: &[u8]) -> core::result::Result<Self, JsError> {
        let state = start_client_login(passphrase, operation_seed(entropy)?).map_err(js_error)?;
        let request = state.request().to_vec();
        Ok(Self {
            state: Some(state),
            request,
        })
    }

    pub fn request(&self) -> core::result::Result<Vec<u8>, JsError> {
        if self.state.is_none() {
            return Err(consumed_error());
        }
        Ok(self.request.clone())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        &mut self,
        passphrase: &[u8],
        relay_id: &[u8],
        username: &str,
        host_id: &[u8],
        expected_pin: &[u8],
        response: &[u8],
        entropy: &[u8],
    ) -> core::result::Result<WasmClientSession, JsError> {
        let state = self.state.take().ok_or_else(consumed_error)?;
        self.request.clear();
        let binding = binding(relay_id, username, host_id)?;
        let pin = match expected_pin {
            [] => None,
            encoded => Some(HostPin::from_bytes(encoded).map_err(js_error)?),
        };
        let result = finish_client_login(
            state,
            passphrase,
            &binding,
            pin,
            response,
            operation_seed(entropy)?,
        )
        .map_err(js_error)?;
        let (finalization, channel, host_pin) = result.into_parts();
        Ok(WasmClientSession {
            finalization: Some(finalization),
            channel,
            host_pin,
        })
    }
}

#[wasm_bindgen(js_name = authorizationTranscript)]
#[allow(clippy::too_many_arguments)]
pub fn wasm_authorization_transcript(
    relay_id: &[u8],
    username: &str,
    host_id: &[u8],
    host_pin: &[u8],
    host_resume_public_key: &[u8],
    authorization_generation: u64,
    authorization_challenge: &[u8],
    client_public_key: &[u8],
    label: &str,
    client_build: Option<String>,
    route_observation: Option<String>,
    browser_observation: Option<String>,
) -> core::result::Result<Vec<u8>, JsError> {
    let binding = binding(relay_id, username, host_id)?;
    let host_pin = HostPin::from_bytes(host_pin).map_err(js_error)?;
    let host_resume_public_key =
        P256PublicKey::from_bytes(host_resume_public_key).map_err(js_error)?;
    let authorization_challenge =
        AuthorizationChallenge::new(fixed(authorization_challenge, "authorization challenge")?);
    let client_public_key = P256PublicKey::from_bytes(client_public_key).map_err(js_error)?;
    let digest = authorization_metadata_digest(
        label,
        client_build.as_deref(),
        route_observation.as_deref(),
        browser_observation.as_deref(),
    );
    Ok(authorization_transcript(
        &binding,
        host_pin,
        host_resume_public_key,
        AuthorizationGeneration::new(authorization_generation),
        authorization_challenge,
        client_public_key,
        digest,
    ))
}

#[wasm_bindgen(js_name = ClientResume)]
pub struct WasmClientResume {
    state: Option<ResumeClientStart>,
    hello: Vec<u8>,
    host_pin: HostPin,
}

#[wasm_bindgen(js_class = ClientResume)]
impl WasmClientResume {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        relay_id: &[u8],
        username: &str,
        host_id: &[u8],
        host_pin: &[u8],
        host_resume_public_key: &[u8],
        client_id: &[u8],
        client_public_key: &[u8],
        authorization_generation: u64,
        client_generation: u64,
        protocol_floor: u16,
        entropy: &[u8],
    ) -> core::result::Result<Self, JsError> {
        let binding = binding(relay_id, username, host_id)?;
        let host_pin = HostPin::from_bytes(host_pin).map_err(js_error)?;
        let context = ResumeContext::new(
            binding,
            host_pin,
            P256PublicKey::from_bytes(host_resume_public_key).map_err(js_error)?,
            ClientId::new(fixed(client_id, "client ID")?),
            P256PublicKey::from_bytes(client_public_key).map_err(js_error)?,
            AuthorizationGeneration::new(authorization_generation),
            AuthorizationGeneration::new(client_generation),
            protocol_floor,
        );
        let state = start_client_resume(context, operation_seed(entropy)?);
        let hello = state.hello().to_bytes().to_vec();
        Ok(Self {
            state: Some(state),
            hello,
            host_pin,
        })
    }

    pub fn request(&self) -> core::result::Result<Vec<u8>, JsError> {
        if self.state.is_none() {
            return Err(consumed_error());
        }
        Ok(self.hello.clone())
    }

    pub fn finish(
        &mut self,
        challenge: &[u8],
    ) -> core::result::Result<WasmClientResumeProof, JsError> {
        let state = self.state.take().ok_or_else(consumed_error)?;
        self.hello.clear();
        let challenge = ResumeServerChallenge::from_bytes(challenge).map_err(js_error)?;
        let finish = finish_client_resume(state, &challenge).map_err(js_error)?;
        let signature_input = finish.client_signature_input().to_vec();
        Ok(WasmClientResumeProof {
            finish: Some(finish),
            signature_input,
            host_pin: self.host_pin,
        })
    }
}

#[wasm_bindgen(js_name = ClientResumeProof)]
pub struct WasmClientResumeProof {
    finish: Option<ClientResumeFinish>,
    signature_input: Vec<u8>,
    host_pin: HostPin,
}

#[wasm_bindgen(js_class = ClientResumeProof)]
impl WasmClientResumeProof {
    pub fn signature_input(&self) -> core::result::Result<Vec<u8>, JsError> {
        if self.finish.is_none() {
            return Err(consumed_error());
        }
        Ok(self.signature_input.clone())
    }

    pub fn complete(
        &mut self,
        signature: &[u8],
    ) -> core::result::Result<WasmClientSession, JsError> {
        let finish = self.finish.take().ok_or_else(consumed_error)?;
        self.signature_input.clear();
        let signature = P256Signature::from_bytes(signature).map_err(js_error)?;
        let finalization = ClientResumeFinish::proof(signature).to_bytes().to_vec();
        Ok(WasmClientSession {
            finalization: Some(finalization),
            channel: finish.into_channel(),
            host_pin: self.host_pin,
        })
    }
}

#[wasm_bindgen(js_name = ClientSession)]
pub struct WasmClientSession {
    finalization: Option<Vec<u8>>,
    channel: SecureChannel,
    host_pin: HostPin,
}

#[wasm_bindgen(js_class = ClientSession)]
impl WasmClientSession {
    pub fn take_finalization(&mut self) -> core::result::Result<Vec<u8>, JsError> {
        self.finalization.take().ok_or_else(consumed_error)
    }

    pub fn host_pin(&self) -> Vec<u8> {
        self.host_pin.to_bytes().to_vec()
    }

    pub fn seal(&mut self, plaintext: &[u8]) -> core::result::Result<Vec<u8>, JsError> {
        self.channel.seal(plaintext).map_err(js_error)
    }

    pub fn seal_close(&mut self) -> core::result::Result<Vec<u8>, JsError> {
        self.channel.seal_close().map_err(js_error)
    }

    pub fn open(&mut self, record: &[u8]) -> core::result::Result<WasmOpenedRecord, JsError> {
        let opened = self.channel.open(record).map_err(js_error)?;
        Ok(WasmOpenedRecord {
            plaintext: opened.plaintext,
            close: opened.is_close,
        })
    }
}

#[wasm_bindgen(js_name = OpenedRecord)]
pub struct WasmOpenedRecord {
    plaintext: Vec<u8>,
    close: bool,
}

#[wasm_bindgen(js_class = OpenedRecord)]
impl WasmOpenedRecord {
    #[wasm_bindgen(getter)]
    pub fn plaintext(&self) -> Vec<u8> {
        self.plaintext.clone()
    }

    #[wasm_bindgen(getter, js_name = isClose)]
    pub fn is_close(&self) -> bool {
        self.close
    }
}

#[cfg(feature = "ksf-bench")]
#[wasm_bindgen(js_name = exerciseArgon2idCandidate)]
pub fn exercise_argon2id_candidate(
    input: &[u8],
    memory_kib: u32,
    passes: u32,
) -> core::result::Result<(), JsError> {
    rstorrent_remote_crypto::exercise_argon2id_candidate(input, memory_kib, passes)
        .map_err(js_error)
}

fn operation_seed(encoded: &[u8]) -> core::result::Result<OperationSeed, JsError> {
    let bytes: [u8; 32] = encoded
        .try_into()
        .map_err(|_| JsError::new("entropy must contain exactly 32 bytes"))?;
    Ok(OperationSeed::new(bytes))
}

fn fixed<const N: usize>(encoded: &[u8], label: &str) -> core::result::Result<[u8; N], JsError> {
    encoded
        .try_into()
        .map_err(|_| JsError::new(&format!("{label} must contain exactly {N} bytes")))
}

fn binding(
    relay_id: &[u8],
    username: &str,
    host_id: &[u8],
) -> core::result::Result<Binding, JsError> {
    let relay_id = relay_id
        .try_into()
        .map(RelayId::new)
        .map_err(|_| JsError::new("relay ID must contain exactly 32 bytes"))?;
    let host_id = host_id
        .try_into()
        .map(HostId::new)
        .map_err(|_| JsError::new("host ID must contain exactly 32 bytes"))?;
    let username = Username::parse(username).map_err(js_error)?;
    Ok(Binding::new(relay_id, username, host_id))
}

fn js_error(error: rstorrent_remote_crypto::RemoteCryptoError) -> JsError {
    JsError::new(&error.to_string())
}

fn consumed_error() -> JsError {
    JsError::new("protocol state has already been consumed")
}

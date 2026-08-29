#![forbid(unsafe_code)]
//! Narrow browser binding for the shared remote-access cryptographic core.

use rstorrent_remote_crypto::{
    Binding, ClientLoginStart, ClientRegistrationStart, HostId, HostPin, OperationSeed, RelayId,
    SecureChannel, Username, finish_client_login, finish_client_registration, start_client_login,
    start_client_registration,
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

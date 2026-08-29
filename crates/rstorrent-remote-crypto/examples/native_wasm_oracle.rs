//! Controlled native endpoint/oracle for the real-browser Wasm proof.
//!
//! The line protocol carries only ordinary protocol messages and benign test
//! application bytes. It never emits the passphrase, authority, password file,
//! session keys, record keys, or RNG seeds.

use std::io::{self, BufRead, Write};

use rstorrent_remote_crypto::{
    Binding, ClientRegistrationStart, HostId, HostPin, OperationSeed, PasswordFile, RelayId,
    SecureChannel, ServerAuthority, ServerLoginStart, Username, finish_client_login,
    finish_client_registration, finish_server_login, finish_server_registration,
    random_operation_seed, start_client_login, start_client_registration, start_server_login,
    start_server_registration,
};

const PASSPHRASE: &[u8] = b"correct horse battery staple";
const USERNAME: &str = "browser-proof";
const RELAY_ID: [u8; 32] = [0x11; 32];
const HOST_ID: [u8; 32] = [0x22; 32];

enum Mode {
    Deterministic,
    BrowserRandom,
}

struct Oracle {
    mode: Mode,
    authority: ServerAuthority,
    binding: Binding,
    password_file: Option<PasswordFile>,
    native_registration: Option<ClientRegistrationStart>,
    expected_upload: Option<Vec<u8>>,
    server_login: Option<ServerLoginStart>,
    expected_finalization: Option<Vec<u8>>,
    host_channel: Option<SecureChannel>,
    native_client_channel: Option<SecureChannel>,
}

impl Oracle {
    fn new(mode: Mode) -> Result<Self, Box<dyn std::error::Error>> {
        let deterministic = matches!(mode, Mode::Deterministic);
        let authority_seed = if deterministic {
            seed(8)
        } else {
            random_operation_seed()?
        };
        let binding = Binding::new(
            RelayId::new(RELAY_ID),
            Username::parse(USERNAME)?,
            HostId::new(HOST_ID),
        );
        let native_registration = if deterministic {
            Some(start_client_registration(PASSPHRASE, seed(3))?)
        } else {
            None
        };
        Ok(Self {
            mode,
            authority: ServerAuthority::generate(authority_seed),
            binding,
            password_file: None,
            native_registration,
            expected_upload: None,
            server_login: None,
            expected_finalization: None,
            host_channel: None,
            native_client_channel: None,
        })
    }

    fn registration_start(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        if let Some(native) = &self.native_registration
            && native.request() != request
        {
            return Err("native/Wasm registration request mismatch".to_owned());
        }
        let response = start_server_registration(&self.authority, &self.binding, request)
            .map_err(safe_error)?;
        if let Some(native) = self.native_registration.take() {
            let result =
                finish_client_registration(native, PASSPHRASE, &self.binding, &response, seed(4))
                    .map_err(safe_error)?;
            self.expected_upload = Some(result.upload().to_vec());
        }
        Ok(response)
    }

    fn registration_finish(&mut self, upload: &[u8]) -> Result<(), String> {
        if self
            .expected_upload
            .take()
            .is_some_and(|expected| expected != upload)
        {
            return Err("native/Wasm registration upload mismatch".to_owned());
        }
        self.password_file = Some(finish_server_registration(upload).map_err(safe_error)?);
        Ok(())
    }

    fn login_start(&mut self, request: &[u8]) -> Result<Vec<u8>, String> {
        if self.password_file.is_none() || self.server_login.is_some() {
            return Err("invalid oracle login state".to_owned());
        }

        let native_client = if matches!(self.mode, Mode::Deterministic) {
            let native = start_client_login(PASSPHRASE, seed(5)).map_err(safe_error)?;
            if native.request() != request {
                return Err("native/Wasm login request mismatch".to_owned());
            }
            Some(native)
        } else {
            None
        };
        let server_seed = if matches!(self.mode, Mode::Deterministic) {
            seed(6)
        } else {
            random_operation_seed().map_err(safe_error)?
        };
        let server = start_server_login(
            &self.authority,
            self.password_file.as_ref(),
            &self.binding,
            request,
            server_seed,
        )
        .map_err(safe_error)?;
        let response = server.response().to_vec();

        if let Some(native) = native_client {
            let result =
                finish_client_login(native, PASSPHRASE, &self.binding, None, &response, seed(7))
                    .map_err(safe_error)?;
            let (finalization, channel, pin) = result.into_parts();
            if pin != self.expected_pin() {
                return Err("native host pin mismatch".to_owned());
            }
            self.expected_finalization = Some(finalization);
            self.native_client_channel = Some(channel);
        }
        self.server_login = Some(server);
        Ok(response)
    }

    fn login_finish(&mut self, finalization: &[u8]) -> Result<(), String> {
        if self
            .expected_finalization
            .take()
            .is_some_and(|expected| expected != finalization)
        {
            return Err("native/Wasm finalization mismatch".to_owned());
        }
        let server = self
            .server_login
            .take()
            .ok_or_else(|| "missing oracle login state".to_owned())?;
        self.host_channel = Some(finish_server_login(server, finalization).map_err(safe_error)?);
        Ok(())
    }

    fn verify_pin(&self, encoded: &[u8]) -> Result<(), String> {
        let pin = HostPin::from_bytes(encoded).map_err(safe_error)?;
        if pin != self.expected_pin() {
            return Err("browser host pin mismatch".to_owned());
        }
        Ok(())
    }

    fn open(&mut self, record: &[u8]) -> Result<(Vec<u8>, bool), String> {
        let opened = self
            .host_channel
            .as_mut()
            .ok_or_else(|| "missing host channel".to_owned())?
            .open(record)
            .map_err(safe_error)?;
        if let Some(native) = &mut self.native_client_channel {
            let expected = if opened.is_close {
                native.seal_close()
            } else {
                native.seal(&opened.plaintext)
            }
            .map_err(safe_error)?;
            if expected != record {
                return Err("native/Wasm client record mismatch".to_owned());
            }
        }
        Ok((opened.plaintext, opened.is_close))
    }

    fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let record = self
            .host_channel
            .as_mut()
            .ok_or_else(|| "missing host channel".to_owned())?
            .seal(plaintext)
            .map_err(safe_error)?;
        if let Some(native) = &mut self.native_client_channel {
            let opened = native.open(&record).map_err(safe_error)?;
            if opened.is_close || opened.plaintext != plaintext {
                return Err("native host record mismatch".to_owned());
            }
        }
        Ok(record)
    }

    fn expected_pin(&self) -> HostPin {
        HostPin::new(self.binding.host_id(), self.authority.public_key())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mode_line = lines.next().ok_or("missing mode")??;
    let mode = match mode_line.as_str() {
        "MODE deterministic" => Mode::Deterministic,
        "MODE browser-random" => Mode::BrowserRandom,
        _ => return Err("invalid mode".into()),
    };
    let mut oracle = Oracle::new(mode)?;
    send(&format!(
        "READY {} {} {USERNAME}",
        encode_hex(&RELAY_ID),
        encode_hex(&HOST_ID)
    ))?;

    for line in lines {
        let line = line?;
        let (command, argument) = line.split_once(' ').unwrap_or((&line, ""));
        let result = handle(&mut oracle, command, argument);
        match result {
            Ok(response) => send(&response)?,
            Err(error) => {
                send(&format!("ERROR {error}"))?;
                return Err(error.into());
            }
        }
        if command == "QUIT" {
            return Ok(());
        }
    }
    Err("oracle input closed before QUIT".into())
}

fn handle(oracle: &mut Oracle, command: &str, argument: &str) -> Result<String, String> {
    match command {
        "REG_START" => oracle
            .registration_start(&decode_hex(argument)?)
            .map(|response| format!("REG_RESPONSE {}", encode_hex(&response))),
        "REG_FINISH" => oracle
            .registration_finish(&decode_hex(argument)?)
            .map(|()| "OK".to_owned()),
        "LOGIN_START" => oracle
            .login_start(&decode_hex(argument)?)
            .map(|response| format!("LOGIN_RESPONSE {}", encode_hex(&response))),
        "LOGIN_FINISH" => oracle
            .login_finish(&decode_hex(argument)?)
            .map(|()| "OK".to_owned()),
        "PIN" => oracle
            .verify_pin(&decode_hex(argument)?)
            .map(|()| "OK".to_owned()),
        "OPEN" => oracle
            .open(&decode_hex(argument)?)
            .map(|(plaintext, close)| {
                format!("OPENED {} {}", u8::from(close), encode_hex(&plaintext))
            }),
        "SEAL" => oracle
            .seal(&decode_hex(argument)?)
            .map(|record| format!("RECORD {}", encode_hex(&record))),
        "QUIT" => Ok("BYE".to_owned()),
        _ => Err("unknown oracle command".to_owned()),
    }
}

fn seed(byte: u8) -> OperationSeed {
    OperationSeed::new([byte; 32])
}

fn safe_error(error: rstorrent_remote_crypto::RemoteCryptoError) -> String {
    error.to_string()
}

fn send(line: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush()
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn decode_hex(encoded: &str) -> Result<Vec<u8>, String> {
    if !encoded.len().is_multiple_of(2) {
        return Err("invalid hex length".to_owned());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_nibble(pair[0])?;
            let low = decode_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn decode_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("invalid hex digit".to_owned()),
    }
}

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use p256::ecdsa::SigningKey;
use rand::rngs::OsRng;
use rstorrent_remote_access::{
    AuthenticationMethod, AuthorityStore, EventId, ProvisioningMaterial, RemoteAuthority,
    SecuritySnapshot, Timestamp,
};
use rstorrent_remote_crypto::{
    ClientId, HostId, OperationSeed, RelayId, Username, random_operation_seed,
};
use rstorrent_remote_relay::{ReserveRouteRequest, ReserveRouteResponse};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_tungstenite::Connector;
use url::Url;
use zeroize::Zeroizing;

use crate::error::Result;
use crate::runtime::run_host;
use crate::{RemoteHostError, wire};

const PROTOCOL_FLOOR: u16 = 1;
const MAX_RELAY_CERTIFICATE_BYTES: usize = 64 * 1024;
const MIN_PASSPHRASE_BYTES: usize = 12;
const MAX_PASSPHRASE_BYTES: usize = 256;

pub struct RemoteHostConfig {
    relay_base: Url,
    relay_connector: Connector,
    relay_http: reqwest::Client,
    gateway_websocket_url: String,
    gateway_origin: String,
    gateway_token: Zeroizing<String>,
    host_build: String,
}

impl RemoteHostConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        relay_base: &str,
        relay_certificate_der: Vec<u8>,
        gateway_websocket_url: impl Into<String>,
        gateway_origin: impl Into<String>,
        gateway_token: String,
        host_build: impl Into<String>,
    ) -> Result<Self> {
        let relay_base =
            Url::parse(relay_base).map_err(|_| RemoteHostError::Configuration("relay URL"))?;
        if relay_base.scheme() != "https"
            || relay_base.username() != ""
            || relay_base.password().is_some()
            || relay_base.path() != "/"
            || relay_base.query().is_some()
            || relay_base.fragment().is_some()
            || !relay_base.host().is_some_and(|host| match host {
                url::Host::Domain(name) => name == "localhost",
                url::Host::Ipv4(address) => address.is_loopback(),
                url::Host::Ipv6(address) => address.is_loopback(),
            })
        {
            return Err(RemoteHostError::Configuration(
                "relay must be one exact loopback HTTPS origin",
            ));
        }
        if relay_certificate_der.is_empty()
            || relay_certificate_der.len() > MAX_RELAY_CERTIFICATE_BYTES
        {
            return Err(RemoteHostError::Configuration("relay certificate"));
        }
        let certificate = CertificateDer::from(relay_certificate_der.clone());
        let mut roots = RootCertStore::empty();
        roots
            .add(certificate.clone())
            .map_err(|_| RemoteHostError::Configuration("relay certificate"))?;
        let relay_tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let relay_http_certificate = reqwest::tls::Certificate::from_der(&relay_certificate_der)
            .map_err(|_| RemoteHostError::Configuration("relay certificate"))?;
        let relay_http = reqwest::Client::builder()
            .add_root_certificate(relay_http_certificate)
            .https_only(true)
            .build()
            .map_err(|_| RemoteHostError::Configuration("relay HTTP client"))?;

        let gateway_websocket_url = gateway_websocket_url.into();
        let gateway_url = Url::parse(&gateway_websocket_url)
            .map_err(|_| RemoteHostError::Configuration("gateway URL"))?;
        if gateway_url.scheme() != "ws"
            || !gateway_url.host().is_some_and(|host| match host {
                url::Host::Domain(name) => name == "localhost",
                url::Host::Ipv4(address) => address.is_loopback(),
                url::Host::Ipv6(address) => address.is_loopback(),
            })
            || gateway_url.path() != "/api/v1/connect"
            || gateway_url.query().is_some()
            || gateway_url.fragment().is_some()
        {
            return Err(RemoteHostError::Configuration(
                "gateway must be one exact loopback WebSocket",
            ));
        }
        let gateway_origin = gateway_origin.into();
        let origin = Url::parse(&gateway_origin)
            .map_err(|_| RemoteHostError::Configuration("gateway origin"))?;
        if origin.scheme() != "http"
            || origin.path() != "/"
            || !origin.host().is_some_and(|host| match host {
                url::Host::Domain(name) => name == "localhost",
                url::Host::Ipv4(address) => address.is_loopback(),
                url::Host::Ipv6(address) => address.is_loopback(),
            })
        {
            return Err(RemoteHostError::Configuration("gateway origin"));
        }
        if gateway_token.is_empty() || gateway_token.len() > 128 {
            return Err(RemoteHostError::Configuration("gateway token"));
        }
        let host_build = host_build.into();
        if host_build.is_empty() || host_build.len() > 160 {
            return Err(RemoteHostError::Configuration("host build"));
        }
        Ok(Self {
            relay_base,
            relay_connector: Connector::Rustls(Arc::new(relay_tls)),
            relay_http,
            gateway_websocket_url,
            gateway_origin,
            gateway_token: Zeroizing::new(gateway_token),
            host_build,
        })
    }

    pub(crate) fn relay_websocket_url(&self, path: &str) -> Result<String> {
        let mut url = self.relay_base.clone();
        url.set_scheme("wss")
            .map_err(|_| RemoteHostError::Configuration("relay URL scheme"))?;
        url.set_path(path);
        Ok(url.into())
    }

    pub(crate) fn relay_http_url(&self, path: &str) -> String {
        let mut url = self.relay_base.clone();
        url.set_path(path);
        url.into()
    }

    pub(crate) fn relay_connector(&self) -> Connector {
        self.relay_connector.clone()
    }

    pub(crate) fn gateway_websocket_url(&self) -> &str {
        &self.gateway_websocket_url
    }

    pub(crate) fn gateway_origin(&self) -> &str {
        &self.gateway_origin
    }

    pub(crate) fn gateway_token(&self) -> &str {
        &self.gateway_token
    }

    pub(crate) fn host_build(&self) -> &str {
        &self.host_build
    }
}

pub(crate) struct SharedOwner {
    pub(crate) store: AuthorityStore,
    pub(crate) state: Mutex<OwnerState>,
    pub(crate) config: RemoteHostConfig,
    pub(crate) shutdown: tokio_util::sync::CancellationToken,
    pub(crate) next_connection_generation: AtomicU64,
}

pub(crate) struct OwnerState {
    pub(crate) authority: Option<RemoteAuthority>,
    pub(crate) circuits: BTreeMap<[u8; 16], LiveCircuit>,
}

pub(crate) struct LiveCircuit {
    pub(crate) client_id: Option<ClientId>,
    pub(crate) authentication_method: AuthenticationMethod,
    pub(crate) connection_generation: u64,
    pub(crate) started: Timestamp,
    pub(crate) last_activity: Timestamp,
    pub(crate) route: String,
    pub(crate) cancellation: tokio_util::sync::CancellationToken,
}

struct HostLifecycle {
    cancellation: tokio_util::sync::CancellationToken,
    task: JoinHandle<()>,
}

pub struct RemoteAccessOwner {
    pub(crate) shared: Arc<SharedOwner>,
    lifecycle: Mutex<Option<HostLifecycle>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCircuitView {
    pub circuit_id: String,
    pub client_id: Option<String>,
    pub authentication_method: AuthenticationMethod,
    pub connection_generation: u64,
    pub started: Timestamp,
    pub last_activity: Timestamp,
    pub route: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSecurityView {
    pub enabled: bool,
    pub username: Option<String>,
    pub route: Option<String>,
    pub relay_id: Option<String>,
    pub host_pin: Option<String>,
    pub authority: Option<SecuritySnapshot>,
    pub retained_history: Option<SecuritySnapshot>,
    pub live_circuits: Vec<LiveCircuitView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisableRemoteAccessOutcome {
    pub authority_file_removed: bool,
    pub route_released: bool,
}

impl RemoteAccessOwner {
    pub async fn open(root: impl Into<PathBuf>, config: RemoteHostConfig) -> Result<Self> {
        let store = AuthorityStore::new(root);
        let authority = store.load()?;
        let owner = Self {
            shared: Arc::new(SharedOwner {
                store,
                state: Mutex::new(OwnerState {
                    authority,
                    circuits: BTreeMap::new(),
                }),
                config,
                shutdown: tokio_util::sync::CancellationToken::new(),
                next_connection_generation: AtomicU64::new(1),
            }),
            lifecycle: Mutex::new(None),
        };
        if owner.shared.state.lock().await.authority.is_some() {
            owner.start_host().await;
        }
        Ok(owner)
    }

    pub async fn enable(&self, username: &str, passphrase: &[u8]) -> Result<RemoteSecurityView> {
        if !(MIN_PASSPHRASE_BYTES..=MAX_PASSPHRASE_BYTES).contains(&passphrase.len()) {
            return Err(RemoteHostError::Configuration("passphrase length"));
        }
        let username =
            Username::parse(username).map_err(|_| RemoteHostError::Configuration("username"))?;
        {
            let state = self.shared.state.lock().await;
            if state.authority.is_some() {
                return Err(RemoteHostError::Configuration(
                    "remote access is already enabled",
                ));
            }
        }
        let relay_key = SigningKey::random(&mut OsRng);
        let relay_secret: [u8; 32] = relay_key.to_bytes().into();
        let relay_public = relay_key.verifying_key().to_encoded_point(false);
        let reservation = ReserveRouteRequest {
            username: username.as_str().to_owned(),
            public_key: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(relay_public.as_bytes()),
        };
        let response = self
            .shared
            .config
            .relay_http
            .post(self.shared.config.relay_http_url("/v1/reservations"))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&reservation).map_err(|_| RemoteHostError::Protocol)?)
            .send()
            .await
            .map_err(|_| RemoteHostError::Relay)?;
        if !response.status().is_success() {
            return Err(RemoteHostError::Relay);
        }
        let response: ReserveRouteResponse =
            serde_json::from_slice(&response.bytes().await.map_err(|_| RemoteHostError::Relay)?)
                .map_err(|_| RemoteHostError::Relay)?;
        let relay_id = RelayId::new(wire::decode_id(&response.relay_id)?);
        let authority = RemoteAuthority::provision(
            username.clone(),
            passphrase,
            username.as_str(),
            PROTOCOL_FLOOR,
            now(),
            random_event_id()?,
            ProvisioningMaterial::new(
                HostId::new(random_array()?),
                relay_id,
                relay_secret,
                random_operation_seed().map_err(|_| RemoteHostError::Protocol)?,
                random_operation_seed().map_err(|_| RemoteHostError::Protocol)?,
                random_operation_seed().map_err(|_| RemoteHostError::Protocol)?,
                random_operation_seed().map_err(|_| RemoteHostError::Protocol)?,
            ),
        )?;
        self.shared.store.create(&authority)?;
        self.shared.state.lock().await.authority = Some(authority);
        self.start_host().await;
        self.security_view().await
    }

    pub async fn security_view(&self) -> Result<RemoteSecurityView> {
        let state = self.shared.state.lock().await;
        let retained_history = self.shared.store.load_history()?;
        let (username, route, relay_id, host_pin, authority) = match &state.authority {
            Some(authority) => (
                Some(authority.binding().username().as_str().to_owned()),
                Some(authority.route().to_owned()),
                Some(wire::encode_id(authority.binding().relay_id().as_bytes())),
                Some(wire::encode_id(&authority.host_pin().to_bytes())),
                Some(authority.security_snapshot()),
            ),
            None => (None, None, None, None, None),
        };
        let live_circuits = state
            .circuits
            .iter()
            .map(|(circuit_id, circuit)| LiveCircuitView {
                circuit_id: wire::encode_id(circuit_id),
                client_id: circuit
                    .client_id
                    .map(|client_id| wire::encode_id(client_id.as_bytes())),
                authentication_method: circuit.authentication_method,
                connection_generation: circuit.connection_generation,
                started: circuit.started,
                last_activity: circuit.last_activity,
                route: circuit.route.clone(),
            })
            .collect();
        Ok(RemoteSecurityView {
            enabled: state.authority.is_some(),
            username,
            route,
            relay_id,
            host_pin,
            authority,
            retained_history,
            live_circuits,
        })
    }

    pub async fn revoke(&self, encoded_client_id: &str) -> Result<()> {
        let client_id = ClientId::new(wire::decode_id(encoded_client_id)?);
        let mut state = self.shared.state.lock().await;
        let event_id = random_event_id()?;
        let authority = state
            .authority
            .as_mut()
            .ok_or(RemoteHostError::Configuration("remote access is disabled"))?;
        self.shared.store.update(authority, |candidate| {
            candidate.revoke_client(client_id, now(), event_id)
        })?;
        for circuit in state
            .circuits
            .values()
            .filter(|circuit| circuit.client_id == Some(client_id))
        {
            circuit.cancellation.cancel();
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        self.shared.shutdown.cancel();
        self.stop_host().await;
    }

    async fn start_host(&self) {
        let mut lifecycle = self.lifecycle.lock().await;
        if lifecycle.is_some() || self.shared.shutdown.is_cancelled() {
            return;
        }
        let cancellation = tokio_util::sync::CancellationToken::new();
        let shared = self.shared.clone();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move { run_host(shared, task_cancellation).await });
        *lifecycle = Some(HostLifecycle { cancellation, task });
    }

    async fn stop_host(&self) {
        let lifecycle = self.lifecycle.lock().await.take();
        if let Some(lifecycle) = lifecycle {
            lifecycle.cancellation.cancel();
            let _ = lifecycle.task.await;
        }
    }
}

pub(crate) fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    Timestamp::from_millis(u64::try_from(millis).unwrap_or(u64::MAX))
}

pub(crate) fn random_array<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::fill(&mut bytes).map_err(|_| RemoteHostError::Protocol)?;
    Ok(bytes)
}

pub(crate) fn random_event_id() -> Result<EventId> {
    Ok(EventId::new(random_array()?))
}

pub(crate) fn random_seed() -> Result<OperationSeed> {
    Ok(OperationSeed::new(random_array()?))
}

impl SharedOwner {
    pub(crate) fn connection_generation(&self) -> u64 {
        self.next_connection_generation
            .fetch_add(1, Ordering::Relaxed)
    }
}

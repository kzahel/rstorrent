//! Bounded IPv4 UPnP IGD v2 control point.
//!
//! The public surface is deliberately one-service and one-mapping shaped. The
//! XML, URL, SOAP, and mapping comparisons remain deterministic; Tokio owns
//! only discovery and HTTP awaits.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::Client;
use reqwest::header::{CONTENT_TYPE, HeaderMap};
use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout_at};
use tokio_util::sync::CancellationToken;
use url::Url;

pub const SSDP_MULTICAST_ENDPOINT: SocketAddrV4 =
    SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1_900);
pub const REQUESTED_LEASE_SECONDS: u32 = 3_600;
pub const RENEWAL_NUMERATOR: u32 = 3;
pub const RENEWAL_DENOMINATOR: u32 = 4;
pub const MAX_SSDP_DATAGRAM_BYTES: usize = 8 * 1_024;
pub const MAX_SSDP_HEADERS: usize = 64;
pub const MAX_DEVICE_LOCATIONS: usize = 8;
pub const MAX_URL_BYTES: usize = 2 * 1_024;
pub const MAX_HTTP_BODY_BYTES: usize = 256 * 1_024;
pub const MAX_XML_DEPTH: usize = 32;
pub const MAX_XML_EVENTS: usize = 8_192;
pub const MAX_XML_TEXT_BYTES: usize = 2 * 1_024;
pub const MAX_SERVICE_CANDIDATES: usize = 64;
pub const MAX_MAPPING_CANDIDATES: usize = 4;
pub const MAX_ERROR_DETAIL_BYTES: usize = 512;

const WAN_IP_CONNECTION_V2: &str = "urn:schemas-upnp-org:service:WANIPConnection:2";
const MAPPING_DESCRIPTION: &str = "RSTorrent";
const DISCOVERY_ATTEMPTS: usize = 3;
const DISCOVERY_WINDOW: Duration = Duration::from_millis(900);
const DISCOVERY_DEADLINE: Duration = Duration::from_secs(8);
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_OPERATION_PAUSE: Duration = Duration::from_millis(100);
const HIGH_PORT_START: u16 = 40_000;
const HIGH_PORT_COUNT: u16 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpnpStage {
    Discovery,
    Description,
    ExternalAddress,
    Add,
    Verify,
    Renewal,
    Delete,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpnpError {
    stage: UpnpStage,
    detail: String,
    fault_code: Option<u16>,
    transport: bool,
}

impl UpnpError {
    fn new(stage: UpnpStage, detail: impl AsRef<str>) -> Self {
        Self {
            stage,
            detail: bounded(detail.as_ref(), MAX_ERROR_DETAIL_BYTES),
            fault_code: None,
            transport: false,
        }
    }

    fn fault(stage: UpnpStage, code: u16, description: &str) -> Self {
        Self {
            stage,
            detail: bounded(
                &format!("UPnP fault {code}: {description}"),
                MAX_ERROR_DETAIL_BYTES,
            ),
            fault_code: Some(code),
            transport: false,
        }
    }

    pub fn stage(&self) -> UpnpStage {
        self.stage
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn fault_code(&self) -> Option<u16> {
        self.fault_code
    }

    pub fn is_transport(&self) -> bool {
        self.transport
    }
}

impl fmt::Display for UpnpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl Error for UpnpError {}

#[derive(Clone, Debug)]
pub struct UpnpDiscoveryConfig {
    local_address: Ipv4Addr,
    discovery_endpoint: SocketAddrV4,
    allow_loopback_gateway: bool,
    attempts: usize,
    response_window: Duration,
    overall_deadline: Duration,
    http_timeout: Duration,
}

impl UpnpDiscoveryConfig {
    pub fn new(local_address: Ipv4Addr) -> Result<Self, UpnpError> {
        if !eligible_local_address(local_address) {
            return Err(UpnpError::new(
                UpnpStage::Discovery,
                "UPnP requires a concrete non-loopback IPv4 listener address",
            ));
        }
        Ok(Self {
            local_address,
            discovery_endpoint: SSDP_MULTICAST_ENDPOINT,
            allow_loopback_gateway: false,
            attempts: DISCOVERY_ATTEMPTS,
            response_window: DISCOVERY_WINDOW,
            overall_deadline: DISCOVERY_DEADLINE,
            http_timeout: HTTP_TIMEOUT,
        })
    }

    /// Builds a loopback-scoped configuration for deterministic protocol tests.
    ///
    /// Product code must use [`Self::new`], which enforces a concrete local-
    /// network address and multicast discovery.
    #[doc(hidden)]
    pub fn scripted_for_testing(local_address: Ipv4Addr, discovery_endpoint: SocketAddrV4) -> Self {
        Self {
            local_address,
            discovery_endpoint,
            allow_loopback_gateway: true,
            attempts: 1,
            response_window: Duration::from_millis(300),
            overall_deadline: Duration::from_secs(2),
            http_timeout: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpnpMappingEntry {
    pub internal_client: Ipv4Addr,
    pub internal_port: u16,
    pub enabled: bool,
    pub description: String,
    pub lease_seconds: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpnpMapping {
    pub local_endpoint: SocketAddrV4,
    pub external_address: Ipv4Addr,
    pub external_port: u16,
    pub lease_seconds: u32,
}

impl UpnpMapping {
    pub fn renewal_delay(&self) -> Duration {
        Duration::from_secs(
            u64::from(self.lease_seconds) * u64::from(RENEWAL_NUMERATOR)
                / u64::from(RENEWAL_DENOMINATOR),
        )
    }
}

#[derive(Clone, Debug)]
pub struct UpnpGateway {
    local_address: Ipv4Addr,
    gateway_address: Ipv4Addr,
    control_url: Url,
    service_type: String,
    client: Client,
}

impl UpnpGateway {
    pub async fn external_address(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Ipv4Addr, UpnpError> {
        let leaves = self
            .soap(
                UpnpStage::ExternalAddress,
                "GetExternalIPAddress",
                &[],
                cancellation,
            )
            .await?;
        let value = unique_leaf(&leaves, "NewExternalIPAddress", UpnpStage::ExternalAddress)?;
        let address = value.parse::<Ipv4Addr>().map_err(|_| {
            UpnpError::new(
                UpnpStage::ExternalAddress,
                "gateway returned an invalid external IPv4 address",
            )
        })?;
        if !eligible_external_address(address) {
            return Err(UpnpError::new(
                UpnpStage::ExternalAddress,
                "gateway returned a non-public external IPv4 address",
            ));
        }
        Ok(address)
    }

    pub async fn query_mapping(
        &self,
        external_port: u16,
        stage: UpnpStage,
        cancellation: &CancellationToken,
    ) -> Result<Option<UpnpMappingEntry>, UpnpError> {
        let port = external_port.to_string();
        let result = self
            .soap(
                stage,
                "GetSpecificPortMappingEntry",
                &[
                    ("NewRemoteHost", ""),
                    ("NewExternalPort", &port),
                    ("NewProtocol", "TCP"),
                ],
                cancellation,
            )
            .await;
        let leaves = match result {
            Ok(leaves) => leaves,
            Err(error) if error.fault_code() == Some(714) => return Ok(None),
            Err(error) => return Err(error),
        };
        let internal_client = unique_leaf(&leaves, "NewInternalClient", stage)?
            .parse::<Ipv4Addr>()
            .map_err(|_| UpnpError::new(stage, "mapping entry has an invalid internal client"))?;
        let internal_port = parse_u16(
            unique_leaf(&leaves, "NewInternalPort", stage)?,
            stage,
            "mapping entry has an invalid internal port",
        )?;
        let enabled = match unique_leaf(&leaves, "NewEnabled", stage)? {
            "0" => false,
            "1" => true,
            _ => {
                return Err(UpnpError::new(
                    stage,
                    "mapping entry has invalid enabled state",
                ));
            }
        };
        let description = unique_leaf(&leaves, "NewPortMappingDescription", stage)?.to_owned();
        let lease_seconds = unique_leaf(&leaves, "NewLeaseDuration", stage)?
            .parse::<u32>()
            .map_err(|_| UpnpError::new(stage, "mapping entry has an invalid lease"))?;
        Ok(Some(UpnpMappingEntry {
            internal_client,
            internal_port,
            enabled,
            description,
            lease_seconds,
        }))
    }

    pub async fn create_mapping(
        &self,
        local_port: u16,
        cancellation: &CancellationToken,
    ) -> Result<UpnpMapping, UpnpError> {
        if local_port == 0 {
            return Err(UpnpError::new(
                UpnpStage::Add,
                "mapping requires a nonzero local port",
            ));
        }
        let external_address = self.external_address(cancellation).await?;
        let candidates = mapping_candidates(local_port)?;
        let mut last_conflict = None;
        for external_port in candidates {
            let existing = self
                .query_mapping(external_port, UpnpStage::Add, cancellation)
                .await?;
            if existing
                .as_ref()
                .is_some_and(|entry| !mapping_entry_matches(entry, self.local_address, local_port))
            {
                last_conflict = Some(external_port);
                continue;
            }
            tokio::time::sleep(HTTP_OPERATION_PAUSE).await;
            let mut transport_failure = None;
            for attempt in 0..2 {
                match self
                    .add_mapping(external_port, local_port, UpnpStage::Add, cancellation)
                    .await
                {
                    Ok(()) => {
                        transport_failure = None;
                        break;
                    }
                    Err(error) if error.fault_code() == Some(718) => {
                        last_conflict = Some(external_port);
                        break;
                    }
                    Err(error) if error.is_transport() => {
                        transport_failure = Some(error);
                        tokio::time::sleep(HTTP_OPERATION_PAUSE).await;
                        match self
                            .query_mapping(external_port, UpnpStage::Verify, cancellation)
                            .await
                        {
                            Ok(Some(entry)) => {
                                verify_mapping_entry(
                                    &entry,
                                    self.local_address,
                                    local_port,
                                    UpnpStage::Verify,
                                )?;
                                return Ok(UpnpMapping {
                                    local_endpoint: SocketAddrV4::new(
                                        self.local_address,
                                        local_port,
                                    ),
                                    external_address,
                                    external_port,
                                    lease_seconds: entry.lease_seconds,
                                });
                            }
                            Ok(None) if attempt == 0 => {
                                tokio::time::sleep(HTTP_OPERATION_PAUSE).await;
                                continue;
                            }
                            Ok(None) => break,
                            Err(query_error) => return Err(query_error),
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            if last_conflict == Some(external_port) {
                continue;
            }
            if let Some(error) = transport_failure {
                return Err(error);
            }
            tokio::time::sleep(HTTP_OPERATION_PAUSE).await;
            let entry = self
                .query_mapping(external_port, UpnpStage::Verify, cancellation)
                .await?
                .ok_or_else(|| {
                    UpnpError::new(
                        UpnpStage::Verify,
                        "gateway did not return the mapping after add",
                    )
                })?;
            verify_mapping_entry(&entry, self.local_address, local_port, UpnpStage::Verify)?;
            return Ok(UpnpMapping {
                local_endpoint: SocketAddrV4::new(self.local_address, local_port),
                external_address,
                external_port,
                lease_seconds: entry.lease_seconds,
            });
        }
        Err(UpnpError::new(
            UpnpStage::Add,
            match last_conflict {
                Some(_) => "all bounded external-port candidates were occupied",
                None => "no external-port candidate was available",
            },
        ))
    }

    pub async fn renew_mapping(
        &self,
        mapping: &mut UpnpMapping,
        cancellation: &CancellationToken,
    ) -> Result<(), UpnpError> {
        if mapping.local_endpoint.ip() != &self.local_address {
            return Err(UpnpError::new(
                UpnpStage::Renewal,
                "mapping belongs to another local address",
            ));
        }
        self.add_mapping(
            mapping.external_port,
            mapping.local_endpoint.port(),
            UpnpStage::Renewal,
            cancellation,
        )
        .await?;
        let entry = self
            .query_mapping(mapping.external_port, UpnpStage::Renewal, cancellation)
            .await?
            .ok_or_else(|| {
                UpnpError::new(
                    UpnpStage::Renewal,
                    "gateway did not return the mapping after renewal",
                )
            })?;
        verify_mapping_entry(
            &entry,
            self.local_address,
            mapping.local_endpoint.port(),
            UpnpStage::Renewal,
        )?;
        mapping.lease_seconds = entry.lease_seconds;
        Ok(())
    }

    pub async fn delete_mapping(
        &self,
        mapping: &UpnpMapping,
        cancellation: &CancellationToken,
    ) -> Result<(), UpnpError> {
        if mapping.local_endpoint.ip() != &self.local_address {
            return Err(UpnpError::new(
                UpnpStage::Delete,
                "mapping belongs to another local address",
            ));
        }
        let port = mapping.external_port.to_string();
        self.soap(
            UpnpStage::Delete,
            "DeletePortMapping",
            &[
                ("NewRemoteHost", ""),
                ("NewExternalPort", &port),
                ("NewProtocol", "TCP"),
            ],
            cancellation,
        )
        .await?;
        if self
            .query_mapping(mapping.external_port, UpnpStage::Delete, cancellation)
            .await?
            .is_some()
        {
            return Err(UpnpError::new(
                UpnpStage::Delete,
                "gateway retained the mapping after delete",
            ));
        }
        Ok(())
    }

    async fn add_mapping(
        &self,
        external_port: u16,
        internal_port: u16,
        stage: UpnpStage,
        cancellation: &CancellationToken,
    ) -> Result<(), UpnpError> {
        let external_port = external_port.to_string();
        let internal_port = internal_port.to_string();
        let internal_client = self.local_address.to_string();
        let lease = REQUESTED_LEASE_SECONDS.to_string();
        self.soap(
            stage,
            "AddPortMapping",
            &[
                ("NewRemoteHost", ""),
                ("NewExternalPort", &external_port),
                ("NewProtocol", "TCP"),
                ("NewInternalPort", &internal_port),
                ("NewInternalClient", &internal_client),
                ("NewEnabled", "1"),
                ("NewPortMappingDescription", MAPPING_DESCRIPTION),
                ("NewLeaseDuration", &lease),
            ],
            cancellation,
        )
        .await?;
        Ok(())
    }

    async fn soap(
        &self,
        stage: UpnpStage,
        action: &str,
        arguments: &[(&str, &str)],
        cancellation: &CancellationToken,
    ) -> Result<Vec<XmlLeaf>, UpnpError> {
        let body = soap_request(&self.service_type, action, arguments);
        let soap_action = format!("\"{}#{action}\"", self.service_type);
        let request = self
            .client
            .post(self.control_url.clone())
            .header("connection", "close")
            .header(CONTENT_TYPE, "text/xml; charset=\"utf-8\"")
            .header("soapaction", soap_action)
            .body(body);
        let response = cancellable(cancellation, request.send(), stage).await?;
        let status = response.status();
        validate_xml_content_type(response.headers(), stage)?;
        let body = read_bounded_body(response, stage, cancellation).await?;
        let leaves = parse_xml_leaves(&body, stage)?;
        if let Some((code, description)) = parse_soap_fault(&leaves, stage)? {
            return Err(UpnpError::fault(stage, code, &description));
        }
        if !status.is_success() {
            return Err(UpnpError::new(
                stage,
                "gateway returned an HTTP error without a SOAP fault",
            ));
        }
        Ok(leaves)
    }

    pub fn gateway_address(&self) -> Ipv4Addr {
        self.gateway_address
    }
}

pub async fn discover_igd_v2(
    config: UpnpDiscoveryConfig,
    cancellation: &CancellationToken,
) -> Result<UpnpGateway, UpnpError> {
    let candidates = discover_locations(&config, cancellation).await?;
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(config.http_timeout)
        .timeout(config.http_timeout)
        .local_address(IpAddr::V4(config.local_address))
        .build()
        .map_err(|_| UpnpError::new(UpnpStage::Description, "build bounded HTTP client"))?;
    let mut last_error = None;
    for candidate in candidates {
        match describe_gateway(&config, &client, candidate, cancellation).await {
            Ok(gateway) => return Ok(gateway),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        UpnpError::new(
            UpnpStage::Discovery,
            "no bounded IGD v2 device response was usable",
        )
    }))
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DeviceLocation {
    source: Ipv4Addr,
    url: Url,
}

async fn discover_locations(
    config: &UpnpDiscoveryConfig,
    cancellation: &CancellationToken,
) -> Result<Vec<DeviceLocation>, UpnpError> {
    let socket = UdpSocket::bind(SocketAddrV4::new(config.local_address, 0))
        .await
        .map_err(|_| UpnpError::new(UpnpStage::Discovery, "bind SSDP discovery socket"))?;
    let request = ssdp_search_request(config.discovery_endpoint);
    let overall = Instant::now() + config.overall_deadline;
    let mut found = BTreeSet::new();
    let mut buffer = vec![0_u8; MAX_SSDP_DATAGRAM_BYTES + 1];
    for _ in 0..config.attempts {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(UpnpError::new(UpnpStage::Discovery, "UPnP discovery cancelled"));
            }
            result = socket.send_to(&request, config.discovery_endpoint) => {
                result.map_err(|_| {
                    UpnpError::new(UpnpStage::Discovery, "send SSDP discovery request")
                })?;
            }
        }
        let round = (Instant::now() + config.response_window).min(overall);
        loop {
            let receive = timeout_at(round, socket.recv_from(&mut buffer));
            let received = tokio::select! {
                _ = cancellation.cancelled() => {
                    return Err(UpnpError::new(UpnpStage::Discovery, "UPnP discovery cancelled"));
                }
                result = receive => result,
            };
            let Ok(result) = received else {
                break;
            };
            let (length, source) = result
                .map_err(|_| UpnpError::new(UpnpStage::Discovery, "receive SSDP response"))?;
            if length > MAX_SSDP_DATAGRAM_BYTES {
                continue;
            }
            let SocketAddr::V4(source) = source else {
                continue;
            };
            if let Ok(location) = parse_ssdp_response(
                &buffer[..length],
                *source.ip(),
                config.allow_loopback_gateway,
            ) {
                found.insert(location);
                if found.len() >= MAX_DEVICE_LOCATIONS {
                    return Ok(found.into_iter().collect());
                }
            }
        }
        if Instant::now() >= overall {
            break;
        }
    }
    if found.is_empty() {
        Err(UpnpError::new(
            UpnpStage::Discovery,
            "no IGD v2 device answered bounded SSDP discovery",
        ))
    } else {
        Ok(found.into_iter().collect())
    }
}

fn ssdp_search_request(endpoint: SocketAddrV4) -> Vec<u8> {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {endpoint}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: upnp:rootdevice\r\n\r\n"
    )
    .into_bytes()
}

fn parse_ssdp_response(
    bytes: &[u8],
    source: Ipv4Addr,
    allow_loopback: bool,
) -> Result<DeviceLocation, UpnpError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpnpError::new(UpnpStage::Discovery, "SSDP response is not UTF-8"))?;
    let mut lines = text.split("\r\n");
    let status = lines.next().unwrap_or_default();
    if !status.starts_with("HTTP/1.1 200") && !status.starts_with("HTTP/1.0 200") {
        return Err(UpnpError::new(
            UpnpStage::Discovery,
            "SSDP response status is not successful",
        ));
    }
    let mut headers = 0_usize;
    let mut location = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_SSDP_HEADERS {
            return Err(UpnpError::new(
                UpnpStage::Discovery,
                "SSDP response exceeds the header bound",
            ));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(UpnpError::new(
                UpnpStage::Discovery,
                "SSDP response contains a malformed header",
            ));
        };
        if name.eq_ignore_ascii_case("location") && location.replace(value.trim()).is_some() {
            return Err(UpnpError::new(
                UpnpStage::Discovery,
                "SSDP response contains duplicate locations",
            ));
        }
    }
    let url = validate_gateway_url(
        location.ok_or_else(|| {
            UpnpError::new(UpnpStage::Discovery, "SSDP response omitted its location")
        })?,
        source,
        allow_loopback,
        UpnpStage::Discovery,
    )?;
    Ok(DeviceLocation { source, url })
}

async fn describe_gateway(
    config: &UpnpDiscoveryConfig,
    client: &Client,
    candidate: DeviceLocation,
    cancellation: &CancellationToken,
) -> Result<UpnpGateway, UpnpError> {
    let description = http_get_xml(client, candidate.url.clone(), cancellation).await?;
    let leaves = parse_xml_leaves(&description, UpnpStage::Description)?;
    let base = optional_unique_leaf(&leaves, "URLBase", UpnpStage::Description)?
        .map(|value| {
            validate_gateway_url(
                value,
                candidate.source,
                config.allow_loopback_gateway,
                UpnpStage::Description,
            )
        })
        .transpose()?
        .unwrap_or_else(|| candidate.url.clone());
    let service = select_wan_ip_service(&leaves)?;
    let control_url = resolve_gateway_url(
        &base,
        &service.control_url,
        candidate.source,
        config.allow_loopback_gateway,
    )?;
    let scpd_url = resolve_gateway_url(
        &base,
        &service.scpd_url,
        candidate.source,
        config.allow_loopback_gateway,
    )?;
    let scpd = http_get_xml(client, scpd_url, cancellation).await?;
    let scpd_leaves = parse_xml_leaves(&scpd, UpnpStage::Description)?;
    require_actions(&scpd_leaves)?;
    Ok(UpnpGateway {
        local_address: config.local_address,
        gateway_address: candidate.source,
        control_url,
        service_type: WAN_IP_CONNECTION_V2.to_owned(),
        client: client.clone(),
    })
}

async fn http_get_xml(
    client: &Client,
    url: Url,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, UpnpError> {
    let response = cancellable(
        cancellation,
        client
            .get(url)
            .header("connection", "close")
            .header("accept", "text/xml, application/xml")
            .send(),
        UpnpStage::Description,
    )
    .await?;
    if !response.status().is_success() {
        return Err(UpnpError::new(
            UpnpStage::Description,
            "gateway description returned an HTTP error",
        ));
    }
    validate_xml_content_type(response.headers(), UpnpStage::Description)?;
    read_bounded_body(response, UpnpStage::Description, cancellation).await
}

async fn read_bounded_body(
    mut response: reqwest::Response,
    stage: UpnpStage,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, UpnpError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_HTTP_BODY_BYTES as u64)
    {
        return Err(UpnpError::new(stage, "gateway HTTP body exceeds its bound"));
    }
    let mut body = Vec::new();
    loop {
        let chunk = cancellable(cancellation, response.chunk(), stage).await?;
        let Some(chunk) = chunk else {
            break;
        };
        if body.len().saturating_add(chunk.len()) > MAX_HTTP_BODY_BYTES {
            return Err(UpnpError::new(stage, "gateway HTTP body exceeds its bound"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn validate_xml_content_type(headers: &HeaderMap, stage: UpnpStage) -> Result<(), UpnpError> {
    let value = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media = value.split(';').next().unwrap_or_default().trim();
    if matches!(media, "text/xml" | "application/xml") {
        Ok(())
    } else {
        Err(UpnpError::new(stage, "gateway response is not XML"))
    }
}

async fn cancellable<F, T>(
    cancellation: &CancellationToken,
    future: F,
    stage: UpnpStage,
) -> Result<T, UpnpError>
where
    F: Future<Output = Result<T, reqwest::Error>>,
{
    tokio::select! {
        _ = cancellation.cancelled() => Err(UpnpError::new(stage, "UPnP operation cancelled")),
        result = future => result.map_err(|error| sanitize_transport_error(error, stage)),
    }
}

fn sanitize_transport_error(error: reqwest::Error, stage: UpnpStage) -> UpnpError {
    let detail = if error.is_timeout() {
        "gateway HTTP request timed out"
    } else if error.is_redirect() {
        "gateway HTTP redirect was rejected"
    } else {
        "gateway HTTP transport failed"
    };
    let mut error = UpnpError::new(stage, detail);
    error.transport = true;
    error
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct XmlLeaf {
    path: Vec<String>,
    parent_id: Option<u64>,
    value: String,
}

#[derive(Debug)]
struct XmlElement {
    id: u64,
    name: String,
    text: String,
    has_child: bool,
}

fn parse_xml_leaves(bytes: &[u8], stage: UpnpStage) -> Result<Vec<XmlLeaf>, UpnpError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_comments = true;
    let mut stack = Vec::<XmlElement>::new();
    let mut leaves = Vec::new();
    let mut events = 0_usize;
    let mut next_element_id = 1_u64;
    loop {
        events += 1;
        if events > MAX_XML_EVENTS {
            return Err(UpnpError::new(stage, "XML exceeds the event bound"));
        }
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(UpnpError::new(stage, "XML exceeds the nesting bound"));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.has_child = true;
                }
                stack.push(XmlElement {
                    id: next_element_id,
                    name: xml_local_name(start.name().as_ref(), stage)?,
                    text: String::new(),
                    has_child: false,
                });
                next_element_id = next_element_id
                    .checked_add(1)
                    .ok_or_else(|| UpnpError::new(stage, "XML element identity overflow"))?;
            }
            Ok(Event::Empty(empty)) => {
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(UpnpError::new(stage, "XML exceeds the nesting bound"));
                }
                if let Some(parent) = stack.last_mut() {
                    parent.has_child = true;
                }
                let _ = xml_local_name(empty.name().as_ref(), stage)?;
            }
            Ok(Event::Text(text)) => {
                if let Some(element) = stack.last_mut() {
                    append_xml_text(
                        element,
                        &text
                            .decode()
                            .map_err(|_| UpnpError::new(stage, "XML text encoding is invalid"))?,
                        stage,
                    )?;
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(element) = stack.last_mut() {
                    append_xml_text(
                        element,
                        &text
                            .decode()
                            .map_err(|_| UpnpError::new(stage, "XML CDATA encoding is invalid"))?,
                        stage,
                    )?;
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(element) = stack.last_mut() {
                    let value = if let Some(character) = reference
                        .resolve_char_ref()
                        .map_err(|_| UpnpError::new(stage, "XML character reference is invalid"))?
                    {
                        character.to_string()
                    } else {
                        match reference
                            .decode()
                            .map_err(|_| UpnpError::new(stage, "XML entity reference is invalid"))?
                            .as_ref()
                        {
                            "amp" => "&".to_owned(),
                            "lt" => "<".to_owned(),
                            "gt" => ">".to_owned(),
                            "apos" => "'".to_owned(),
                            "quot" => "\"".to_owned(),
                            _ => {
                                return Err(UpnpError::new(
                                    stage,
                                    "XML contains an unsupported entity reference",
                                ));
                            }
                        }
                    };
                    append_xml_text(element, &value, stage)?;
                }
            }
            Ok(Event::End(_)) => {
                let Some(element) = stack.pop() else {
                    return Err(UpnpError::new(stage, "XML closes an unopened element"));
                };
                let value = element.text.trim();
                if !element.has_child && !value.is_empty() {
                    let mut path = stack
                        .iter()
                        .map(|element| element.name.clone())
                        .collect::<Vec<_>>();
                    path.push(element.name);
                    leaves.push(XmlLeaf {
                        path,
                        parent_id: stack.last().map(|element| element.id),
                        value: value.to_owned(),
                    });
                }
            }
            Ok(Event::Eof) => break,
            Ok(Event::Decl(_) | Event::PI(_) | Event::DocType(_) | Event::Comment(_)) => {}
            Err(_) => return Err(UpnpError::new(stage, "gateway XML is malformed")),
        }
    }
    if !stack.is_empty() {
        return Err(UpnpError::new(stage, "gateway XML is truncated"));
    }
    Ok(leaves)
}

fn append_xml_text(
    element: &mut XmlElement,
    text: &str,
    stage: UpnpStage,
) -> Result<(), UpnpError> {
    if element.text.len().saturating_add(text.len()) > MAX_XML_TEXT_BYTES {
        return Err(UpnpError::new(stage, "XML text exceeds its bound"));
    }
    element.text.push_str(text);
    Ok(())
}

fn xml_local_name(bytes: &[u8], stage: UpnpStage) -> Result<String, UpnpError> {
    let bytes = bytes.rsplit(|byte| *byte == b':').next().unwrap_or(bytes);
    if bytes.is_empty() || bytes.len() > 128 {
        return Err(UpnpError::new(stage, "XML element name is invalid"));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| UpnpError::new(stage, "XML element name is not UTF-8"))
}

#[derive(Default)]
struct ServiceParts {
    service_type: Option<String>,
    control_url: Option<String>,
    scpd_url: Option<String>,
}

struct SelectedService {
    control_url: String,
    scpd_url: String,
}

fn select_wan_ip_service(leaves: &[XmlLeaf]) -> Result<SelectedService, UpnpError> {
    let mut services = BTreeMap::<u64, ServiceParts>::new();
    for leaf in leaves {
        let Some(last) = leaf.path.last() else {
            continue;
        };
        if !matches!(last.as_str(), "serviceType" | "controlURL" | "SCPDURL")
            || leaf.path.len() < 2
            || leaf.path[leaf.path.len() - 2] != "service"
        {
            continue;
        }
        let key = leaf.parent_id.ok_or_else(|| {
            UpnpError::new(
                UpnpStage::Description,
                "service value has no enclosing service",
            )
        })?;
        if !services.contains_key(&key) && services.len() >= MAX_SERVICE_CANDIDATES {
            return Err(UpnpError::new(
                UpnpStage::Description,
                "device description exceeds the service bound",
            ));
        }
        let parts = services.entry(key).or_default();
        let slot = match last.as_str() {
            "serviceType" => &mut parts.service_type,
            "controlURL" => &mut parts.control_url,
            "SCPDURL" => &mut parts.scpd_url,
            _ => unreachable!(),
        };
        if slot.replace(leaf.value.clone()).is_some() {
            return Err(UpnpError::new(
                UpnpStage::Description,
                "device description duplicates a critical service value",
            ));
        }
    }
    let mut selected = None;
    for parts in services.into_values() {
        if parts.service_type.as_deref() != Some(WAN_IP_CONNECTION_V2) {
            continue;
        }
        let service = SelectedService {
            control_url: parts.control_url.ok_or_else(|| {
                UpnpError::new(UpnpStage::Description, "WAN IP service omitted controlURL")
            })?,
            scpd_url: parts.scpd_url.ok_or_else(|| {
                UpnpError::new(UpnpStage::Description, "WAN IP service omitted SCPDURL")
            })?,
        };
        if selected.replace(service).is_some() {
            return Err(UpnpError::new(
                UpnpStage::Description,
                "device description contains duplicate WAN IP v2 services",
            ));
        }
    }
    selected.ok_or_else(|| {
        UpnpError::new(
            UpnpStage::Description,
            "device does not advertise WANIPConnection:2",
        )
    })
}

fn require_actions(leaves: &[XmlLeaf]) -> Result<(), UpnpError> {
    let actions = leaves
        .iter()
        .filter(|leaf| path_ends_with(&leaf.path, &["action", "name"]))
        .map(|leaf| leaf.value.as_str())
        .collect::<BTreeSet<_>>();
    for required in [
        "GetExternalIPAddress",
        "GetSpecificPortMappingEntry",
        "AddPortMapping",
        "DeletePortMapping",
    ] {
        if !actions.contains(required) {
            return Err(UpnpError::new(
                UpnpStage::Description,
                format!("WAN IP service omits required action {required}"),
            ));
        }
    }
    Ok(())
}

fn soap_request(service_type: &str, action: &str, arguments: &[(&str, &str)]) -> String {
    let mut body = format!(
        "<?xml version=\"1.0\"?>\r\n<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service_type}\">"
    );
    for (name, value) in arguments {
        body.push('<');
        body.push_str(name);
        body.push('>');
        body.push_str(value);
        body.push_str("</");
        body.push_str(name);
        body.push('>');
    }
    body.push_str("</u:");
    body.push_str(action);
    body.push_str("></s:Body></s:Envelope>");
    body
}

fn parse_soap_fault(
    leaves: &[XmlLeaf],
    stage: UpnpStage,
) -> Result<Option<(u16, String)>, UpnpError> {
    let Some(code) = optional_unique_leaf(leaves, "errorCode", stage)? else {
        return Ok(None);
    };
    let code = code
        .parse::<u16>()
        .map_err(|_| UpnpError::new(stage, "SOAP fault code is invalid"))?;
    let description = optional_unique_leaf(leaves, "errorDescription", stage)?
        .unwrap_or("unspecified gateway error");
    Ok(Some((code, bounded(description, MAX_ERROR_DETAIL_BYTES))))
}

fn unique_leaf<'a>(
    leaves: &'a [XmlLeaf],
    name: &str,
    stage: UpnpStage,
) -> Result<&'a str, UpnpError> {
    optional_unique_leaf(leaves, name, stage)?
        .ok_or_else(|| UpnpError::new(stage, format!("gateway response omitted {name}")))
}

fn optional_unique_leaf<'a>(
    leaves: &'a [XmlLeaf],
    name: &str,
    stage: UpnpStage,
) -> Result<Option<&'a str>, UpnpError> {
    let mut found = None;
    for leaf in leaves
        .iter()
        .filter(|leaf| leaf.path.last().is_some_and(|candidate| candidate == name))
    {
        if found.replace(leaf.value.as_str()).is_some() {
            return Err(UpnpError::new(
                stage,
                format!("gateway response duplicates {name}"),
            ));
        }
    }
    Ok(found)
}

fn path_ends_with(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(left, right)| left == right)
}

fn validate_gateway_url(
    value: &str,
    source: Ipv4Addr,
    allow_loopback: bool,
    stage: UpnpStage,
) -> Result<Url, UpnpError> {
    if value.len() > MAX_URL_BYTES {
        return Err(UpnpError::new(stage, "gateway URL exceeds its bound"));
    }
    let url = Url::parse(value).map_err(|_| UpnpError::new(stage, "gateway URL is invalid"))?;
    validate_resolved_gateway_url(&url, source, allow_loopback, stage)?;
    Ok(url)
}

fn resolve_gateway_url(
    base: &Url,
    value: &str,
    source: Ipv4Addr,
    allow_loopback: bool,
) -> Result<Url, UpnpError> {
    if value.len() > MAX_URL_BYTES {
        return Err(UpnpError::new(
            UpnpStage::Description,
            "service URL exceeds its bound",
        ));
    }
    let url = base
        .join(value)
        .map_err(|_| UpnpError::new(UpnpStage::Description, "service URL is invalid"))?;
    validate_resolved_gateway_url(&url, source, allow_loopback, UpnpStage::Description)?;
    Ok(url)
}

fn validate_resolved_gateway_url(
    url: &Url,
    source: Ipv4Addr,
    allow_loopback: bool,
    stage: UpnpStage,
) -> Result<(), UpnpError> {
    if url.as_str().len() > MAX_URL_BYTES
        || url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default().is_none_or(|port| port == 0)
    {
        return Err(UpnpError::new(
            stage,
            "gateway URL violates HTTP safety policy",
        ));
    }
    let Some(url::Host::Ipv4(host)) = url.host() else {
        return Err(UpnpError::new(
            stage,
            "gateway URL host must be an IPv4 literal",
        ));
    };
    if host != source {
        return Err(UpnpError::new(
            stage,
            "gateway URL host differs from the SSDP response source",
        ));
    }
    if !eligible_gateway_address(host, allow_loopback) {
        return Err(UpnpError::new(
            stage,
            "gateway URL host is outside the local IPv4 scope",
        ));
    }
    Ok(())
}

fn eligible_local_address(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && address != Ipv4Addr::BROADCAST
}

fn eligible_gateway_address(address: Ipv4Addr, allow_loopback: bool) -> bool {
    (allow_loopback && address.is_loopback()) || is_private_or_link_local(address)
}

fn eligible_external_address(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && address != Ipv4Addr::BROADCAST
        && !is_private_or_link_local(address)
}

fn is_private_or_link_local(address: Ipv4Addr) -> bool {
    let [a, b, _, _] = address.octets();
    a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
}

fn mapping_candidates(local_port: u16) -> Result<Vec<u16>, UpnpError> {
    let mut random = [0_u8; (MAX_MAPPING_CANDIDATES - 1) * 2];
    getrandom::fill(&mut random)
        .map_err(|_| UpnpError::new(UpnpStage::Add, "generate external-port candidates"))?;
    let mut candidates = Vec::with_capacity(MAX_MAPPING_CANDIDATES);
    if (HIGH_PORT_START..HIGH_PORT_START + HIGH_PORT_COUNT).contains(&local_port) {
        candidates.push(local_port);
    }
    for bytes in random.chunks_exact(2) {
        let candidate =
            HIGH_PORT_START + (u16::from_be_bytes([bytes[0], bytes[1]]) % HIGH_PORT_COUNT);
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
        if candidates.len() == MAX_MAPPING_CANDIDATES {
            break;
        }
    }
    let mut candidate = HIGH_PORT_START;
    while candidates.len() < MAX_MAPPING_CANDIDATES {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
        candidate += 1;
    }
    Ok(candidates)
}

fn mapping_entry_matches(entry: &UpnpMappingEntry, address: Ipv4Addr, port: u16) -> bool {
    entry.internal_client == address
        && entry.internal_port == port
        && entry.enabled
        && entry.description == MAPPING_DESCRIPTION
        && entry.lease_seconds > 0
}

fn verify_mapping_entry(
    entry: &UpnpMappingEntry,
    address: Ipv4Addr,
    port: u16,
    stage: UpnpStage,
) -> Result<(), UpnpError> {
    if !mapping_entry_matches(entry, address, port) {
        return Err(UpnpError::new(
            stage,
            "installed mapping does not match the requested finite TCP entry",
        ));
    }
    Ok(())
}

fn parse_u16(value: &str, stage: UpnpStage, detail: &str) -> Result<u16, UpnpError> {
    let value = value
        .parse::<u16>()
        .map_err(|_| UpnpError::new(stage, detail))?;
    if value == 0 {
        Err(UpnpError::new(stage, detail))
    } else {
        Ok(value)
    }
}

fn bounded(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn ssdp_request_and_response_are_closed_and_source_bound() {
        let request = String::from_utf8(ssdp_search_request(SSDP_MULTICAST_ENDPOINT)).unwrap();
        assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(request.contains("ST: upnp:rootdevice\r\n"));
        assert!(request.ends_with("\r\n\r\n"));

        let source = Ipv4Addr::new(192, 168, 1, 1);
        let response = b"HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.1:5000/root.xml\r\nUSN: uuid:test::upnp:rootdevice\r\n\r\n";
        let parsed = parse_ssdp_response(response, source, false).unwrap();
        assert_eq!(parsed.source, source);
        assert_eq!(parsed.url.path(), "/root.xml");
        assert!(parse_ssdp_response(response, Ipv4Addr::new(192, 168, 1, 2), false).is_err());
        assert!(
            parse_ssdp_response(
                b"HTTP/1.1 200 OK\r\nLOCATION: https://192.168.1.1/root.xml\r\n\r\n",
                source,
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn device_and_service_documents_select_only_complete_v2_service() {
        let description = br#"<?xml version="1.0"?>
          <root><URLBase>http://192.168.1.1:5000/base/</URLBase><device><serviceList>
          <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
          <controlURL>/old</controlURL><SCPDURL>/old.xml</SCPDURL></service>
          <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:2</serviceType>
          <controlURL>control</controlURL><SCPDURL>wan.xml</SCPDURL></service>
          </serviceList></device></root>"#;
        let leaves = parse_xml_leaves(description, UpnpStage::Description).unwrap();
        let service = select_wan_ip_service(&leaves).unwrap();
        assert_eq!(service.control_url, "control");
        assert_eq!(service.scpd_url, "wan.xml");
        let actions = br#"<scpd><actionList>
          <action><name>GetExternalIPAddress</name></action>
          <action><name>GetSpecificPortMappingEntry</name></action>
          <action><name>AddPortMapping</name></action>
          <action><name>DeletePortMapping</name></action>
          </actionList></scpd>"#;
        require_actions(&parse_xml_leaves(actions, UpnpStage::Description).unwrap()).unwrap();
    }

    #[test]
    fn xml_and_soap_fault_parsing_enforce_bounds_and_typed_codes() {
        let fault = br#"<s:Envelope><s:Body><s:Fault><detail><UPnPError>
          <errorCode>725</errorCode><errorDescription>OnlyPermanentLeasesSupported</errorDescription>
          </UPnPError></detail></s:Fault></s:Body></s:Envelope>"#;
        let leaves = parse_xml_leaves(fault, UpnpStage::Add).unwrap();
        assert_eq!(
            parse_soap_fault(&leaves, UpnpStage::Add).unwrap(),
            Some((725, "OnlyPermanentLeasesSupported".to_owned()))
        );
        let deep = format!(
            "{}x{}",
            "<a>".repeat(MAX_XML_DEPTH + 1),
            "</a>".repeat(MAX_XML_DEPTH + 1)
        );
        assert!(parse_xml_leaves(deep.as_bytes(), UpnpStage::Description).is_err());
        let long = format!("<a>{}</a>", "x".repeat(MAX_XML_TEXT_BYTES + 1));
        assert!(parse_xml_leaves(long.as_bytes(), UpnpStage::Description).is_err());
    }

    #[test]
    fn mapping_verification_rejects_permanent_or_foreign_entries() {
        let exact = UpnpMappingEntry {
            internal_client: Ipv4Addr::new(192, 168, 1, 20),
            internal_port: 42_000,
            enabled: true,
            description: MAPPING_DESCRIPTION.to_owned(),
            lease_seconds: REQUESTED_LEASE_SECONDS,
        };
        assert!(mapping_entry_matches(
            &exact,
            exact.internal_client,
            exact.internal_port
        ));
        let mut permanent = exact.clone();
        permanent.lease_seconds = 0;
        assert!(!mapping_entry_matches(
            &permanent,
            exact.internal_client,
            exact.internal_port
        ));
        let mut foreign = exact.clone();
        foreign.description = "another client".to_owned();
        assert!(!mapping_entry_matches(
            &foreign,
            exact.internal_client,
            exact.internal_port
        ));
    }

    #[test]
    fn renewal_uses_three_quarters_of_finite_lease() {
        let mapping = UpnpMapping {
            local_endpoint: SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 20), 42_000),
            external_address: Ipv4Addr::new(203, 0, 113, 10),
            external_port: 48_000,
            lease_seconds: REQUESTED_LEASE_SECONDS,
        };
        assert_eq!(mapping.renewal_delay(), Duration::from_secs(2_700));
    }

    #[test]
    fn url_policy_rejects_dns_credentials_fragments_and_cross_host() {
        let source = Ipv4Addr::new(192, 168, 1, 1);
        for value in [
            "http://gateway.local/root.xml",
            "http://user@192.168.1.1/root.xml",
            "http://192.168.1.1/root.xml#fragment",
            "http://192.168.1.2/root.xml",
        ] {
            assert!(validate_gateway_url(value, source, false, UpnpStage::Discovery).is_err());
        }
    }

    #[test]
    fn scripted_config_is_strictly_loopback_scoped() {
        let config = UpnpDiscoveryConfig::scripted_for_testing(
            Ipv4Addr::LOCALHOST,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 19_000),
        );
        assert!(config.allow_loopback_gateway);
        assert_eq!(config.attempts, 1);
    }

    #[tokio::test]
    async fn scripted_gateway_runs_exact_add_verify_renew_delete_lifecycle() {
        let (config, transcript, udp_task, http_task) =
            scripted_gateway(ScriptedAddBehavior::Normal).await;
        let cancellation = CancellationToken::new();
        let gateway = discover_igd_v2(config, &cancellation)
            .await
            .expect("discover scripted IGD v2 gateway");
        assert_eq!(gateway.gateway_address(), Ipv4Addr::LOCALHOST);
        let mut mapping = gateway
            .create_mapping(42_000, &cancellation)
            .await
            .expect("add and verify finite mapping");
        assert_eq!(mapping.external_port, 42_000);
        assert_eq!(mapping.lease_seconds, REQUESTED_LEASE_SECONDS);
        gateway
            .renew_mapping(&mut mapping, &cancellation)
            .await
            .expect("renew and verify mapping");
        gateway
            .delete_mapping(&mapping, &cancellation)
            .await
            .expect("delete and independently query absence");
        udp_task.await.expect("join SSDP task");
        http_task.await.expect("join HTTP task");
        assert_eq!(
            transcript.lock().unwrap().as_slice(),
            [
                "GET /root.xml",
                "GET /wan.xml",
                "GetExternalIPAddress",
                "GetSpecificPortMappingEntry",
                "AddPortMapping",
                "GetSpecificPortMappingEntry",
                "AddPortMapping",
                "GetSpecificPortMappingEntry",
                "DeletePortMapping",
                "GetSpecificPortMappingEntry",
            ]
        );
    }

    #[tokio::test]
    async fn add_transport_failure_reconciles_an_installed_mapping() {
        let (config, transcript, udp_task, http_task) =
            scripted_gateway(ScriptedAddBehavior::DropAfterApply).await;
        let cancellation = CancellationToken::new();
        let gateway = discover_igd_v2(config, &cancellation)
            .await
            .expect("discover scripted gateway");
        let mapping = gateway
            .create_mapping(42_000, &cancellation)
            .await
            .expect("reconcile mapping installed before transport failure");
        assert_eq!(mapping.external_port, 42_000);
        gateway
            .delete_mapping(&mapping, &cancellation)
            .await
            .expect("delete reconciled mapping");
        udp_task.await.expect("join SSDP task");
        http_task.await.expect("join HTTP task");
        assert_eq!(
            transcript.lock().unwrap().as_slice(),
            [
                "GET /root.xml",
                "GET /wan.xml",
                "GetExternalIPAddress",
                "GetSpecificPortMappingEntry",
                "AddPortMapping",
                "GetSpecificPortMappingEntry",
                "DeletePortMapping",
                "GetSpecificPortMappingEntry",
            ]
        );
    }

    #[tokio::test]
    async fn permanent_lease_fault_is_a_typed_hard_stop() {
        let (config, _, udp_task, http_task) =
            scripted_gateway(ScriptedAddBehavior::PermanentLeaseFault).await;
        let cancellation = CancellationToken::new();
        let gateway = discover_igd_v2(config, &cancellation)
            .await
            .expect("discover scripted gateway");
        let error = gateway
            .create_mapping(42_000, &cancellation)
            .await
            .expect_err("fault 725 must not retry with a permanent lease");
        assert_eq!(error.stage(), UpnpStage::Add);
        assert_eq!(error.fault_code(), Some(725));
        udp_task.await.expect("join SSDP task");
        http_task.await.expect("join HTTP task");
    }

    #[tokio::test]
    async fn discovery_cancellation_preempts_the_response_window() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let endpoint = match socket.local_addr().unwrap() {
            SocketAddr::V4(endpoint) => endpoint,
            SocketAddr::V6(_) => unreachable!(),
        };
        let config = UpnpDiscoveryConfig::scripted_for_testing(Ipv4Addr::LOCALHOST, endpoint);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = discover_igd_v2(config, &cancellation)
            .await
            .expect_err("cancelled discovery must terminate");
        assert_eq!(error.stage(), UpnpStage::Discovery);
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum ScriptedAddBehavior {
        Normal,
        PermanentLeaseFault,
        DropAfterApply,
    }

    async fn scripted_gateway(
        add_behavior: ScriptedAddBehavior,
    ) -> (
        UpnpDiscoveryConfig,
        Arc<Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_port = listener.local_addr().unwrap().port();
        let ssdp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let ssdp_endpoint = match ssdp.local_addr().unwrap() {
            SocketAddr::V4(endpoint) => endpoint,
            SocketAddr::V6(_) => unreachable!(),
        };
        let response_count = match add_behavior {
            ScriptedAddBehavior::Normal => 10,
            ScriptedAddBehavior::PermanentLeaseFault => 5,
            ScriptedAddBehavior::DropAfterApply => 8,
        };
        let transcript = Arc::new(Mutex::new(Vec::new()));
        let http_transcript = transcript.clone();
        let http_task = tokio::spawn(async move {
            let mut mapped = false;
            for _ in 0..response_count {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_http_request(&mut stream).await;
                let first_line = request.lines().next().unwrap_or_default();
                let (status, content_type, body, event, drop_response) = if first_line
                    .starts_with("GET /root.xml ")
                {
                    (
                        "200 OK",
                        "text/xml",
                        device_description(http_port),
                        "GET /root.xml",
                        false,
                    )
                } else if first_line.starts_with("GET /wan.xml ") {
                    (
                        "200 OK",
                        "application/xml",
                        scpd_description(),
                        "GET /wan.xml",
                        false,
                    )
                } else {
                    let action = soap_action(&request);
                    let (status, body, drop_response) = match action {
                        "GetExternalIPAddress" => (
                            "200 OK",
                            soap_response(
                                action,
                                "<NewExternalIPAddress>203.0.113.10</NewExternalIPAddress>",
                            ),
                            false,
                        ),
                        "GetSpecificPortMappingEntry" if mapped => (
                            "200 OK",
                            soap_response(
                                action,
                                "<NewInternalPort>42000</NewInternalPort><NewInternalClient>127.0.0.1</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>RSTorrent</NewPortMappingDescription><NewLeaseDuration>3600</NewLeaseDuration>",
                            ),
                            false,
                        ),
                        "GetSpecificPortMappingEntry" => (
                            "500 Internal Server Error",
                            soap_fault(714, "NoSuchEntryInArray"),
                            false,
                        ),
                        "AddPortMapping"
                            if add_behavior == ScriptedAddBehavior::PermanentLeaseFault =>
                        {
                            (
                                "500 Internal Server Error",
                                soap_fault(725, "OnlyPermanentLeasesSupported"),
                                false,
                            )
                        }
                        "AddPortMapping" => {
                            assert!(request.contains("<NewLeaseDuration>3600</NewLeaseDuration>"));
                            assert!(
                                request
                                    .contains("<NewInternalClient>127.0.0.1</NewInternalClient>")
                            );
                            mapped = true;
                            (
                                "200 OK",
                                soap_response(action, ""),
                                add_behavior == ScriptedAddBehavior::DropAfterApply,
                            )
                        }
                        "DeletePortMapping" => {
                            mapped = false;
                            ("200 OK", soap_response(action, ""), false)
                        }
                        other => panic!("unexpected scripted action {other}"),
                    };
                    (
                        status,
                        "text/xml; charset=utf-8",
                        body,
                        action,
                        drop_response,
                    )
                };
                http_transcript.lock().unwrap().push(event.to_owned());
                if drop_response {
                    continue;
                }
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let udp_task = tokio::spawn(async move {
            let mut request = [0_u8; 1_024];
            let (length, peer) = ssdp.recv_from(&mut request).await.unwrap();
            let request = std::str::from_utf8(&request[..length]).unwrap();
            assert!(request.contains("ST: upnp:rootdevice\r\n"));
            let response = format!(
                "HTTP/1.1 200 OK\r\nLOCATION: http://127.0.0.1:{http_port}/root.xml\r\nUSN: uuid:scripted::upnp:rootdevice\r\n\r\n"
            );
            ssdp.send_to(response.as_bytes(), peer).await.unwrap();
        });
        (
            UpnpDiscoveryConfig::scripted_for_testing(Ipv4Addr::LOCALHOST, ssdp_endpoint),
            transcript,
            udp_task,
            http_task,
        )
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4_096];
        let header_end = loop {
            let length = stream.read(&mut buffer).await.unwrap();
            assert_ne!(length, 0, "request ended before headers");
            bytes.extend_from_slice(&buffer[..length]);
            if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let length = stream.read(&mut buffer).await.unwrap();
            assert_ne!(length, 0, "request ended before body");
            bytes.extend_from_slice(&buffer[..length]);
        }
        String::from_utf8(bytes).unwrap()
    }

    fn soap_action(request: &str) -> &str {
        request
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("soapaction")
                    .then(|| value.trim().trim_matches('"').rsplit('#').next().unwrap())
            })
            .expect("SOAPAction header")
    }

    fn device_description(port: u16) -> String {
        format!(
            "<?xml version=\"1.0\"?><root><URLBase>http://127.0.0.1:{port}/</URLBase><device><serviceList><service><serviceType>{WAN_IP_CONNECTION_V2}</serviceType><controlURL>/control</controlURL><SCPDURL>/wan.xml</SCPDURL></service></serviceList></device></root>"
        )
    }

    fn scpd_description() -> String {
        "<scpd><actionList><action><name>GetExternalIPAddress</name></action><action><name>GetSpecificPortMappingEntry</name></action><action><name>AddPortMapping</name></action><action><name>DeletePortMapping</name></action></actionList></scpd>".to_owned()
    }

    fn soap_response(action: &str, arguments: &str) -> String {
        format!(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"{WAN_IP_CONNECTION_V2}\">{arguments}</u:{action}Response></s:Body></s:Envelope>"
        )
    }

    fn soap_fault(code: u16, description: &str) -> String {
        format!(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><detail><UPnPError><errorCode>{code}</errorCode><errorDescription>{description}</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"
        )
    }
}

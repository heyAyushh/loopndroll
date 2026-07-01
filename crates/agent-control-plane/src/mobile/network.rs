// allow: SIZE_OK — mobile network boundary owns advertised URLs, route policy, Tailscale detection, and listener safety together.
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;

pub const DEFAULT_AGENT_CONTROL_PLANE_PORT: u16 = 8765;
pub const DEFAULT_GRPC_PORT_OFFSET: u16 = 1;
pub const DEFAULT_GRPC_CONTROL_PLANE_PORT: u16 =
    DEFAULT_AGENT_CONTROL_PLANE_PORT + DEFAULT_GRPC_PORT_OFFSET;

const LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_LISTEN";
const GRPC_LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_GRPC_LISTEN";
const MOBILE_BONJOUR_INSTANCE_NAME: &str = "Looper";
const MOBILE_BONJOUR_SERVICE_TYPE: &str = "_looper._tcp";
const MOBILE_BONJOUR_DOMAIN: &str = "local.";
const DNS_SD_EXECUTABLE: &str = "/usr/bin/dns-sd";
const IPCONFIG_EXECUTABLE: &str = "/usr/sbin/ipconfig";
const IFCONFIG_EXECUTABLE: &str = "/sbin/ifconfig";
const IPV4_OCTET_COUNT: usize = 4;
const LOOPBACK_FIRST_OCTET: u8 = 127;
const LINK_LOCAL_FIRST_OCTET: u8 = 169;
const LINK_LOCAL_SECOND_OCTET: u8 = 254;
const UNSPECIFIED_FIRST_OCTET: u8 = 0;
const MULTICAST_FIRST_OCTET_LOWER_BOUND: u8 = 224;
const PRIVATE_10_FIRST_OCTET: u8 = 10;
const PRIVATE_172_FIRST_OCTET: u8 = 172;
const PRIVATE_172_SECOND_OCTET_LOWER_BOUND: u8 = 16;
const PRIVATE_172_SECOND_OCTET_UPPER_BOUND: u8 = 31;
const PRIVATE_192_FIRST_OCTET: u8 = 192;
const PRIVATE_192_SECOND_OCTET: u8 = 168;
const TAILSCALE_CGNAT_FIRST_OCTET: u8 = 100;
const TAILSCALE_CGNAT_SECOND_OCTET_LOWER_BOUND: u8 = 64;
const TAILSCALE_CGNAT_SECOND_OCTET_UPPER_BOUND: u8 = 127;
const TAILSCALE_ULA_FIRST_SEGMENT: u16 = 0xfd7a;
const TAILSCALE_ULA_SECOND_SEGMENT: u16 = 0x115c;
const TAILSCALE_ULA_THIRD_SEGMENT: u16 = 0xa1e0;
const TAILSCALE_DNS_SUFFIX: &str = ".ts.net";
const LOCAL_DNS_SUFFIX: &str = ".local";
const TAILSCALE_STATUS_TIMEOUT_MS: u64 = 900;
const TAILSCALE_CLI_EXECUTABLE: &str = "tailscale";
const TAILSCALE_SOCKET_ENV: &str = "LOOPER_TAILSCALE_SOCKET";
const DEFAULT_TAILSCALE_SOCKET_PATHS: &[&str] = &[
    "/var/run/tailscale/tailscaled.sock",
    "/var/run/tailscaled.socket",
];
const LOCAL_INTERFACE_NAMES: &[&str] = &["en0", "en1", "bridge100"];
const MOBILE_BASE_URL_ENV_KEYS: &[&str] = &[
    "AGENT_CONTROL_PLANE_MOBILE_BASE_URLS",
    "AGENT_CONTROL_PLANE_MOBILE_BASE_URL",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URLS",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URL",
];
const MOBILE_GRPC_BASE_URL_ENV_KEYS: &[&str] = &[
    "AGENT_CONTROL_PLANE_MOBILE_GRPC_BASE_URLS",
    "AGENT_CONTROL_PLANE_MOBILE_GRPC_BASE_URL",
];
const MOBILE_GRPC_H3_BASE_URL_ENV_KEYS: &[&str] = &[
    "AGENT_CONTROL_PLANE_MOBILE_GRPC_H3_BASE_URLS",
    "AGENT_CONTROL_PLANE_MOBILE_GRPC_H3_BASE_URL",
];

pub fn advertised_mobile_base_urls(preferred_base_url: Option<&str>) -> Vec<String> {
    let local_addresses = if configured_listener_accepts_remote_connections() {
        local_ipv4_candidates()
    } else {
        Vec::new()
    };

    advertised_mobile_base_urls_from_sources(
        preferred_base_url,
        local_addresses,
        explicit_mobile_base_urls(),
        configured_control_plane_port(),
    )
}

pub fn configured_control_plane_port() -> u16 {
    std::env::var(LISTEN_ENV)
        .ok()
        .and_then(|listen| listen.parse::<SocketAddr>().ok())
        .map(|address| address.port())
        .unwrap_or(DEFAULT_AGENT_CONTROL_PLANE_PORT)
}

pub fn configured_grpc_control_plane_port() -> u16 {
    std::env::var(GRPC_LISTEN_ENV)
        .ok()
        .and_then(|listen| listen.parse::<SocketAddr>().ok())
        .map(|address| address.port())
        .or_else(|| derived_grpc_port(configured_control_plane_port()))
        .unwrap_or(DEFAULT_GRPC_CONTROL_PLANE_PORT)
}

pub fn configured_grpc_h3_control_plane_port() -> u16 {
    std::env::var(crate::grpc::GRPC_H3_LISTEN_ENV)
        .ok()
        .and_then(|listen| listen.parse::<SocketAddr>().ok())
        .map(|address| address.port())
        .unwrap_or_else(configured_grpc_control_plane_port)
}

pub fn default_grpc_listen_address(http_listen_address: SocketAddr) -> Result<SocketAddr> {
    if let Ok(listen_address) = std::env::var(GRPC_LISTEN_ENV) {
        return Ok(listen_address.parse::<SocketAddr>()?);
    }

    let grpc_port = derived_grpc_port(http_listen_address.port())
        .ok_or_else(|| anyhow!("HTTP listen port is too high to derive a gRPC listen port"))?;
    Ok(SocketAddr::new(http_listen_address.ip(), grpc_port))
}

pub fn advertised_mobile_grpc_base_urls(http_base_urls: &[String]) -> Vec<String> {
    advertised_mobile_grpc_base_urls_from_sources(
        http_base_urls,
        configured_grpc_control_plane_port(),
        explicit_mobile_grpc_base_urls(),
    )
}

pub fn advertised_mobile_grpc_h3_base_urls(http_base_urls: &[String]) -> Vec<String> {
    advertised_mobile_grpc_h3_base_urls_from_sources(
        http_base_urls,
        configured_grpc_h3_control_plane_port(),
        explicit_mobile_grpc_h3_base_urls(),
    )
}

pub async fn advertised_mobile_pairing_base_urls(preferred_base_url: Option<&str>) -> Vec<String> {
    let base_urls = advertised_mobile_base_urls(preferred_base_url);
    let grpc_base_urls = advertised_mobile_grpc_base_urls(&base_urls);
    let grpc_h3_base_urls = advertised_mobile_grpc_h3_base_urls(&base_urls);
    let tailscale = mobile_tailscale_status(&base_urls, &grpc_base_urls, &grpc_h3_base_urls).await;
    advertised_mobile_pairing_base_urls_from_sources(base_urls, tailscale.base_url)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MobileTailscaleStatus {
    pub available: bool,
    pub running: bool,
    #[serde(rename = "backendState", skip_serializing_if = "Option::is_none")]
    pub backend_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(rename = "dnsName", skip_serializing_if = "Option::is_none")]
    pub dns_name: Option<String>,
    #[serde(rename = "tailnetName", skip_serializing_if = "Option::is_none")]
    pub tailnet_name: Option<String>,
    #[serde(rename = "magicDNSSuffix", skip_serializing_if = "Option::is_none")]
    pub magic_dns_suffix: Option<String>,
    #[serde(rename = "magicDNSEnabled", skip_serializing_if = "Option::is_none")]
    pub magic_dns_enabled: Option<bool>,
    #[serde(rename = "ipAddresses")]
    pub ip_addresses: Vec<String>,
    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(rename = "grpcBaseURL", skip_serializing_if = "Option::is_none")]
    pub grpc_base_url: Option<String>,
    #[serde(rename = "grpcH3BaseURL", skip_serializing_if = "Option::is_none")]
    pub grpc_h3_base_url: Option<String>,
    pub health: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TailscaleDiscovery {
    source: String,
    running: Option<bool>,
    backend_state: Option<String>,
    version: Option<String>,
    hostname: Option<String>,
    dns_name: Option<String>,
    tailnet_name: Option<String>,
    magic_dns_suffix: Option<String>,
    magic_dns_enabled: Option<bool>,
    ip_addresses: Vec<String>,
    health: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TailscaleAdvertisedRoute {
    base_url: String,
    grpc_base_url: Option<String>,
    grpc_h3_base_url: Option<String>,
    ip_address: Option<String>,
}

pub async fn mobile_tailscale_status(
    http_base_urls: &[String],
    grpc_base_urls: &[String],
    grpc_h3_base_urls: &[String],
) -> MobileTailscaleStatus {
    let advertised_route =
        advertised_tailscale_route(http_base_urls, grpc_base_urls, grpc_h3_base_urls);
    let discovery_result = discover_tailscale_status().await;
    mobile_tailscale_status_from_sources(
        discovery_result,
        advertised_route,
        configured_control_plane_port(),
        configured_grpc_control_plane_port(),
    )
}

fn mobile_tailscale_status_from_sources(
    discovery_result: Result<TailscaleDiscovery>,
    advertised_route: Option<TailscaleAdvertisedRoute>,
    http_port: u16,
    grpc_port: u16,
) -> MobileTailscaleStatus {
    let route_ip_address = advertised_route
        .as_ref()
        .and_then(|route| route.ip_address.clone());
    let route_base_url = advertised_route
        .as_ref()
        .map(|route| route.base_url.clone());
    let route_grpc_base_url = advertised_route
        .as_ref()
        .and_then(|route| route.grpc_base_url.clone());
    let route_grpc_h3_base_url = advertised_route
        .as_ref()
        .and_then(|route| route.grpc_h3_base_url.clone());

    let (discovery, discovery_error) = match discovery_result {
        Ok(discovery) => (Some(discovery), None),
        Err(error) => (None, Some(error.to_string())),
    };

    let ip_addresses = unique_values(
        discovery
            .as_ref()
            .map(|status| status.ip_addresses.clone())
            .unwrap_or_default()
            .into_iter()
            .chain(route_ip_address)
            .collect(),
    );
    let has_route_candidate = route_base_url.is_some() || !ip_addresses.is_empty();
    let available = has_route_candidate;
    let running = discovery
        .as_ref()
        .and_then(|status| status.running)
        .unwrap_or(available);
    let base_url = running
        .then(|| {
            route_base_url.or_else(|| base_url_for_tailscale_addresses(&ip_addresses, http_port))
        })
        .flatten();
    let grpc_base_url = running
        .then(|| {
            route_grpc_base_url
                .or_else(|| base_url_for_tailscale_addresses(&ip_addresses, grpc_port))
        })
        .flatten();
    let grpc_h3_base_url = running.then(|| route_grpc_h3_base_url).flatten();
    let source = discovery
        .as_ref()
        .map(|status| status.source.clone())
        .or_else(|| available.then(|| "interface".to_owned()));
    let error = (!available).then_some(discovery_error).flatten();

    MobileTailscaleStatus {
        available,
        running,
        backend_state: discovery
            .as_ref()
            .and_then(|status| status.backend_state.clone()),
        source,
        version: discovery.as_ref().and_then(|status| status.version.clone()),
        hostname: discovery
            .as_ref()
            .and_then(|status| status.hostname.clone()),
        dns_name: discovery
            .as_ref()
            .and_then(|status| status.dns_name.clone()),
        tailnet_name: discovery
            .as_ref()
            .and_then(|status| status.tailnet_name.clone()),
        magic_dns_suffix: discovery
            .as_ref()
            .and_then(|status| status.magic_dns_suffix.clone()),
        magic_dns_enabled: discovery
            .as_ref()
            .and_then(|status| status.magic_dns_enabled),
        ip_addresses,
        base_url,
        grpc_base_url,
        grpc_h3_base_url,
        health: discovery
            .as_ref()
            .map(|status| status.health.clone())
            .unwrap_or_default(),
        error,
    }
}

async fn discover_tailscale_status() -> Result<TailscaleDiscovery> {
    match discover_tailscale_status_with_localapi().await {
        Ok(status) => Ok(status),
        Err(localapi_error) => match discover_tailscale_status_with_cli().await {
            Ok(status) => Ok(status),
            Err(cli_error) => Err(anyhow!(
                "localapi unavailable: {localapi_error}; cli unavailable: {cli_error}"
            )),
        },
    }
}

async fn discover_tailscale_status_with_localapi() -> Result<TailscaleDiscovery> {
    let socket_path = tailscale_socket_paths()
        .into_iter()
        .find(|path| Path::new(path).exists())
        .ok_or_else(|| anyhow!("tailscaled socket not found"))?;
    let client = tailscale_localapi::LocalApi::new_with_socket_path(socket_path);
    let status = timeout(tailscale_status_timeout(), client.status()).await??;
    Ok(tailscale_discovery_from_localapi_status(status))
}

async fn discover_tailscale_status_with_cli() -> Result<TailscaleDiscovery> {
    let output = timeout(
        tailscale_status_timeout(),
        AsyncCommand::new(TAILSCALE_CLI_EXECUTABLE)
            .args(["status", "--json"])
            .output(),
    )
    .await??;

    if !output.status.success() {
        return Err(anyhow!(
            "tailscale status exited with {}",
            output.status.code().unwrap_or_default()
        ));
    }

    let status: TailscaleCliStatus = serde_json::from_slice(&output.stdout)?;
    Ok(tailscale_discovery_from_cli_status(status))
}

fn tailscale_status_timeout() -> Duration {
    Duration::from_millis(TAILSCALE_STATUS_TIMEOUT_MS)
}

fn tailscale_socket_paths() -> Vec<String> {
    std::env::var(TAILSCALE_SOCKET_ENV)
        .ok()
        .into_iter()
        .chain(
            DEFAULT_TAILSCALE_SOCKET_PATHS
                .iter()
                .map(|path| path.to_string()),
        )
        .collect()
}

fn tailscale_discovery_from_localapi_status(
    status: tailscale_localapi::Status,
) -> TailscaleDiscovery {
    let backend_state = localapi_backend_state_label(&status.backend_state).to_owned();
    let ip_addresses = unique_values(
        status
            .tailscale_ips
            .into_iter()
            .chain(status.self_status.tailscale_ips.clone())
            .map(|address| address.to_string())
            .filter(|address| is_tailscale_ip_address(address))
            .collect(),
    );
    let current_tailnet = status.current_tailnet;

    TailscaleDiscovery {
        source: "localapi".to_owned(),
        running: Some(localapi_backend_is_running(&status.backend_state)),
        backend_state: Some(backend_state),
        version: clean_optional_string(status.version),
        hostname: clean_optional_string(status.self_status.hostname),
        dns_name: clean_optional_dns_name(status.self_status.dnsname),
        tailnet_name: current_tailnet
            .as_ref()
            .and_then(|tailnet| clean_optional_string(tailnet.name.clone())),
        magic_dns_suffix: current_tailnet
            .as_ref()
            .and_then(|tailnet| clean_optional_dns_name(tailnet.magic_dns_suffix.clone())),
        magic_dns_enabled: current_tailnet.map(|tailnet| tailnet.magic_dns_enabled),
        ip_addresses,
        health: status.health,
    }
}

fn localapi_backend_state_label(state: &tailscale_localapi::BackendState) -> &'static str {
    match state {
        tailscale_localapi::BackendState::NoState => "NoState",
        tailscale_localapi::BackendState::NeedsLogin => "NeedsLogin",
        tailscale_localapi::BackendState::NeedsMachineAuth => "NeedsMachineAuth",
        tailscale_localapi::BackendState::Stopped => "Stopped",
        tailscale_localapi::BackendState::Starting => "Starting",
        tailscale_localapi::BackendState::Running => "Running",
        _ => "Unknown",
    }
}

fn localapi_backend_is_running(state: &tailscale_localapi::BackendState) -> bool {
    matches!(state, tailscale_localapi::BackendState::Running)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TailscaleCliStatus {
    #[serde(default)]
    version: String,
    #[serde(default)]
    backend_state: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
    #[serde(rename = "Self", default)]
    self_status: Option<TailscaleCliSelfStatus>,
    #[serde(default)]
    health: Vec<String>,
    #[serde(default)]
    current_tailnet: Option<TailscaleCliTailnetStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TailscaleCliSelfStatus {
    #[serde(rename = "HostName", default)]
    hostname: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    tailscale_ips: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct TailscaleCliTailnetStatus {
    #[serde(default)]
    name: String,
    #[serde(rename = "MagicDNSSuffix", default)]
    magic_dns_suffix: String,
    #[serde(rename = "MagicDNSEnabled", default)]
    magic_dns_enabled: bool,
}

fn tailscale_discovery_from_cli_status(status: TailscaleCliStatus) -> TailscaleDiscovery {
    let self_status = status.self_status;
    let self_ip_addresses = self_status
        .as_ref()
        .map(|self_status| self_status.tailscale_ips.clone())
        .unwrap_or_default();
    let ip_addresses = unique_values(
        status
            .tailscale_ips
            .into_iter()
            .chain(self_ip_addresses)
            .filter(|address| is_tailscale_ip_address(address))
            .collect(),
    );
    let current_tailnet = status.current_tailnet;
    let backend_state = clean_optional_string(status.backend_state);
    let running = backend_state
        .as_deref()
        .map(|state| state.eq_ignore_ascii_case("running"));

    TailscaleDiscovery {
        source: "cli".to_owned(),
        running,
        backend_state,
        version: clean_optional_string(status.version),
        hostname: self_status
            .as_ref()
            .and_then(|self_status| clean_optional_string(self_status.hostname.clone())),
        dns_name: self_status
            .as_ref()
            .and_then(|self_status| clean_optional_dns_name(self_status.dns_name.clone())),
        tailnet_name: current_tailnet
            .as_ref()
            .and_then(|tailnet| clean_optional_string(tailnet.name.clone())),
        magic_dns_suffix: current_tailnet
            .as_ref()
            .and_then(|tailnet| clean_optional_dns_name(tailnet.magic_dns_suffix.clone())),
        magic_dns_enabled: current_tailnet.map(|tailnet| tailnet.magic_dns_enabled),
        ip_addresses,
        health: status.health,
    }
}

fn advertised_tailscale_route(
    http_base_urls: &[String],
    grpc_base_urls: &[String],
    grpc_h3_base_urls: &[String],
) -> Option<TailscaleAdvertisedRoute> {
    let base_url = http_base_urls
        .iter()
        .find(|base_url| base_url_host(base_url).is_some_and(|host| is_tailscale_host(&host)))?;
    let base_host = base_url_host(base_url);
    let grpc_base_url = base_host.as_ref().and_then(|base_host| {
        grpc_base_urls
            .iter()
            .find(|grpc_base_url| {
                base_url_host(grpc_base_url)
                    .as_ref()
                    .is_some_and(|grpc_host| same_tailscale_host(base_host, grpc_host))
            })
            .cloned()
    });
    let grpc_h3_base_url = base_host.as_ref().and_then(|base_host| {
        grpc_h3_base_urls
            .iter()
            .find(|grpc_h3_base_url| {
                base_url_host(grpc_h3_base_url)
                    .as_ref()
                    .is_some_and(|grpc_h3_host| same_tailscale_host(base_host, grpc_h3_host))
            })
            .cloned()
    });

    Some(TailscaleAdvertisedRoute {
        base_url: base_url.clone(),
        grpc_base_url,
        grpc_h3_base_url,
        ip_address: base_host.filter(|host| is_tailscale_ip_address(host)),
    })
}

fn base_url_for_tailscale_addresses(ip_addresses: &[String], port: u16) -> Option<String> {
    let host = ip_addresses
        .iter()
        .filter_map(|address| tailscale_url_host(address))
        .min_by_key(|host| host.starts_with('['))?;
    Some(format!("http://{host}:{port}"))
}

fn tailscale_url_host(value: &str) -> Option<String> {
    let normalized = normalize_url_host(value);
    let address = normalized.parse::<IpAddr>().ok()?;
    if !is_tailscale_ip_address(&normalized) {
        return None;
    }

    Some(match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    })
}

fn is_tailscale_host(host: &str) -> bool {
    let normalized = normalize_url_host(host);
    is_tailscale_ip_address(&normalized) || normalized.ends_with(TAILSCALE_DNS_SUFFIX)
}

fn same_tailscale_host(lhs: &str, rhs: &str) -> bool {
    normalize_url_host(lhs) == normalize_url_host(rhs)
}

fn is_tailscale_ip_address(value: &str) -> bool {
    match normalize_url_host(value).parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => is_tailscale_ipv4_octets(address.octets()),
        Ok(IpAddr::V6(address)) => is_tailscale_ipv6_segments(address.segments()),
        Err(_) => false,
    }
}

fn is_tailscale_ipv4_octets(octets: [u8; IPV4_OCTET_COUNT]) -> bool {
    let [first, second, _, _] = octets;
    first == TAILSCALE_CGNAT_FIRST_OCTET
        && (TAILSCALE_CGNAT_SECOND_OCTET_LOWER_BOUND..=TAILSCALE_CGNAT_SECOND_OCTET_UPPER_BOUND)
            .contains(&second)
}

fn is_tailscale_ipv6_segments(segments: [u16; 8]) -> bool {
    segments[0] == TAILSCALE_ULA_FIRST_SEGMENT
        && segments[1] == TAILSCALE_ULA_SECOND_SEGMENT
        && segments[2] == TAILSCALE_ULA_THIRD_SEGMENT
}

fn normalize_url_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn clean_optional_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn clean_optional_dns_name(value: String) -> Option<String> {
    clean_optional_string(value).map(|value| value.trim_end_matches('.').to_owned())
}

fn configured_listener_accepts_remote_connections() -> bool {
    std::env::var(LISTEN_ENV)
        .ok()
        .as_deref()
        .and_then(listen_address_accepts_remote_connections)
        .unwrap_or(false)
}

pub struct BonjourAdvertisement {
    child: Option<Child>,
}

impl BonjourAdvertisement {
    pub fn start_for_listener(listener_address: SocketAddr) -> std::io::Result<Option<Self>> {
        if !should_advertise_listener(listener_address) {
            return Ok(None);
        }

        Self::start(listener_address.port()).map(Some)
    }

    fn start(port: u16) -> std::io::Result<Self> {
        let child = Command::new(DNS_SD_EXECUTABLE)
            .args([
                "-R",
                MOBILE_BONJOUR_INSTANCE_NAME,
                MOBILE_BONJOUR_SERVICE_TYPE,
                MOBILE_BONJOUR_DOMAIN,
                &port.to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(Self { child: Some(child) })
    }
}

impl Drop for BonjourAdvertisement {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        let _ = child.kill();
        let _ = child.wait();
    }
}

fn advertised_mobile_base_urls_from_sources(
    preferred_base_url: Option<&str>,
    local_addresses: Vec<String>,
    explicit_urls: Vec<String>,
    port: u16,
) -> Vec<String> {
    let preferred_urls = preferred_base_url
        .filter(|value| is_mobile_reachable_base_url(value))
        .and_then(normalize_base_url)
        .into_iter();
    let local_urls = local_addresses
        .into_iter()
        .filter(|address| is_first_class_mobile_host(address))
        .map(|address| format!("http://{address}:{port}"));
    let explicit_first_class_urls = explicit_urls
        .into_iter()
        .filter(|base_url| is_first_class_mobile_base_url(base_url));
    let fallback_url = format!("http://127.0.0.1:{port}");

    unique_values(
        preferred_urls
            .chain(local_urls)
            .chain(explicit_first_class_urls)
            .chain(std::iter::once(fallback_url))
            .collect(),
    )
}

fn advertised_mobile_pairing_base_urls_from_sources(
    mut base_urls: Vec<String>,
    tailscale_base_url: Option<String>,
) -> Vec<String> {
    let Some(tailscale_base_url) = tailscale_base_url else {
        return base_urls;
    };

    let primary_route_is_phone_reachable = base_urls
        .first()
        .is_some_and(|base_url| is_mobile_reachable_base_url(base_url));

    if let Some(existing_index) = base_urls
        .iter()
        .position(|base_url| base_url == &tailscale_base_url)
    {
        if !primary_route_is_phone_reachable && existing_index != 0 {
            let base_url = base_urls.remove(existing_index);
            base_urls.insert(0, base_url);
        }
        return base_urls;
    }

    if primary_route_is_phone_reachable {
        base_urls.push(tailscale_base_url);
    } else {
        base_urls.insert(0, tailscale_base_url);
    }
    base_urls
}

fn advertised_mobile_grpc_base_urls_from_sources(
    http_base_urls: &[String],
    grpc_port: u16,
    explicit_urls: Vec<String>,
) -> Vec<String> {
    let derived_urls = http_base_urls
        .iter()
        .filter_map(|base_url| grpc_base_url_for_http_base_url(base_url, grpc_port));
    let explicit_first_class_urls = explicit_urls
        .into_iter()
        .filter(|base_url| is_first_class_mobile_base_url(base_url));
    unique_values(derived_urls.chain(explicit_first_class_urls).collect())
}

fn advertised_mobile_grpc_h3_base_urls_from_sources(
    http_base_urls: &[String],
    grpc_h3_port: u16,
    explicit_urls: Vec<String>,
) -> Vec<String> {
    let derived_urls = http_base_urls
        .iter()
        .filter_map(|base_url| grpc_h3_base_url_for_http_base_url(base_url, grpc_h3_port));
    let explicit_first_class_urls = explicit_urls.into_iter().filter_map(|base_url| {
        let normalized = normalize_grpc_h3_base_url(&base_url)?;
        is_first_class_mobile_base_url(&normalized).then_some(normalized)
    });
    unique_values(derived_urls.chain(explicit_first_class_urls).collect())
}

fn explicit_mobile_base_urls() -> Vec<String> {
    MOBILE_BASE_URL_ENV_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .flat_map(|value| split_base_url_values(&value))
        .filter_map(|value| normalize_base_url(&value))
        .collect()
}

fn explicit_mobile_grpc_base_urls() -> Vec<String> {
    MOBILE_GRPC_BASE_URL_ENV_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .flat_map(|value| split_base_url_values(&value))
        .filter_map(|value| normalize_base_url(&value))
        .collect()
}

fn explicit_mobile_grpc_h3_base_urls() -> Vec<String> {
    MOBILE_GRPC_H3_BASE_URL_ENV_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .flat_map(|value| split_base_url_values(&value))
        .filter_map(|value| normalize_grpc_h3_base_url(&value))
        .collect()
}

fn split_base_url_values(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            character.is_ascii_whitespace() || character == ',' || character == ';'
        })
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

fn normalize_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("http://{trimmed}")
    };
    if !(candidate.starts_with("http://") || candidate.starts_with("https://")) {
        return None;
    }
    Some(candidate.trim_end_matches('/').to_owned())
}

fn local_ipv4_candidates() -> Vec<String> {
    unique_values(
        preferred_interface_ipv4_candidates()
            .into_iter()
            .chain(ifconfig_ipv4_candidates())
            .collect(),
    )
}

fn preferred_interface_ipv4_candidates() -> Vec<String> {
    LOCAL_INTERFACE_NAMES
        .iter()
        .filter_map(|interface_name| {
            let output = Command::new(IPCONFIG_EXECUTABLE)
                .args(["getifaddr", interface_name])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let address = String::from_utf8(output.stdout).ok()?.trim().to_owned();
            is_advertisable_ipv4_address(&address).then_some(address)
        })
        .collect()
}

fn ifconfig_ipv4_candidates() -> Vec<String> {
    let output = Command::new(IFCONFIG_EXECUTABLE).output().ok();
    output
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| parse_ifconfig_ipv4_addresses(&output))
        .unwrap_or_default()
}

fn parse_ifconfig_ipv4_addresses(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let address = line
                .trim_start()
                .strip_prefix("inet ")?
                .split_whitespace()
                .next()?;
            is_advertisable_ipv4_address(address).then_some(address.to_owned())
        })
        .collect()
}

fn is_mobile_reachable_base_url(value: &str) -> bool {
    normalize_base_url(value)
        .and_then(|base_url| base_url_host(&base_url).map(|host| is_first_class_mobile_host(&host)))
        .unwrap_or(false)
}

fn is_first_class_mobile_base_url(value: &str) -> bool {
    normalize_base_url(value)
        .and_then(|base_url| base_url_host(&base_url).map(|host| is_first_class_mobile_host(&host)))
        .unwrap_or(false)
}

fn base_url_host(base_url: &str) -> Option<String> {
    let without_scheme = base_url.split_once("://")?.1;
    let authority = without_scheme.split('/').next()?.trim();
    base_url_authority_host(authority)
}

fn grpc_base_url_for_http_base_url(base_url: &str, grpc_port: u16) -> Option<String> {
    let normalized_base_url = normalize_base_url(base_url)?;
    let (scheme, without_scheme) = normalized_base_url.split_once("://")?;
    let authority = without_scheme.split('/').next()?.trim();
    let host = base_url_authority_host(authority)?;
    Some(format!("{scheme}://{host}:{grpc_port}"))
}

fn grpc_h3_base_url_for_http_base_url(base_url: &str, grpc_h3_port: u16) -> Option<String> {
    let normalized_base_url = normalize_base_url(base_url)?;
    let (_, without_scheme) = normalized_base_url.split_once("://")?;
    let authority = without_scheme.split('/').next()?.trim();
    let host = base_url_authority_host(authority)?;
    Some(format!("https://{}:{grpc_h3_port}", bracketed_host(&host)))
}

fn normalize_grpc_h3_base_url(value: &str) -> Option<String> {
    let normalized_base_url = normalize_base_url(value)?;
    let (_, without_scheme) = normalized_base_url.split_once("://")?;
    Some(format!("https://{}", without_scheme.trim_end_matches('/')))
}

fn base_url_authority_host(authority: &str) -> Option<String> {
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map(|(host, _)| host.trim_start_matches('[').to_owned());
    }

    Some(authority.split(':').next()?.to_owned())
}

fn bracketed_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        return format!("[{host}]");
    }
    host.to_owned()
}

fn derived_grpc_port(http_port: u16) -> Option<u16> {
    http_port.checked_add(DEFAULT_GRPC_PORT_OFFSET)
}

fn is_first_class_mobile_host(host: &str) -> bool {
    let normalized = host.trim().trim_matches('.').to_ascii_lowercase();
    if normalized.is_empty() || normalized == "localhost" {
        return false;
    }

    if is_tailscale_host(&normalized) || normalized.ends_with(LOCAL_DNS_SUFFIX) {
        return true;
    }

    parse_ipv4_octets(&normalized)
        .map(|octets| is_lan_ipv4_octets(octets) && is_advertisable_ipv4_octets(octets))
        .unwrap_or(false)
}

fn is_advertisable_ipv4_address(value: &str) -> bool {
    parse_ipv4_octets(value)
        .map(is_advertisable_ipv4_octets)
        .unwrap_or(false)
}

fn parse_ipv4_octets(value: &str) -> Option<[u8; IPV4_OCTET_COUNT]> {
    let octets = value
        .split('.')
        .map(str::parse::<u8>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    octets.try_into().ok()
}

fn is_advertisable_ipv4_octets(octets: [u8; IPV4_OCTET_COUNT]) -> bool {
    let [first, second, _, _] = octets;
    first != LOOPBACK_FIRST_OCTET
        && first != UNSPECIFIED_FIRST_OCTET
        && first < MULTICAST_FIRST_OCTET_LOWER_BOUND
        && !(first == LINK_LOCAL_FIRST_OCTET && second == LINK_LOCAL_SECOND_OCTET)
}

fn is_lan_ipv4_octets(octets: [u8; IPV4_OCTET_COUNT]) -> bool {
    let [first, second, _, _] = octets;
    first == PRIVATE_10_FIRST_OCTET
        || (first == PRIVATE_172_FIRST_OCTET
            && (PRIVATE_172_SECOND_OCTET_LOWER_BOUND..=PRIVATE_172_SECOND_OCTET_UPPER_BOUND)
                .contains(&second))
        || (first == PRIVATE_192_FIRST_OCTET && second == PRIVATE_192_SECOND_OCTET)
}

fn should_advertise_listener(listener_address: SocketAddr) -> bool {
    !listener_address.ip().is_loopback()
}

fn listen_address_accepts_remote_connections(value: &str) -> Option<bool> {
    value
        .parse::<SocketAddr>()
        .ok()
        .map(|address| !address.ip().is_loopback())
}

fn unique_values(values: Vec<String>) -> Vec<String> {
    values.into_iter().fold(Vec::new(), |mut unique, value| {
        if !unique.contains(&value) {
            unique.push(value);
        }
        unique
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PORT: u16 = 8765;
    const TEST_GRPC_PORT: u16 = 8766;
    const TEST_GRPC_H3_PORT: u16 = 8767;
    const TEST_LAN_BASE_URL: &str = "http://192.168.1.4:8765";
    const TEST_LOOPBACK_BASE_URL: &str = "http://127.0.0.1:8765";
    const TEST_TAILSCALE_BASE_URL: &str = "http://100.119.200.69:8765";
    const TEST_REMOTE_BASE_URL: &str = "https://looper.example.test";

    #[test]
    fn advertised_urls_prefer_reachable_request_and_current_interfaces() {
        let urls = advertised_mobile_base_urls_from_sources(
            Some("http://192.168.1.4:8765"),
            vec!["192.168.1.4".to_owned(), "100.119.200.69".to_owned()],
            vec!["http://192.168.1.10:8765".to_owned()],
            TEST_PORT,
        );

        assert_eq!(
            urls,
            vec![
                "http://192.168.1.4:8765",
                "http://100.119.200.69:8765",
                "http://192.168.1.10:8765",
                "http://127.0.0.1:8765",
            ]
        );
    }

    #[test]
    fn advertised_urls_keep_lan_and_tailscale_routes_before_loopback() {
        let urls = advertised_mobile_base_urls_from_sources(
            Some(TEST_LAN_BASE_URL),
            vec![
                "203.0.113.8".to_owned(),
                "100.119.200.69".to_owned(),
                "10.10.0.42".to_owned(),
            ],
            vec![
                TEST_REMOTE_BASE_URL.to_owned(),
                "https://macbook-pro.local:8765".to_owned(),
                "https://100.119.200.70:8765".to_owned(),
            ],
            TEST_PORT,
        );

        assert_eq!(
            urls,
            vec![
                TEST_LAN_BASE_URL.to_owned(),
                "http://100.119.200.69:8765".to_owned(),
                "http://10.10.0.42:8765".to_owned(),
                "https://macbook-pro.local:8765".to_owned(),
                "https://100.119.200.70:8765".to_owned(),
                TEST_LOOPBACK_BASE_URL.to_owned(),
            ]
        );
    }

    #[test]
    fn advertised_urls_do_not_promote_remote_routes() {
        let urls = advertised_mobile_base_urls_from_sources(
            Some(TEST_REMOTE_BASE_URL),
            vec!["203.0.113.8".to_owned()],
            vec![TEST_REMOTE_BASE_URL.to_owned()],
            TEST_PORT,
        );

        assert_eq!(urls, vec![TEST_LOOPBACK_BASE_URL.to_owned()]);
    }

    #[test]
    fn advertised_urls_ignore_loopback_request_host_for_mobile_ordering() {
        let urls = advertised_mobile_base_urls_from_sources(
            Some("http://127.0.0.1:8765"),
            vec!["192.168.1.4".to_owned()],
            Vec::new(),
            TEST_PORT,
        );

        assert_eq!(
            urls,
            vec!["http://192.168.1.4:8765", "http://127.0.0.1:8765"]
        );
    }

    #[test]
    fn pairing_urls_add_tailscale_route_for_loopback_only_desktop_requests() {
        let urls = advertised_mobile_pairing_base_urls_from_sources(
            vec![TEST_LOOPBACK_BASE_URL.to_owned()],
            Some(TEST_TAILSCALE_BASE_URL.to_owned()),
        );

        assert_eq!(
            urls,
            vec![
                TEST_TAILSCALE_BASE_URL.to_owned(),
                TEST_LOOPBACK_BASE_URL.to_owned()
            ]
        );
    }

    #[test]
    fn pairing_urls_keep_reachable_primary_route_before_tailscale() {
        let urls = advertised_mobile_pairing_base_urls_from_sources(
            vec![
                TEST_LAN_BASE_URL.to_owned(),
                TEST_LOOPBACK_BASE_URL.to_owned(),
            ],
            Some(TEST_TAILSCALE_BASE_URL.to_owned()),
        );

        assert_eq!(
            urls,
            vec![
                TEST_LAN_BASE_URL.to_owned(),
                TEST_LOOPBACK_BASE_URL.to_owned(),
                TEST_TAILSCALE_BASE_URL.to_owned()
            ]
        );
    }

    #[test]
    fn loopback_listener_does_not_accept_remote_connections() {
        assert_eq!(
            listen_address_accepts_remote_connections("127.0.0.1:8765"),
            Some(false)
        );
        assert_eq!(
            listen_address_accepts_remote_connections("0.0.0.0:8765"),
            Some(true)
        );
    }

    #[test]
    fn grpc_advertised_urls_reuse_mobile_hosts_with_grpc_port() {
        let urls = vec![
            "http://192.168.1.4:8765".to_owned(),
            "http://127.0.0.1:8765".to_owned(),
        ];

        assert_eq!(
            advertised_mobile_grpc_base_urls_from_sources(&urls, TEST_GRPC_PORT, Vec::new()),
            vec![
                "http://192.168.1.4:8766".to_owned(),
                "http://127.0.0.1:8766".to_owned(),
            ]
        );
    }

    #[test]
    fn grpc_advertised_urls_filter_explicit_remote_routes() {
        let http_urls = vec![TEST_LAN_BASE_URL.to_owned()];

        assert_eq!(
            advertised_mobile_grpc_base_urls_from_sources(
                &http_urls,
                TEST_GRPC_PORT,
                vec![
                    "https://looper.example.test:8766".to_owned(),
                    "https://100.119.200.69:8766".to_owned(),
                ],
            ),
            vec![
                "http://192.168.1.4:8766".to_owned(),
                "https://100.119.200.69:8766".to_owned(),
            ]
        );
    }

    #[test]
    fn grpc_h3_advertised_urls_reuse_mobile_hosts_with_h3_port_and_https() {
        let urls = vec![
            "http://192.168.1.4:8765".to_owned(),
            "http://127.0.0.1:8765".to_owned(),
        ];

        assert_eq!(
            advertised_mobile_grpc_h3_base_urls_from_sources(&urls, TEST_GRPC_H3_PORT, Vec::new()),
            vec![
                "https://192.168.1.4:8767".to_owned(),
                "https://127.0.0.1:8767".to_owned(),
            ]
        );
    }

    #[test]
    fn grpc_h3_advertised_urls_filter_explicit_remote_routes() {
        let http_urls = vec![TEST_LAN_BASE_URL.to_owned()];

        assert_eq!(
            advertised_mobile_grpc_h3_base_urls_from_sources(
                &http_urls,
                TEST_GRPC_H3_PORT,
                vec![
                    "https://looper.example.test:8767".to_owned(),
                    "http://100.119.200.69:8767".to_owned(),
                ],
            ),
            vec![
                "https://192.168.1.4:8767".to_owned(),
                "https://100.119.200.69:8767".to_owned(),
            ]
        );
    }

    #[test]
    fn grpc_listen_address_uses_control_plane_port_offset() {
        let http_address: SocketAddr = "127.0.0.1:8765".parse().expect("http address");

        assert_eq!(
            default_grpc_listen_address(http_address).expect("grpc listen"),
            "127.0.0.1:8766"
                .parse::<SocketAddr>()
                .expect("grpc address")
        );
    }

    #[test]
    fn ifconfig_parser_keeps_lan_and_tailscale_addresses() {
        let addresses = parse_ifconfig_ipv4_addresses(
            r#"
lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384
    inet 127.0.0.1 netmask 0xff000000
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
    inet 192.168.1.4 netmask 0xffffff00 broadcast 192.168.1.255
utun6: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1280
    inet 100.119.200.69 --> 100.119.200.69 netmask 0xffffffff
awdl0: flags=8943<UP,BROADCAST,RUNNING,PROMISC,SIMPLEX,MULTICAST> mtu 1484
    inet 169.254.245.140 netmask 0xffff0000 broadcast 169.254.255.255
"#,
        );

        assert_eq!(
            addresses,
            vec!["192.168.1.4".to_owned(), "100.119.200.69".to_owned()]
        );
    }

    #[test]
    fn tailscale_status_prefers_advertised_route_and_keeps_discovery_metadata() {
        let status = mobile_tailscale_status_from_sources(
            Ok(TailscaleDiscovery {
                source: "cli".to_owned(),
                running: Some(true),
                backend_state: Some("Running".to_owned()),
                version: Some("1.98.5".to_owned()),
                hostname: Some("Ayush's MacBook Pro".to_owned()),
                dns_name: Some("ayushs-macbook-pro.tail62d9a8.ts.net".to_owned()),
                tailnet_name: Some("heyayushh.github".to_owned()),
                magic_dns_suffix: Some("tail62d9a8.ts.net".to_owned()),
                magic_dns_enabled: Some(true),
                ip_addresses: vec!["100.119.200.69".to_owned()],
                health: Vec::new(),
            }),
            advertised_tailscale_route(
                &[
                    "http://172.20.10.2:8765".to_owned(),
                    "http://100.119.200.69:8765".to_owned(),
                ],
                &[
                    "http://172.20.10.2:8766".to_owned(),
                    "http://100.119.200.69:8766".to_owned(),
                ],
                &[
                    "https://172.20.10.2:8767".to_owned(),
                    "https://100.119.200.69:8767".to_owned(),
                ],
            ),
            TEST_PORT,
            TEST_GRPC_PORT,
        );

        assert!(status.available);
        assert!(status.running);
        assert_eq!(status.source.as_deref(), Some("cli"));
        assert_eq!(
            status.base_url.as_deref(),
            Some("http://100.119.200.69:8765")
        );
        assert_eq!(
            status.grpc_base_url.as_deref(),
            Some("http://100.119.200.69:8766")
        );
        assert_eq!(
            status.grpc_h3_base_url.as_deref(),
            Some("https://100.119.200.69:8767")
        );
        assert_eq!(
            status.magic_dns_suffix.as_deref(),
            Some("tail62d9a8.ts.net")
        );
    }

    #[test]
    fn tailscale_status_builds_route_from_discovered_ip_without_advertised_route() {
        let status = mobile_tailscale_status_from_sources(
            Ok(TailscaleDiscovery {
                source: "localapi".to_owned(),
                running: Some(true),
                backend_state: Some("Running".to_owned()),
                version: None,
                hostname: None,
                dns_name: None,
                tailnet_name: None,
                magic_dns_suffix: None,
                magic_dns_enabled: None,
                ip_addresses: vec![
                    "fd7a:115c:a1e0::9634:c845".to_owned(),
                    "100.119.200.69".to_owned(),
                ],
                health: Vec::new(),
            }),
            None,
            TEST_PORT,
            TEST_GRPC_PORT,
        );

        assert!(status.available);
        assert_eq!(
            status.base_url.as_deref(),
            Some("http://100.119.200.69:8765")
        );
        assert_eq!(
            status.grpc_base_url.as_deref(),
            Some("http://100.119.200.69:8766")
        );
        assert_eq!(
            status.ip_addresses,
            vec![
                "fd7a:115c:a1e0::9634:c845".to_owned(),
                "100.119.200.69".to_owned(),
            ]
        );
    }

    #[test]
    fn tailscale_status_does_not_advertise_stopped_routes() {
        let status = mobile_tailscale_status_from_sources(
            Ok(TailscaleDiscovery {
                source: "cli".to_owned(),
                running: Some(false),
                backend_state: Some("Stopped".to_owned()),
                version: Some("1.98.5".to_owned()),
                hostname: Some("Ayush's MacBook Pro".to_owned()),
                dns_name: Some("ayushs-macbook-pro.tail62d9a8.ts.net".to_owned()),
                tailnet_name: Some("heyayushh.github".to_owned()),
                magic_dns_suffix: Some("tail62d9a8.ts.net".to_owned()),
                magic_dns_enabled: Some(true),
                ip_addresses: vec!["100.119.200.69".to_owned()],
                health: vec!["Tailscale is stopped.".to_owned()],
            }),
            advertised_tailscale_route(
                &["http://100.119.200.69:8765".to_owned()],
                &["http://100.119.200.69:8766".to_owned()],
                &["https://100.119.200.69:8767".to_owned()],
            ),
            TEST_PORT,
            TEST_GRPC_PORT,
        );

        assert!(status.available);
        assert!(!status.running);
        assert_eq!(status.backend_state.as_deref(), Some("Stopped"));
        assert_eq!(status.base_url, None);
        assert_eq!(status.grpc_base_url, None);
        assert_eq!(status.grpc_h3_base_url, None);
        assert_eq!(status.health, vec!["Tailscale is stopped.".to_owned()]);
    }

    #[test]
    fn tailscale_host_detection_handles_cgnat_ipv6_and_magic_dns() {
        assert!(is_tailscale_host("100.64.0.1"));
        assert!(is_tailscale_host("100.127.255.254"));
        assert!(is_tailscale_host("[fd7a:115c:a1e0::9634:c845]"));
        assert!(is_tailscale_host("ayushs-macbook-pro.tail62d9a8.ts.net."));
        assert!(!is_tailscale_host("100.128.0.1"));
        assert!(!is_tailscale_host("192.168.1.4"));
    }
}

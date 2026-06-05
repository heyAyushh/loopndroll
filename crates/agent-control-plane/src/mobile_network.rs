use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};

pub const DEFAULT_AGENT_CONTROL_PLANE_PORT: u16 = 8765;

const LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_LISTEN";
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
const LOCAL_INTERFACE_NAMES: &[&str] = &["en0", "en1", "bridge100"];
const MOBILE_BASE_URL_ENV_KEYS: &[&str] = &[
    "AGENT_CONTROL_PLANE_MOBILE_BASE_URLS",
    "AGENT_CONTROL_PLANE_MOBILE_BASE_URL",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URLS",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URL",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URLS",
    "LOOPER_MOBILE_DEV_SERVER_PUBLIC_BASE_URL",
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
        .and_then(|listen| listen.rsplit_once(':').map(|(_, port)| port.to_owned()))
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(DEFAULT_AGENT_CONTROL_PLANE_PORT)
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
        .map(|address| format!("http://{address}:{port}"));
    let fallback_url = format!("http://127.0.0.1:{port}");

    unique_values(
        preferred_urls
            .chain(local_urls)
            .chain(explicit_urls)
            .chain(std::iter::once(fallback_url))
            .collect(),
    )
}

fn explicit_mobile_base_urls() -> Vec<String> {
    MOBILE_BASE_URL_ENV_KEYS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .flat_map(|value| split_base_url_values(&value))
        .filter_map(|value| normalize_base_url(&value))
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
        .and_then(|base_url| base_url_host(&base_url).map(|host| is_mobile_reachable_host(&host)))
        .unwrap_or(false)
}

fn base_url_host(base_url: &str) -> Option<String> {
    let without_scheme = base_url.split_once("://")?.1;
    let authority = without_scheme.split('/').next()?.trim();
    if authority.starts_with('[') {
        return authority
            .split_once(']')
            .map(|(host, _)| host.trim_start_matches('[').to_owned());
    }

    Some(authority.split(':').next()?.to_owned())
}

fn is_mobile_reachable_host(host: &str) -> bool {
    let normalized = host.trim().trim_matches('.').to_ascii_lowercase();
    if normalized.is_empty() || normalized == "localhost" {
        return false;
    }

    parse_ipv4_octets(&normalized)
        .map(is_advertisable_ipv4_octets)
        .unwrap_or(true)
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
}

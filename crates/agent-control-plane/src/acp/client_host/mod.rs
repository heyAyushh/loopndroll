//! Desktop ACP client-host response mapping.
//!
//! The HTTP layer exposes a single normalized ACP host model, while each
//! desktop client has different ownership boundaries. Keep provider-specific
//! behavior in child modules so Devin's managed bridge and Zed's read-only
//! visibility path cannot drift into each other.

mod devin;
mod model;
mod zed;

pub use devin::{
    DEVIN_ACP_CLIENT_HOST_ID, acp_client_host_install, acp_client_host_probe, devin_acp_client_host,
};
pub use model::{
    AcpClientHost, AcpClientHostAction, AcpClientHostAgent, AcpClientHostInstall,
    AcpClientHostInstallResponse, AcpClientHostProbe, AcpClientHostProbeResponse,
    AcpClientHostRegistry, AcpClientHostResponse, AcpClientHostRuntime, AcpClientHostSession,
    AcpClientHostsResponse,
};
pub use zed::zed_acp_client_host_install;
pub use zed::{ZED_ACP_CLIENT_HOST_ID, zed_acp_client_host, zed_acp_client_host_probe};

const ACP_CLIENT_HOSTS_ROUTE: &str = "/desktop/acp-client-hosts";
pub(super) const ACP_CLIENT_HOST_PROBE_ACTION_ID: &str = "probe";
pub(super) const ACP_CLIENT_HOST_INSTALL_ACTION_ID: &str = "install";

pub(super) fn acp_client_host_action_path(client_id: &str, action_id: &str) -> String {
    format!("{ACP_CLIENT_HOSTS_ROUTE}/{client_id}/{action_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_path_uses_client_host_id() {
        assert_eq!(
            acp_client_host_action_path("zed", ACP_CLIENT_HOST_INSTALL_ACTION_ID),
            "/desktop/acp-client-hosts/zed/install"
        );
        assert_eq!(
            acp_client_host_action_path("zed", ACP_CLIENT_HOST_PROBE_ACTION_ID),
            "/desktop/acp-client-hosts/zed/probe"
        );
    }
}

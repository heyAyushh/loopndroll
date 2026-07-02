use super::*;

mod control_sessions;
mod runtime_threads;
pub(in crate::control_plane) use runtime_threads::{
    active_zed_acp_runtime_session_count, devin_acp_runtime_session_capabilities,
    devin_acp_runtime_session_to_desktop_thread, zed_acp_connection,
    zed_acp_runtime_session_capabilities, zed_acp_runtime_session_to_desktop_thread,
};

pub(super) const ACP_CLIENT_HOST_SESSION_LIMIT: usize = DESKTOP_MENU_THREAD_LIMIT;
const ACP_CLIENT_HOST_PROVIDERS: &[AcpClientHostProvider] =
    &[AcpClientHostProvider::Devin, AcpClientHostProvider::Zed];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AcpClientHostProvider {
    Devin,
    Zed,
}

impl AcpClientHostProvider {
    fn id(self) -> &'static str {
        match self {
            Self::Devin => DEVIN_ACP_CLIENT_HOST_ID,
            Self::Zed => ZED_ACP_CLIENT_HOST_ID,
        }
    }

    fn supports_install(self) -> bool {
        matches!(self, Self::Devin | Self::Zed)
    }
}

impl ControlPlane {
    pub fn acp_client_hosts_response(&self) -> AcpClientHostsResponse {
        self.response_cache
            .acp_client_hosts
            .get_or_refresh_infallible(DESKTOP_MENU_INSPECTION_CACHE_TTL, || {
                self.acp_client_hosts_response_uncached()
            })
    }

    fn acp_client_hosts_response_uncached(&self) -> AcpClientHostsResponse {
        AcpClientHostsResponse {
            hosts: ACP_CLIENT_HOST_PROVIDERS
                .iter()
                .map(|provider| self.acp_client_host_status(*provider))
                .collect(),
        }
    }

    pub fn acp_client_host_response(&self, client_id: &str) -> Option<AcpClientHostResponse> {
        self.acp_client_hosts_response()
            .hosts
            .into_iter()
            .find(|host| host.id == client_id)
            .map(|host| AcpClientHostResponse { host })
    }

    pub fn acp_client_host_probe_response(
        &self,
        client_id: &str,
        agent_id: Option<&str>,
    ) -> Option<AcpClientHostProbeResponse> {
        let provider = self.acp_client_host_provider(client_id)?;
        Some(match provider {
            AcpClientHostProvider::Devin => {
                let status = self.inspect_devin_desktop_status();
                let probe =
                    build_acp_bridge_probe(&status.installations, &status.acp_registry, agent_id);
                let runtime = self.devin_acp_runtime.status();
                let sessions = self
                    .recent_devin_sessions_for_snapshot(ACP_CLIENT_HOST_SESSION_LIMIT)
                    .sessions;
                AcpClientHostProbeResponse {
                    host: devin_acp_client_host(&status, &sessions, &runtime),
                    probe: acp_client_host_probe(probe),
                }
            }
            AcpClientHostProvider::Zed => {
                let status = self.inspect_zed_status();
                let runtime = self.zed_acp_runtime.status();
                AcpClientHostProbeResponse {
                    host: zed_acp_client_host(&status, &runtime),
                    probe: zed_acp_client_host_probe(&status, agent_id),
                }
            }
        })
    }

    pub fn acp_client_host_exists(&self, client_id: &str) -> bool {
        self.acp_client_host_provider(client_id).is_some()
    }

    pub fn acp_client_host_install_supported(&self, client_id: &str) -> bool {
        self.acp_client_host_provider(client_id)
            .map(AcpClientHostProvider::supports_install)
            .unwrap_or(false)
    }

    pub fn install_acp_client_host_response(
        &self,
        client_id: &str,
    ) -> Result<Option<AcpClientHostInstallResponse>> {
        let Some(provider) = self.acp_client_host_provider(client_id) else {
            return Ok(None);
        };
        let response = Some(match provider {
            AcpClientHostProvider::Devin => {
                let install = install_looper_acp_agent_for_home(
                    self.home_path(),
                    &crate::runtime::default_server_base_url(),
                )?;
                let status = self.inspect_devin_desktop_status();
                let runtime = self.devin_acp_runtime.status();
                let sessions = self
                    .recent_devin_sessions_for_snapshot(ACP_CLIENT_HOST_SESSION_LIMIT)
                    .sessions;
                AcpClientHostInstallResponse {
                    host: devin_acp_client_host(&status, &sessions, &runtime),
                    install: acp_client_host_install(provider.id(), install),
                }
            }
            AcpClientHostProvider::Zed => {
                let install = install_looper_zed_acp_agent_for_home(self.home_path())?;
                let status = self.inspect_zed_status();
                let runtime = self.zed_acp_runtime.status();
                AcpClientHostInstallResponse {
                    host: zed_acp_client_host(&status, &runtime),
                    install: zed_acp_client_host_install(provider.id(), install),
                }
            }
        });
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(response)
    }

    pub(super) fn acp_client_host_provider(
        &self,
        client_id: &str,
    ) -> Option<AcpClientHostProvider> {
        ACP_CLIENT_HOST_PROVIDERS
            .iter()
            .copied()
            .find(|provider| provider.id() == client_id)
    }

    fn acp_client_host_status(
        &self,
        provider: AcpClientHostProvider,
    ) -> crate::acp::client_host::AcpClientHost {
        match provider {
            AcpClientHostProvider::Devin => {
                let status = self.cached_devin_desktop_status();
                let sessions = self
                    .recent_devin_sessions_for_snapshot(ACP_CLIENT_HOST_SESSION_LIMIT)
                    .sessions;
                let runtime = self.devin_acp_runtime.status();
                devin_acp_client_host(&status, &sessions, &runtime)
            }
            AcpClientHostProvider::Zed => {
                let status = self.cached_zed_status();
                let runtime = self.zed_acp_runtime.status();
                zed_acp_client_host(&status, &runtime)
            }
        }
    }
}

use super::super::*;

impl ControlPlane {
    pub fn devin_acp_bridge_response(&self) -> DevinAcpBridgeResponse {
        DevinAcpBridgeResponse {
            bridge: inspect_devin_desktop_for_home(&self.config.home_path).acp_bridge,
            runtime: self.devin_acp_runtime.status(),
        }
    }

    pub fn devin_acp_bridge_probe_response(
        &self,
        agent_id: Option<&str>,
    ) -> DevinAcpBridgeProbeResponse {
        let status = inspect_devin_desktop_for_home(&self.config.home_path);
        DevinAcpBridgeProbeResponse {
            probe: build_acp_bridge_probe(&status.installations, &status.acp_registry, agent_id),
            bridge: status.acp_bridge,
            runtime: self.devin_acp_runtime.status(),
        }
    }

    pub fn install_devin_acp_bridge_response(&self) -> Result<DevinAcpInstallResponse> {
        let install = install_looper_acp_agent_for_home(
            &self.config.home_path,
            &crate::runtime::default_server_base_url(),
        )?;
        let status = inspect_devin_desktop_for_home(&self.config.home_path);
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(DevinAcpInstallResponse {
            install,
            bridge: status.acp_bridge,
            runtime: self.devin_acp_runtime.status(),
        })
    }

    pub fn create_devin_acp_control_session_response(
        &self,
        cwd: Option<String>,
    ) -> Option<DevinAcpControlSessionResponse> {
        self.create_acp_client_host_control_session_response(DEVIN_ACP_CLIENT_HOST_ID, cwd)
    }

    pub fn prompt_devin_acp_control_session_response(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<DevinAcpControlPromptResponse, DevinAcpControlError> {
        self.prompt_acp_client_host_control_session_response(
            DEVIN_ACP_CLIENT_HOST_ID,
            session_id,
            prompt,
        )
    }

    pub fn cancel_devin_acp_control_session_response(
        &self,
        session_id: &str,
    ) -> Result<DevinAcpControlCancelResponse, DevinAcpControlError> {
        self.cancel_acp_client_host_control_session_response(DEVIN_ACP_CLIENT_HOST_ID, session_id)
    }

    pub fn create_acp_client_host_control_session_response(
        &self,
        client_id: &str,
        cwd: Option<String>,
    ) -> Option<LooperAcpControlSessionResponse> {
        let runtime = self.acp_runtime_for_client(client_id)?;
        let session = runtime.create_control_session(normalized_optional_text(cwd));
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.emit_acp_session_changed(&session.public_thread_id, "session-created");
        Some(LooperAcpControlSessionResponse {
            session,
            runtime: runtime.status(),
        })
    }

    pub fn observe_acp_client_host_session_response(
        &self,
        client_id: &str,
        input: LooperAcpObservedSession,
    ) -> Option<LooperAcpControlSessionResponse> {
        let runtime = self.acp_runtime_for_client(client_id)?;
        let session = runtime.observe_session(input);
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.emit_acp_session_changed(&session.public_thread_id, "session-observed");
        Some(LooperAcpControlSessionResponse {
            session,
            runtime: runtime.status(),
        })
    }

    pub fn prompt_acp_client_host_control_session_response(
        &self,
        client_id: &str,
        session_id: &str,
        prompt: &str,
    ) -> Result<LooperAcpControlPromptResponse, LooperAcpControlError> {
        let runtime = self
            .acp_runtime_for_client(client_id)
            .ok_or(LooperAcpControlError::SessionNotFound)?;
        let result = runtime.prompt_control_session(session_id, prompt)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.emit_mobile_session_event(
            MobileEventInput {
                kind: MobileEventKind::PromptDelivered,
                thread_id: Some(result.public_thread_id.clone()),
                prompt_id: Some(result.prompt_id.clone()),
                detail: Some(format!("{client_id}-acp-control")),
            },
            &result.public_thread_id,
        );
        self.emit_acp_session_changed(&result.public_thread_id, "prompt-delivered");
        Ok(LooperAcpControlPromptResponse {
            prompt_id: result.prompt_id,
            delivered_to_connection: result.delivered_to_connection,
            session: result.session,
            runtime: runtime.status(),
        })
    }

    pub fn cancel_acp_client_host_control_session_response(
        &self,
        client_id: &str,
        session_id: &str,
    ) -> Result<LooperAcpControlCancelResponse, LooperAcpControlError> {
        let runtime = self
            .acp_runtime_for_client(client_id)
            .ok_or(LooperAcpControlError::SessionNotFound)?;
        let result = runtime.cancel_control_session(session_id)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.emit_mobile_session_event(
            MobileEventInput {
                kind: MobileEventKind::LifecycleChanged,
                thread_id: Some(result.public_thread_id.clone()),
                prompt_id: None,
                detail: Some(format!("{client_id}-acp-cancelled")),
            },
            &result.public_thread_id,
        );
        self.emit_acp_session_changed(&result.public_thread_id, "session-cancelled");
        Ok(LooperAcpControlCancelResponse {
            session: result.session,
            runtime: runtime.status(),
        })
    }

    fn emit_acp_session_changed(&self, thread_id: &str, detail: &str) {
        self.emit_mobile_session_event(
            MobileEventInput {
                kind: MobileEventKind::SessionChanged,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: None,
                detail: Some(detail.to_owned()),
            },
            thread_id,
        );
    }
}

fn normalized_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

pub use crate::acp::runtime::{
    LOOPER_ACP_AGENT_ID, LOOPER_ACP_AGENT_NAME,
    LooperAcpControlCancelResponse as DevinAcpControlCancelResponse,
    LooperAcpControlError as DevinAcpControlError,
    LooperAcpControlPromptResponse as DevinAcpControlPromptResponse,
    LooperAcpControlSessionResponse as DevinAcpControlSessionResponse,
    LooperAcpDeliveredPrompt as DevinAcpDeliveredPrompt, LooperAcpRuntime as DevinAcpRuntime,
    LooperAcpRuntimeSession as DevinAcpRuntimeSession,
    LooperAcpRuntimeStatus as DevinAcpRuntimeStatus, acp_session_id_for_client_public_thread_id,
    public_thread_id_for_client_acp_session,
};

pub const LOOPER_ACP_ROUTE: &str = "/acp/client-hosts/devin";
pub const LEGACY_LOOPER_ACP_ROUTE: &str = "/acp/devin";
const DEVIN_ACP_CLIENT_ID: &str = "devin";

pub fn websocket_url_for_base_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let websocket_base_url = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{base_url}")
    };
    format!("{websocket_base_url}{LOOPER_ACP_ROUTE}")
}

pub fn public_thread_id_for_acp_session(session_id: &str) -> String {
    public_thread_id_for_client_acp_session(DEVIN_ACP_CLIENT_ID, session_id)
}

pub fn acp_session_id_for_public_thread_id(thread_id: &str) -> Option<String> {
    acp_session_id_for_client_public_thread_id(DEVIN_ACP_CLIENT_ID, thread_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_acp_sessions_to_public_devin_threads() {
        assert_eq!(
            public_thread_id_for_acp_session("acp/looper/123"),
            "devin:looper:123"
        );
        assert_eq!(
            acp_session_id_for_public_thread_id("devin:looper:123").as_deref(),
            Some("acp/looper/123")
        );
    }

    #[test]
    fn builds_devin_websocket_url() {
        assert_eq!(
            websocket_url_for_base_url("http://127.0.0.1:8765"),
            "ws://127.0.0.1:8765/acp/client-hosts/devin"
        );
        assert_eq!(
            websocket_url_for_base_url("https://looper.test/"),
            "wss://looper.test/acp/client-hosts/devin"
        );
    }
}

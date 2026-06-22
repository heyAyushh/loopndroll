use crate::acp::runtime::{LooperAcpRuntimeSession, public_agent_id_for_client_agent_id};

use super::ZED_ACP_CLIENT_HOST_ID;
use crate::acp::client_host::AcpClientHostSession;

pub(super) fn zed_acp_client_host_session(
    session: &LooperAcpRuntimeSession,
) -> AcpClientHostSession {
    let public_agent_id =
        public_agent_id_for_client_agent_id(ZED_ACP_CLIENT_HOST_ID, &session.agent_id);
    AcpClientHostSession {
        thread_id: session.public_thread_id.clone(),
        session_id: session.session_id.clone(),
        provider_id: public_agent_id.clone(),
        title: Some(zed_public_agent_title(&public_agent_id)),
        cwd: session.cwd.clone(),
        status: if session.cancelled {
            "stopped"
        } else {
            "active"
        }
        .to_owned(),
        archived: false,
        updated_at_ms: Some(session.updated_at_ms),
    }
}

fn zed_public_agent_title(public_agent_id: &str) -> String {
    match public_agent_id {
        "codex" => "Codex".to_owned(),
        "codex-direct" => "Codex Direct".to_owned(),
        "looper" => "Looper".to_owned(),
        _ => public_agent_id.to_owned(),
    }
}

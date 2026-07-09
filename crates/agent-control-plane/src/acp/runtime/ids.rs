pub use crate::entity_id::{
    acp_agent_and_session_id_for_client_public_thread_id,
    acp_session_id_for_client_public_thread_id, public_agent_id_for_client_agent_id,
    public_thread_id_for_client_acp_session, public_thread_id_for_client_agent_acp_session,
};

pub(super) fn runtime_session_key(client_id: &str, session_id: &str) -> String {
    acp_agent_and_session_id_for_client_public_thread_id(client_id, session_id)
        .map(|(_, session_id)| session_id)
        .or_else(|| acp_session_id_for_client_public_thread_id(client_id, session_id))
        .unwrap_or_else(|| session_id.to_owned())
}

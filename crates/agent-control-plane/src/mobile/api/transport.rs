use crate::acp::runtime::acp_agent_and_session_id_for_client_public_thread_id;
use crate::control_plane::DesktopThread;
use crate::devin::{
    DevinPromptTransport, DevinThreadIdentity, devin_prompt_transport_for_provider,
    devin_thread_identity_from_public_thread_id,
};
use crate::zed::ZED_CLIENT_ID;

pub(super) fn devin_prompt_transport(
    thread: &DesktopThread,
) -> Option<(DevinPromptTransport, String)> {
    let DevinThreadIdentity {
        provider_id,
        session_id,
    } = devin_thread_identity_from_public_thread_id(&thread.thread_id)?;
    let transport = devin_prompt_transport_for_provider(&provider_id)?;
    Some((transport, session_id))
}

pub(super) fn zed_acp_session_id(thread: &DesktopThread) -> Option<String> {
    acp_agent_and_session_id_for_client_public_thread_id(ZED_CLIENT_ID, &thread.thread_id)
        .map(|(_agent_id, session_id)| session_id)
}

use super::{
    LOOPER_ACP_AGENT_ID, LOOPER_SESSION_PREFIX, ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS,
    ZED_CODEX_ACP_AGENT_ID, ZED_CODEX_DIRECT_AGENT_ID, ZED_CODEX_DIRECT_PUBLIC_AGENT_ID,
    ZED_CODEX_PUBLIC_AGENT_ID,
};

pub fn public_thread_id_for_client_acp_session(client_id: &str, session_id: &str) -> String {
    public_thread_id_for_client_agent_acp_session(client_id, LOOPER_ACP_AGENT_ID, session_id)
}

pub fn public_thread_id_for_client_agent_acp_session(
    client_id: &str,
    agent_id: &str,
    session_id: &str,
) -> String {
    let zed_looper_control_session =
        client_id == ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS && agent_id == LOOPER_ACP_AGENT_ID;
    let public_agent_id = if zed_looper_control_session {
        ZED_CODEX_PUBLIC_AGENT_ID.to_owned()
    } else {
        public_agent_id_for_client_agent_id(client_id, agent_id)
    };
    let normalized_session_id = if zed_looper_control_session {
        session_id.strip_prefix("acp/").unwrap_or(session_id)
    } else if agent_id == LOOPER_ACP_AGENT_ID {
        session_id
            .strip_prefix(LOOPER_SESSION_PREFIX)
            .unwrap_or(session_id)
    } else {
        session_id.strip_prefix("acp/").unwrap_or(session_id)
    }
    .replace('/', ":");
    format!("{client_id}:{public_agent_id}:{normalized_session_id}")
}

pub fn public_agent_id_for_client_agent_id(client_id: &str, agent_id: &str) -> String {
    match (client_id, agent_id) {
        (ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS, ZED_CODEX_ACP_AGENT_ID) => {
            ZED_CODEX_PUBLIC_AGENT_ID.to_owned()
        }
        (ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS, ZED_CODEX_DIRECT_AGENT_ID) => {
            ZED_CODEX_DIRECT_PUBLIC_AGENT_ID.to_owned()
        }
        _ => agent_id.to_owned(),
    }
}

fn internal_agent_id_for_client_public_agent_id(client_id: &str, agent_id: &str) -> String {
    match (client_id, agent_id) {
        (ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS, ZED_CODEX_PUBLIC_AGENT_ID) => {
            ZED_CODEX_ACP_AGENT_ID.to_owned()
        }
        (ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS, ZED_CODEX_DIRECT_PUBLIC_AGENT_ID) => {
            ZED_CODEX_DIRECT_AGENT_ID.to_owned()
        }
        _ => agent_id.to_owned(),
    }
}

pub fn acp_session_id_for_client_public_thread_id(
    client_id: &str,
    thread_id: &str,
) -> Option<String> {
    let legacy_prefix = format!("{client_id}:{LOOPER_ACP_AGENT_ID}:");
    if let Some(remainder) = thread_id.strip_prefix(&legacy_prefix) {
        return Some(looper_acp_session_id_from_public_remainder(remainder));
    }

    if client_id == ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS {
        let zed_codex_looper_prefix =
            format!("{client_id}:{ZED_CODEX_PUBLIC_AGENT_ID}:{LOOPER_ACP_AGENT_ID}:");
        if let Some(remainder) = thread_id.strip_prefix(&zed_codex_looper_prefix) {
            return Some(looper_acp_session_id_from_public_remainder(remainder));
        }
    }

    None
}

fn looper_acp_session_id_from_public_remainder(remainder: &str) -> String {
    format!("{LOOPER_SESSION_PREFIX}{}", remainder.replace(':', "/"))
}

pub fn acp_agent_and_session_id_for_client_public_thread_id(
    client_id: &str,
    thread_id: &str,
) -> Option<(String, String)> {
    let prefix = format!("{client_id}:");
    let remainder = thread_id.strip_prefix(&prefix)?;
    let (public_agent_id, normalized_session_id) = remainder.split_once(':')?;
    let agent_id = internal_agent_id_for_client_public_agent_id(client_id, public_agent_id);
    let session_id = if agent_id == LOOPER_ACP_AGENT_ID {
        format!(
            "{LOOPER_SESSION_PREFIX}{}",
            normalized_session_id.replace(':', "/")
        )
    } else if let Some(looper_session_id) = normalized_session_id.strip_prefix("looper:") {
        format!(
            "{LOOPER_SESSION_PREFIX}{}",
            looper_session_id.replace(':', "/")
        )
    } else {
        normalized_session_id.replace(':', "/")
    };
    Some((agent_id, session_id))
}

pub(super) fn runtime_session_key(client_id: &str, session_id: &str) -> String {
    acp_agent_and_session_id_for_client_public_thread_id(client_id, session_id)
        .map(|(_, session_id)| session_id)
        .or_else(|| acp_session_id_for_client_public_thread_id(client_id, session_id))
        .unwrap_or_else(|| session_id.to_owned())
}

use super::super::*;

const ZED_ACP_CONNECTION_ID: &str = "zed-acp";
const ZED_ACP_CONNECTION_LABEL: &str = "Zed ACP";
const ZED_ACP_CONNECTION_ACTION_HINT: &str = "Zed External Agents are configured in ~/.zed/settings.json or ~/.config/zed/settings.json agent_servers; Looper controls wrapped targets such as Codex.";

pub(in crate::control_plane) fn zed_acp_connection(status: &ZedStatus) -> ManagedConnection {
    let connection_status = if status.running {
        "connected"
    } else if status.settings_exists || !status.acp_targets.is_empty() {
        "configured"
    } else if status.installed {
        "installed"
    } else {
        "missing"
    };
    ManagedConnection {
        id: ZED_ACP_CONNECTION_ID.to_owned(),
        kind: ZED_CLIENT_ID.to_owned(),
        label: ZED_ACP_CONNECTION_LABEL.to_owned(),
        status: connection_status.to_owned(),
        subtitle: Some(format!(
            "{} ACP target{}",
            status.acp_target_count,
            if status.acp_target_count == 1 {
                ""
            } else {
                "s"
            }
        )),
        detail: Some(status.summary.clone()),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(ZED_ACP_CONNECTION_ACTION_HINT.to_owned()),
    }
}

pub(in crate::control_plane) fn devin_acp_runtime_session_to_desktop_thread(
    session: &DevinAcpRuntimeSession,
) -> DesktopThread {
    acp_runtime_session_to_desktop_thread(
        session,
        "devin-desktop",
        "Devin Next",
        AssistantKind::DevinDesktop,
    )
}

pub(in crate::control_plane) fn zed_acp_runtime_session_to_desktop_thread(
    session: &LooperAcpRuntimeSession,
) -> DesktopThread {
    let mut thread = acp_runtime_session_to_desktop_thread(
        session,
        ZED_CLIENT_ID,
        ZED_CLIENT_NAME,
        AssistantKind::Zed,
    );
    if let Some(agent_id) = zed_public_agent_id_from_thread_id(&thread.thread_id) {
        thread.title = Some(zed_public_agent_title(agent_id));
        thread.agent_nickname = Some(agent_id.to_owned());
        thread.agent_role = Some(agent_id.to_owned());
    }
    if session.connection_id == LOCAL_CONTROL_CONNECTION_ID {
        thread.runtime_status = Some("stopped".to_owned());
    }
    thread
}

pub(in crate::control_plane) fn active_zed_acp_runtime_session_count(
    sessions: &[LooperAcpRuntimeSession],
) -> usize {
    sessions
        .iter()
        .filter(|session| {
            !session.cancelled && session.connection_id != LOCAL_CONTROL_CONNECTION_ID
        })
        .count()
}

fn acp_runtime_session_to_desktop_thread(
    session: &LooperAcpRuntimeSession,
    source: &str,
    originator: &str,
    assistant_kind: AssistantKind,
) -> DesktopThread {
    DesktopThread {
        thread_id: session.public_thread_id.clone(),
        title: Some("Looper ACP".to_owned()),
        cwd: session.cwd.clone(),
        transcript_path: None,
        source: Some(source.to_owned()),
        originator: Some(originator.to_owned()),
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: Some("Looper".to_owned()),
        agent_role: Some("looper".to_owned()),
        agent_path: None,
        created_at_ms: Some(session.created_at_ms),
        updated_at_ms: Some(session.updated_at_ms),
        latest_message_at_ms: Some(session.updated_at_ms),
        assistant_preview: session.latest_assistant_message.clone(),
        latest_assistant_message_full: session.latest_assistant_message.clone(),
        first_user_prompt: None,
        runtime_status: Some(if session.cancelled {
            "stopped".to_owned()
        } else {
            "active".to_owned()
        }),
        archived: false,
        goal: None,
        capabilities: acp_runtime_session_capabilities(session, assistant_kind),
    }
}

pub(in crate::control_plane) fn devin_acp_runtime_session_capabilities(
    session: &DevinAcpRuntimeSession,
) -> ThreadCapabilities {
    acp_runtime_session_capabilities(session, AssistantKind::DevinDesktop)
}

pub(in crate::control_plane) fn zed_acp_runtime_session_capabilities(
    session: &LooperAcpRuntimeSession,
) -> ThreadCapabilities {
    acp_runtime_session_capabilities(session, AssistantKind::Zed)
}

fn acp_runtime_session_capabilities(
    session: &LooperAcpRuntimeSession,
    assistant_kind: AssistantKind,
) -> ThreadCapabilities {
    let public_agent_id = public_agent_id_for_runtime_session(session);
    ThreadCapabilities {
        thread_id: session.public_thread_id.clone(),
        assistant_kind,
        tools: Vec::new(),
        mcp_tools: Vec::new(),
        app_tools: Vec::new(),
        automation_tools: Vec::new(),
        spawn: SpawnGraph {
            parent_thread_id: None,
            root_thread_id: session.public_thread_id.clone(),
            children: Vec::new(),
            launch_kind: LaunchKind::Main,
        },
        diff: DiffSummary {
            git_branch: None,
            git_sha: None,
            produced_file_changes: false,
            paths: Vec::new(),
        },
        agent_nickname: Some(public_agent_id.clone()),
        agent_role: Some(public_agent_id),
        agent_path: None,
    }
}

fn public_agent_id_for_runtime_session(session: &LooperAcpRuntimeSession) -> String {
    let Some((client_id, remainder)) = session.public_thread_id.split_once(':') else {
        return session.agent_id.clone();
    };
    let Some((public_agent_id, _)) = remainder.split_once(':') else {
        return session.agent_id.clone();
    };
    match public_agent_id {
        CODEX_ACP_SOURCE => public_agent_id_for_client_agent_id(client_id, public_agent_id),
        _ => public_agent_id.to_owned(),
    }
}

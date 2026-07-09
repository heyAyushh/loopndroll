use crate::assistant::AssistantKind;
use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};
use crate::control_plane::DesktopThread;
use crate::mobile::session::{MobileSessionLifecycle, MobileSessionState};

pub(super) fn session_state_with_lifecycle(thread_id: &str, status: &str) -> MobileSessionState {
    let mut session_state = MobileSessionState::default();
    session_state.lifecycle.insert(
        thread_id.to_owned(),
        MobileSessionLifecycle {
            status: status.to_owned(),
            updated_at: "2026-06-07T00:00:00Z".to_owned(),
        },
    );
    session_state
}

pub(super) fn test_thread(
    thread_id: &str,
    assistant_kind: AssistantKind,
    runtime_status: Option<&str>,
) -> DesktopThread {
    DesktopThread {
        thread_id: thread_id.to_owned(),
        title: Some("Test session".to_owned()),
        cwd: None,
        transcript_path: None,
        source: None,
        originator: None,
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: None,
        agent_role: None,
        agent_path: None,
        created_at_ms: Some(1),
        updated_at_ms: Some(2),
        latest_message_at_ms: Some(3),
        assistant_preview: None,
        latest_assistant_message_full: None,
        first_user_prompt: None,
        runtime_status: runtime_status.map(str::to_owned),
        archived: false,
        goal: None,
        capabilities: ThreadCapabilities {
            thread_id: thread_id.to_owned(),
            assistant_kind,
            tools: Vec::new(),
            mcp_tools: Vec::new(),
            app_tools: Vec::new(),
            automation_tools: Vec::new(),
            spawn: SpawnGraph {
                parent_thread_id: None,
                root_thread_id: thread_id.to_owned(),
                children: Vec::new(),
                launch_kind: LaunchKind::Main,
            },
            diff: DiffSummary {
                git_branch: None,
                git_sha: None,
                produced_file_changes: false,
                paths: Vec::new(),
            },
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
        },
    }
}

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::error::ClientCoreError;
use crate::model::{ClientPendingCommand, ClientPendingCommandKind, ClientStateMini};

const DEFAULT_ASSISTANT_SURFACE: &str = "codex";
const DEFAULT_PROMPT: &str = "Continue";
const DEFAULT_SESSION_STATUS: &str = "stopped";
const GLOBAL_SCOPE: &str = "global";
const HOST_ID: &str = "local-session-mini-cache";
const HOST_NAME: &str = "Looper";
const REVISION_PREFIX: &str = "mini:";
const REVISION_SURFACE_FIELD_PREFIX: &str = "surface=";
const STATUS_ARCHIVED: &str = "archived";

const KNOWN_ASSISTANT_SURFACES: [&str; 5] = ["codex", "claude-code", "devin", "grok-build", "zed"];
const CODEX_SURFACE_ASSISTANT_CLIENTS: [&str; 4] = [
    DEFAULT_ASSISTANT_SURFACE,
    "cursor",
    "super-engineering",
    "openclaw",
];

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSnapshotProjection {
    pub has_snapshot: bool,
    pub snapshot: ClientMobileSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSnapshot {
    pub revision: String,
    pub has_revision: bool,
    pub host: ClientMobileHost,
    pub global_settings: ClientMobileGlobalSettings,
    pub sessions: Vec<ClientMobileSession>,
    pub surface_sessions: Vec<ClientMobileSurfaceSessions>,
    pub notifications: Vec<ClientMobileNotificationDestination>,
    pub completion_checks: Vec<ClientMobileCompletionCheckSummary>,
    pub work_status: ClientMobileWorkStatusSummary,
    pub devin_desktop: ClientMobileDevinDesktopStatus,
    pub has_devin_desktop: bool,
    pub grok_build: ClientMobileGrokBuildStatus,
    pub has_grok_build: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub grpc_address: String,
    pub grpc_addresses: Vec<String>,
    pub is_reachable: bool,
    pub last_synced_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileGlobalSettings {
    pub default_prompt: String,
    pub global_mode: String,
    pub has_global_mode: bool,
    pub scope: String,
    pub notification_label: String,
    pub has_notification_label: bool,
    pub completion_check_label: String,
    pub has_completion_check_label: bool,
    pub completion_check_wait_for_reply: bool,
    pub assistant_surface: String,
    pub siri_default_session_id: String,
    pub has_siri_default_session_id: bool,
    pub siri_default_assistant_surface: String,
    pub has_siri_default_assistant_surface: bool,
    pub siri_current_session_id: String,
    pub has_siri_current_session_id: bool,
    pub siri_current_assistant_surface: String,
    pub has_siri_current_assistant_surface: bool,
    pub siri_current_updated_at_ms: i64,
    pub has_siri_current_updated_at_ms: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSurfaceSessions {
    pub surface: String,
    pub sessions: Vec<ClientMobileSession>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSession {
    pub id: String,
    pub ref_id: String,
    pub title: String,
    pub status: String,
    pub effective_mode: String,
    pub has_effective_mode: bool,
    pub last_updated_at: String,
    pub created_at_ms: i64,
    pub has_created_at_ms: bool,
    pub updated_at_ms: i64,
    pub has_updated_at_ms: bool,
    pub latest_message_at_ms: i64,
    pub has_latest_message_at_ms: bool,
    pub last_activity_at_ms: i64,
    pub has_last_activity_at_ms: bool,
    pub last_activity_at: String,
    pub last_message_at_ms: i64,
    pub has_last_message_at_ms: bool,
    pub last_message_at: String,
    pub has_last_message_at: bool,
    pub assistant_preview: String,
    pub has_assistant_preview: bool,
    pub is_archived: bool,
    pub can_send_prompt: bool,
    pub prompt_delivery_unavailable_reason: String,
    pub has_prompt_delivery_unavailable_reason: bool,
    pub assistant_client: String,
    pub goal: ClientMobileSessionGoal,
    pub has_goal: bool,
    pub metadata: ClientMobileSessionMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSessionGoal {
    pub id: String,
    pub title: String,
    pub status: String,
    pub lifecycle: String,
    pub running: bool,
    pub token_budget: i64,
    pub has_token_budget: bool,
    pub tokens_used: i64,
    pub has_tokens_used: bool,
    pub time_used_seconds: i64,
    pub has_time_used_seconds: bool,
    pub updated_at_ms: i64,
    pub has_updated_at_ms: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSessionMetadata {
    pub kind: String,
    pub source: String,
    pub source_display_name: String,
    pub assistant_kind: String,
    pub has_assistant_kind: bool,
    pub originator: String,
    pub has_originator: bool,
    pub project_name: String,
    pub has_project_name: bool,
    pub project_path: String,
    pub has_project_path: bool,
    pub task_kind: String,
    pub transcript_available: bool,
    pub git_repository: ClientMobileGitRepositoryMetadata,
    pub has_git_repository: bool,
    pub pull_request_url: String,
    pub has_pull_request_url: bool,
    pub supports_subagents: bool,
    pub spawn: ClientMobileSessionSpawnMetadata,
    pub has_spawn: bool,
    pub installed_plugins: Vec<ClientMobileInstalledPluginSummary>,
    pub sources: Vec<ClientMobileSessionSourceReference>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileGitRepositoryMetadata {
    pub repository_name: String,
    pub repository_path: String,
    pub remote_url: String,
    pub has_remote_url: bool,
    pub branch: String,
    pub has_branch: bool,
    pub commit: String,
    pub has_commit: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSessionSpawnMetadata {
    pub parent_thread_id: String,
    pub has_parent_thread_id: bool,
    pub root_thread_id: String,
    pub has_root_thread_id: bool,
    pub children: Vec<String>,
    pub launch_kind: String,
    pub has_launch_kind: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileInstalledPluginSummary {
    pub id: String,
    pub name: String,
    pub source: String,
    pub has_source: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSessionSourceReference {
    pub kind: String,
    pub label: String,
    pub value: String,
    pub url: String,
    pub has_url: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileNotificationDestination {
    pub id: String,
    pub label: String,
    pub channel: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileCompletionCheckSummary {
    pub id: String,
    pub label: String,
    pub command_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileWorkStatusSummary {
    pub goal_count: i64,
    pub running_goal_count: i64,
    pub automation_count: i64,
    pub active_automation_count: i64,
    pub covered_automation_count: i64,
    pub running_goals: Vec<ClientMobileWorkStatusGoal>,
    pub active_automations: Vec<ClientMobileWorkStatusAutomation>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileWorkStatusGoal {
    pub id: String,
    pub title: String,
    pub status: String,
    pub target_thread_id: String,
    pub has_target_thread_id: bool,
    pub target_known: bool,
    pub updated_at_ms: i64,
    pub has_updated_at_ms: bool,
    pub tokens_used: i64,
    pub has_tokens_used: bool,
    pub token_budget: i64,
    pub has_token_budget: bool,
    pub time_used_seconds: i64,
    pub has_time_used_seconds: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileWorkStatusAutomation {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub schedule_summary: String,
    pub target_thread_id: String,
    pub has_target_thread_id: bool,
    pub target_known: bool,
    pub control_plane_covered: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileDevinDesktopStatus {
    pub running: bool,
    pub installed: bool,
    pub acp_available: bool,
    pub registry_exists: bool,
    pub registry_agent_count: i64,
    pub enabled_agent_count: i64,
    pub preferred_agent_ids: Vec<String>,
    pub session_count: i64,
    pub active_session_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileGrokBuildStatus {
    pub hooks: ClientMobileGrokBuildHookStatus,
    pub session_count: i64,
    pub active_session_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileGrokBuildHookStatus {
    pub health: String,
    pub owner: String,
    pub registered_events: Vec<String>,
    pub hooks_path: String,
    pub has_hooks_path: bool,
}

#[uniffi::export]
pub fn reduce_state_minis_mobile_snapshot(
    latest_seq: i64,
    sessions: Vec<ClientStateMini>,
    server_time: String,
) -> Result<ClientMobileSnapshotProjection, ClientCoreError> {
    let sessions = decodable_sessions(&sessions);
    if sessions.is_empty() {
        return Ok(ClientMobileSnapshotProjection {
            has_snapshot: false,
            snapshot: ClientMobileSnapshot::empty(),
        });
    }

    let mut sessions_by_surface: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut latest_global_settings: Option<(i64, Value)> = None;
    for (mini, session) in &sessions {
        if let Some(settings) = session.get("globalSettings").cloned() {
            if latest_global_settings
                .as_ref()
                .map(|(seq, _)| mini.seq >= *seq)
                .unwrap_or(true)
            {
                latest_global_settings = Some((mini.seq, settings));
            }
        }
        sessions_by_surface
            .entry(surface_bucket_for_assistant_client(&mini.assistant_surface).to_owned())
            .or_default()
            .push(session.clone());
    }
    for sessions in sessions_by_surface.values_mut() {
        sort_sessions_by_freshness(sessions);
    }

    let selected_surface = selected_surface(&sessions);
    let visible_sessions = sessions_by_surface
        .get(&selected_surface)
        .cloned()
        .unwrap_or_default();
    let global_settings = global_settings(
        latest_global_settings.map(|(_, settings)| settings),
        &selected_surface,
    );

    let snapshot = json!({
        "revision": revision(latest_seq, &sessions),
        "host": {
            "id": HOST_ID,
            "name": HOST_NAME,
            "address": "",
            "isReachable": false,
            "lastSyncedAt": server_time,
        },
        "globalSettings": global_settings,
        "sessions": visible_sessions,
        "surfaceSessions": sessions_by_surface,
        "notifications": [],
        "completionChecks": [],
    });

    Ok(ClientMobileSnapshotProjection {
        has_snapshot: true,
        snapshot: ClientMobileSnapshot::from_value(snapshot)?,
    })
}

#[uniffi::export]
pub fn reduce_state_minis_mobile_snapshot_with_pending_commands(
    latest_seq: i64,
    sessions: Vec<ClientStateMini>,
    pending_commands: Vec<ClientPendingCommand>,
    server_time: String,
) -> Result<ClientMobileSnapshotProjection, ClientCoreError> {
    let mut projection = reduce_state_minis_mobile_snapshot(latest_seq, sessions, server_time)?;
    if !projection.has_snapshot || pending_commands.is_empty() {
        return Ok(projection);
    }

    let mut snapshot = projection.snapshot.to_value();
    apply_pending_commands(&mut snapshot, &pending_commands);
    projection.snapshot = ClientMobileSnapshot::from_value(snapshot)?;
    Ok(projection)
}

fn apply_pending_commands(snapshot: &mut Value, commands: &[ClientPendingCommand]) {
    for command in commands {
        match command.kind {
            ClientPendingCommandKind::SetSessionMode => {
                apply_pending_mode(snapshot, &command.thread_id, &command.preset);
            }
            ClientPendingCommandKind::SetSiriCurrentSession => apply_pending_siri_session(
                snapshot,
                "siriCurrentSessionId",
                "siriCurrentAssistantSurface",
                &command.thread_id,
                &command.assistant_surface,
            ),
            ClientPendingCommandKind::SetSiriDefaultSession => apply_pending_siri_session(
                snapshot,
                "siriDefaultSessionId",
                "siriDefaultAssistantSurface",
                &command.thread_id,
                &command.assistant_surface,
            ),
            ClientPendingCommandKind::SaveDefaultPrompt => {
                apply_pending_default_prompt(snapshot, &command.prompt);
            }
            ClientPendingCommandKind::SetSessionArchived => {
                apply_pending_archive(snapshot, &command.thread_id, command.archived);
            }
            ClientPendingCommandKind::DeleteSession => {
                apply_pending_delete(snapshot, &command.thread_id);
            }
            ClientPendingCommandKind::SendSessionPrompt
            | ClientPendingCommandKind::SubmitNotificationReply
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SetDefaultNotificationTargets
            | ClientPendingCommandKind::MuteSession => {}
        }
    }
    refresh_visible_sessions_from_selected_surface(snapshot);
}

fn apply_pending_mode(snapshot: &mut Value, session_id: &str, preset: &str) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    let mode = pending_session_mode_value(preset);
    update_pending_sessions(snapshot, &session_id, &mut |session| {
        session.insert("effectiveMode".to_owned(), mode.clone());
    });
}

fn apply_pending_siri_session(
    snapshot: &mut Value,
    session_field: &str,
    surface_field: &str,
    session_id: &str,
    assistant_surface: &str,
) {
    let normalized_session_id = normalized_pending_text(session_id);
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    if normalized_session_id.is_empty() {
        settings.insert(session_field.to_owned(), Value::Null);
        settings.insert(surface_field.to_owned(), Value::Null);
        if session_field == "siriCurrentSessionId" {
            settings.insert("siriCurrentUpdatedAtMs".to_owned(), Value::Null);
        }
        return;
    }
    settings.insert(
        session_field.to_owned(),
        Value::String(normalized_session_id),
    );
    settings.insert(
        surface_field.to_owned(),
        pending_assistant_surface_value(assistant_surface),
    );
}

fn apply_pending_default_prompt(snapshot: &mut Value, prompt: &str) {
    let prompt = normalized_pending_text(prompt);
    if prompt.is_empty() {
        return;
    }
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    settings.insert("defaultPrompt".to_owned(), Value::String(prompt));
}

fn apply_pending_archive(snapshot: &mut Value, session_id: &str, archived: bool) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    update_pending_sessions(snapshot, &session_id, &mut |session| {
        session.insert("isArchived".to_owned(), Value::Bool(archived));
        if archived {
            session.insert(
                "status".to_owned(),
                Value::String(STATUS_ARCHIVED.to_owned()),
            );
        }
    });
}

fn apply_pending_delete(snapshot: &mut Value, session_id: &str) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    if let Some(surface_sessions) = snapshot
        .get_mut("surfaceSessions")
        .and_then(Value::as_object_mut)
    {
        for sessions in surface_sessions.values_mut() {
            remove_session_from_array(sessions, &session_id);
        }
    } else if let Some(sessions) = snapshot.get_mut("sessions") {
        remove_session_from_array(sessions, &session_id);
    }
    clear_pending_siri_targets(snapshot, &session_id);
}

fn update_pending_sessions(
    snapshot: &mut Value,
    session_id: &str,
    mutate: &mut impl FnMut(&mut serde_json::Map<String, Value>),
) {
    if let Some(surface_sessions) = snapshot
        .get_mut("surfaceSessions")
        .and_then(Value::as_object_mut)
    {
        for sessions in surface_sessions.values_mut() {
            update_session_array(sessions, session_id, mutate);
        }
        return;
    }

    if let Some(sessions) = snapshot.get_mut("sessions") {
        update_session_array(sessions, session_id, mutate);
    }
}

fn update_session_array(
    sessions: &mut Value,
    session_id: &str,
    mutate: &mut impl FnMut(&mut serde_json::Map<String, Value>),
) {
    let Some(sessions) = sessions.as_array_mut() else {
        return;
    };
    for session in sessions {
        let Some(session) = session.as_object_mut() else {
            continue;
        };
        if session
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == session_id)
        {
            mutate(session);
        }
    }
}

fn remove_session_from_array(sessions: &mut Value, session_id: &str) {
    let Some(sessions) = sessions.as_array_mut() else {
        return;
    };
    sessions.retain(|session| {
        session
            .get("id")
            .and_then(Value::as_str)
            .map(|id| id != session_id)
            .unwrap_or(true)
    });
}

fn clear_pending_siri_targets(snapshot: &mut Value, session_id: &str) {
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    if settings
        .get("siriCurrentSessionId")
        .and_then(Value::as_str)
        .is_some_and(|current| current == session_id)
    {
        settings.insert("siriCurrentSessionId".to_owned(), Value::Null);
        settings.insert("siriCurrentAssistantSurface".to_owned(), Value::Null);
        settings.insert("siriCurrentUpdatedAtMs".to_owned(), Value::Null);
    }
    if settings
        .get("siriDefaultSessionId")
        .and_then(Value::as_str)
        .is_some_and(|default| default == session_id)
    {
        settings.insert("siriDefaultSessionId".to_owned(), Value::Null);
        settings.insert("siriDefaultAssistantSurface".to_owned(), Value::Null);
    }
}

fn refresh_visible_sessions_from_selected_surface(snapshot: &mut Value) {
    let selected_surface = snapshot
        .get("globalSettings")
        .and_then(|settings| settings.get("assistantSurface"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_ASSISTANT_SURFACE)
        .to_owned();
    let Some(surface_sessions) = snapshot
        .get("surfaceSessions")
        .and_then(Value::as_object)
        .filter(|surface_sessions| !surface_sessions.is_empty())
    else {
        return;
    };
    let sessions = surface_sessions
        .get(&selected_surface)
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    if let Some(snapshot) = snapshot.as_object_mut() {
        snapshot.insert("sessions".to_owned(), sessions);
    }
}

fn global_settings_mut(snapshot: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    snapshot
        .get_mut("globalSettings")
        .and_then(Value::as_object_mut)
}

fn pending_session_mode_value(preset: &str) -> Value {
    let preset = normalized_pending_text(preset);
    if is_known_session_mode(&preset) {
        Value::String(preset)
    } else {
        Value::Null
    }
}

fn pending_assistant_surface_value(assistant_surface: &str) -> Value {
    let assistant_surface = normalized_pending_text(assistant_surface);
    if is_known_assistant_surface(&assistant_surface) {
        Value::String(assistant_surface)
    } else {
        Value::Null
    }
}

fn is_known_session_mode(mode: &str) -> bool {
    matches!(
        mode,
        "infinite"
            | "await-reply"
            | "completion-checks"
            | "max-turns-1"
            | "max-turns-2"
            | "max-turns-3"
    )
}

fn normalized_pending_text(value: &str) -> String {
    value.trim().to_owned()
}

fn decodable_sessions(sessions: &[ClientStateMini]) -> Vec<(&ClientStateMini, Value)> {
    sessions
        .iter()
        .filter_map(|mini| {
            decode_session_payload(mini)
                .ok()
                .map(|session| (mini, session))
        })
        .collect()
}

fn global_settings(settings: Option<Value>, selected_surface: &str) -> Value {
    let mut merged = json!({
        "defaultPrompt": DEFAULT_PROMPT,
        "scope": GLOBAL_SCOPE,
        "completionCheckWaitForReply": false,
        "assistantSurface": selected_surface,
    });
    let Some(Value::Object(settings)) = settings else {
        return merged;
    };
    let Some(merged_object) = merged.as_object_mut() else {
        return merged;
    };
    for (key, value) in settings {
        merged_object.insert(key, value);
    }
    if !merged_object
        .get("assistantSurface")
        .and_then(Value::as_str)
        .is_some_and(|surface| !surface.trim().is_empty())
    {
        merged_object.insert(
            "assistantSurface".to_owned(),
            Value::String(selected_surface.to_owned()),
        );
    }
    merged
}

fn decode_session_payload(mini: &ClientStateMini) -> Result<Value, ClientCoreError> {
    let mut session: Value = serde_json::from_str(&mini.payload_json)
        .map_err(|_| ClientCoreError::InvalidStateMiniPayloadJson)?;
    let session = session
        .as_object_mut()
        .ok_or(ClientCoreError::InvalidStateMiniPayloadJson)?;

    reject_mismatched_session_id(session.get("id"), &mini.session_id)?;
    reject_mismatched_session_id(session.get("sessionId"), &mini.session_id)?;
    session.insert("id".to_owned(), Value::String(mini.session_id.clone()));
    session.insert(
        "sessionId".to_owned(),
        Value::String(mini.session_id.clone()),
    );
    let assistant_client = fallback_assistant_client(&mini.assistant_surface);
    insert_string_default(session, "assistantSurface", assistant_client);
    insert_string_default(session, "assistantClient", assistant_client);
    insert_string_default(session, "ref", &mini.session_id);
    insert_string_default(session, "title", &mini.session_id);
    insert_string_default(session, "status", DEFAULT_SESSION_STATUS);
    insert_string_default(session, "lastUpdatedAt", "");
    insert_string_default(session, "lastActivityAt", "");
    insert_bool_default(session, "isArchived", false);
    insert_bool_default(session, "canSendPrompt", true);
    session
        .entry("metadata".to_owned())
        .or_insert_with(|| json!({}));
    repair_git_repository_metadata(session);

    Ok(Value::Object(session.clone()))
}

fn reject_mismatched_session_id(
    value: Option<&Value>,
    session_id: &str,
) -> Result<(), ClientCoreError> {
    let Some(value) = value.and_then(Value::as_str) else {
        return Ok(());
    };
    if value == session_id {
        Ok(())
    } else {
        Err(ClientCoreError::StateMiniSessionIdMismatch)
    }
}

fn insert_string_default(session: &mut serde_json::Map<String, Value>, key: &str, fallback: &str) {
    let needs_default = session
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if needs_default {
        session.insert(key.to_owned(), Value::String(fallback.to_owned()));
    }
}

fn insert_bool_default(session: &mut serde_json::Map<String, Value>, key: &str, fallback: bool) {
    if !session.get(key).is_some_and(Value::is_boolean) {
        session.insert(key.to_owned(), Value::Bool(fallback));
    }
}

fn repair_git_repository_metadata(session: &mut serde_json::Map<String, Value>) {
    let fallback_name = session
        .get("metadata")
        .and_then(|metadata| metadata.get("projectName"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            session
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        });
    let Some(repository) = session
        .get_mut("metadata")
        .and_then(|metadata| metadata.as_object_mut())
        .and_then(|metadata| metadata.get_mut("gitRepository"))
        .and_then(|repository| repository.as_object_mut())
    else {
        return;
    };
    insert_string_default(repository, "repositoryName", &fallback_name);
    insert_string_default(repository, "repositoryPath", "");
}

fn fallback_assistant_client(client: &str) -> &str {
    if client.trim().is_empty() {
        DEFAULT_ASSISTANT_SURFACE
    } else {
        client
    }
}

impl ClientMobileSnapshot {
    pub(crate) fn empty() -> Self {
        Self {
            revision: String::new(),
            has_revision: false,
            host: ClientMobileHost::empty(),
            global_settings: ClientMobileGlobalSettings::empty(),
            sessions: Vec::new(),
            surface_sessions: Vec::new(),
            notifications: Vec::new(),
            completion_checks: Vec::new(),
            work_status: ClientMobileWorkStatusSummary::empty(),
            devin_desktop: ClientMobileDevinDesktopStatus::empty(),
            has_devin_desktop: false,
            grok_build: ClientMobileGrokBuildStatus::empty(),
            has_grok_build: false,
        }
    }

    pub(crate) fn from_value(value: Value) -> Result<Self, ClientCoreError> {
        let object = value
            .as_object()
            .ok_or(ClientCoreError::InvalidSnapshotJson)?;
        let revision = object_string(object, "revision");
        let host = object
            .get("host")
            .and_then(Value::as_object)
            .map(ClientMobileHost::from_object)
            .unwrap_or_else(ClientMobileHost::empty);
        let global_settings = object
            .get("globalSettings")
            .and_then(Value::as_object)
            .map(ClientMobileGlobalSettings::from_object)
            .unwrap_or_else(ClientMobileGlobalSettings::empty);
        let sessions = object_array(object, "sessions")
            .into_iter()
            .filter_map(ClientMobileSession::from_value)
            .collect::<Vec<_>>();
        let surface_sessions = object
            .get("surfaceSessions")
            .and_then(Value::as_object)
            .map(|surfaces| {
                surfaces
                    .iter()
                    .map(|(surface, sessions)| ClientMobileSurfaceSessions {
                        surface: surface.clone(),
                        sessions: sessions
                            .as_array()
                            .map(|sessions| {
                                sessions
                                    .iter()
                                    .filter_map(ClientMobileSession::from_value)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let notifications = object_array(object, "notifications")
            .into_iter()
            .filter_map(ClientMobileNotificationDestination::from_value)
            .collect::<Vec<_>>();
        let completion_checks = object_array(object, "completionChecks")
            .into_iter()
            .filter_map(ClientMobileCompletionCheckSummary::from_value)
            .collect::<Vec<_>>();
        let work_status = object
            .get("workStatus")
            .and_then(Value::as_object)
            .map(ClientMobileWorkStatusSummary::from_object)
            .unwrap_or_else(ClientMobileWorkStatusSummary::empty);
        let devin_desktop = object
            .get("devinDesktop")
            .and_then(Value::as_object)
            .map(ClientMobileDevinDesktopStatus::from_object);
        let grok_build = object
            .get("grokBuild")
            .and_then(Value::as_object)
            .map(ClientMobileGrokBuildStatus::from_object);

        Ok(Self {
            revision: revision.clone().unwrap_or_default(),
            has_revision: revision.is_some(),
            host,
            global_settings,
            sessions,
            surface_sessions,
            notifications,
            completion_checks,
            work_status,
            devin_desktop: devin_desktop
                .clone()
                .unwrap_or_else(ClientMobileDevinDesktopStatus::empty),
            has_devin_desktop: devin_desktop.is_some(),
            grok_build: grok_build
                .clone()
                .unwrap_or_else(ClientMobileGrokBuildStatus::empty),
            has_grok_build: grok_build.is_some(),
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        insert_optional_string(&mut object, "revision", self.has_revision, &self.revision);
        object.insert("host".to_owned(), self.host.to_value());
        object.insert("globalSettings".to_owned(), self.global_settings.to_value());
        object.insert(
            "sessions".to_owned(),
            Value::Array(
                self.sessions
                    .iter()
                    .map(ClientMobileSession::to_value)
                    .collect(),
            ),
        );
        let surface_sessions = self
            .surface_sessions
            .iter()
            .map(|surface| {
                (
                    surface.surface.clone(),
                    Value::Array(
                        surface
                            .sessions
                            .iter()
                            .map(ClientMobileSession::to_value)
                            .collect(),
                    ),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        object.insert(
            "surfaceSessions".to_owned(),
            Value::Object(surface_sessions),
        );
        object.insert(
            "notifications".to_owned(),
            Value::Array(
                self.notifications
                    .iter()
                    .map(ClientMobileNotificationDestination::to_value)
                    .collect(),
            ),
        );
        object.insert(
            "completionChecks".to_owned(),
            Value::Array(
                self.completion_checks
                    .iter()
                    .map(ClientMobileCompletionCheckSummary::to_value)
                    .collect(),
            ),
        );
        object.insert("workStatus".to_owned(), self.work_status.to_value());
        if self.has_devin_desktop {
            object.insert("devinDesktop".to_owned(), self.devin_desktop.to_value());
        }
        if self.has_grok_build {
            object.insert("grokBuild".to_owned(), self.grok_build.to_value());
        }
        Value::Object(object)
    }
}

impl ClientMobileHost {
    fn empty() -> Self {
        Self {
            id: "rust-control-plane".to_owned(),
            name: HOST_NAME.to_owned(),
            address: String::new(),
            grpc_address: String::new(),
            grpc_addresses: Vec::new(),
            is_reachable: false,
            last_synced_at: String::new(),
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        Self {
            id: object_string(object, "id").unwrap_or_else(|| "rust-control-plane".to_owned()),
            name: object_string(object, "name").unwrap_or_else(|| HOST_NAME.to_owned()),
            address: object_string(object, "address").unwrap_or_default(),
            grpc_address: object_string(object, "grpcAddress").unwrap_or_default(),
            grpc_addresses: object_string_array(object, "grpcAddresses"),
            is_reachable: object_bool(object, "isReachable").unwrap_or(false),
            last_synced_at: object_string(object, "lastSyncedAt").unwrap_or_default(),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "address": self.address,
            "grpcAddress": self.grpc_address,
            "grpcAddresses": self.grpc_addresses,
            "isReachable": self.is_reachable,
            "lastSyncedAt": self.last_synced_at,
        })
    }
}

impl ClientMobileGlobalSettings {
    fn empty() -> Self {
        Self {
            default_prompt: String::new(),
            global_mode: String::new(),
            has_global_mode: false,
            scope: GLOBAL_SCOPE.to_owned(),
            notification_label: String::new(),
            has_notification_label: false,
            completion_check_label: String::new(),
            has_completion_check_label: false,
            completion_check_wait_for_reply: false,
            assistant_surface: DEFAULT_ASSISTANT_SURFACE.to_owned(),
            siri_default_session_id: String::new(),
            has_siri_default_session_id: false,
            siri_default_assistant_surface: String::new(),
            has_siri_default_assistant_surface: false,
            siri_current_session_id: String::new(),
            has_siri_current_session_id: false,
            siri_current_assistant_surface: String::new(),
            has_siri_current_assistant_surface: false,
            siri_current_updated_at_ms: 0,
            has_siri_current_updated_at_ms: false,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let global_mode = object_string(object, "globalMode");
        let notification_label = object_string(object, "notificationLabel");
        let completion_check_label = object_string(object, "completionCheckLabel");
        let siri_default_session_id = object_string(object, "siriDefaultSessionId");
        let siri_default_assistant_surface = object_string(object, "siriDefaultAssistantSurface");
        let siri_current_session_id = object_string(object, "siriCurrentSessionId");
        let siri_current_assistant_surface = object_string(object, "siriCurrentAssistantSurface");
        let siri_current_updated_at_ms = object_i64(object, "siriCurrentUpdatedAtMs");

        Self {
            default_prompt: object_string(object, "defaultPrompt").unwrap_or_default(),
            global_mode: global_mode.clone().unwrap_or_default(),
            has_global_mode: global_mode.is_some(),
            scope: object_string(object, "scope").unwrap_or_else(|| GLOBAL_SCOPE.to_owned()),
            notification_label: notification_label.clone().unwrap_or_default(),
            has_notification_label: notification_label.is_some(),
            completion_check_label: completion_check_label.clone().unwrap_or_default(),
            has_completion_check_label: completion_check_label.is_some(),
            completion_check_wait_for_reply: object_bool(object, "completionCheckWaitForReply")
                .unwrap_or(false),
            assistant_surface: object_string(object, "assistantSurface")
                .unwrap_or_else(|| DEFAULT_ASSISTANT_SURFACE.to_owned()),
            siri_default_session_id: siri_default_session_id.clone().unwrap_or_default(),
            has_siri_default_session_id: siri_default_session_id.is_some(),
            siri_default_assistant_surface: siri_default_assistant_surface
                .clone()
                .unwrap_or_default(),
            has_siri_default_assistant_surface: siri_default_assistant_surface.is_some(),
            siri_current_session_id: siri_current_session_id.clone().unwrap_or_default(),
            has_siri_current_session_id: siri_current_session_id.is_some(),
            siri_current_assistant_surface: siri_current_assistant_surface
                .clone()
                .unwrap_or_default(),
            has_siri_current_assistant_surface: siri_current_assistant_surface.is_some(),
            siri_current_updated_at_ms: siri_current_updated_at_ms.unwrap_or_default(),
            has_siri_current_updated_at_ms: siri_current_updated_at_ms.is_some(),
        }
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert(
            "defaultPrompt".to_owned(),
            Value::String(self.default_prompt.clone()),
        );
        insert_optional_string(
            &mut object,
            "globalMode",
            self.has_global_mode,
            &self.global_mode,
        );
        object.insert("scope".to_owned(), Value::String(self.scope.clone()));
        insert_optional_string(
            &mut object,
            "notificationLabel",
            self.has_notification_label,
            &self.notification_label,
        );
        insert_optional_string(
            &mut object,
            "completionCheckLabel",
            self.has_completion_check_label,
            &self.completion_check_label,
        );
        object.insert(
            "completionCheckWaitForReply".to_owned(),
            Value::Bool(self.completion_check_wait_for_reply),
        );
        object.insert(
            "assistantSurface".to_owned(),
            Value::String(self.assistant_surface.clone()),
        );
        insert_optional_string(
            &mut object,
            "siriDefaultSessionId",
            self.has_siri_default_session_id,
            &self.siri_default_session_id,
        );
        insert_optional_string(
            &mut object,
            "siriDefaultAssistantSurface",
            self.has_siri_default_assistant_surface,
            &self.siri_default_assistant_surface,
        );
        insert_optional_string(
            &mut object,
            "siriCurrentSessionId",
            self.has_siri_current_session_id,
            &self.siri_current_session_id,
        );
        insert_optional_string(
            &mut object,
            "siriCurrentAssistantSurface",
            self.has_siri_current_assistant_surface,
            &self.siri_current_assistant_surface,
        );
        insert_optional_i64(
            &mut object,
            "siriCurrentUpdatedAtMs",
            self.has_siri_current_updated_at_ms,
            self.siri_current_updated_at_ms,
        );
        Value::Object(object)
    }
}

impl ClientMobileSession {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let effective_mode = object_string(object, "effectiveMode");
        let created_at_ms = object_i64(object, "createdAtMs");
        let updated_at_ms = object_i64(object, "updatedAtMs");
        let latest_message_at_ms = object_i64(object, "latestMessageAtMs");
        let last_activity_at_ms = object_i64(object, "lastActivityAtMs");
        let last_message_at_ms = object_i64(object, "lastMessageAtMs");
        let last_message_at = object_string(object, "lastMessageAt");
        let assistant_preview = object_string(object, "assistantPreview");
        let prompt_delivery_unavailable_reason =
            object_string(object, "promptDeliveryUnavailableReason");
        let goal = object
            .get("goal")
            .or_else(|| object.get("blockedGoal"))
            .and_then(Value::as_object)
            .map(ClientMobileSessionGoal::from_object);
        let metadata = object
            .get("metadata")
            .and_then(Value::as_object)
            .map(ClientMobileSessionMetadata::from_object)
            .unwrap_or_else(ClientMobileSessionMetadata::empty);
        let status =
            object_string(object, "status").unwrap_or_else(|| DEFAULT_SESSION_STATUS.to_owned());

        Some(Self {
            id: object_string(object, "id")
                .or_else(|| object_string(object, "sessionId"))
                .unwrap_or_default(),
            ref_id: object_string(object, "ref").unwrap_or_default(),
            title: object_string(object, "title").unwrap_or_default(),
            status: status.clone(),
            effective_mode: effective_mode.clone().unwrap_or_default(),
            has_effective_mode: effective_mode.is_some(),
            last_updated_at: object_string(object, "lastUpdatedAt").unwrap_or_default(),
            created_at_ms: created_at_ms.unwrap_or_default(),
            has_created_at_ms: created_at_ms.is_some(),
            updated_at_ms: updated_at_ms.unwrap_or_default(),
            has_updated_at_ms: updated_at_ms.is_some(),
            latest_message_at_ms: latest_message_at_ms.unwrap_or_default(),
            has_latest_message_at_ms: latest_message_at_ms.is_some(),
            last_activity_at_ms: last_activity_at_ms.unwrap_or_default(),
            has_last_activity_at_ms: last_activity_at_ms.is_some(),
            last_activity_at: object_string(object, "lastActivityAt").unwrap_or_default(),
            last_message_at_ms: last_message_at_ms.unwrap_or_default(),
            has_last_message_at_ms: last_message_at_ms.is_some(),
            last_message_at: last_message_at.clone().unwrap_or_default(),
            has_last_message_at: last_message_at.is_some(),
            assistant_preview: assistant_preview.clone().unwrap_or_default(),
            has_assistant_preview: assistant_preview.is_some(),
            is_archived: object_bool(object, "isArchived")
                .unwrap_or_else(|| status == STATUS_ARCHIVED),
            can_send_prompt: object_bool(object, "canSendPrompt").unwrap_or(true),
            prompt_delivery_unavailable_reason: prompt_delivery_unavailable_reason
                .clone()
                .unwrap_or_default(),
            has_prompt_delivery_unavailable_reason: prompt_delivery_unavailable_reason.is_some(),
            assistant_client: object_string(object, "assistantClient")
                .or_else(|| object_string(object, "assistantSurface"))
                .unwrap_or_else(|| DEFAULT_ASSISTANT_SURFACE.to_owned()),
            goal: goal.clone().unwrap_or_else(ClientMobileSessionGoal::empty),
            has_goal: goal.is_some(),
            metadata,
        })
    }

    pub(crate) fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("id".to_owned(), Value::String(self.id.clone()));
        object.insert("sessionId".to_owned(), Value::String(self.id.clone()));
        object.insert("ref".to_owned(), Value::String(self.ref_id.clone()));
        object.insert("title".to_owned(), Value::String(self.title.clone()));
        object.insert("status".to_owned(), Value::String(self.status.clone()));
        insert_optional_string(
            &mut object,
            "effectiveMode",
            self.has_effective_mode,
            &self.effective_mode,
        );
        object.insert(
            "lastUpdatedAt".to_owned(),
            Value::String(self.last_updated_at.clone()),
        );
        insert_optional_i64(
            &mut object,
            "createdAtMs",
            self.has_created_at_ms,
            self.created_at_ms,
        );
        insert_optional_i64(
            &mut object,
            "updatedAtMs",
            self.has_updated_at_ms,
            self.updated_at_ms,
        );
        insert_optional_i64(
            &mut object,
            "latestMessageAtMs",
            self.has_latest_message_at_ms,
            self.latest_message_at_ms,
        );
        insert_optional_i64(
            &mut object,
            "lastActivityAtMs",
            self.has_last_activity_at_ms,
            self.last_activity_at_ms,
        );
        object.insert(
            "lastActivityAt".to_owned(),
            Value::String(self.last_activity_at.clone()),
        );
        insert_optional_i64(
            &mut object,
            "lastMessageAtMs",
            self.has_last_message_at_ms,
            self.last_message_at_ms,
        );
        insert_optional_string(
            &mut object,
            "lastMessageAt",
            self.has_last_message_at,
            &self.last_message_at,
        );
        insert_optional_string(
            &mut object,
            "assistantPreview",
            self.has_assistant_preview,
            &self.assistant_preview,
        );
        object.insert("isArchived".to_owned(), Value::Bool(self.is_archived));
        object.insert(
            "canSendPrompt".to_owned(),
            Value::Bool(self.can_send_prompt),
        );
        insert_optional_string(
            &mut object,
            "promptDeliveryUnavailableReason",
            self.has_prompt_delivery_unavailable_reason,
            &self.prompt_delivery_unavailable_reason,
        );
        object.insert(
            "assistantClient".to_owned(),
            Value::String(self.assistant_client.clone()),
        );
        object.insert(
            "assistantSurface".to_owned(),
            Value::String(self.assistant_client.clone()),
        );
        if self.has_goal {
            object.insert("goal".to_owned(), self.goal.to_value());
        }
        object.insert("metadata".to_owned(), self.metadata.to_value());
        Value::Object(object)
    }
}

impl ClientMobileSessionGoal {
    fn empty() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            status: String::new(),
            lifecycle: String::new(),
            running: false,
            token_budget: 0,
            has_token_budget: false,
            tokens_used: 0,
            has_tokens_used: false,
            time_used_seconds: 0,
            has_time_used_seconds: false,
            updated_at_ms: 0,
            has_updated_at_ms: false,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let token_budget = object_i64(object, "tokenBudget");
        let tokens_used = object_i64(object, "tokensUsed");
        let time_used_seconds = object_i64(object, "timeUsedSeconds");
        let updated_at_ms = object_i64(object, "updatedAtMs");
        let status = object_string(object, "status")
            .or_else(|| object_string(object, "reason"))
            .unwrap_or_default();
        Self {
            id: object_string(object, "id").unwrap_or_default(),
            title: object_string(object, "title").unwrap_or_default(),
            lifecycle: object_string(object, "lifecycle").unwrap_or_else(|| status.clone()),
            status,
            running: object_bool(object, "running").unwrap_or(false),
            token_budget: token_budget.unwrap_or_default(),
            has_token_budget: token_budget.is_some(),
            tokens_used: tokens_used.unwrap_or_default(),
            has_tokens_used: tokens_used.is_some(),
            time_used_seconds: time_used_seconds.unwrap_or_default(),
            has_time_used_seconds: time_used_seconds.is_some(),
            updated_at_ms: updated_at_ms.unwrap_or_default(),
            has_updated_at_ms: updated_at_ms.is_some(),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("id".to_owned(), Value::String(self.id.clone()));
        object.insert("title".to_owned(), Value::String(self.title.clone()));
        object.insert("status".to_owned(), Value::String(self.status.clone()));
        object.insert(
            "lifecycle".to_owned(),
            Value::String(self.lifecycle.clone()),
        );
        object.insert("running".to_owned(), Value::Bool(self.running));
        insert_optional_i64(
            &mut object,
            "tokenBudget",
            self.has_token_budget,
            self.token_budget,
        );
        insert_optional_i64(
            &mut object,
            "tokensUsed",
            self.has_tokens_used,
            self.tokens_used,
        );
        insert_optional_i64(
            &mut object,
            "timeUsedSeconds",
            self.has_time_used_seconds,
            self.time_used_seconds,
        );
        insert_optional_i64(
            &mut object,
            "updatedAtMs",
            self.has_updated_at_ms,
            self.updated_at_ms,
        );
        Value::Object(object)
    }
}

impl ClientMobileSessionMetadata {
    fn empty() -> Self {
        Self {
            kind: "instantChat".to_owned(),
            source: "unknown".to_owned(),
            source_display_name: "Unknown".to_owned(),
            assistant_kind: String::new(),
            has_assistant_kind: false,
            originator: String::new(),
            has_originator: false,
            project_name: String::new(),
            has_project_name: false,
            project_path: String::new(),
            has_project_path: false,
            task_kind: "unknown".to_owned(),
            transcript_available: false,
            git_repository: ClientMobileGitRepositoryMetadata::empty(),
            has_git_repository: false,
            pull_request_url: String::new(),
            has_pull_request_url: false,
            supports_subagents: false,
            spawn: ClientMobileSessionSpawnMetadata::empty(),
            has_spawn: false,
            installed_plugins: Vec::new(),
            sources: Vec::new(),
            tags: Vec::new(),
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let assistant_kind = object_string(object, "assistantKind");
        let originator = object_string(object, "originator");
        let project_name = object_string(object, "projectName");
        let project_path = object_string(object, "projectPath");
        let git_repository = object
            .get("gitRepository")
            .and_then(Value::as_object)
            .map(ClientMobileGitRepositoryMetadata::from_object);
        let pull_request_url = object_string(object, "pullRequestURL");
        let spawn = object
            .get("spawn")
            .and_then(Value::as_object)
            .map(ClientMobileSessionSpawnMetadata::from_object);
        Self {
            kind: object_string(object, "kind").unwrap_or_else(|| "instantChat".to_owned()),
            source: object_string(object, "source").unwrap_or_else(|| "unknown".to_owned()),
            source_display_name: object_string(object, "sourceDisplayName")
                .unwrap_or_else(|| "Unknown".to_owned()),
            assistant_kind: assistant_kind.clone().unwrap_or_default(),
            has_assistant_kind: assistant_kind.is_some(),
            originator: originator.clone().unwrap_or_default(),
            has_originator: originator.is_some(),
            project_name: project_name.clone().unwrap_or_default(),
            has_project_name: project_name.is_some(),
            project_path: project_path.clone().unwrap_or_default(),
            has_project_path: project_path.is_some(),
            task_kind: object_string(object, "taskKind").unwrap_or_else(|| "unknown".to_owned()),
            transcript_available: object_bool(object, "transcriptAvailable").unwrap_or(false),
            git_repository: git_repository
                .clone()
                .unwrap_or_else(ClientMobileGitRepositoryMetadata::empty),
            has_git_repository: git_repository.is_some(),
            pull_request_url: pull_request_url.clone().unwrap_or_default(),
            has_pull_request_url: pull_request_url.is_some(),
            supports_subagents: object_bool(object, "supportsSubagents").unwrap_or(false),
            spawn: spawn
                .clone()
                .unwrap_or_else(ClientMobileSessionSpawnMetadata::empty),
            has_spawn: spawn.is_some(),
            installed_plugins: object_array(object, "installedPlugins")
                .into_iter()
                .filter_map(ClientMobileInstalledPluginSummary::from_value)
                .collect(),
            sources: object_array(object, "sources")
                .into_iter()
                .filter_map(ClientMobileSessionSourceReference::from_value)
                .collect(),
            tags: object_string_array(object, "tags"),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("kind".to_owned(), Value::String(self.kind.clone()));
        object.insert("source".to_owned(), Value::String(self.source.clone()));
        object.insert(
            "sourceDisplayName".to_owned(),
            Value::String(self.source_display_name.clone()),
        );
        insert_optional_string(
            &mut object,
            "assistantKind",
            self.has_assistant_kind,
            &self.assistant_kind,
        );
        insert_optional_string(
            &mut object,
            "originator",
            self.has_originator,
            &self.originator,
        );
        insert_optional_string(
            &mut object,
            "projectName",
            self.has_project_name,
            &self.project_name,
        );
        insert_optional_string(
            &mut object,
            "projectPath",
            self.has_project_path,
            &self.project_path,
        );
        object.insert("taskKind".to_owned(), Value::String(self.task_kind.clone()));
        object.insert(
            "transcriptAvailable".to_owned(),
            Value::Bool(self.transcript_available),
        );
        if self.has_git_repository {
            object.insert("gitRepository".to_owned(), self.git_repository.to_value());
        }
        insert_optional_string(
            &mut object,
            "pullRequestURL",
            self.has_pull_request_url,
            &self.pull_request_url,
        );
        object.insert(
            "supportsSubagents".to_owned(),
            Value::Bool(self.supports_subagents),
        );
        if self.has_spawn {
            object.insert("spawn".to_owned(), self.spawn.to_value());
        }
        object.insert(
            "installedPlugins".to_owned(),
            Value::Array(
                self.installed_plugins
                    .iter()
                    .map(ClientMobileInstalledPluginSummary::to_value)
                    .collect(),
            ),
        );
        object.insert(
            "sources".to_owned(),
            Value::Array(
                self.sources
                    .iter()
                    .map(ClientMobileSessionSourceReference::to_value)
                    .collect(),
            ),
        );
        object.insert(
            "tags".to_owned(),
            Value::Array(self.tags.iter().cloned().map(Value::String).collect()),
        );
        Value::Object(object)
    }
}

impl ClientMobileGitRepositoryMetadata {
    fn empty() -> Self {
        Self {
            repository_name: String::new(),
            repository_path: String::new(),
            remote_url: String::new(),
            has_remote_url: false,
            branch: String::new(),
            has_branch: false,
            commit: String::new(),
            has_commit: false,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let remote_url = object_string(object, "remoteURL");
        let branch = object_string(object, "branch");
        let commit = object_string(object, "commit");
        Self {
            repository_name: object_string(object, "repositoryName").unwrap_or_default(),
            repository_path: object_string(object, "repositoryPath").unwrap_or_default(),
            remote_url: remote_url.clone().unwrap_or_default(),
            has_remote_url: remote_url.is_some(),
            branch: branch.clone().unwrap_or_default(),
            has_branch: branch.is_some(),
            commit: commit.clone().unwrap_or_default(),
            has_commit: commit.is_some(),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert(
            "repositoryName".to_owned(),
            Value::String(self.repository_name.clone()),
        );
        object.insert(
            "repositoryPath".to_owned(),
            Value::String(self.repository_path.clone()),
        );
        insert_optional_string(
            &mut object,
            "remoteURL",
            self.has_remote_url,
            &self.remote_url,
        );
        insert_optional_string(&mut object, "branch", self.has_branch, &self.branch);
        insert_optional_string(&mut object, "commit", self.has_commit, &self.commit);
        Value::Object(object)
    }
}

impl ClientMobileSessionSpawnMetadata {
    fn empty() -> Self {
        Self {
            parent_thread_id: String::new(),
            has_parent_thread_id: false,
            root_thread_id: String::new(),
            has_root_thread_id: false,
            children: Vec::new(),
            launch_kind: String::new(),
            has_launch_kind: false,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let parent_thread_id = object_string(object, "parentThreadId");
        let root_thread_id = object_string(object, "rootThreadId");
        let launch_kind = object_string(object, "launchKind");
        Self {
            parent_thread_id: parent_thread_id.clone().unwrap_or_default(),
            has_parent_thread_id: parent_thread_id.is_some(),
            root_thread_id: root_thread_id.clone().unwrap_or_default(),
            has_root_thread_id: root_thread_id.is_some(),
            children: object_string_array(object, "children"),
            launch_kind: launch_kind.clone().unwrap_or_default(),
            has_launch_kind: launch_kind.is_some(),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        insert_optional_string(
            &mut object,
            "parentThreadId",
            self.has_parent_thread_id,
            &self.parent_thread_id,
        );
        insert_optional_string(
            &mut object,
            "rootThreadId",
            self.has_root_thread_id,
            &self.root_thread_id,
        );
        object.insert(
            "children".to_owned(),
            Value::Array(self.children.iter().cloned().map(Value::String).collect()),
        );
        insert_optional_string(
            &mut object,
            "launchKind",
            self.has_launch_kind,
            &self.launch_kind,
        );
        Value::Object(object)
    }
}

impl ClientMobileInstalledPluginSummary {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let source = object_string(object, "source");
        Some(Self {
            id: object_string(object, "id").unwrap_or_default(),
            name: object_string(object, "name").unwrap_or_default(),
            source: source.clone().unwrap_or_default(),
            has_source: source.is_some(),
        })
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("id".to_owned(), Value::String(self.id.clone()));
        object.insert("name".to_owned(), Value::String(self.name.clone()));
        insert_optional_string(&mut object, "source", self.has_source, &self.source);
        Value::Object(object)
    }
}

impl ClientMobileSessionSourceReference {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let url = object_string(object, "url");
        Some(Self {
            kind: object_string(object, "kind").unwrap_or_default(),
            label: object_string(object, "label").unwrap_or_default(),
            value: object_string(object, "value").unwrap_or_default(),
            url: url.clone().unwrap_or_default(),
            has_url: url.is_some(),
        })
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("kind".to_owned(), Value::String(self.kind.clone()));
        object.insert("label".to_owned(), Value::String(self.label.clone()));
        object.insert("value".to_owned(), Value::String(self.value.clone()));
        insert_optional_string(&mut object, "url", self.has_url, &self.url);
        Value::Object(object)
    }
}

impl ClientMobileNotificationDestination {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            id: object_string(object, "id").unwrap_or_default(),
            label: object_string(object, "label").unwrap_or_default(),
            channel: object_string(object, "channel").unwrap_or_default(),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "channel": self.channel,
        })
    }
}

impl ClientMobileCompletionCheckSummary {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        Some(Self {
            id: object_string(object, "id").unwrap_or_default(),
            label: object_string(object, "label").unwrap_or_default(),
            command_count: object_i64(object, "commandCount").unwrap_or_default(),
        })
    }

    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "label": self.label,
            "commandCount": self.command_count,
        })
    }
}

impl ClientMobileWorkStatusSummary {
    fn empty() -> Self {
        Self {
            goal_count: 0,
            running_goal_count: 0,
            automation_count: 0,
            active_automation_count: 0,
            covered_automation_count: 0,
            running_goals: Vec::new(),
            active_automations: Vec::new(),
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        Self {
            goal_count: object_i64(object, "goalCount").unwrap_or_default(),
            running_goal_count: object_i64(object, "runningGoalCount").unwrap_or_default(),
            automation_count: object_i64(object, "automationCount").unwrap_or_default(),
            active_automation_count: object_i64(object, "activeAutomationCount")
                .unwrap_or_default(),
            covered_automation_count: object_i64(object, "coveredAutomationCount")
                .unwrap_or_default(),
            running_goals: object_array(object, "runningGoals")
                .into_iter()
                .filter_map(ClientMobileWorkStatusGoal::from_value)
                .collect(),
            active_automations: object_array(object, "activeAutomations")
                .into_iter()
                .filter_map(ClientMobileWorkStatusAutomation::from_value)
                .collect(),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "goalCount": self.goal_count,
            "runningGoalCount": self.running_goal_count,
            "automationCount": self.automation_count,
            "activeAutomationCount": self.active_automation_count,
            "coveredAutomationCount": self.covered_automation_count,
            "runningGoals": self.running_goals.iter().map(ClientMobileWorkStatusGoal::to_value).collect::<Vec<_>>(),
            "activeAutomations": self.active_automations.iter().map(ClientMobileWorkStatusAutomation::to_value).collect::<Vec<_>>(),
        })
    }
}

impl ClientMobileWorkStatusGoal {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let target_thread_id = object_string(object, "targetThreadId");
        let updated_at_ms = object_i64(object, "updatedAtMs");
        let tokens_used = object_i64(object, "tokensUsed");
        let token_budget = object_i64(object, "tokenBudget");
        let time_used_seconds = object_i64(object, "timeUsedSeconds");
        Some(Self {
            id: object_string(object, "id").unwrap_or_default(),
            title: object_string(object, "title").unwrap_or_default(),
            status: object_string(object, "status").unwrap_or_default(),
            target_thread_id: target_thread_id.clone().unwrap_or_default(),
            has_target_thread_id: target_thread_id.is_some(),
            target_known: object_bool(object, "targetKnown").unwrap_or(false),
            updated_at_ms: updated_at_ms.unwrap_or_default(),
            has_updated_at_ms: updated_at_ms.is_some(),
            tokens_used: tokens_used.unwrap_or_default(),
            has_tokens_used: tokens_used.is_some(),
            token_budget: token_budget.unwrap_or_default(),
            has_token_budget: token_budget.is_some(),
            time_used_seconds: time_used_seconds.unwrap_or_default(),
            has_time_used_seconds: time_used_seconds.is_some(),
        })
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("id".to_owned(), Value::String(self.id.clone()));
        object.insert("title".to_owned(), Value::String(self.title.clone()));
        object.insert("status".to_owned(), Value::String(self.status.clone()));
        insert_optional_string(
            &mut object,
            "targetThreadId",
            self.has_target_thread_id,
            &self.target_thread_id,
        );
        object.insert("targetKnown".to_owned(), Value::Bool(self.target_known));
        insert_optional_i64(
            &mut object,
            "updatedAtMs",
            self.has_updated_at_ms,
            self.updated_at_ms,
        );
        insert_optional_i64(
            &mut object,
            "tokensUsed",
            self.has_tokens_used,
            self.tokens_used,
        );
        insert_optional_i64(
            &mut object,
            "tokenBudget",
            self.has_token_budget,
            self.token_budget,
        );
        insert_optional_i64(
            &mut object,
            "timeUsedSeconds",
            self.has_time_used_seconds,
            self.time_used_seconds,
        );
        Value::Object(object)
    }
}

impl ClientMobileWorkStatusAutomation {
    fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let target_thread_id = object_string(object, "targetThreadId");
        Some(Self {
            id: object_string(object, "id").unwrap_or_default(),
            kind: object_string(object, "kind").unwrap_or_default(),
            name: object_string(object, "name").unwrap_or_default(),
            status: object_string(object, "status").unwrap_or_default(),
            schedule_summary: object_string(object, "scheduleSummary").unwrap_or_default(),
            target_thread_id: target_thread_id.clone().unwrap_or_default(),
            has_target_thread_id: target_thread_id.is_some(),
            target_known: object_bool(object, "targetKnown").unwrap_or(false),
            control_plane_covered: object_bool(object, "controlPlaneCovered").unwrap_or(false),
        })
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("id".to_owned(), Value::String(self.id.clone()));
        object.insert("kind".to_owned(), Value::String(self.kind.clone()));
        object.insert("name".to_owned(), Value::String(self.name.clone()));
        object.insert("status".to_owned(), Value::String(self.status.clone()));
        object.insert(
            "scheduleSummary".to_owned(),
            Value::String(self.schedule_summary.clone()),
        );
        insert_optional_string(
            &mut object,
            "targetThreadId",
            self.has_target_thread_id,
            &self.target_thread_id,
        );
        object.insert("targetKnown".to_owned(), Value::Bool(self.target_known));
        object.insert(
            "controlPlaneCovered".to_owned(),
            Value::Bool(self.control_plane_covered),
        );
        Value::Object(object)
    }
}

impl ClientMobileDevinDesktopStatus {
    fn empty() -> Self {
        Self {
            running: false,
            installed: false,
            acp_available: false,
            registry_exists: false,
            registry_agent_count: 0,
            enabled_agent_count: 0,
            preferred_agent_ids: Vec::new(),
            session_count: 0,
            active_session_count: 0,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        Self {
            running: object_bool(object, "running").unwrap_or(false),
            installed: object_bool(object, "installed").unwrap_or(false),
            acp_available: object_bool(object, "acpAvailable").unwrap_or(false),
            registry_exists: object_bool(object, "registryExists").unwrap_or(false),
            registry_agent_count: object_i64(object, "registryAgentCount").unwrap_or_default(),
            enabled_agent_count: object_i64(object, "enabledAgentCount").unwrap_or_default(),
            preferred_agent_ids: object_string_array(object, "preferredAgentIds"),
            session_count: object_i64(object, "sessionCount").unwrap_or_default(),
            active_session_count: object_i64(object, "activeSessionCount").unwrap_or_default(),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "running": self.running,
            "installed": self.installed,
            "acpAvailable": self.acp_available,
            "registryExists": self.registry_exists,
            "registryAgentCount": self.registry_agent_count,
            "enabledAgentCount": self.enabled_agent_count,
            "preferredAgentIds": self.preferred_agent_ids,
            "sessionCount": self.session_count,
            "activeSessionCount": self.active_session_count,
        })
    }
}

impl ClientMobileGrokBuildStatus {
    fn empty() -> Self {
        Self {
            hooks: ClientMobileGrokBuildHookStatus::empty(),
            session_count: 0,
            active_session_count: 0,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        Self {
            hooks: object
                .get("hooks")
                .and_then(Value::as_object)
                .map(ClientMobileGrokBuildHookStatus::from_object)
                .unwrap_or_else(ClientMobileGrokBuildHookStatus::empty),
            session_count: object_i64(object, "sessionCount").unwrap_or_default(),
            active_session_count: object_i64(object, "activeSessionCount").unwrap_or_default(),
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "hooks": self.hooks.to_value(),
            "sessionCount": self.session_count,
            "activeSessionCount": self.active_session_count,
        })
    }
}

impl ClientMobileGrokBuildHookStatus {
    fn empty() -> Self {
        Self {
            health: String::new(),
            owner: String::new(),
            registered_events: Vec::new(),
            hooks_path: String::new(),
            has_hooks_path: false,
        }
    }

    fn from_object(object: &serde_json::Map<String, Value>) -> Self {
        let hooks_path = object_string(object, "hooksPath");
        Self {
            health: object_string(object, "health").unwrap_or_default(),
            owner: object_string(object, "owner").unwrap_or_default(),
            registered_events: object_string_array(object, "registeredEvents"),
            hooks_path: hooks_path.clone().unwrap_or_default(),
            has_hooks_path: hooks_path.is_some(),
        }
    }

    fn to_value(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("health".to_owned(), Value::String(self.health.clone()));
        object.insert("owner".to_owned(), Value::String(self.owner.clone()));
        object.insert(
            "registeredEvents".to_owned(),
            Value::Array(
                self.registered_events
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        insert_optional_string(
            &mut object,
            "hooksPath",
            self.has_hooks_path,
            &self.hooks_path,
        );
        Value::Object(object)
    }
}

fn object_array<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Vec<&'a Value> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|values| values.iter().collect())
        .unwrap_or_default()
}

fn object_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn object_string_array(object: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn object_bool(object: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(Value::as_bool)
}

fn object_i64(object: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(Value::as_i64)
}

fn insert_optional_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    has_value: bool,
    value: &str,
) {
    if has_value {
        object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn insert_optional_i64(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    has_value: bool,
    value: i64,
) {
    if has_value {
        object.insert(key.to_owned(), Value::Number(value.into()));
    }
}

fn selected_surface(sessions: &[(&ClientStateMini, Value)]) -> String {
    if let Some(surface) = selected_surface_from_revision(sessions) {
        return surface;
    }

    selected_surface_from_sessions(sessions)
}

fn selected_surface_from_revision(sessions: &[(&ClientStateMini, Value)]) -> Option<String> {
    sessions
        .iter()
        .filter_map(|(session, _)| {
            revision_assistant_surface(&session.revision)
                .map(|surface| (session.seq, session.session_id.as_str(), surface))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, surface)| surface)
}

fn revision_assistant_surface(revision: &str) -> Option<String> {
    revision.split(':').find_map(|part| {
        let surface = part
            .trim()
            .strip_prefix(REVISION_SURFACE_FIELD_PREFIX)?
            .trim();
        is_known_assistant_surface(surface).then(|| surface.to_owned())
    })
}

fn selected_surface_from_sessions(sessions: &[(&ClientStateMini, Value)]) -> String {
    let mut candidates = sessions
        .iter()
        .filter(|(session, _)| {
            is_known_assistant_surface(surface_bucket_for_assistant_client(
                &session.assistant_surface,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .0
            .seq
            .cmp(&left.0.seq)
            .then_with(|| left.0.session_id.cmp(&right.0.session_id))
    });
    candidates
        .first()
        .map(|(session, _)| {
            surface_bucket_for_assistant_client(&session.assistant_surface).to_owned()
        })
        .unwrap_or_else(|| DEFAULT_ASSISTANT_SURFACE.to_owned())
}

fn is_known_assistant_surface(surface: &str) -> bool {
    KNOWN_ASSISTANT_SURFACES.contains(&surface)
}

fn surface_bucket_for_assistant_client(client: &str) -> &str {
    let assistant_client = fallback_assistant_client(client);
    if CODEX_SURFACE_ASSISTANT_CLIENTS.contains(&assistant_client) {
        DEFAULT_ASSISTANT_SURFACE
    } else {
        assistant_client
    }
}

fn revision(latest_seq: i64, sessions: &[(&ClientStateMini, Value)]) -> String {
    sessions
        .iter()
        .filter(|(session, _)| !session.revision.trim().is_empty())
        .max_by(|left, right| {
            left.0
                .seq
                .cmp(&right.0.seq)
                .then_with(|| left.0.assistant_surface.cmp(&right.0.assistant_surface))
                .then_with(|| left.0.session_id.cmp(&right.0.session_id))
        })
        .map(|(session, _)| session.revision.trim().to_owned())
        .unwrap_or_else(|| format!("{REVISION_PREFIX}{latest_seq}"))
}

fn sort_sessions_by_freshness(sessions: &mut [Value]) {
    sessions.sort_by(compare_session_freshness);
}

fn compare_session_freshness(left: &Value, right: &Value) -> Ordering {
    match (
        field_i64(left, "lastActivityAtMs"),
        field_i64(right, "lastActivityAtMs"),
    ) {
        (Some(left_ms), Some(right_ms)) if left_ms != right_ms => {
            return right_ms.cmp(&left_ms);
        }
        _ => {}
    }

    let left_activity = field_string(left, "lastActivityAt");
    let right_activity = field_string(right, "lastActivityAt");
    if left_activity != right_activity {
        return right_activity.cmp(&left_activity);
    }

    field_string(left, "ref").cmp(&field_string(right, "ref"))
}

fn field_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn field_string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER_TIME: &str = "2026-06-25T00:00:02Z";

    #[test]
    fn empty_minis_return_no_mobile_snapshot() {
        let projection =
            reduce_state_minis_mobile_snapshot(0, vec![], String::new()).expect("empty projection");

        assert!(!projection.has_snapshot);
        assert!(projection.snapshot.sessions.is_empty());
    }

    #[test]
    fn mobile_snapshot_groups_surfaces_and_selects_latest_known_surface() {
        let projection = reduce_state_minis_mobile_snapshot(
            12,
            vec![
                mini(
                    "thread-codex",
                    "codex",
                    10,
                    "rev-10",
                    "C1",
                    1_781_596_920_123,
                ),
                mini(
                    "thread-devin",
                    "devin",
                    12,
                    "rev-12",
                    "D1",
                    1_781_596_920_321,
                ),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = projection.snapshot.to_value();

        assert!(projection.has_snapshot);
        assert_eq!(snapshot["revision"], "rev-12");
        assert_eq!(snapshot["host"]["id"], HOST_ID);
        assert_eq!(snapshot["host"]["lastSyncedAt"], SERVER_TIME);
        assert_eq!(snapshot["globalSettings"]["assistantSurface"], "devin");
        assert_eq!(snapshot["sessions"][0]["id"], "thread-devin");
        assert_eq!(
            snapshot["surfaceSessions"]["codex"][0]["id"],
            "thread-codex"
        );
    }

    #[test]
    fn mobile_snapshot_selects_surface_from_compact_revision() {
        let projection = reduce_state_minis_mobile_snapshot(
            42,
            vec![mini(
                "thread-codex",
                "codex",
                42,
                "threads=thread-codex:surface=claude-code:mobile-state=hash",
                "C1",
                1_781_596_920_123,
            )],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = projection.snapshot.to_value();

        assert!(projection.has_snapshot);
        assert_eq!(
            snapshot["globalSettings"]["assistantSurface"],
            "claude-code"
        );
        assert!(
            snapshot["sessions"]
                .as_array()
                .expect("sessions")
                .is_empty()
        );
        assert_eq!(
            snapshot["surfaceSessions"]["codex"][0]["id"],
            "thread-codex"
        );
    }

    #[test]
    fn mobile_snapshot_sorts_sessions_by_activity_then_ref() {
        let projection = reduce_state_minis_mobile_snapshot(
            14,
            vec![
                mini("older", "codex", 11, "rev-11", "S2", 100),
                mini("tie-lower-ref", "codex", 12, "rev-12", "S1", 200),
                mini("newer", "codex", 13, "rev-13", "S3", 300),
                mini("tie-higher-ref", "codex", 14, "rev-14", "S4", 200),
            ],
            String::new(),
        )
        .expect("projection");
        let snapshot: Value = projection.snapshot.to_value();
        let session_ids = snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(
            session_ids,
            vec!["newer", "tie-lower-ref", "tie-higher-ref", "older"]
        );
    }

    #[test]
    fn mobile_snapshot_groups_codex_compatible_clients_under_codex_surface() {
        let projection = reduce_state_minis_mobile_snapshot(
            31,
            vec![
                mini("thread-cursor", "cursor", 30, "rev-30", "C1", 300),
                mini("thread-super", "super-engineering", 31, "rev-31", "S1", 200),
                mini("thread-codex", "codex", 29, "rev-29", "Z1", 100),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = projection.snapshot.to_value();
        let codex_sessions = snapshot["surfaceSessions"]["codex"]
            .as_array()
            .expect("codex surface sessions");
        let visible_session_ids = snapshot["sessions"]
            .as_array()
            .expect("visible sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(snapshot["globalSettings"]["assistantSurface"], "codex");
        assert!(snapshot["surfaceSessions"].get("cursor").is_none());
        assert!(
            snapshot["surfaceSessions"]
                .get("super-engineering")
                .is_none()
        );
        assert_eq!(codex_sessions.len(), 3);
        assert_eq!(
            visible_session_ids,
            vec!["thread-cursor", "thread-super", "thread-codex"]
        );
        assert_eq!(codex_sessions[0]["assistantClient"], "cursor");
        assert_eq!(codex_sessions[1]["assistantClient"], "super-engineering");
        assert_eq!(codex_sessions[2]["assistantClient"], "codex");
    }

    #[test]
    fn mobile_snapshot_skips_stale_invalid_minis() {
        let projection = reduce_state_minis_mobile_snapshot(
            2,
            vec![
                ClientStateMini {
                    session_id: "envelope-id".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 1,
                    revision: "rev-1".to_owned(),
                    payload_json: session_json("payload-id", "S1", 100),
                },
                mini("thread-valid", "codex", 2, "rev-2", "S2", 200),
            ],
            String::new(),
        )
        .expect("projection skips invalid mini");
        let snapshot: Value = projection.snapshot.to_value();

        assert!(projection.has_snapshot);
        assert_eq!(snapshot["sessions"].as_array().expect("sessions").len(), 1);
        assert_eq!(snapshot["sessions"][0]["id"], "thread-valid");
        assert_eq!(snapshot["revision"], "rev-2");
    }

    #[test]
    fn mobile_snapshot_returns_empty_when_all_minis_are_invalid() {
        let projection = reduce_state_minis_mobile_snapshot(
            1,
            vec![ClientStateMini {
                session_id: "envelope-id".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 1,
                revision: "rev-1".to_owned(),
                payload_json: session_json("payload-id", "S1", 100),
            }],
            String::new(),
        )
        .expect("projection");

        assert!(!projection.has_snapshot);
        assert!(projection.snapshot.sessions.is_empty());
    }

    #[test]
    fn mobile_snapshot_accepts_compact_minis_with_model_defaults() {
        let projection = reduce_state_minis_mobile_snapshot(
            21,
            vec![ClientStateMini {
                session_id: "thread-compact".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 21,
                revision: "rev-21".to_owned(),
                payload_json: json!({
                    "sessionId": "thread-compact",
                    "assistantSurface": "codex",
                    "effectiveMode": "await-reply",
                })
                .to_string(),
            }],
            SERVER_TIME.to_owned(),
        )
        .expect("compact projection");
        let snapshot: Value = projection.snapshot.to_value();
        let session = &snapshot["sessions"][0];

        assert!(projection.has_snapshot);
        assert_eq!(session["id"], "thread-compact");
        assert_eq!(session["sessionId"], "thread-compact");
        assert_eq!(session["assistantSurface"], "codex");
        assert_eq!(session["assistantClient"], "codex");
        assert_eq!(session["ref"], "thread-compact");
        assert_eq!(session["title"], "thread-compact");
        assert_eq!(session["status"], DEFAULT_SESSION_STATUS);
        assert_eq!(session["lastUpdatedAt"], "");
        assert_eq!(session["lastActivityAt"], "");
        assert_eq!(session["isArchived"], false);
        assert_eq!(session["canSendPrompt"], true);
    }

    #[test]
    fn mobile_snapshot_ignores_pending_assistant_surface_when_refreshing_visible_sessions() {
        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            32,
            vec![
                mini("thread-codex", "codex", 32, "rev-32", "C32", 400),
                mini("thread-zed", "zed", 31, "rev-31", "Z31", 300),
            ],
            vec![pending_command(
                ClientPendingCommandKind::SetAssistantSurface,
                "",
                "zed",
            )],
            SERVER_TIME.to_owned(),
        )
        .expect("pending assistant surface projection");
        let snapshot: Value = projection.snapshot.to_value();
        let visible_session_ids = snapshot["sessions"]
            .as_array()
            .expect("visible sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert!(projection.has_snapshot);
        assert_eq!(snapshot["globalSettings"]["assistantSurface"], "codex");
        assert_eq!(visible_session_ids, vec!["thread-codex"]);
    }

    #[test]
    fn mobile_snapshot_applies_pending_commands_in_rust_projection() {
        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            31,
            vec![mini("thread-main", "codex", 31, "rev-31", "S31", 300)],
            vec![
                pending_command(
                    ClientPendingCommandKind::SetSessionMode,
                    "thread-main",
                    "max-turns-2",
                ),
                pending_command(
                    ClientPendingCommandKind::SetSiriCurrentSession,
                    "thread-main",
                    "codex",
                ),
                pending_command(
                    ClientPendingCommandKind::SaveDefaultPrompt,
                    "",
                    "Keep going",
                ),
                pending_command(
                    ClientPendingCommandKind::SetSessionArchived,
                    "thread-main",
                    "",
                ),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("pending projection");
        let snapshot: Value = projection.snapshot.to_value();
        let session = &snapshot["sessions"][0];

        assert!(projection.has_snapshot);
        assert_eq!(session["effectiveMode"], "max-turns-2");
        assert_eq!(session["isArchived"], true);
        assert_eq!(session["status"], STATUS_ARCHIVED);
        assert_eq!(snapshot["globalSettings"]["defaultPrompt"], "Keep going");
        assert_eq!(
            snapshot["globalSettings"]["siriCurrentSessionId"],
            "thread-main"
        );
        assert_eq!(
            snapshot["globalSettings"]["siriCurrentAssistantSurface"],
            "codex"
        );
    }

    #[test]
    fn mobile_snapshot_applies_pending_delete_across_surfaces_in_rust_projection() {
        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            42,
            vec![
                mini("thread-delete", "codex", 42, "rev-42", "S42", 400),
                mini("thread-keep", "codex", 41, "rev-41", "S41", 300),
            ],
            vec![
                pending_command(
                    ClientPendingCommandKind::SetSiriDefaultSession,
                    "thread-delete",
                    "codex",
                ),
                pending_command(ClientPendingCommandKind::DeleteSession, "thread-delete", ""),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("pending delete projection");
        let snapshot: Value = projection.snapshot.to_value();
        let session_ids = snapshot["surfaceSessions"]["codex"]
            .as_array()
            .expect("codex sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(session_ids, vec!["thread-keep"]);
        assert!(snapshot["globalSettings"]["siriDefaultSessionId"].is_null());
        assert!(snapshot["globalSettings"]["siriDefaultAssistantSurface"].is_null());
    }

    #[test]
    fn mobile_snapshot_repairs_partial_git_repository_metadata() {
        let projection = reduce_state_minis_mobile_snapshot(
            22,
            vec![ClientStateMini {
                session_id: "thread-partial-git".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 22,
                revision: "rev-22".to_owned(),
                payload_json: json!({
                    "id": "thread-partial-git",
                    "ref": "S22",
                    "title": "Partial Git",
                    "status": "active",
                    "lastUpdatedAt": "2026-06-16T08:02:00Z",
                    "lastActivityAt": "2026-06-16T08:02:00Z",
                    "metadata": {
                        "projectName": "looper",
                        "gitRepository": {
                            "repositoryName": "looper",
                            "branch": "main",
                            "remoteURL": null
                        }
                    }
                })
                .to_string(),
            }],
            SERVER_TIME.to_owned(),
        )
        .expect("partial git metadata projection");
        let snapshot: Value = projection.snapshot.to_value();
        let repository = &snapshot["sessions"][0]["metadata"]["gitRepository"];

        assert!(projection.has_snapshot);
        assert_eq!(repository["repositoryName"], "looper");
        assert_eq!(repository["repositoryPath"], "");
        assert_eq!(repository["branch"], "main");
    }

    fn mini(
        session_id: &str,
        assistant_surface: &str,
        seq: i64,
        revision: &str,
        ref_id: &str,
        activity_ms: i64,
    ) -> ClientStateMini {
        ClientStateMini {
            session_id: session_id.to_owned(),
            assistant_surface: assistant_surface.to_owned(),
            seq,
            revision: revision.to_owned(),
            payload_json: session_json(session_id, ref_id, activity_ms),
        }
    }

    fn pending_command(
        kind: ClientPendingCommandKind,
        thread_id: &str,
        value: &str,
    ) -> ClientPendingCommand {
        ClientPendingCommand {
            kind,
            client_mutation_id: format!("pending-{kind:?}-{thread_id}"),
            thread_id: thread_id.to_owned(),
            preset: if kind == ClientPendingCommandKind::SetSessionMode {
                value.to_owned()
            } else {
                String::new()
            },
            assistant_surface: if matches!(
                kind,
                ClientPendingCommandKind::SetSiriCurrentSession
                    | ClientPendingCommandKind::SetSiriDefaultSession
                    | ClientPendingCommandKind::SetAssistantSurface
            ) {
                value.to_owned()
            } else {
                String::new()
            },
            prompt_intent: String::new(),
            prompt: if kind == ClientPendingCommandKind::SaveDefaultPrompt {
                value.to_owned()
            } else {
                String::new()
            },
            notification_id: String::new(),
            notification_target_ids: Vec::new(),
            archived: kind == ClientPendingCommandKind::SetSessionArchived,
            attempt_count: 0,
        }
    }

    fn session_json(session_id: &str, ref_id: &str, activity_ms: i64) -> String {
        json!({
            "id": session_id,
            "ref": ref_id,
            "title": session_id,
            "status": "active",
            "lastUpdatedAt": "2026-06-16T08:02:00Z",
            "lastActivityAt": "2026-06-16T08:02:00Z",
            "lastActivityAtMs": activity_ms,
            "lastMessageAt": "2026-06-16T08:01:00Z",
            "lastMessageAtMs": activity_ms - 1,
            "metadata": {"source": "test"},
        })
        .to_string()
    }
}

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct DesktopSnapshotQuery {
    pub(super) profile: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AcpClientHostProbeRequest {
    pub(super) agent_id: Option<String>,
    #[serde(default)]
    pub(super) _meta: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DevinAcpSessionCreateRequest {
    pub(super) cwd: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DevinAcpSessionPromptRequest {
    pub(super) prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AcpClientHostSessionObserveRequest {
    pub(super) agent_id: String,
    pub(super) session_id: String,
    pub(super) connection_id: Option<String>,
    pub(super) cwd: Option<String>,
    pub(super) latest_user_prompt: Option<String>,
    pub(super) latest_assistant_message: Option<String>,
    #[serde(default)]
    pub(super) cancelled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobilePasskeyAuthenticationChallengeRequest {
    pub(super) credential_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobilePushTestRequest {
    pub(super) installation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionModeRequest {
    pub(super) preset: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionArchiveRequest {
    pub(super) archived: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionPromptRequest {
    pub(super) prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopNotificationReplyRequest {
    pub(super) notification_id: String,
    pub(super) prompt: String,
    pub(super) assistant_surface: Option<String>,
    pub(super) client_mutation_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionDetailQuery {
    pub(super) assistant_surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionPromptQuery {
    pub(super) assistant_surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopSessionBatchPromptRequest {
    pub(super) thread_ids: Vec<String>,
    pub(super) prompt: String,
    pub(super) preset: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileDefaultPromptRequest {
    pub(super) default_prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopDefaultPromptRequest {
    pub(super) default_prompt: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopScopeRequest {
    pub(super) scope: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileAssistantSurfaceRequest {
    pub(super) assistant_surface: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSiriDefaultSessionRequest {
    pub(super) session_id: Option<String>,
    pub(super) assistant_surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSiriCurrentSessionRequest {
    pub(super) session_id: Option<String>,
    pub(super) assistant_surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopGlobalNotificationRequest {
    pub(super) notification_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopDefaultNotificationTargetsRequest {
    pub(super) notification_target_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopCompletionCheckConfigRequest {
    pub(super) completion_check_id: Option<String>,
    pub(super) wait_for_reply_after_completion: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopNotificationRequest {
    pub(super) id: Option<String>,
    pub(super) label: Option<String>,
    pub(super) channel: String,
    pub(super) webhook_url: Option<String>,
    pub(super) chat_id: Option<String>,
    pub(super) bot_token: Option<String>,
    pub(super) chat_username: Option<String>,
    pub(super) chat_display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopTelegramChatsRequest {
    pub(super) bot_token: String,
    pub(super) wait_for_updates: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopCompletionCheckRequest {
    pub(super) id: Option<String>,
    pub(super) label: String,
    pub(super) commands: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopSessionNotificationsRequest {
    pub(super) notification_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopConnectionRenameRequest {
    pub(super) label: String,
}

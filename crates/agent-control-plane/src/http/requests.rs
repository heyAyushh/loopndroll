use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct DesktopSnapshotQuery {
    pub(super) profile: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopDevinAcpBridgeProbeRequest {
    pub(super) agent_id: Option<String>,
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
pub(super) struct MobileSessionDetailQuery {
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
pub(super) struct DesktopGlobalNotificationRequest {
    pub(super) notification_id: Option<String>,
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

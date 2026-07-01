use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct DesktopSnapshotQuery {
    pub(super) profile: Option<String>,
    pub(super) offset: Option<usize>,
    pub(super) limit: Option<usize>,
}

impl DesktopSnapshotQuery {
    pub(super) fn requested_thread_count(&self) -> Option<usize> {
        let limit = self.limit?;
        Some(self.offset.unwrap_or_default().saturating_add(limit))
    }

    pub(super) fn has_thread_range(&self) -> bool {
        self.offset.unwrap_or_default() > 0 || self.limit.is_some()
    }
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
    pub(super) latest_assistant_message_id: Option<String>,
    #[serde(default)]
    pub(super) latest_assistant_message_is_final: bool,
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
pub(super) struct MobileSessionDetailQuery {
    pub(super) assistant_surface: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MobileSessionContentQuery {
    pub(super) range: Option<String>,
    pub(super) limit: Option<usize>,
    pub(super) cursor: Option<String>,
    pub(super) revision: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopTelegramChatsRequest {
    pub(super) bot_token: String,
    pub(super) wait_for_updates: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DesktopConnectionRenameRequest {
    pub(super) label: String,
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod completion_checks;
mod legacy;
mod normalization;
mod notifications;
mod presets;
mod queries;
mod runtime;
mod schema;
mod session_overrides;
mod settings;

pub use self::notifications::build_telegram_bot_url;

pub(crate) use self::normalization::ASSISTANT_SURFACES;
use self::normalization::{ENABLED_FLAG, normalized_assistant_surface, normalized_preset};
use self::queries::{
    read_completion_checks, read_notifications, read_session_lifecycle, read_session_notifications,
};
use self::schema::initialize_store;
use self::settings::MobileSettingsRow;

pub const DEFAULT_REMOTE_PROMPT: &str = "Continue from where this session stopped.";
pub(crate) const MOBILE_SESSION_STATUS_ACTIVE: &str = "active";
pub(crate) const MOBILE_SESSION_STATUS_STOPPED: &str = "stopped";

#[derive(Clone, Debug)]
pub struct MobileSessionService {
    store_path: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileSessionState {
    pub default_prompt: String,
    pub scope: String,
    pub global_preset: Option<String>,
    pub global_notification_id: Option<String>,
    pub global_completion_check_id: Option<String>,
    pub global_completion_check_wait_for_reply: bool,
    pub assistant_surface: String,
    pub notifications: Vec<MobileNotificationRoute>,
    pub completion_checks: Vec<MobileCompletionCheck>,
    pub sessions: BTreeMap<String, MobileSessionOverride>,
    pub lifecycle: BTreeMap<String, MobileSessionLifecycle>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileSessionOverride {
    pub preset: Option<String>,
    pub archived: Option<bool>,
    pub muted: bool,
    pub deleted: bool,
    pub deleted_at: Option<String>,
    pub notification_ids: Vec<String>,
    pub completion_check_id: Option<String>,
    pub completion_check_wait_for_reply: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileSessionLifecycle {
    pub status: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileNotificationRoute {
    pub id: String,
    pub label: String,
    pub channel: String,
    pub webhook_url: Option<String>,
    pub chat_id: Option<String>,
    pub bot_token: Option<String>,
    pub bot_url: Option<String>,
    pub chat_username: Option<String>,
    pub chat_display_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpsertMobileNotificationRoute {
    pub id: Option<String>,
    pub label: Option<String>,
    pub channel: String,
    pub webhook_url: Option<String>,
    pub chat_id: Option<String>,
    pub bot_token: Option<String>,
    pub chat_username: Option<String>,
    pub chat_display_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileCompletionCheck {
    pub id: String,
    pub label: String,
    pub commands: Vec<String>,
}

impl MobileCompletionCheck {
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileQueuedPrompt {
    pub id: String,
    pub thread_id: String,
    pub prompt: String,
    pub status: String,
    pub delivery_mode: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileHookPayload {
    #[serde(alias = "hook_event_name")]
    pub hook_event_name: String,
    #[serde(alias = "session_id")]
    pub session_id: Option<String>,
    #[serde(alias = "turn_id")]
    pub turn_id: Option<String>,
    pub cwd: Option<String>,
    #[serde(alias = "last_assistant_message")]
    pub last_assistant_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileStopDecision {
    pub decision: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileHookOutcome {
    pub decision: Option<MobileStopDecision>,
    pub delivered_prompt_id: Option<String>,
}

#[derive(Debug, Error)]
pub enum MobileSessionError {
    #[error("mobile session store failed: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("mobile session filesystem failed: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("mobile session timestamp failed: {0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("session not found")]
    SessionNotFound,
    #[error("session mode must be a known mode or null")]
    InvalidPreset,
    #[error("scope must be global or per-task")]
    InvalidScope,
    #[error("assistant surface must be codex, devin, or grok-build")]
    InvalidAssistantSurface,
    #[error("prompt is required")]
    PromptRequired,
    #[error("set a session mode before sending a prompt")]
    ModeRequired,
    #[error("archived sessions cannot receive prompts")]
    SessionArchived,
    #[error("notification route not found")]
    NotificationNotFound,
    #[error("completion check not found")]
    CompletionCheckNotFound,
    #[error("notification route channel must be slack or telegram")]
    InvalidNotificationChannel,
    #[error("notification route is missing required channel config")]
    MissingNotificationConfig,
    #[error("completion check must include at least one command")]
    InvalidCompletionCheck,
}

type MobileSessionResult<T> = Result<T, MobileSessionError>;

impl MobileSessionService {
    pub fn new(store_path: PathBuf) -> Self {
        Self { store_path }
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn initialize(&self) -> MobileSessionResult<()> {
        initialize_store(&self.store_path)
    }

    pub fn state(&self) -> MobileSessionResult<MobileSessionState> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        let settings = connection
            .query_row(
                "select
                    default_prompt,
                    scope,
                    global_preset,
                    global_notification_id,
                    global_completion_check_id,
                    global_completion_check_wait_for_reply,
                    assistant_surface
                 from mobile_settings
                 where id = 1",
                [],
                |row| {
                    Ok(MobileSettingsRow {
                        default_prompt: row.get(0)?,
                        scope: row.get(1)?,
                        global_preset: row.get(2)?,
                        global_notification_id: row.get(3)?,
                        global_completion_check_id: row.get(4)?,
                        global_completion_check_wait_for_reply: row.get::<_, i64>(5)?
                            == ENABLED_FLAG,
                        assistant_surface: row.get(6)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        let notifications = read_notifications(&connection)?;
        let completion_checks = read_completion_checks(&connection)?;
        let known_notification_ids = notifications
            .iter()
            .map(|notification| notification.id.clone())
            .collect::<BTreeSet<_>>();
        let known_completion_check_ids = completion_checks
            .iter()
            .map(|completion_check| completion_check.id.clone())
            .collect::<BTreeSet<_>>();
        let notification_ids_by_thread =
            read_session_notifications(&connection, &known_notification_ids)?;
        let mut statement = connection.prepare(
            "select
                thread_id,
                preset,
                archived,
                muted,
                deleted_at,
                completion_check_id,
                completion_check_wait_for_reply
             from mobile_session_overrides",
        )?;
        let rows = statement.query_map([], |row| {
            let thread_id = row.get::<_, String>(0)?;
            let archived = row
                .get::<_, Option<i64>>(2)?
                .map(|value| value == ENABLED_FLAG);
            let deleted_at = row.get::<_, Option<String>>(4)?;
            let completion_check_id = row
                .get::<_, Option<String>>(5)?
                .filter(|id| known_completion_check_ids.contains(id));
            Ok((
                thread_id.clone(),
                MobileSessionOverride {
                    preset: row.get(1)?,
                    archived,
                    muted: row.get::<_, i64>(3)? == ENABLED_FLAG,
                    deleted: deleted_at.is_some(),
                    deleted_at,
                    notification_ids: notification_ids_by_thread
                        .get(&thread_id)
                        .cloned()
                        .unwrap_or_default(),
                    completion_check_id,
                    completion_check_wait_for_reply: row.get::<_, i64>(6)? == ENABLED_FLAG,
                },
            ))
        })?;
        let sessions = rows.collect::<Result<BTreeMap<_, _>, _>>()?;
        let lifecycle = read_session_lifecycle(&connection)?;

        Ok(MobileSessionState {
            default_prompt: settings.default_prompt,
            scope: settings.scope,
            global_preset: normalized_preset(settings.global_preset.as_deref())?,
            global_notification_id: settings
                .global_notification_id
                .filter(|id| known_notification_ids.contains(id)),
            global_completion_check_id: settings
                .global_completion_check_id
                .filter(|id| known_completion_check_ids.contains(id)),
            global_completion_check_wait_for_reply: settings.global_completion_check_wait_for_reply,
            assistant_surface: normalized_assistant_surface(&settings.assistant_surface)?,
            notifications,
            completion_checks,
            sessions,
            lifecycle,
        })
    }
}

#[cfg(test)]
mod tests;

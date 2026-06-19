use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::normalization::{
    DEFAULT_ASSISTANT_SURFACE, ENABLED_FLAG, bool_to_flag, normalized_option, normalized_preset,
};
use super::notifications::{
    NOTIFICATION_CHANNEL_SLACK, NOTIFICATION_CHANNEL_TELEGRAM, telegram_token_from_bot_url,
};
use super::schema::{MOBILE_LEGACY_IMPORTS_TABLE, table_columns, table_exists};
use super::settings::MobileSettingsRow;
use super::{
    MobileCompletionCheck, MobileNotificationRoute, MobileSessionError, MobileSessionResult,
    MobileSessionService,
};

struct LegacyMobileState {
    settings: MobileSettingsRow,
    notifications: Vec<MobileNotificationRoute>,
    completion_checks: Vec<MobileCompletionCheck>,
    session_overrides: Vec<LegacySessionOverride>,
    session_notifications: Vec<LegacySessionNotification>,
}

struct LegacySessionOverride {
    thread_id: String,
    preset: Option<String>,
    archived: Option<bool>,
    completion_check_id: Option<String>,
    completion_check_wait_for_reply: bool,
}

struct LegacySessionNotification {
    thread_id: String,
    notification_id: String,
}

impl MobileSessionService {
    pub fn import_legacy_bun_mobile_config(&self, legacy_path: &Path) -> MobileSessionResult<bool> {
        if !legacy_path.is_file() {
            return Ok(false);
        }
        self.initialize()?;
        let mut connection = Connection::open(&self.store_path)?;
        let source_path = legacy_path.display().to_string();
        let already_imported = connection
            .query_row(
                &format!(
                    "select source_path from {MOBILE_LEGACY_IMPORTS_TABLE} where source_path = ?1"
                ),
                [&source_path],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some();
        if already_imported {
            return Ok(false);
        }

        let legacy_connection =
            Connection::open_with_flags(legacy_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let legacy_state = read_legacy_mobile_state(&legacy_connection)?;
        let timestamp = super::normalization::now_iso_string()?;
        let transaction = connection.transaction()?;
        import_legacy_settings(&transaction, &legacy_state.settings, &timestamp)?;
        for notification in &legacy_state.notifications {
            transaction.execute(
                "insert into mobile_notification_routes (
                    id,
                    label,
                    channel,
                    webhook_url,
                    chat_id,
                    bot_token,
                    bot_url,
                    chat_username,
                    chat_display_name,
                    created_at,
                    updated_at
                 ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
                 on conflict(id) do update set
                    label = excluded.label,
                    channel = excluded.channel,
                    webhook_url = excluded.webhook_url,
                    chat_id = excluded.chat_id,
                    bot_token = excluded.bot_token,
                    bot_url = excluded.bot_url,
                    chat_username = excluded.chat_username,
                    chat_display_name = excluded.chat_display_name,
                    updated_at = excluded.updated_at",
                params![
                    &notification.id,
                    &notification.label,
                    &notification.channel,
                    &notification.webhook_url,
                    &notification.chat_id,
                    &notification.bot_token,
                    &notification.bot_url,
                    &notification.chat_username,
                    &notification.chat_display_name,
                    &timestamp,
                ],
            )?;
        }
        for completion_check in &legacy_state.completion_checks {
            transaction.execute(
                "insert into mobile_completion_checks (id, label, commands_json, created_at, updated_at)
                 values (?1, ?2, ?3, ?4, ?4)
                 on conflict(id) do update set
                    label = excluded.label,
                    commands_json = excluded.commands_json,
                    updated_at = excluded.updated_at",
                params![
                    &completion_check.id,
                    &completion_check.label,
                    serde_json::to_string(&completion_check.commands)
                        .map_err(|_| MobileSessionError::InvalidCompletionCheck)?,
                    &timestamp,
                ],
            )?;
        }
        for override_state in &legacy_state.session_overrides {
            transaction.execute(
                "insert into mobile_session_overrides (
                    thread_id,
                    preset,
                    archived,
                    muted,
                    deleted_at,
                    completion_check_id,
                    completion_check_wait_for_reply,
                    updated_at
                 ) values (?1, ?2, ?3, 0, null, ?4, ?5, ?6)
                 on conflict(thread_id) do update set
                    preset = coalesce(mobile_session_overrides.preset, excluded.preset),
                    archived = coalesce(mobile_session_overrides.archived, excluded.archived),
                    completion_check_id = coalesce(
                        mobile_session_overrides.completion_check_id,
                        excluded.completion_check_id
                    ),
                    completion_check_wait_for_reply =
                        excluded.completion_check_wait_for_reply,
                    updated_at = excluded.updated_at",
                params![
                    &override_state.thread_id,
                    &override_state.preset,
                    override_state.archived.map(bool_to_flag),
                    &override_state.completion_check_id,
                    bool_to_flag(override_state.completion_check_wait_for_reply),
                    &timestamp,
                ],
            )?;
        }
        for session_notification in &legacy_state.session_notifications {
            transaction.execute(
                "insert into mobile_session_notifications (thread_id, notification_id)
                 values (?1, ?2)
                 on conflict(thread_id, notification_id) do nothing",
                params![
                    &session_notification.thread_id,
                    &session_notification.notification_id,
                ],
            )?;
        }
        transaction.execute(
            &format!(
                "insert into {MOBILE_LEGACY_IMPORTS_TABLE} (source_path, imported_at)
                 values (?1, ?2)"
            ),
            params![source_path, timestamp],
        )?;
        transaction.commit()?;
        Ok(true)
    }
}

fn read_legacy_mobile_state(connection: &Connection) -> MobileSessionResult<LegacyMobileState> {
    let settings = if table_exists(connection, "settings")? {
        connection
            .query_row(
                "select
                    default_prompt,
                    scope,
                    global_preset,
                    global_notification_id,
                    global_completion_check_id,
                    global_completion_check_wait_for_reply
                 from settings
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
                        assistant_surface: DEFAULT_ASSISTANT_SURFACE.to_owned(),
                        siri_default_thread_id: None,
                        siri_default_assistant_surface: None,
                        siri_current_thread_id: None,
                        siri_current_assistant_surface: None,
                        siri_current_updated_at_ms: None,
                    })
                },
            )
            .optional()?
            .unwrap_or_default()
    } else {
        MobileSettingsRow::default()
    };

    Ok(LegacyMobileState {
        settings,
        notifications: read_legacy_notifications(connection)?,
        completion_checks: read_legacy_completion_checks(connection)?,
        session_overrides: read_legacy_session_overrides(connection)?,
        session_notifications: read_legacy_session_notifications(connection)?,
    })
}

fn import_legacy_settings(
    connection: &Connection,
    settings: &MobileSettingsRow,
    timestamp: &str,
) -> MobileSessionResult<()> {
    connection.execute(
        "update mobile_settings
         set default_prompt = ?1,
             scope = ?2,
             global_preset = ?3,
             global_notification_id = ?4,
             global_completion_check_id = ?5,
             global_completion_check_wait_for_reply = ?6,
             updated_at = ?7
         where id = 1",
        params![
            settings.default_prompt,
            settings.scope,
            normalized_preset(settings.global_preset.as_deref())?,
            settings.global_notification_id,
            settings.global_completion_check_id,
            bool_to_flag(settings.global_completion_check_wait_for_reply),
            timestamp,
        ],
    )?;
    Ok(())
}

fn read_legacy_notifications(
    connection: &Connection,
) -> MobileSessionResult<Vec<MobileNotificationRoute>> {
    if !table_exists(connection, "notifications")? {
        return Ok(Vec::new());
    }
    let columns = table_columns(connection, "notifications")?;
    let query = format!(
        "select
            id,
            label,
            channel,
            {},
            {},
            {},
            {},
            {},
            {}
         from notifications
         order by created_at asc, id asc",
        column_or_null(&columns, "webhook_url"),
        column_or_null(&columns, "chat_id"),
        column_or_null(&columns, "bot_token"),
        column_or_null(&columns, "bot_url"),
        column_or_null(&columns, "chat_username"),
        column_or_null(&columns, "chat_display_name"),
    );
    let mut statement = connection.prepare(&query)?;
    let rows = statement.query_map([], |row| {
        let channel = row.get::<_, String>(2)?;
        let bot_url = normalized_option(&row.get::<_, Option<String>>(6)?);
        Ok(MobileNotificationRoute {
            id: row.get(0)?,
            label: row.get(1)?,
            channel,
            webhook_url: normalized_option(&row.get::<_, Option<String>>(3)?),
            chat_id: normalized_option(&row.get::<_, Option<String>>(4)?),
            bot_token: normalized_option(&row.get::<_, Option<String>>(5)?)
                .or_else(|| telegram_token_from_bot_url(bot_url.as_deref())),
            bot_url,
            chat_username: normalized_option(&row.get::<_, Option<String>>(7)?),
            chat_display_name: normalized_option(&row.get::<_, Option<String>>(8)?),
        })
    })?;
    let notifications = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|notification| {
            matches!(
                notification.channel.as_str(),
                NOTIFICATION_CHANNEL_SLACK | NOTIFICATION_CHANNEL_TELEGRAM
            )
        })
        .collect();
    Ok(notifications)
}

fn read_legacy_completion_checks(
    connection: &Connection,
) -> MobileSessionResult<Vec<MobileCompletionCheck>> {
    if !table_exists(connection, "completion_checks")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "select id, label, commands_json
         from completion_checks
         order by created_at asc, id asc",
    )?;
    let rows = statement.query_map([], |row| {
        let commands_json = row.get::<_, String>(2)?;
        Ok(MobileCompletionCheck {
            id: row.get(0)?,
            label: row.get(1)?,
            commands: super::normalization::parse_commands_json(&commands_json),
        })
    })?;
    let completion_checks = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|completion_check| !completion_check.commands.is_empty())
        .collect();
    Ok(completion_checks)
}

fn read_legacy_session_overrides(
    connection: &Connection,
) -> MobileSessionResult<Vec<LegacySessionOverride>> {
    if !table_exists(connection, "sessions")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "select
            session_id,
            preset,
            archived,
            completion_check_id,
            completion_check_wait_for_reply
         from sessions
         order by session_id asc",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(LegacySessionOverride {
            thread_id: row.get(0)?,
            preset: normalized_preset(row.get::<_, Option<String>>(1)?.as_deref())
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            archived: row
                .get::<_, Option<i64>>(2)?
                .map(|archived| archived == ENABLED_FLAG),
            completion_check_id: row.get(3)?,
            completion_check_wait_for_reply: row.get::<_, i64>(4)? == ENABLED_FLAG,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(MobileSessionError::Store)
}

fn read_legacy_session_notifications(
    connection: &Connection,
) -> MobileSessionResult<Vec<LegacySessionNotification>> {
    if !table_exists(connection, "session_notifications")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "select session_id, notification_id
         from session_notifications
         order by session_id asc, notification_id asc",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(LegacySessionNotification {
            thread_id: row.get(0)?,
            notification_id: row.get(1)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(MobileSessionError::Store)
}

fn column_or_null(columns: &[String], column_name: &str) -> &'static str {
    match column_name {
        "webhook_url" if columns.iter().any(|column| column == column_name) => "webhook_url",
        "chat_id" if columns.iter().any(|column| column == column_name) => "chat_id",
        "bot_token" if columns.iter().any(|column| column == column_name) => "bot_token",
        "bot_url" if columns.iter().any(|column| column == column_name) => "bot_url",
        "chat_username" if columns.iter().any(|column| column == column_name) => "chat_username",
        "chat_display_name" if columns.iter().any(|column| column == column_name) => {
            "chat_display_name"
        }
        _ => "null",
    }
}

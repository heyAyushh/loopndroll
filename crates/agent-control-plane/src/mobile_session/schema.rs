use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use super::{MobileSessionError, MobileSessionResult};

pub(super) const MOBILE_SETTINGS_TABLE: &str = "mobile_settings";
pub(super) const MOBILE_SESSION_OVERRIDES_TABLE: &str = "mobile_session_overrides";
pub(super) const MOBILE_SESSION_RUNTIME_TABLE: &str = "mobile_session_runtime";
pub(super) const MOBILE_REMOTE_PROMPTS_TABLE: &str = "mobile_remote_prompts";
pub(super) const MOBILE_SESSION_NOTIFICATIONS_TABLE: &str = "mobile_session_notifications";
pub(super) const MOBILE_SESSION_LIFECYCLE_TABLE: &str = "mobile_session_lifecycle";
pub(super) const MOBILE_LEGACY_IMPORTS_TABLE: &str = "mobile_legacy_imports";

const MOBILE_SCHEMA_SQL: &str = r#"
create table if not exists mobile_settings (
  id integer primary key check (id = 1),
  default_prompt text not null,
  updated_at text not null
);

insert or ignore into mobile_settings (id, default_prompt, updated_at)
values (1, 'Continue from where this session stopped.', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

create table if not exists mobile_session_overrides (
  thread_id text primary key,
  preset text,
  archived integer check (archived in (0, 1)),
  muted integer not null default 0 check (muted in (0, 1)),
  deleted_at text,
  updated_at text not null
);

create table if not exists mobile_notification_routes (
  id text primary key,
  label text not null,
  channel text not null,
  webhook_url text,
  chat_id text,
  bot_token text,
  bot_url text,
  chat_username text,
  chat_display_name text,
  created_at text not null,
  updated_at text not null
);

create table if not exists mobile_completion_checks (
  id text primary key,
  label text not null,
  commands_json text not null,
  created_at text not null,
  updated_at text not null
);

create table if not exists mobile_session_notifications (
  thread_id text not null,
  notification_id text not null,
  primary key (thread_id, notification_id)
);

create table if not exists mobile_telegram_delivery_receipts (
  id text primary key,
  notification_id text,
  thread_id text not null,
  bot_token text not null,
  chat_id text not null,
  telegram_message_id integer not null,
  created_at text not null
);

create index if not exists mobile_telegram_delivery_receipts_message_idx
  on mobile_telegram_delivery_receipts(bot_token, chat_id, telegram_message_id, created_at desc);

create table if not exists mobile_legacy_imports (
  source_path text primary key,
  imported_at text not null
);

create table if not exists mobile_remote_prompts (
  id text primary key,
  thread_id text not null,
  prompt text not null,
  status text not null,
  delivery_mode text not null default 'once',
  created_at text not null,
  delivered_at text
);

create index if not exists mobile_remote_prompts_thread_idx
  on mobile_remote_prompts(thread_id, created_at desc);

create unique index if not exists mobile_remote_prompts_thread_delivery_mode_idx
  on mobile_remote_prompts(thread_id, delivery_mode);

create table if not exists mobile_session_runtime (
  thread_id text primary key,
  remaining_turns integer,
  updated_at text not null
);

create table if not exists mobile_session_lifecycle (
  thread_id text primary key,
  status text not null,
  updated_at text not null
);
"#;

pub(super) fn initialize_store(store_path: &Path) -> MobileSessionResult<()> {
    if let Some(parent) = store_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(store_path)?;
    connection.execute_batch(MOBILE_SCHEMA_SQL)?;
    ensure_column(
        &connection,
        MOBILE_SETTINGS_TABLE,
        "scope",
        "text not null default 'global'",
    )?;
    ensure_column(&connection, MOBILE_SETTINGS_TABLE, "global_preset", "text")?;
    ensure_column(
        &connection,
        MOBILE_SETTINGS_TABLE,
        "global_notification_id",
        "text",
    )?;
    ensure_column(
        &connection,
        MOBILE_SETTINGS_TABLE,
        "global_completion_check_id",
        "text",
    )?;
    ensure_column(
        &connection,
        MOBILE_SETTINGS_TABLE,
        "global_completion_check_wait_for_reply",
        "integer not null default 0",
    )?;
    ensure_column(
        &connection,
        MOBILE_SETTINGS_TABLE,
        "assistant_surface",
        "text not null default 'codex'",
    )?;
    ensure_column(
        &connection,
        "mobile_notification_routes",
        "webhook_url",
        "text",
    )?;
    ensure_column(&connection, "mobile_notification_routes", "chat_id", "text")?;
    ensure_column(
        &connection,
        "mobile_notification_routes",
        "bot_token",
        "text",
    )?;
    ensure_column(&connection, "mobile_notification_routes", "bot_url", "text")?;
    ensure_column(
        &connection,
        "mobile_notification_routes",
        "chat_username",
        "text",
    )?;
    ensure_column(
        &connection,
        "mobile_notification_routes",
        "chat_display_name",
        "text",
    )?;
    ensure_column(
        &connection,
        MOBILE_SESSION_OVERRIDES_TABLE,
        "completion_check_id",
        "text",
    )?;
    ensure_column(
        &connection,
        MOBILE_SESSION_OVERRIDES_TABLE,
        "completion_check_wait_for_reply",
        "integer not null default 0",
    )?;
    ensure_column(
        &connection,
        MOBILE_REMOTE_PROMPTS_TABLE,
        "delivery_mode",
        "text not null default 'once'",
    )?;
    Ok(())
}

pub(super) fn table_columns(
    connection: &Connection,
    table_name: &str,
) -> MobileSessionResult<Vec<String>> {
    let escaped_table_name = table_name.replace('\'', "''");
    let mut statement =
        connection.prepare(&format!("pragma table_info('{escaped_table_name}')"))?;
    statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(MobileSessionError::Store)
}

pub(super) fn table_exists(connection: &Connection, table_name: &str) -> MobileSessionResult<bool> {
    connection
        .query_row(
            "select name from sqlite_master where type = 'table' and name = ?1",
            [table_name],
            |_row| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(MobileSessionError::Store)
}

fn ensure_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    column_definition: &str,
) -> MobileSessionResult<()> {
    let columns = table_columns(connection, table_name)?;
    if columns.iter().any(|column| column == column_name) {
        return Ok(());
    }

    connection.execute(
        &format!("alter table {table_name} add column {column_name} {column_definition}"),
        [],
    )?;
    Ok(())
}

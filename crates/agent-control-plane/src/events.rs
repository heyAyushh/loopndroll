// allow: SIZE_OK — event store boundary keeps append, cursor, replay, and serialization semantics in one ordered log module.
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;

use crate::mobile::events::{
    MobileEvent, MobileEventKind, MobileEventRecord, mobile_event_wire_name,
};

const ENABLED_SETTING: i64 = 1;
const DISABLED_SETTING: i64 = 0;
const DEFAULT_MOBILE_ENTITY_ID: &str = "mobile";
const MOBILE_STATE_EVENT_ID_PREFIX: &str = "mobile-state-event-";
const MOBILE_STATE_EVENT_ID_WIDTH: usize = 20;
const MOBILE_COMMAND_RESERVATION_STALE_AFTER_MS: i64 = 60_000;
const NANOS_PER_MILLISECOND: i128 = 1_000_000;
const MILLIS_PER_SECOND: i64 = 1_000;
const SECONDS_PER_MINUTE: i64 = 60;
const MINUTES_PER_HOUR: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const MOBILE_EVENT_RETENTION_DAYS: i64 = 30;
pub const MOBILE_STATE_EVENT_ENTITY_RETENTION_ROWS: i64 = 500;
pub const MOBILE_STATE_EVENT_RETENTION_AGE_MS: i64 = MOBILE_EVENT_RETENTION_DAYS
    * HOURS_PER_DAY
    * MINUTES_PER_HOUR
    * SECONDS_PER_MINUTE
    * MILLIS_PER_SECOND;
pub const MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS: i64 = 1_000;
const MOBILE_COMMAND_ACK_RETENTION_AGE_MS: i64 = MOBILE_STATE_EVENT_RETENTION_AGE_MS;
const MOBILE_EVENT_RETENTION_RECLAIM_DELETED_ROWS_THRESHOLD: usize = 10_000;
const SQLITE_INCREMENTAL_AUTO_VACUUM_MODE: i64 = 2;
const SQLITE_RECLAIM_FREELIST_MIN_PAGES: i64 = 512;

/// Tracks which store paths have already run schema setup + legacy migration this process,
/// so `EventStore::ensure_initialized` is a cheap lock+lookup after the first call per path
/// instead of replaying DDL, `pragma table_info`, and the legacy anti-join migration on every
/// call. Keyed by path (not by `EventStore` instance) because `EventStore` is cheaply `Clone`d
/// across async tasks and connection-pool-free call sites that all point at the same file.
fn initialized_store_paths() -> &'static Mutex<HashSet<PathBuf>> {
    static PATHS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(HashSet::new()))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationRunRecord {
    pub run_id: String,
    pub automation_id: String,
    pub target_thread_id: Option<String>,
    pub scheduled_at_ms: i64,
    pub fired_at_ms: i64,
    pub delivery_mode: String,
    pub result: String,
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationRunInput<'a> {
    pub automation_id: &'a str,
    pub target_thread_id: Option<&'a str>,
    pub scheduled_at_ms: i64,
    pub fired_at_ms: i64,
    pub delivery_mode: &'a str,
    pub result: &'a str,
    pub detail: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceSettingsRecord {
    pub hooks_auto_registration: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncManifestSnapshotRecord {
    pub snapshot_id: String,
    pub generated_at_ms: i64,
    pub privacy_class: String,
    pub body_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileStateEventRecord {
    pub seq: i64,
    pub entity_id: String,
    pub kind: MobileEventKind,
    pub revision: String,
    pub server_time: String,
    pub payload_json: String,
    pub client_mutation_id: Option<String>,
    pub command_kind: Option<String>,
    pub command_request_hash: Option<String>,
    pub command_response_json: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct MobileStateEventInput {
    pub entity_id: String,
    pub kind: MobileEventKind,
    pub revision: String,
    pub server_time: String,
    pub payload_json: Value,
    pub client_mutation_id: Option<String>,
    pub command_kind: Option<String>,
    pub command_request_hash: Option<String>,
    pub command_response_json: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileSessionMiniRecord {
    pub session_id: String,
    pub assistant_surface: String,
    pub seq: i64,
    pub revision: String,
    pub body_json: String,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct MobileSessionMiniProjectionInput {
    pub session_id: String,
    pub assistant_surface: String,
    pub body_json: Value,
    pub latest_assistant_message_full: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileSessionMiniSnapshotRecord {
    pub latest_seq: i64,
    pub sessions: Vec<MobileSessionMiniRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileCommandAckRecord {
    pub command_kind: String,
    pub client_mutation_id: String,
    pub request_hash: String,
    pub ack_seq: i64,
    pub response_json: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct MobileCommandAckInput {
    pub command_kind: String,
    pub client_mutation_id: String,
    pub request_hash: String,
    pub response_json: Value,
    pub state_event: MobileStateEventInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MobileCommandAckResult {
    Recorded(MobileCommandAckRecord),
    Duplicate(MobileCommandAckRecord),
    Conflict(MobileCommandAckRecord),
}

impl MobileCommandAckResult {
    pub fn record(&self) -> &MobileCommandAckRecord {
        match self {
            Self::Recorded(record) | Self::Duplicate(record) | Self::Conflict(record) => record,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MobileCommandReservationResult {
    Reserved(MobileCommandAckRecord),
    Duplicate(MobileCommandAckRecord),
    Conflict(MobileCommandAckRecord),
    InFlight(MobileCommandAckRecord),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobileStateEventGap {
    pub requested_after_seq: i64,
    pub latest_seq: i64,
    pub oldest_seq: i64,
}

impl fmt::Display for MobileStateEventGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "mobile state event replay gap: requested after seq {} but retained seq range is {}..={}",
            self.requested_after_seq, self.oldest_seq, self.latest_seq
        )
    }
}

impl std::error::Error for MobileStateEventGap {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MobileEventCursor {
    pub created_at_ms: i64,
    pub event_id: String,
}

impl From<&MobileEventRecord> for MobileEventCursor {
    fn from(record: &MobileEventRecord) -> Self {
        Self {
            created_at_ms: record.created_at_ms,
            event_id: record.event_id.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventStore {
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventRetentionReclaimMode {
    Startup,
    Periodic,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventRetentionPruneReport {
    pub deleted_mobile_state_events: usize,
    pub deleted_mobile_command_acks: usize,
    pub reclaim: EventRetentionReclaimReport,
}

impl EventRetentionPruneReport {
    pub fn deleted_rows(&self) -> usize {
        self.deleted_mobile_state_events + self.deleted_mobile_command_acks
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventRetentionReclaimReport {
    Skipped,
    IncrementalVacuum { freelist_pages: i64 },
    Vacuum { freelist_pages: i64 },
}

impl Default for EventRetentionReclaimReport {
    fn default() -> Self {
        Self::Skipped
    }
}

impl EventStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let connection = Connection::open(&self.path)
            .with_context(|| format!("open {}", self.path.display()))?;
        // New databases can return freed pages with short incremental-vacuum work later.
        // Existing non-incremental databases only switch modes after a full VACUUM, so reclaim
        // stays startup-only and guarded by a freelist threshold.
        connection.execute_batch("pragma auto_vacuum = incremental;")?;
        connection.execute_batch(
            r#"
create table if not exists automation_runs (
  run_id text primary key,
  automation_id text not null,
  target_thread_id text,
  scheduled_at_ms integer not null,
  fired_at_ms integer not null,
  delivery_mode text not null,
  result text not null,
  detail text,
  unique(automation_id, scheduled_at_ms)
);

create table if not exists service_settings (
  id integer primary key check (id = 1),
  hooks_auto_registration integer not null default 1 check (hooks_auto_registration in (0, 1))
);

create table if not exists sync_manifest_snapshots (
  snapshot_id text primary key,
  generated_at_ms integer not null,
  privacy_class text not null,
  body_json text not null
);

insert or ignore into service_settings (id, hooks_auto_registration) values (1, 1);

create table if not exists mobile_event_log (
  event_id text primary key,
  event_type text not null,
  thread_id text,
  prompt_id text,
  detail text,
  created_at_ms integer not null
);

create index if not exists mobile_event_log_created_at_ms
  on mobile_event_log(created_at_ms desc);
create index if not exists mobile_event_log_replay_cursor
  on mobile_event_log(created_at_ms asc, event_id asc);

create table if not exists mobile_state_event_log (
  seq integer primary key autoincrement,
  entity_id text not null,
  kind text not null,
  revision text not null,
  server_time text not null,
  payload_json text not null,
  client_mutation_id text,
  command_kind text,
  command_request_hash text,
  command_response_json text,
  legacy_event_id text unique,
  created_at_ms integer not null
);

create index if not exists mobile_state_event_log_replay_seq
  on mobile_state_event_log(seq asc);
create index if not exists mobile_state_event_log_created_at_ms
  on mobile_state_event_log(created_at_ms asc, seq asc);
create index if not exists mobile_state_event_log_client_mutation
  on mobile_state_event_log(command_kind, client_mutation_id);

create table if not exists mobile_command_log (
  command_kind text not null,
  client_mutation_id text not null,
  request_hash text not null,
  ack_seq integer not null,
  response_json text not null,
  created_at_ms integer not null,
  primary key(command_kind, client_mutation_id)
);

create table if not exists mobile_session_minis (
  session_id text not null,
  assistant_surface text not null,
  seq integer not null,
  revision text not null,
  body_json text not null,
  updated_at_ms integer not null,
  primary key(session_id, assistant_surface)
);

create table if not exists mobile_session_mini_replacements (
  seq integer primary key,
  revision text not null,
  updated_at_ms integer not null
);
"#,
        )?;
        migrate_mobile_session_minis_schema(&connection)?;
        connection.execute_batch(
            r#"
create index if not exists mobile_session_minis_seq
  on mobile_session_minis(seq asc, assistant_surface asc, session_id asc);
create index if not exists mobile_state_event_log_entity_seq
  on mobile_state_event_log(entity_id, seq asc);
"#,
        )?;
        migrate_legacy_mobile_events(&connection)?;
        Ok(())
    }

    /// Runs `initialize` at most once per store path for the lifetime of the process.
    ///
    /// `initialize` replays the full DDL, a `pragma table_info` migration check, and the
    /// legacy-event anti-join migration scan on every call, plus opens a throwaway connection
    /// just to do so. Every read/write method on `EventStore` used to call `initialize`
    /// unconditionally, which made that cost part of every store call — including hot paths
    /// like the gRPC heartbeat (every 15s per connected client) and replayed state-delta
    /// frames (two store calls per record). Schema setup only needs to happen once per
    /// database file, so subsequent calls just check a process-wide set of already-initialized
    /// paths under a short-lived lock.
    fn ensure_initialized(&self) -> Result<()> {
        let paths = initialized_store_paths();
        if paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&self.path)
        {
            return Ok(());
        }
        self.initialize()?;
        paths
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(self.path.clone());
        Ok(())
    }

    pub fn record_mobile_event(&self, event: &MobileEvent) -> Result<MobileEventRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let input = mobile_state_event_input_for_mobile_event(event);
        let state_record =
            insert_mobile_state_event(&connection, &input, current_time_millis(), None)?;
        Ok(mobile_event_record_from_state_record(&state_record, None))
    }

    pub fn record_mobile_event_with_session_mini(
        &self,
        event: &MobileEvent,
        mini: MobileSessionMiniProjectionInput,
    ) -> Result<MobileEventRecord> {
        self.record_mobile_event_with_session_minis(event, vec![mini])
    }

    pub fn record_mobile_event_with_session_minis(
        &self,
        event: &MobileEvent,
        minis: Vec<MobileSessionMiniProjectionInput>,
    ) -> Result<MobileEventRecord> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let input = mobile_state_event_input_for_mobile_event(event);
        let state_record = insert_mobile_state_event(&transaction, &input, created_at_ms, None)?;
        for mut mini in minis {
            // Clients read a mini's seq from the body, not the record row; a cached
            // overlay body still carries the seq it was projected at, and a stale body
            // seq makes clients drop this update as older than what they already have.
            if let Some(body) = mini.body_json.as_object_mut() {
                body.insert("seq".to_owned(), serde_json::Value::from(state_record.seq));
            }
            upsert_mobile_session_mini(
                &transaction,
                &mini,
                state_record.seq,
                &state_record.revision,
                created_at_ms,
            )?;
        }
        transaction.commit()?;
        Ok(mobile_event_record_from_state_record(&state_record, None))
    }

    pub fn record_mobile_event_replacing_session_minis(
        &self,
        event: &MobileEvent,
        minis: Vec<MobileSessionMiniProjectionInput>,
    ) -> Result<MobileEventRecord> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let input = mobile_state_event_input_for_mobile_event(event);
        let state_record = insert_mobile_state_event(&transaction, &input, created_at_ms, None)?;
        let projection_revision = event
            .revision
            .as_deref()
            .filter(|revision| !revision.is_empty())
            .unwrap_or(state_record.revision.as_str());
        replace_mobile_session_minis_in_transaction(
            &transaction,
            minis,
            state_record.seq,
            projection_revision,
            created_at_ms,
        )?;
        transaction.commit()?;
        Ok(mobile_event_record_from_state_record(&state_record, None))
    }

    pub fn record_mobile_state_event(
        &self,
        input: MobileStateEventInput,
    ) -> Result<MobileStateEventRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        insert_mobile_state_event(&connection, &input, current_time_millis(), None)
    }

    pub fn record_mobile_state_event_with_session_mini(
        &self,
        input: MobileStateEventInput,
        mini: MobileSessionMiniProjectionInput,
    ) -> Result<MobileStateEventRecord> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let state_record = insert_mobile_state_event(&transaction, &input, created_at_ms, None)?;
        upsert_mobile_session_mini(
            &transaction,
            &mini,
            state_record.seq,
            &state_record.revision,
            created_at_ms,
        )?;
        transaction.commit()?;
        Ok(state_record)
    }

    pub fn upsert_mobile_session_mini(
        &self,
        mini: MobileSessionMiniProjectionInput,
        seq: i64,
        revision: &str,
    ) -> Result<MobileSessionMiniRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        upsert_mobile_session_mini(&connection, &mini, seq, revision, current_time_millis())
    }

    pub fn replace_mobile_session_minis(
        &self,
        minis: Vec<MobileSessionMiniProjectionInput>,
        seq: i64,
        revision: &str,
    ) -> Result<Vec<MobileSessionMiniRecord>> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let updated_at_ms = current_time_millis();
        let records = replace_mobile_session_minis_in_transaction(
            &transaction,
            minis,
            seq,
            revision,
            updated_at_ms,
        )?;
        transaction.commit()?;
        Ok(records)
    }

    pub fn mobile_session_minis(&self) -> Result<Vec<MobileSessionMiniRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_minis(&connection)
    }

    pub fn mobile_session_minis_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<MobileSessionMiniRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_minis_for_session(&connection, session_id)
    }

    pub fn latest_mobile_session_mini_revision(&self) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        latest_mobile_session_mini_revision(&connection)
    }

    pub fn has_mobile_session_minis(&self) -> Result<bool> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        has_mobile_session_minis(&connection)
    }

    pub fn mobile_session_minis_at_seq(&self, seq: i64) -> Result<Vec<MobileSessionMiniRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_minis_at_seq(&connection, seq)
    }

    pub fn mobile_session_minis_replaced_at_seq(&self, seq: i64) -> Result<bool> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_minis_replaced_at_seq(&connection, seq)
    }

    pub fn latest_mobile_session_mini_replacement_event_seq_after(
        &self,
        after_seq: i64,
    ) -> Result<Option<i64>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        latest_mobile_session_mini_replacement_event_seq_after(&connection, after_seq)
    }

    pub fn mobile_session_mini_replacement_revision_at_seq(
        &self,
        seq: i64,
    ) -> Result<Option<String>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_mini_replacement_revision_at_seq(&connection, seq)
    }

    pub fn latest_mobile_session_mini_snapshot(&self) -> Result<MobileSessionMiniSnapshotRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        Ok(MobileSessionMiniSnapshotRecord {
            latest_seq: latest_mobile_state_event_seq(&connection)?,
            sessions: mobile_session_minis(&connection)?,
        })
    }

    pub fn mobile_session_minis_after_seq(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<MobileSessionMiniRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let latest_seq = latest_mobile_state_event_seq(&connection)?;
        let oldest_seq = oldest_mobile_state_event_seq(&connection)?;
        if let Some(gap) = mobile_state_event_gap(after_seq, oldest_seq, latest_seq) {
            return Err(gap.into());
        }

        let mut statement = connection.prepare(
            "select session_id, assistant_surface, seq, revision, body_json, updated_at_ms
             from mobile_session_minis
             where seq > ?1
             order by seq asc, assistant_surface asc, session_id asc
             limit ?2",
        )?;
        let rows =
            statement.query_map(params![after_seq, limit as i64], mobile_session_mini_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn mobile_state_events_after_seq(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<MobileStateEventRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let latest_seq = latest_mobile_state_event_seq(&connection)?;
        let oldest_seq = oldest_mobile_state_event_seq(&connection)?;
        if let Some(gap) = mobile_state_event_gap(after_seq, oldest_seq, latest_seq) {
            return Err(gap.into());
        }

        let mut statement = connection.prepare(
            "select seq, entity_id, kind, revision, server_time, payload_json,
                    client_mutation_id, command_kind, command_request_hash,
                    command_response_json, created_at_ms
             from mobile_state_event_log
             where seq > ?1
             order by seq asc
             limit ?2",
        )?;
        let rows = statement.query_map(params![after_seq, limit as i64], mobile_state_event_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn mobile_state_events(&self) -> Result<Vec<MobileStateEventRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select seq, entity_id, kind, revision, server_time, payload_json,
                    client_mutation_id, command_kind, command_request_hash,
                    command_response_json, created_at_ms
             from mobile_state_event_log
             order by seq asc",
        )?;
        let rows = statement.query_map([], mobile_state_event_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn mobile_state_events_for_entity(
        &self,
        entity_id: &str,
    ) -> Result<Vec<MobileStateEventRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select seq, entity_id, kind, revision, server_time, payload_json,
                    client_mutation_id, command_kind, command_request_hash,
                    command_response_json, created_at_ms
             from mobile_state_event_log
             where entity_id = ?1
             order by seq asc",
        )?;
        let rows = statement.query_map(params![entity_id], mobile_state_event_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn latest_mobile_state_event_seq(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        latest_mobile_state_event_seq(&connection)
    }

    pub fn prune_mobile_event_retention(
        &self,
        now_ms: i64,
        reclaim_mode: EventRetentionReclaimMode,
    ) -> Result<EventRetentionPruneReport> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let cutoff_ms = now_ms.saturating_sub(MOBILE_STATE_EVENT_RETENTION_AGE_MS);
        let mut report = EventRetentionPruneReport::default();

        loop {
            let state_events_deleted =
                prune_mobile_state_event_log_batch(&mut connection, cutoff_ms)?;
            let command_acks_deleted = prune_mobile_command_log_batch(&mut connection, cutoff_ms)?;
            report.deleted_mobile_state_events += state_events_deleted;
            report.deleted_mobile_command_acks += command_acks_deleted;

            if state_events_deleted < MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS as usize
                && command_acks_deleted < MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS as usize
            {
                break;
            }
        }

        if reclaim_mode == EventRetentionReclaimMode::Startup
            && report.deleted_rows() >= MOBILE_EVENT_RETENTION_RECLAIM_DELETED_ROWS_THRESHOLD
        {
            report.reclaim = reclaim_mobile_event_store_space_if_worthwhile(&connection)?;
        }

        Ok(report)
    }

    pub fn mobile_command_ack(
        &self,
        command_kind: &str,
        client_mutation_id: &str,
    ) -> Result<Option<MobileCommandAckRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        mobile_command_ack(&connection, command_kind, client_mutation_id)
    }

    pub fn reserve_mobile_command_ack(
        &self,
        command_kind: &str,
        client_mutation_id: &str,
        request_hash: &str,
    ) -> Result<MobileCommandReservationResult> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let inserted = transaction.execute(
            "insert or ignore into mobile_command_log (
                command_kind, client_mutation_id, request_hash, ack_seq, response_json,
                created_at_ms
             ) values (?1, ?2, ?3, 0, ?4, ?5)",
            params![
                command_kind,
                client_mutation_id,
                request_hash,
                "{}",
                created_at_ms,
            ],
        )?;
        let record = mobile_command_ack(&transaction, command_kind, client_mutation_id)?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "mobile command reservation missing for {command_kind}:{client_mutation_id}"
                )
            })?;
        let result = if record.request_hash != request_hash {
            MobileCommandReservationResult::Conflict(record)
        } else if record.ack_seq != 0 {
            MobileCommandReservationResult::Duplicate(record)
        } else if inserted == 0 && is_stale_mobile_command_reservation(&record, created_at_ms) {
            transaction.execute(
                "update mobile_command_log
                 set created_at_ms = ?4
                 where command_kind = ?1
                   and client_mutation_id = ?2
                   and request_hash = ?3
                   and ack_seq = 0",
                params![
                    command_kind,
                    client_mutation_id,
                    request_hash,
                    created_at_ms
                ],
            )?;
            MobileCommandReservationResult::Reserved(MobileCommandAckRecord {
                created_at_ms,
                ..record
            })
        } else if inserted == 0 {
            MobileCommandReservationResult::InFlight(record)
        } else {
            MobileCommandReservationResult::Reserved(record)
        };
        transaction.commit()?;
        Ok(result)
    }

    pub fn clear_mobile_command_reservation(
        &self,
        command_kind: &str,
        client_mutation_id: &str,
        request_hash: &str,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let deleted = connection.execute(
            "delete from mobile_command_log
             where command_kind = ?1
               and client_mutation_id = ?2
               and request_hash = ?3
               and ack_seq = 0",
            params![command_kind, client_mutation_id, request_hash],
        )?;
        Ok(deleted > 0)
    }

    pub fn record_mobile_command_ack(
        &self,
        input: MobileCommandAckInput,
    ) -> Result<MobileCommandAckResult> {
        self.ensure_initialized()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let response_json = input.response_json.to_string();
        transaction.execute(
            "insert or ignore into mobile_command_log (
                command_kind, client_mutation_id, request_hash, ack_seq, response_json,
                created_at_ms
             ) values (?1, ?2, ?3, 0, ?4, ?5)",
            params![
                input.command_kind.as_str(),
                input.client_mutation_id.as_str(),
                input.request_hash.as_str(),
                response_json.as_str(),
                created_at_ms,
            ],
        )?;
        let reserved =
            mobile_command_ack(&transaction, &input.command_kind, &input.client_mutation_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "mobile command ack reservation missing for {}:{}",
                        input.command_kind,
                        input.client_mutation_id
                    )
                })?;
        if reserved.ack_seq != 0 {
            if reserved.request_hash == input.request_hash {
                return Ok(MobileCommandAckResult::Duplicate(reserved));
            }
            return Ok(MobileCommandAckResult::Conflict(reserved));
        }
        if reserved.request_hash != input.request_hash {
            return Ok(MobileCommandAckResult::Conflict(reserved));
        }

        let command_kind = input.command_kind;
        let client_mutation_id = input.client_mutation_id;
        let request_hash = input.request_hash;
        let mut state_event = input.state_event;
        state_event.client_mutation_id = Some(client_mutation_id.clone());
        state_event.command_kind = Some(command_kind.clone());
        state_event.command_request_hash = Some(request_hash.clone());
        state_event.command_response_json = Some(input.response_json);
        let state_record =
            insert_mobile_state_event(&transaction, &state_event, created_at_ms, None)?;
        let ack_record = MobileCommandAckRecord {
            command_kind,
            client_mutation_id,
            request_hash,
            ack_seq: state_record.seq,
            response_json,
            created_at_ms,
        };
        transaction.execute(
            "update mobile_command_log
             set ack_seq = ?3,
                 response_json = ?4
             where command_kind = ?1 and client_mutation_id = ?2 and ack_seq = 0",
            params![
                ack_record.command_kind.as_str(),
                ack_record.client_mutation_id.as_str(),
                ack_record.ack_seq,
                ack_record.response_json.as_str(),
            ],
        )?;
        transaction.commit()?;
        Ok(MobileCommandAckResult::Recorded(ack_record))
    }

    pub fn mobile_events_since(
        &self,
        since_created_at_ms: i64,
        limit: usize,
    ) -> Result<Vec<MobileEventRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select seq, kind, payload_json, created_at_ms, legacy_event_id
             from mobile_state_event_log
             where created_at_ms > ?1
             order by created_at_ms asc, seq asc
             limit ?2",
        )?;
        let rows = statement.query_map(
            params![since_created_at_ms, limit as i64],
            mobile_event_record_row,
        )?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn mobile_events_after(
        &self,
        cursor: &MobileEventCursor,
        limit: usize,
    ) -> Result<Vec<MobileEventRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let cursor_seq = mobile_event_cursor_seq(&connection, cursor)?;
        let mut statement = connection.prepare(
            "select seq, kind, payload_json, created_at_ms, legacy_event_id
             from mobile_state_event_log
             where seq > ?1
             order by seq asc
             limit ?2",
        )?;
        let rows =
            statement.query_map(params![cursor_seq, limit as i64], mobile_event_record_row)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn record_automation_run(
        &self,
        input: AutomationRunInput<'_>,
    ) -> Result<Option<AutomationRunRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let existing: Option<String> = connection
            .query_row(
                "select run_id from automation_runs where automation_id = ?1 and scheduled_at_ms = ?2",
                params![input.automation_id, input.scheduled_at_ms],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(None);
        }

        let record = AutomationRunRecord {
            run_id: format!("automation-run-{}", uuid::Uuid::new_v4()),
            automation_id: input.automation_id.to_owned(),
            target_thread_id: input.target_thread_id.map(str::to_owned),
            scheduled_at_ms: input.scheduled_at_ms,
            fired_at_ms: input.fired_at_ms,
            delivery_mode: input.delivery_mode.to_owned(),
            result: input.result.to_owned(),
            detail: input.detail.map(str::to_owned),
        };
        connection.execute(
            "insert into automation_runs (
                run_id, automation_id, target_thread_id, scheduled_at_ms, fired_at_ms,
                delivery_mode, result, detail
            ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.run_id,
                record.automation_id,
                record.target_thread_id,
                record.scheduled_at_ms,
                record.fired_at_ms,
                record.delivery_mode,
                record.result,
                record.detail,
            ],
        )?;
        Ok(Some(record))
    }

    pub fn automation_runs(&self) -> Result<Vec<AutomationRunRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select run_id, automation_id, target_thread_id, scheduled_at_ms, fired_at_ms,
                    delivery_mode, result, detail
             from automation_runs
             order by fired_at_ms desc",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AutomationRunRecord {
                run_id: row.get(0)?,
                automation_id: row.get(1)?,
                target_thread_id: row.get(2)?,
                scheduled_at_ms: row.get(3)?,
                fired_at_ms: row.get(4)?,
                delivery_mode: row.get(5)?,
                result: row.get(6)?,
                detail: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn update_automation_run_result(
        &self,
        run_id: &str,
        delivery_mode: &str,
        result: &str,
        detail: Option<&str>,
    ) -> Result<AutomationRunRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        connection.execute(
            "update automation_runs
             set delivery_mode = ?2,
                 result = ?3,
                 detail = ?4
             where run_id = ?1",
            params![run_id, delivery_mode, result, detail],
        )?;
        connection
            .query_row(
                "select run_id, automation_id, target_thread_id, scheduled_at_ms, fired_at_ms,
                        delivery_mode, result, detail
                 from automation_runs
                 where run_id = ?1
                 limit 1",
                [run_id],
                |row| {
                    Ok(AutomationRunRecord {
                        run_id: row.get(0)?,
                        automation_id: row.get(1)?,
                        target_thread_id: row.get(2)?,
                        scheduled_at_ms: row.get(3)?,
                        fired_at_ms: row.get(4)?,
                        delivery_mode: row.get(5)?,
                        result: row.get(6)?,
                        detail: row.get(7)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn service_settings(&self) -> Result<ServiceSettingsRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let hooks_auto_registration = connection.query_row(
            "select hooks_auto_registration from service_settings where id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(ServiceSettingsRecord {
            hooks_auto_registration: hooks_auto_registration == ENABLED_SETTING,
        })
    }

    pub fn set_hooks_auto_registration(&self, enabled: bool) -> Result<ServiceSettingsRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        connection.execute(
            "update service_settings set hooks_auto_registration = ?1 where id = 1",
            [if enabled {
                ENABLED_SETTING
            } else {
                DISABLED_SETTING
            }],
        )?;
        self.service_settings()
    }

    pub fn record_sync_manifest_snapshot(
        &self,
        generated_at_ms: i64,
        body_json: &str,
    ) -> Result<SyncManifestSnapshotRecord> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        let record = SyncManifestSnapshotRecord {
            snapshot_id: format!("sync-snapshot-{}", uuid::Uuid::new_v4()),
            generated_at_ms,
            privacy_class: "metadata-only".to_owned(),
            body_json: body_json.to_owned(),
        };
        connection.execute(
            "insert into sync_manifest_snapshots (
                snapshot_id, generated_at_ms, privacy_class, body_json
            ) values (?1, ?2, ?3, ?4)",
            params![
                record.snapshot_id,
                record.generated_at_ms,
                record.privacy_class,
                record.body_json
            ],
        )?;
        Ok(record)
    }

    pub fn latest_mobile_event_created_at_ms(&self) -> Result<i64> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        connection
            .query_row(
                "select coalesce(max(created_at_ms), 0) from mobile_state_event_log",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn latest_mobile_event_cursor(&self) -> Result<MobileEventCursor> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        connection
            .query_row(
                "select seq, created_at_ms, legacy_event_id
                 from mobile_state_event_log
                 order by seq desc
                 limit 1",
                [],
                |row| {
                    let seq = row.get(0)?;
                    let legacy_event_id: Option<String> = row.get(2)?;
                    Ok(MobileEventCursor {
                        created_at_ms: row.get(1)?,
                        event_id: legacy_event_id.unwrap_or_else(|| mobile_state_event_id(seq)),
                    })
                },
            )
            .optional()
            .map(|row| row.unwrap_or_default())
            .map_err(Into::into)
    }

    pub fn latest_sync_manifest_snapshot(&self) -> Result<Option<SyncManifestSnapshotRecord>> {
        self.ensure_initialized()?;
        let connection = Connection::open(&self.path)?;
        connection
            .query_row(
                "select snapshot_id, generated_at_ms, privacy_class, body_json
                 from sync_manifest_snapshots
                 order by generated_at_ms desc
                 limit 1",
                [],
                |row| {
                    Ok(SyncManifestSnapshotRecord {
                        snapshot_id: row.get(0)?,
                        generated_at_ms: row.get(1)?,
                        privacy_class: row.get(2)?,
                        body_json: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

fn migrate_mobile_session_minis_schema(connection: &Connection) -> Result<()> {
    let columns = table_columns(connection, "mobile_session_minis")?;
    if columns.iter().any(|column| column == "assistant_surface") {
        return Ok(());
    }

    connection.execute_batch(
        r#"
drop index if exists mobile_session_minis_seq;

alter table mobile_session_minis rename to mobile_session_minis_legacy;

create table mobile_session_minis (
  session_id text not null,
  assistant_surface text not null,
  seq integer not null,
  revision text not null,
  body_json text not null,
  updated_at_ms integer not null,
  primary key(session_id, assistant_surface)
);

insert into mobile_session_minis (
  session_id, assistant_surface, seq, revision, body_json, updated_at_ms
)
select session_id, '', seq, revision, body_json, updated_at_ms
from mobile_session_minis_legacy;

drop table mobile_session_minis_legacy;

create index if not exists mobile_session_minis_seq
  on mobile_session_minis(seq asc, assistant_surface asc, session_id asc);
"#,
    )?;
    Ok(())
}

fn table_columns(connection: &Connection, table_name: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("pragma table_info({table_name})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn migrate_legacy_mobile_events(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare(
        "select event_id, event_type, thread_id, prompt_id, detail, created_at_ms
         from mobile_event_log legacy
         where not exists (
           select 1
           from mobile_state_event_log state
           where state.legacy_event_id = legacy.event_id
         )
         order by created_at_ms asc, event_id asc",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(LegacyMobileEvent {
            event_id: row.get(0)?,
            event_type: parse_mobile_event_kind(&row.get::<_, String>(1)?),
            thread_id: row.get(2)?,
            prompt_id: row.get(3)?,
            detail: row.get(4)?,
            created_at_ms: row.get(5)?,
        })
    })?;
    let legacy_events = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);

    for legacy_event in legacy_events {
        let entity_id = legacy_event.entity_id();
        let payload_json = serde_json::json!({
            "eventType": mobile_event_wire_name(legacy_event.event_type),
            "threadId": legacy_event.thread_id.as_deref(),
            "promptId": legacy_event.prompt_id.as_deref(),
            "detail": legacy_event.detail.as_deref(),
        });
        let input = MobileStateEventInput {
            entity_id,
            kind: legacy_event.event_type,
            revision: String::new(),
            server_time: timestamp_millis_to_iso(legacy_event.created_at_ms)
                .unwrap_or_else(|| legacy_event.created_at_ms.to_string()),
            payload_json,
            client_mutation_id: None,
            command_kind: None,
            command_request_hash: None,
            command_response_json: None,
        };
        insert_mobile_state_event(
            connection,
            &input,
            legacy_event.created_at_ms,
            Some(&legacy_event.event_id),
        )?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct LegacyMobileEvent {
    event_id: String,
    event_type: MobileEventKind,
    thread_id: Option<String>,
    prompt_id: Option<String>,
    detail: Option<String>,
    created_at_ms: i64,
}

impl LegacyMobileEvent {
    fn entity_id(&self) -> String {
        self.thread_id
            .clone()
            .or_else(|| self.prompt_id.clone())
            .unwrap_or_else(|| self.event_id.clone())
    }
}

fn insert_mobile_state_event(
    connection: &Connection,
    input: &MobileStateEventInput,
    created_at_ms: i64,
    legacy_event_id: Option<&str>,
) -> Result<MobileStateEventRecord> {
    let payload_json = input.payload_json.to_string();
    let command_response_json = input
        .command_response_json
        .as_ref()
        .map(|value| value.to_string());
    let inserted = connection.execute(
        "insert or ignore into mobile_state_event_log (
            entity_id, kind, revision, server_time, payload_json, client_mutation_id,
            command_kind, command_request_hash, command_response_json, legacy_event_id,
            created_at_ms
         ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            input.entity_id.as_str(),
            mobile_event_wire_name(input.kind),
            input.revision.as_str(),
            input.server_time.as_str(),
            payload_json.as_str(),
            input.client_mutation_id.as_deref(),
            input.command_kind.as_deref(),
            input.command_request_hash.as_deref(),
            command_response_json.as_deref(),
            legacy_event_id,
            created_at_ms,
        ],
    )?;
    let seq = match (inserted, legacy_event_id) {
        (0, Some(event_id)) => mobile_state_event_seq_for_legacy_event_id(connection, event_id)?
            .unwrap_or_else(|| connection.last_insert_rowid()),
        _ => connection.last_insert_rowid(),
    };
    Ok(MobileStateEventRecord {
        seq,
        entity_id: input.entity_id.clone(),
        kind: input.kind,
        revision: input.revision.clone(),
        server_time: input.server_time.clone(),
        payload_json,
        client_mutation_id: input.client_mutation_id.clone(),
        command_kind: input.command_kind.clone(),
        command_request_hash: input.command_request_hash.clone(),
        command_response_json,
        created_at_ms,
    })
}

fn prune_mobile_state_event_log_batch(
    connection: &mut Connection,
    cutoff_ms: i64,
) -> Result<usize> {
    let transaction = connection.transaction()?;
    let deleted = transaction.execute(
        "with ranked as (
           select seq,
                  created_at_ms,
                  row_number() over (
                    partition by entity_id
                    order by seq desc
                  ) as entity_rank
           from mobile_state_event_log
         ),
         candidates as (
           select seq
           from ranked
           where entity_rank > 1
             and (
               created_at_ms < ?1
               or entity_rank > ?2
             )
           order by seq asc
           limit ?3
         )
         delete from mobile_state_event_log
         where seq in (select seq from candidates)",
        params![
            cutoff_ms,
            MOBILE_STATE_EVENT_ENTITY_RETENTION_ROWS,
            MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS,
        ],
    )?;
    transaction.execute(
        "delete from mobile_session_mini_replacements
         where not exists (
           select 1
           from mobile_state_event_log events
           where events.seq = mobile_session_mini_replacements.seq
         )",
        [],
    )?;
    transaction.commit()?;
    Ok(deleted)
}

fn prune_mobile_command_log_batch(connection: &mut Connection, cutoff_ms: i64) -> Result<usize> {
    let command_cutoff_ms = cutoff_ms.saturating_add(
        MOBILE_STATE_EVENT_RETENTION_AGE_MS.saturating_sub(MOBILE_COMMAND_ACK_RETENTION_AGE_MS),
    );
    let transaction = connection.transaction()?;
    let deleted = transaction.execute(
        "delete from mobile_command_log
         where rowid in (
           select rowid
           from mobile_command_log
           where created_at_ms < ?1
           order by created_at_ms asc, rowid asc
           limit ?2
         )",
        params![command_cutoff_ms, MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS],
    )?;
    transaction.commit()?;
    Ok(deleted)
}

fn reclaim_mobile_event_store_space_if_worthwhile(
    connection: &Connection,
) -> Result<EventRetentionReclaimReport> {
    let freelist_pages = sqlite_pragma_i64(connection, "freelist_count")?;
    if freelist_pages < SQLITE_RECLAIM_FREELIST_MIN_PAGES {
        return Ok(EventRetentionReclaimReport::Skipped);
    }

    let auto_vacuum = sqlite_pragma_i64(connection, "auto_vacuum")?;
    if auto_vacuum == SQLITE_INCREMENTAL_AUTO_VACUUM_MODE {
        connection.execute_batch(&format!("pragma incremental_vacuum({freelist_pages});"))?;
        return Ok(EventRetentionReclaimReport::IncrementalVacuum { freelist_pages });
    }

    // VACUUM rewrites the database and can hold the store longer than serving paths tolerate.
    // Keep it startup-only, after large prunes, so production can reclaim an old non-incremental
    // database without adding surprise latency to hot command/replay traffic.
    connection.execute_batch("vacuum;")?;
    Ok(EventRetentionReclaimReport::Vacuum { freelist_pages })
}

fn sqlite_pragma_i64(connection: &Connection, name: &str) -> Result<i64> {
    connection
        .query_row(&format!("pragma {name}"), [], |row| row.get(0))
        .map_err(Into::into)
}

fn upsert_mobile_session_mini(
    connection: &Connection,
    mini: &MobileSessionMiniProjectionInput,
    seq: i64,
    revision: &str,
    updated_at_ms: i64,
) -> Result<MobileSessionMiniRecord> {
    let body_json = mobile_session_mini_body_json(
        &mini.body_json,
        &mini.session_id,
        &mini.assistant_surface,
        seq,
        revision,
    );
    connection.execute(
        "insert into mobile_session_minis (
            session_id, assistant_surface, seq, revision, body_json, updated_at_ms
         ) values (?1, ?2, ?3, ?4, ?5, ?6)
         on conflict(session_id, assistant_surface) do update set
            seq = excluded.seq,
            revision = excluded.revision,
            body_json = excluded.body_json,
            updated_at_ms = excluded.updated_at_ms",
        params![
            mini.session_id.as_str(),
            mini.assistant_surface.as_str(),
            seq,
            revision,
            body_json.as_str(),
            updated_at_ms,
        ],
    )?;
    Ok(MobileSessionMiniRecord {
        session_id: mini.session_id.clone(),
        assistant_surface: mini.assistant_surface.clone(),
        seq,
        revision: revision.to_owned(),
        body_json,
        updated_at_ms,
    })
}

fn replace_mobile_session_minis_in_transaction(
    connection: &Connection,
    minis: Vec<MobileSessionMiniProjectionInput>,
    seq: i64,
    revision: &str,
    updated_at_ms: i64,
) -> Result<Vec<MobileSessionMiniRecord>> {
    connection.execute("delete from mobile_session_minis", [])?;
    connection.execute(
        "insert or replace into mobile_session_mini_replacements
         (seq, revision, updated_at_ms)
         values (?1, ?2, ?3)",
        params![seq, revision, updated_at_ms],
    )?;
    let mut records = Vec::with_capacity(minis.len());
    for mini in minis {
        records.push(upsert_mobile_session_mini(
            connection,
            &mini,
            seq,
            revision,
            updated_at_ms,
        )?);
    }
    Ok(records)
}

fn mobile_session_minis(connection: &Connection) -> Result<Vec<MobileSessionMiniRecord>> {
    let mut statement = connection.prepare(
        "select session_id, assistant_surface, seq, revision, body_json, updated_at_ms
         from mobile_session_minis
         order by seq asc, assistant_surface asc, session_id asc",
    )?;
    let rows = statement.query_map([], mobile_session_mini_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn mobile_session_minis_for_session(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<MobileSessionMiniRecord>> {
    let mut statement = connection.prepare(
        "select session_id, assistant_surface, seq, revision, body_json, updated_at_ms
         from mobile_session_minis
         where session_id = ?1
         order by assistant_surface asc",
    )?;
    let rows = statement.query_map(params![session_id], mobile_session_mini_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn latest_mobile_session_mini_revision(connection: &Connection) -> Result<Option<String>> {
    connection
        .query_row(
            "select revision
             from mobile_session_minis
             where revision != ''
             order by seq desc
             limit 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn has_mobile_session_minis(connection: &Connection) -> Result<bool> {
    let count: i64 = connection.query_row(
        "select exists(select 1 from mobile_session_minis limit 1)",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn mobile_session_minis_at_seq(
    connection: &Connection,
    seq: i64,
) -> Result<Vec<MobileSessionMiniRecord>> {
    let mut statement = connection.prepare(
        "select session_id, assistant_surface, seq, revision, body_json, updated_at_ms
         from mobile_session_minis
         where seq = ?1
         order by assistant_surface asc, session_id asc",
    )?;
    let rows = statement.query_map(params![seq], mobile_session_mini_row)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn mobile_session_minis_replaced_at_seq(connection: &Connection, seq: i64) -> Result<bool> {
    let count: i64 = connection.query_row(
        "select count(*) from mobile_session_mini_replacements where seq = ?1",
        params![seq],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn latest_mobile_session_mini_replacement_event_seq_after(
    connection: &Connection,
    after_seq: i64,
) -> Result<Option<i64>> {
    if after_seq >= 0 {
        let latest_seq = latest_mobile_state_event_seq(connection)?;
        let oldest_seq = oldest_mobile_state_event_seq(connection)?;
        if let Some(gap) = mobile_state_event_gap(after_seq, oldest_seq, latest_seq) {
            return Err(gap.into());
        }
    }
    connection
        .query_row(
            "select max(replacements.seq)
             from mobile_session_mini_replacements replacements
             where replacements.seq > ?1
               and exists (
                 select 1
                 from mobile_state_event_log events
                 where events.seq = replacements.seq
               )",
            params![after_seq],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn mobile_session_mini_replacement_revision_at_seq(
    connection: &Connection,
    seq: i64,
) -> Result<Option<String>> {
    connection
        .query_row(
            "select replacements.revision
             from mobile_session_mini_replacements replacements
             where replacements.seq = ?1
               and exists (
                 select 1
                 from mobile_state_event_log events
                 where events.seq = replacements.seq
               )",
            params![seq],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn mobile_session_mini_body_json(
    body_json: &Value,
    session_id: &str,
    assistant_surface: &str,
    seq: i64,
    _revision: &str,
) -> String {
    let mut body_json =
        normalized_mobile_session_mini_body(body_json, session_id, assistant_surface);
    if let Some(body_object) = body_json.as_object_mut() {
        body_object.insert("seq".to_owned(), serde_json::json!(seq));
    }
    body_json.to_string()
}

/// Strips the same non-content fields that storage strips (`revision`, `globalSettings`,
/// `metadata.spawn/sources/tags`) and pins `id`/`sessionId`/`assistantSurface` to the given
/// identity, but — unlike `mobile_session_mini_body_json` — leaves `seq` out entirely. `seq`
/// is write-time metadata, not projection content, so including it would make every reconcile
/// look "changed" purely because the log advanced.
fn normalized_mobile_session_mini_body(
    body_json: &Value,
    session_id: &str,
    assistant_surface: &str,
) -> Value {
    let mut body_json = body_json.clone();
    if let Some(body_object) = body_json.as_object_mut() {
        body_object.remove("revision");
        body_object.remove("globalSettings");
        body_object.remove("seq");
        if let Some(metadata) = body_object
            .get_mut("metadata")
            .and_then(serde_json::Value::as_object_mut)
        {
            metadata.remove("spawn");
            metadata.remove("sources");
            metadata.remove("tags");
        }
        body_object.insert("id".to_owned(), serde_json::json!(session_id));
        body_object.insert("sessionId".to_owned(), serde_json::json!(session_id));
        body_object.insert(
            "assistantSurface".to_owned(),
            serde_json::json!(assistant_surface),
        );
    }
    body_json
}

/// A content-only fingerprint for a session-mini projection body, suitable for detecting
/// whether a freshly computed projection differs from what is already stored — independent of
/// `seq` (which always changes) and the control-only fields storage strips on write. Used by
/// `ControlPlane::stored_mobile_session_mini_projection_matches` so a reconcile can skip the
/// write only when the actual content is unchanged, not just when the same set of sessions is
/// present (the previous key-set-only check silently missed in-place field changes, which is
/// why a `force` flag existed as a workaround).
pub fn mobile_session_mini_content_fingerprint(
    body_json: &Value,
    session_id: &str,
    assistant_surface: &str,
) -> String {
    normalized_mobile_session_mini_body(body_json, session_id, assistant_surface).to_string()
}

fn mobile_session_mini_row(row: &Row<'_>) -> rusqlite::Result<MobileSessionMiniRecord> {
    Ok(MobileSessionMiniRecord {
        session_id: row.get(0)?,
        assistant_surface: row.get(1)?,
        seq: row.get(2)?,
        revision: row.get(3)?,
        body_json: row.get(4)?,
        updated_at_ms: row.get(5)?,
    })
}

fn is_stale_mobile_command_reservation(record: &MobileCommandAckRecord, now_ms: i64) -> bool {
    record.ack_seq == 0
        && now_ms.saturating_sub(record.created_at_ms) >= MOBILE_COMMAND_RESERVATION_STALE_AFTER_MS
}

fn mobile_state_event_input_for_mobile_event(event: &MobileEvent) -> MobileStateEventInput {
    MobileStateEventInput {
        entity_id: mobile_event_entity_id(event),
        kind: event.event_type,
        revision: event.revision.clone().unwrap_or_default(),
        server_time: event.server_time.clone(),
        payload_json: serde_json::to_value(event).unwrap_or_else(|_| serde_json::json!({})),
        client_mutation_id: None,
        command_kind: None,
        command_request_hash: None,
        command_response_json: None,
    }
}

fn mobile_event_entity_id(event: &MobileEvent) -> String {
    event
        .thread_id
        .clone()
        .or_else(|| event.prompt_id.clone())
        .unwrap_or_else(|| DEFAULT_MOBILE_ENTITY_ID.to_owned())
}

fn mobile_state_event_row(row: &Row<'_>) -> rusqlite::Result<MobileStateEventRecord> {
    let kind = parse_mobile_event_kind(&row.get::<_, String>(2)?);
    Ok(MobileStateEventRecord {
        seq: row.get(0)?,
        entity_id: row.get(1)?,
        kind,
        revision: row.get(3)?,
        server_time: row.get(4)?,
        payload_json: row.get(5)?,
        client_mutation_id: row.get(6)?,
        command_kind: row.get(7)?,
        command_request_hash: row.get(8)?,
        command_response_json: row.get(9)?,
        created_at_ms: row.get(10)?,
    })
}

fn mobile_event_record_row(row: &Row<'_>) -> rusqlite::Result<MobileEventRecord> {
    let seq = row.get(0)?;
    let event_type = parse_mobile_event_kind(&row.get::<_, String>(1)?);
    let payload_json: String = row.get(2)?;
    let payload = serde_json::from_str::<Value>(&payload_json).unwrap_or(Value::Null);
    let created_at_ms = row.get(3)?;
    let legacy_event_id: Option<String> = row.get(4)?;
    Ok(MobileEventRecord {
        event_id: legacy_event_id.unwrap_or_else(|| mobile_state_event_id(seq)),
        event_type,
        thread_id: payload_string(&payload, "threadId")
            .or_else(|| payload_string(&payload, "thread_id")),
        prompt_id: payload_string(&payload, "promptId")
            .or_else(|| payload_string(&payload, "prompt_id")),
        detail: payload_string(&payload, "detail"),
        created_at_ms,
    })
}

fn mobile_event_record_from_state_record(
    record: &MobileStateEventRecord,
    legacy_event_id: Option<String>,
) -> MobileEventRecord {
    let payload = serde_json::from_str::<Value>(&record.payload_json).unwrap_or(Value::Null);
    MobileEventRecord {
        event_id: legacy_event_id.unwrap_or_else(|| mobile_state_event_id(record.seq)),
        event_type: record.kind,
        thread_id: payload_string(&payload, "threadId")
            .or_else(|| payload_string(&payload, "thread_id")),
        prompt_id: payload_string(&payload, "promptId")
            .or_else(|| payload_string(&payload, "prompt_id")),
        detail: payload_string(&payload, "detail"),
        created_at_ms: record.created_at_ms,
    }
}

fn payload_string(payload: &Value, key: &str) -> Option<String> {
    payload.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn mobile_event_cursor_seq(connection: &Connection, cursor: &MobileEventCursor) -> Result<i64> {
    if cursor.event_id.is_empty() {
        return Ok(0);
    }
    if let Some(seq) = parse_mobile_state_event_id(&cursor.event_id) {
        return Ok(seq);
    }
    let legacy_seq = connection
        .query_row(
            "select seq
             from mobile_state_event_log
             where legacy_event_id = ?1
             order by seq desc
             limit 1",
            [&cursor.event_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(legacy_seq.unwrap_or_default())
}

fn mobile_state_event_seq_for_legacy_event_id(
    connection: &Connection,
    legacy_event_id: &str,
) -> Result<Option<i64>> {
    connection
        .query_row(
            "select seq
             from mobile_state_event_log
             where legacy_event_id = ?1
             order by seq desc
             limit 1",
            [legacy_event_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

fn mobile_state_event_id(seq: i64) -> String {
    format!(
        "{MOBILE_STATE_EVENT_ID_PREFIX}{seq:0width$}",
        width = MOBILE_STATE_EVENT_ID_WIDTH
    )
}

fn parse_mobile_state_event_id(value: &str) -> Option<i64> {
    value
        .strip_prefix(MOBILE_STATE_EVENT_ID_PREFIX)?
        .parse::<i64>()
        .ok()
}

fn latest_mobile_state_event_seq(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "select coalesce(max(seq), 0) from mobile_state_event_log",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn oldest_mobile_state_event_seq(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "select coalesce(min(seq), 0) from mobile_state_event_log",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn mobile_state_event_gap(
    after_seq: i64,
    oldest_seq: i64,
    latest_seq: i64,
) -> Option<MobileStateEventGap> {
    if after_seq > latest_seq {
        return Some(MobileStateEventGap {
            requested_after_seq: after_seq,
            latest_seq,
            oldest_seq,
        });
    }
    if oldest_seq > 0 && after_seq < oldest_seq.saturating_sub(1) {
        return Some(MobileStateEventGap {
            requested_after_seq: after_seq,
            latest_seq,
            oldest_seq,
        });
    }
    None
}

fn mobile_command_ack(
    connection: &Connection,
    command_kind: &str,
    client_mutation_id: &str,
) -> Result<Option<MobileCommandAckRecord>> {
    connection
        .query_row(
            "select command_kind, client_mutation_id, request_hash, ack_seq, response_json,
                    created_at_ms
             from mobile_command_log
             where command_kind = ?1 and client_mutation_id = ?2
             limit 1",
            params![command_kind, client_mutation_id],
            |row| {
                Ok(MobileCommandAckRecord {
                    command_kind: row.get(0)?,
                    client_mutation_id: row.get(1)?,
                    request_hash: row.get(2)?,
                    ack_seq: row.get(3)?,
                    response_json: row.get(4)?,
                    created_at_ms: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn current_time_millis() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / NANOS_PER_MILLISECOND) as i64
}

fn timestamp_millis_to_iso(timestamp_millis: i64) -> Option<String> {
    let timestamp_nanos = i128::from(timestamp_millis).checked_mul(NANOS_PER_MILLISECOND)?;
    time::OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

fn parse_mobile_event_kind(value: &str) -> MobileEventKind {
    match value {
        "prompt.queued" => MobileEventKind::PromptQueued,
        "prompt.delivered" => MobileEventKind::PromptDelivered,
        "lifecycle.changed" => MobileEventKind::LifecycleChanged,
        _ => MobileEventKind::SessionChanged,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EventRetentionReclaimMode, EventStore, MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS,
        MOBILE_STATE_EVENT_ENTITY_RETENTION_ROWS, MOBILE_STATE_EVENT_RETENTION_AGE_MS,
        MobileCommandAckInput, MobileCommandAckResult, MobileEventCursor, MobileStateEventGap,
        MobileStateEventInput, current_time_millis, insert_mobile_state_event,
        mobile_session_mini_content_fingerprint,
    };
    use crate::mobile::events::MobileEventKind;
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    #[test]
    fn mobile_events_after_replays_same_millisecond_records_by_event_id() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        for event_id in ["event-a", "event-b", "event-c"] {
            connection
                .execute(
                    "insert into mobile_event_log (
                        event_id, event_type, thread_id, prompt_id, detail, created_at_ms
                    ) values (?1, 'session.changed', null, null, null, ?2)",
                    params![event_id, 42_i64],
                )
                .expect("insert event");
        }
        connection
            .execute(
                "insert into mobile_event_log (
                    event_id, event_type, thread_id, prompt_id, detail, created_at_ms
                ) values ('event-d', 'prompt.queued', null, 'prompt-id', null, 43)",
                [],
            )
            .expect("insert later event");

        let records = store
            .mobile_events_after(
                &MobileEventCursor {
                    created_at_ms: 42,
                    event_id: "event-a".to_owned(),
                },
                10,
            )
            .expect("events after cursor");

        assert_eq!(
            records
                .iter()
                .map(|record| record.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-b", "event-c", "event-d"]
        );
        assert_eq!(records[2].event_type, MobileEventKind::PromptQueued);
    }

    #[test]
    fn legacy_mobile_state_event_insert_is_idempotent() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        let input = MobileStateEventInput {
            entity_id: "thread-main".to_owned(),
            kind: MobileEventKind::SessionChanged,
            revision: "revision-1".to_owned(),
            server_time: "2026-06-24T00:00:00Z".to_owned(),
            payload_json: serde_json::json!({
                "threadId": "thread-main",
                "detail": "legacy-migration",
            }),
            client_mutation_id: None,
            command_kind: None,
            command_request_hash: None,
            command_response_json: None,
        };

        let first = insert_mobile_state_event(&connection, &input, 42, Some("event-a"))
            .expect("first insert");
        let duplicate = insert_mobile_state_event(&connection, &input, 42, Some("event-a"))
            .expect("duplicate insert");

        assert_eq!(duplicate.seq, first.seq);
        assert_eq!(
            store
                .mobile_state_events_after_seq(0, 10)
                .expect("state events")
                .len(),
            1
        );
    }

    #[test]
    fn ensure_initialized_runs_schema_setup_exactly_once_per_path() {
        let tempdir = tempdir().expect("tempdir");
        let path = tempdir.path().join("events.sqlite");
        let store = EventStore::new(path.clone());
        store
            .ensure_initialized()
            .expect("first ensure_initialized");
        // A second `EventStore` pointed at the same path (as happens whenever `ControlPlane`
        // is cloned across async tasks) must observe the process-wide cache too, not just the
        // original instance.
        let cloned_store = EventStore::new(path);
        cloned_store
            .ensure_initialized()
            .expect("second ensure_initialized via a distinct EventStore for the same path");

        // The schema must actually be in place after the memoized path: a real query against
        // a table created only by `initialize` should succeed.
        assert_eq!(
            store
                .mobile_state_events_after_seq(0, 10)
                .expect("query succeeds once schema is initialized")
                .len(),
            0
        );
    }

    #[test]
    fn mobile_state_event_retention_respects_entity_cap_age_cutoff_and_newest_event() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        let now_ms = MOBILE_STATE_EVENT_RETENTION_AGE_MS * 2;
        let old_ms = now_ms - MOBILE_STATE_EVENT_RETENTION_AGE_MS - 1;

        seed_mobile_state_event_log_rows(
            &connection,
            "thread-cap",
            MOBILE_STATE_EVENT_ENTITY_RETENTION_ROWS + 20,
            now_ms,
        );
        seed_mobile_state_event_log_rows(&connection, "thread-old", 3, old_ms);
        seed_mobile_state_event_log_rows(&connection, "thread-recent-small", 3, now_ms);

        let report = store
            .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Periodic)
            .expect("prune retention");
        assert_eq!(
            report.deleted_mobile_state_events as i64, 22,
            "must delete rows outside the entity cap and age window"
        );
        assert_eq!(
            row_count_for_entity(&connection, "thread-cap"),
            MOBILE_STATE_EVENT_ENTITY_RETENTION_ROWS,
            "must keep only the newest capped rows for a hot entity"
        );
        assert_eq!(row_count_for_entity(&connection, "thread-old"), 1);
        assert_eq!(row_count_for_entity(&connection, "thread-recent-small"), 3);

        let (old_min_seq, old_max_seq) = entity_seq_bounds(&connection, "thread-old");
        assert_eq!(
            old_min_seq, old_max_seq,
            "must keep the newest event even when the whole entity is older than the cutoff"
        );
    }

    #[test]
    fn mobile_event_retention_keeps_recent_command_ack_idempotency_replay() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        let first = store
            .record_mobile_command_ack(command_ack_input("sha256:recent", "resumed"))
            .expect("record recent command ack");
        let first_record = match first {
            MobileCommandAckResult::Recorded(record) => record,
            other => panic!("expected recorded ack, got {other:?}"),
        };
        let now_ms = current_time_millis();
        let old_ms = now_ms - MOBILE_STATE_EVENT_RETENTION_AGE_MS - 1;
        seed_mobile_command_log_rows(&connection, 5, old_ms);

        let report = store
            .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Periodic)
            .expect("prune retention");
        assert_eq!(report.deleted_mobile_command_acks, 5);

        let duplicate = store
            .record_mobile_command_ack(command_ack_input("sha256:recent", "ignored"))
            .expect("duplicate command ack");
        let duplicate_record = match duplicate {
            MobileCommandAckResult::Duplicate(record) => record,
            other => panic!("expected duplicate ack, got {other:?}"),
        };
        assert_eq!(duplicate_record.ack_seq, first_record.ack_seq);
        assert_eq!(duplicate_record.response_json, first_record.response_json);
        assert_eq!(
            store
                .mobile_command_ack("SendSessionPrompt", "retention-command-mutation")
                .expect("mobile command ack")
                .expect("recent command ack survives"),
            first_record
        );
    }

    #[test]
    fn mobile_state_events_after_seq_reports_gap_when_requested_seq_was_pruned() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        let now_ms = MOBILE_STATE_EVENT_RETENTION_AGE_MS * 2;
        let old_ms = now_ms - MOBILE_STATE_EVENT_RETENTION_AGE_MS - 1;
        seed_mobile_state_event_log_rows(&connection, "thread-main", 3, old_ms);

        let report = store
            .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Periodic)
            .expect("prune retention");
        assert_eq!(report.deleted_mobile_state_events, 2);

        let error = store
            .mobile_state_events_after_seq(0, 10)
            .expect_err("after_seq older than retained history must report a gap");
        let gap = error
            .downcast_ref::<MobileStateEventGap>()
            .expect("typed mobile state event gap");
        assert_eq!(gap.requested_after_seq, 0);
        assert_eq!(gap.oldest_seq, 3);
        assert_eq!(gap.latest_seq, 3);

        let replay = store
            .mobile_state_events_after_seq(2, 10)
            .expect("replay from retained floor");
        assert_eq!(
            replay.iter().map(|record| record.seq).collect::<Vec<_>>(),
            vec![3]
        );
    }

    #[test]
    fn mobile_event_retention_batched_delete_terminates_for_thousands_of_events() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open events");
        let now_ms = MOBILE_STATE_EVENT_RETENTION_AGE_MS * 2;
        let old_ms = now_ms - MOBILE_STATE_EVENT_RETENTION_AGE_MS - 1;
        let seeded_rows = MOBILE_EVENT_RETENTION_PRUNE_BATCH_ROWS * 3 + 25;
        seed_mobile_state_event_log_rows(&connection, "thread-batch", seeded_rows, old_ms);

        let report = store
            .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Periodic)
            .expect("prune retention");

        assert_eq!(
            report.deleted_mobile_state_events as i64,
            seeded_rows - 1,
            "must drain more than one delete batch and keep the newest entity event"
        );
        assert_eq!(row_count_for_entity(&connection, "thread-batch"), 1);
    }

    #[test]
    fn mobile_session_mini_content_fingerprint_ignores_seq_and_stripped_fields_but_not_content() {
        let same_content_different_seq_and_revision = mobile_session_mini_content_fingerprint(
            &serde_json::json!({
                "seq": 1,
                "revision": "rev-a",
                "globalSettings": {"anything": true},
                "isArchived": false,
                "lifecycle": "active",
            }),
            "thread-main",
            "codex",
        );
        let same_content_different_seq_and_revision_2 = mobile_session_mini_content_fingerprint(
            &serde_json::json!({
                "seq": 99,
                "revision": "rev-z",
                "globalSettings": {"anything else": 1},
                "isArchived": false,
                "lifecycle": "active",
            }),
            "thread-main",
            "codex",
        );
        assert_eq!(
            same_content_different_seq_and_revision, same_content_different_seq_and_revision_2,
            "seq/revision/globalSettings must not affect the content fingerprint"
        );

        let different_lifecycle = mobile_session_mini_content_fingerprint(
            &serde_json::json!({
                "seq": 1,
                "revision": "rev-a",
                "isArchived": false,
                "lifecycle": "idle",
            }),
            "thread-main",
            "codex",
        );
        assert_ne!(
            same_content_different_seq_and_revision, different_lifecycle,
            "an actual content change (lifecycle) must change the fingerprint"
        );
    }

    fn command_ack_input(request_hash: &str, dispatch_kind: &str) -> MobileCommandAckInput {
        MobileCommandAckInput {
            command_kind: "SendSessionPrompt".to_owned(),
            client_mutation_id: "retention-command-mutation".to_owned(),
            request_hash: request_hash.to_owned(),
            response_json: serde_json::json!({
                "accepted": true,
                "dispatchKind": dispatch_kind,
            }),
            state_event: MobileStateEventInput {
                entity_id: "thread-main".to_owned(),
                kind: MobileEventKind::SessionChanged,
                revision: "revision-command-ack".to_owned(),
                server_time: "2026-06-24T00:00:00Z".to_owned(),
                payload_json: serde_json::json!({
                    "threadId": "thread-main",
                    "detail": "prompt-resumed",
                }),
                client_mutation_id: None,
                command_kind: None,
                command_request_hash: None,
                command_response_json: None,
            },
        }
    }

    fn seed_mobile_state_event_log_rows(
        connection: &Connection,
        entity_id: &str,
        count: i64,
        created_at_ms: i64,
    ) {
        let transaction_connection = connection.unchecked_transaction().expect("transaction");
        for index in 0..count {
            transaction_connection
                .execute(
                    "insert into mobile_state_event_log (
                        entity_id, kind, revision, server_time, payload_json, created_at_ms
                    ) values (?1, 'session.changed', ?2, '', '{}', ?3)",
                    params![entity_id, format!("revision-{index}"), created_at_ms],
                )
                .expect("seed state event row");
        }
        transaction_connection.commit().expect("commit seed rows");
    }

    fn seed_mobile_command_log_rows(connection: &Connection, count: i64, created_at_ms: i64) {
        let transaction_connection = connection.unchecked_transaction().expect("transaction");
        for index in 0..count {
            transaction_connection
                .execute(
                    "insert into mobile_command_log (
                        command_kind, client_mutation_id, request_hash, ack_seq, response_json,
                        created_at_ms
                    ) values ('SendSessionPrompt', ?1, 'hash', 1, '{}', ?2)",
                    params![format!("old-mutation-{index}"), created_at_ms],
                )
                .expect("seed command log row");
        }
        transaction_connection.commit().expect("commit seed rows");
    }

    fn row_count_for_entity(connection: &Connection, entity_id: &str) -> i64 {
        connection
            .query_row(
                "select count(*) from mobile_state_event_log where entity_id = ?1",
                [entity_id],
                |row| row.get(0),
            )
            .expect("entity row count")
    }

    fn entity_seq_bounds(connection: &Connection, entity_id: &str) -> (i64, i64) {
        connection
            .query_row(
                "select min(seq), max(seq) from mobile_state_event_log where entity_id = ?1",
                [entity_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("entity seq bounds")
    }
}

// allow: SIZE_OK — event store boundary keeps append, cursor, replay, and serialization semantics in one ordered log module.
use std::fmt;
use std::path::{Path, PathBuf};

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
}

impl fmt::Display for MobileStateEventGap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "mobile state event replay gap: requested after seq {} but latest seq is {}",
            self.requested_after_seq, self.latest_seq
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
"#,
        )?;
        migrate_mobile_session_minis_schema(&connection)?;
        connection.execute_batch(
            r#"
create index if not exists mobile_session_minis_seq
  on mobile_session_minis(seq asc, assistant_surface asc, session_id asc);
"#,
        )?;
        migrate_legacy_mobile_events(&connection)?;
        Ok(())
    }

    pub fn record_mobile_event(&self, event: &MobileEvent) -> Result<MobileEventRecord> {
        self.initialize()?;
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
        self.initialize()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let input = mobile_state_event_input_for_mobile_event(event);
        let state_record = insert_mobile_state_event(&transaction, &input, created_at_ms, None)?;
        for mini in minis {
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
        self.initialize()?;
        let mut connection = Connection::open(&self.path)?;
        let transaction = connection.transaction()?;
        let created_at_ms = current_time_millis();
        let input = mobile_state_event_input_for_mobile_event(event);
        let state_record = insert_mobile_state_event(&transaction, &input, created_at_ms, None)?;
        replace_mobile_session_minis_in_transaction(
            &transaction,
            minis,
            state_record.seq,
            &state_record.revision,
            created_at_ms,
        )?;
        transaction.commit()?;
        Ok(mobile_event_record_from_state_record(&state_record, None))
    }

    pub fn record_mobile_state_event(
        &self,
        input: MobileStateEventInput,
    ) -> Result<MobileStateEventRecord> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        insert_mobile_state_event(&connection, &input, current_time_millis(), None)
    }

    pub fn record_mobile_state_event_with_session_mini(
        &self,
        input: MobileStateEventInput,
        mini: MobileSessionMiniProjectionInput,
    ) -> Result<MobileStateEventRecord> {
        self.initialize()?;
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
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        upsert_mobile_session_mini(&connection, &mini, seq, revision, current_time_millis())
    }

    pub fn replace_mobile_session_minis(
        &self,
        minis: Vec<MobileSessionMiniProjectionInput>,
        seq: i64,
        revision: &str,
    ) -> Result<Vec<MobileSessionMiniRecord>> {
        self.initialize()?;
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
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        mobile_session_minis(&connection)
    }

    pub fn latest_mobile_session_mini_snapshot(&self) -> Result<MobileSessionMiniSnapshotRecord> {
        self.initialize()?;
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
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let latest_seq = latest_mobile_state_event_seq(&connection)?;
        if after_seq > latest_seq {
            return Err(MobileStateEventGap {
                requested_after_seq: after_seq,
                latest_seq,
            }
            .into());
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
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let latest_seq = latest_mobile_state_event_seq(&connection)?;
        if after_seq > latest_seq {
            return Err(MobileStateEventGap {
                requested_after_seq: after_seq,
                latest_seq,
            }
            .into());
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

    pub fn latest_mobile_state_event_seq(&self) -> Result<i64> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        latest_mobile_state_event_seq(&connection)
    }

    pub fn mobile_command_ack(
        &self,
        command_kind: &str,
        client_mutation_id: &str,
    ) -> Result<Option<MobileCommandAckRecord>> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        mobile_command_ack(&connection, command_kind, client_mutation_id)
    }

    pub fn reserve_mobile_command_ack(
        &self,
        command_kind: &str,
        client_mutation_id: &str,
        request_hash: &str,
    ) -> Result<MobileCommandReservationResult> {
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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
        self.initialize()?;
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

fn upsert_mobile_session_mini(
    connection: &Connection,
    mini: &MobileSessionMiniProjectionInput,
    seq: i64,
    revision: &str,
    updated_at_ms: i64,
) -> Result<MobileSessionMiniRecord> {
    let body_json = mobile_session_mini_body_json(&mini.body_json, seq, revision);
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

fn mobile_session_mini_body_json(body_json: &Value, seq: i64, revision: &str) -> String {
    let mut body_json = body_json.clone();
    if let Some(body_json) = body_json.as_object_mut() {
        body_json.insert("seq".to_owned(), serde_json::json!(seq));
        body_json.insert("revision".to_owned(), serde_json::json!(revision));
    }
    body_json.to_string()
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
    use super::{EventStore, MobileEventCursor, MobileStateEventInput, insert_mobile_state_event};
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
}

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::mobile_events::{
    MobileEvent, MobileEventKind, MobileEventRecord, mobile_event_sse_name,
};

const ENABLED_SETTING: i64 = 1;
const DISABLED_SETTING: i64 = 0;

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
"#,
        )?;
        Ok(())
    }

    pub fn record_mobile_event(&self, event: &MobileEvent) -> Result<MobileEventRecord> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let created_at_ms =
            (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64;
        let record = MobileEventRecord {
            event_id: format!("mobile-event-{}", uuid::Uuid::new_v4()),
            event_type: event.event_type,
            thread_id: event.thread_id.clone(),
            prompt_id: event.prompt_id.clone(),
            detail: event.detail.clone(),
            created_at_ms,
        };
        connection.execute(
            "insert into mobile_event_log (
                event_id, event_type, thread_id, prompt_id, detail, created_at_ms
             ) values (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.event_id,
                mobile_event_sse_name(record.event_type),
                record.thread_id,
                record.prompt_id,
                record.detail,
                record.created_at_ms,
            ],
        )?;
        Ok(record)
    }

    pub fn mobile_events_since(
        &self,
        since_created_at_ms: i64,
        limit: usize,
    ) -> Result<Vec<MobileEventRecord>> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select event_id, event_type, thread_id, prompt_id, detail, created_at_ms
             from mobile_event_log
             where created_at_ms > ?1
             order by created_at_ms asc, event_id asc
             limit ?2",
        )?;
        let rows = statement.query_map(params![since_created_at_ms, limit as i64], |row| {
            let event_type = parse_mobile_event_kind(&row.get::<_, String>(1)?);
            Ok(MobileEventRecord {
                event_id: row.get(0)?,
                event_type,
                thread_id: row.get(2)?,
                prompt_id: row.get(3)?,
                detail: row.get(4)?,
                created_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn mobile_events_after(
        &self,
        cursor: &MobileEventCursor,
        limit: usize,
    ) -> Result<Vec<MobileEventRecord>> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let mut statement = connection.prepare(
            "select event_id, event_type, thread_id, prompt_id, detail, created_at_ms
             from mobile_event_log
             where created_at_ms > ?1
                or (created_at_ms = ?1 and event_id > ?2)
             order by created_at_ms asc, event_id asc
             limit ?3",
        )?;
        let rows = statement.query_map(
            params![cursor.created_at_ms, cursor.event_id, limit as i64],
            |row| {
                let event_type = parse_mobile_event_kind(&row.get::<_, String>(1)?);
                Ok(MobileEventRecord {
                    event_id: row.get(0)?,
                    event_type,
                    thread_id: row.get(2)?,
                    prompt_id: row.get(3)?,
                    detail: row.get(4)?,
                    created_at_ms: row.get(5)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_automation_run(
        &self,
        automation_id: &str,
        target_thread_id: Option<&str>,
        scheduled_at_ms: i64,
        fired_at_ms: i64,
        delivery_mode: &str,
        result: &str,
        detail: Option<&str>,
    ) -> Result<Option<AutomationRunRecord>> {
        self.initialize()?;
        let connection = Connection::open(&self.path)?;
        let existing: Option<String> = connection
            .query_row(
                "select run_id from automation_runs where automation_id = ?1 and scheduled_at_ms = ?2",
                params![automation_id, scheduled_at_ms],
                |row| row.get(0),
            )
            .optional()?;
        if existing.is_some() {
            return Ok(None);
        }

        let record = AutomationRunRecord {
            run_id: format!("automation-run-{}", uuid::Uuid::new_v4()),
            automation_id: automation_id.to_owned(),
            target_thread_id: target_thread_id.map(str::to_owned),
            scheduled_at_ms,
            fired_at_ms,
            delivery_mode: delivery_mode.to_owned(),
            result: result.to_owned(),
            detail: detail.map(str::to_owned),
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
                "select coalesce(max(created_at_ms), 0) from mobile_event_log",
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
                "select event_id, created_at_ms
                 from mobile_event_log
                 order by created_at_ms desc, event_id desc
                 limit 1",
                [],
                |row| {
                    Ok(MobileEventCursor {
                        event_id: row.get(0)?,
                        created_at_ms: row.get(1)?,
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
    use super::{EventStore, MobileEventCursor};
    use crate::mobile_events::MobileEventKind;
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
}

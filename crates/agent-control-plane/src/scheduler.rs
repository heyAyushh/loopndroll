use anyhow::Result;

use crate::automations::read_automations;
use crate::control_plane::ControlPlane;
use crate::events::{
    AutomationRunInput, AutomationRunRecord, EventRetentionPruneReport, EventRetentionReclaimMode,
    EventStore,
};
use crate::mobile::prompt_delivery::{PromptDispatch, send_non_acp_session_prompt};

const AUTOMATION_DELIVERY_MODE: &str = "local-prompt-dispatch";
const AUTOMATION_RESULT_PENDING: &str = "pending";
const AUTOMATION_RESULT_QUEUED: &str = "queued";
const AUTOMATION_RESULT_RESUMED: &str = "resumed";
const AUTOMATION_RESULT_DELIVERED: &str = "delivered";
const AUTOMATION_RESULT_FAILED: &str = "failed";
const EVENT_RETENTION_PRUNE_INTERVAL_MS: i64 = 6 * 60 * 60 * 1_000;

pub struct AutomationRunner {
    control_plane: ControlPlane,
    retention: EventRetentionRunner,
}

impl AutomationRunner {
    pub fn new(control_plane: ControlPlane) -> Self {
        let retention = EventRetentionRunner::new(control_plane.store().clone());
        Self {
            control_plane,
            retention,
        }
    }

    pub fn tick(&mut self, now_ms: i64) -> Result<Vec<AutomationRunRecord>> {
        if let Some(report) = self.retention.tick(now_ms)? {
            if report.deleted_rows() > 0 {
                eprintln!(
                    "event retention pruned {} state events and {} command acks",
                    report.deleted_mobile_state_events, report.deleted_mobile_command_acks
                );
            }
        }
        let automations = read_automations(self.control_plane.codex_home())?;
        let mut fired = Vec::new();
        for automation in automations {
            let Some(scheduled_at_ms) = automation.due_scheduled_at_ms(now_ms) else {
                continue;
            };
            let Some(record) = self
                .control_plane
                .record_automation_fire(AutomationRunInput {
                    automation_id: &automation.id,
                    target_thread_id: automation.target_thread_id.as_deref(),
                    scheduled_at_ms,
                    fired_at_ms: now_ms,
                    delivery_mode: AUTOMATION_DELIVERY_MODE,
                    result: AUTOMATION_RESULT_PENDING,
                    detail: Some("automation prompt dispatch reserved"),
                })?
            else {
                continue;
            };

            let (result, detail) = match automation.target_thread_id.as_deref() {
                Some(thread_id) if !automation.prompt.trim().is_empty() => {
                    match send_non_acp_session_prompt(
                        &self.control_plane,
                        thread_id,
                        &automation.prompt,
                    ) {
                        Ok(dispatch) => automation_dispatch_result(dispatch),
                        Err(error) => (AUTOMATION_RESULT_FAILED, Some(error.to_string())),
                    }
                }
                Some(_) => (
                    AUTOMATION_RESULT_FAILED,
                    Some("automation prompt is empty".to_owned()),
                ),
                None => (
                    AUTOMATION_RESULT_FAILED,
                    Some("automation target thread is missing".to_owned()),
                ),
            };
            fired.push(self.control_plane.update_automation_fire_result(
                &record.run_id,
                AUTOMATION_DELIVERY_MODE,
                result,
                detail.as_deref(),
            )?);
        }
        Ok(fired)
    }
}

struct EventRetentionRunner {
    store: EventStore,
    startup_prune_complete: bool,
    last_prune_at_ms: Option<i64>,
}

impl EventRetentionRunner {
    fn new(store: EventStore) -> Self {
        Self {
            store,
            startup_prune_complete: false,
            last_prune_at_ms: None,
        }
    }

    fn tick(&mut self, now_ms: i64) -> Result<Option<EventRetentionPruneReport>> {
        if !self.startup_prune_complete {
            let report = self
                .store
                .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Startup)?;
            self.startup_prune_complete = true;
            self.last_prune_at_ms = Some(now_ms);
            return Ok(Some(report));
        }

        let should_prune = self
            .last_prune_at_ms
            .map(|last_prune_at_ms| {
                now_ms.saturating_sub(last_prune_at_ms) >= EVENT_RETENTION_PRUNE_INTERVAL_MS
            })
            .unwrap_or(true);
        if !should_prune {
            return Ok(None);
        }

        let report = self
            .store
            .prune_mobile_event_retention(now_ms, EventRetentionReclaimMode::Periodic)?;
        self.last_prune_at_ms = Some(now_ms);
        Ok(Some(report))
    }
}

fn automation_dispatch_result(dispatch: PromptDispatch) -> (&'static str, Option<String>) {
    match dispatch {
        PromptDispatch::Accepted => (
            AUTOMATION_RESULT_FAILED,
            Some("prompt accepted without synchronous delivery".to_owned()),
        ),
        PromptDispatch::Queued { prompt_id } => (
            AUTOMATION_RESULT_QUEUED,
            Some(format!("queued prompt {prompt_id}")),
        ),
        PromptDispatch::Delivered { prompt_id } => (
            AUTOMATION_RESULT_DELIVERED,
            Some(format!("delivered prompt {prompt_id}")),
        ),
        PromptDispatch::Resumed => (
            AUTOMATION_RESULT_RESUMED,
            Some("resumed target thread".to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{EVENT_RETENTION_PRUNE_INTERVAL_MS, EventRetentionRunner};
    use crate::events::{EventStore, MOBILE_STATE_EVENT_RETENTION_AGE_MS};
    use rusqlite::{Connection, params};
    use tempfile::tempdir;

    #[test]
    fn event_retention_scheduler_runs_startup_then_waits_for_interval() {
        let tempdir = tempdir().expect("tempdir");
        let store = EventStore::new(tempdir.path().join("events.sqlite"));
        store.initialize().expect("initialize");
        let connection = Connection::open(store.path()).expect("open store");
        let now_ms = MOBILE_STATE_EVENT_RETENTION_AGE_MS * 2;
        let old_ms = now_ms - MOBILE_STATE_EVENT_RETENTION_AGE_MS - 1;
        seed_state_events(&connection, 3, old_ms);

        let mut runner = EventRetentionRunner::new(store);
        let startup_report = runner
            .tick(now_ms)
            .expect("startup retention tick")
            .expect("startup report");
        assert_eq!(startup_report.deleted_mobile_state_events, 2);
        assert!(
            runner
                .tick(now_ms + EVENT_RETENTION_PRUNE_INTERVAL_MS - 1)
                .expect("early retention tick")
                .is_none()
        );

        seed_state_events(&connection, 2, old_ms);
        let periodic_report = runner
            .tick(now_ms + EVENT_RETENTION_PRUNE_INTERVAL_MS)
            .expect("periodic retention tick")
            .expect("periodic report");
        assert_eq!(periodic_report.deleted_mobile_state_events, 2);
    }

    fn seed_state_events(connection: &Connection, count: i64, created_at_ms: i64) {
        let transaction = connection.unchecked_transaction().expect("transaction");
        for index in 0..count {
            transaction
                .execute(
                    "insert into mobile_state_event_log (
                        entity_id, kind, revision, server_time, payload_json, created_at_ms
                    ) values ('scheduler-thread', 'session.changed', ?1, '', '{}', ?2)",
                    params![format!("revision-{index}"), created_at_ms],
                )
                .expect("insert state event");
        }
        transaction.commit().expect("commit seed events");
    }
}

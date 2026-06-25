use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::control_plane::session_fsm;

use super::completion_checks::{
    active_completion_check, active_completion_check_wait_for_reply,
    completion_check_failure_reason,
};
use super::normalization::{normalized_optional, normalized_required, now_iso_string};
use super::presets::{
    PROMPT_DELIVERY_MODE_ONCE, PROMPT_DELIVERY_MODE_PERSISTENT, is_persistent_prompt_preset,
    max_turns_for_preset, remote_prompt_delivery_mode, render_prompt,
};
use super::queries::{REMOTE_PROMPT_STATUS_QUEUED, prompt_for_mode};
use super::schema::{
    MOBILE_REMOTE_PROMPTS_TABLE, MOBILE_SESSION_LIFECYCLE_TABLE, MOBILE_SESSION_RUNTIME_TABLE,
};
use super::{
    MobileHookOutcome, MobileHookPayload, MobileQueuedPrompt, MobileSessionError,
    MobileSessionResult, MobileSessionService, MobileSessionState, MobileStopDecision,
};

const STOP_HOOK_EVENT: &str = "Stop";

struct ConsumedPrompt {
    prompt: Option<String>,
    prompt_id: Option<String>,
}

impl MobileSessionService {
    pub fn queue_prompt(
        &self,
        thread_id: &str,
        prompt: &str,
    ) -> MobileSessionResult<MobileQueuedPrompt> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        let prompt = normalized_required(prompt).ok_or(MobileSessionError::PromptRequired)?;
        self.initialize()?;
        let state = self.state()?;
        let override_state = state.sessions.get(&thread_id).cloned().unwrap_or_default();
        if override_state.archived.unwrap_or(false) {
            return Err(MobileSessionError::SessionArchived);
        }
        let preset = override_state
            .preset
            .as_deref()
            .or(state.global_preset.as_deref())
            .ok_or(MobileSessionError::ModeRequired)?;
        let delivery_mode = remote_prompt_delivery_mode(preset);
        let record = MobileQueuedPrompt {
            id: format!("mobile-prompt-{}", uuid::Uuid::new_v4()),
            thread_id,
            prompt,
            status: REMOTE_PROMPT_STATUS_QUEUED.to_owned(),
            delivery_mode: delivery_mode.to_owned(),
            created_at: now_iso_string()?,
        };
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_remote_prompts (
                id, thread_id, prompt, status, delivery_mode, created_at, delivered_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, null)
             on conflict(thread_id, delivery_mode) do update set
                id = excluded.id,
                prompt = excluded.prompt,
                status = excluded.status,
                created_at = excluded.created_at,
                delivered_at = excluded.delivered_at",
            params![
                &record.id,
                &record.thread_id,
                &record.prompt,
                &record.status,
                &record.delivery_mode,
                &record.created_at,
            ],
        )?;
        Ok(record)
    }

    pub fn queued_prompt_counts(&self) -> MobileSessionResult<BTreeMap<String, i64>> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        let mut statement = connection.prepare(
            "select thread_id, count(*)
             from mobile_remote_prompts
             where status = ?1
             group by thread_id",
        )?;
        let rows = statement.query_map([REMOTE_PROMPT_STATUS_QUEUED], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(MobileSessionError::Store)
    }

    pub fn hook_decision_for_payload(
        &self,
        payload: &MobileHookPayload,
    ) -> MobileSessionResult<Option<MobileStopDecision>> {
        Ok(self.hook_outcome_for_payload(payload)?.decision)
    }

    pub fn hook_outcome_for_payload(
        &self,
        payload: &MobileHookPayload,
    ) -> MobileSessionResult<MobileHookOutcome> {
        if payload.hook_event_name != STOP_HOOK_EVENT {
            return Ok(MobileHookOutcome::default());
        }
        let Some(thread_id) = payload.session_id.as_deref().and_then(normalized_optional) else {
            return Ok(MobileHookOutcome::default());
        };
        self.stop_outcome_with_context(&thread_id, payload.cwd.as_deref())
    }

    pub fn record_hook_lifecycle(
        &self,
        payload: &MobileHookPayload,
        did_continue: bool,
    ) -> MobileSessionResult<()> {
        let Some(thread_id) = payload.session_id.as_deref().and_then(normalized_optional) else {
            return Ok(());
        };
        let Some(status) = hook_lifecycle_status(&payload.hook_event_name, did_continue) else {
            return Ok(());
        };
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            &format!(
                "insert into {MOBILE_SESSION_LIFECYCLE_TABLE} (thread_id, status, updated_at)
                 values (?1, ?2, ?3)
                 on conflict(thread_id) do update set
                    status = excluded.status,
                    updated_at = excluded.updated_at"
            ),
            params![thread_id, status, now_iso_string()?],
        )?;
        Ok(())
    }

    pub fn stop_decision(
        &self,
        thread_id: &str,
    ) -> MobileSessionResult<Option<MobileStopDecision>> {
        self.stop_decision_with_context(thread_id, None)
    }

    pub(super) fn clear_runtime(&self, thread_id: &str) -> MobileSessionResult<()> {
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            &format!("delete from {MOBILE_SESSION_RUNTIME_TABLE} where thread_id = ?1"),
            [thread_id],
        )?;
        Ok(())
    }

    pub(super) fn clear_prompts(&self, thread_id: &str) -> MobileSessionResult<()> {
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            &format!("delete from {MOBILE_REMOTE_PROMPTS_TABLE} where thread_id = ?1"),
            [thread_id],
        )?;
        Ok(())
    }

    pub(super) fn clear_prompts_for_mode_change(
        &self,
        thread_id: &str,
        preset: Option<&str>,
    ) -> MobileSessionResult<()> {
        match preset {
            None => self.clear_prompts(thread_id),
            Some(preset) if is_persistent_prompt_preset(preset) => Ok(()),
            Some(_) => {
                self.initialize()?;
                Connection::open(&self.store_path)?.execute(
                    &format!(
                        "delete from {MOBILE_REMOTE_PROMPTS_TABLE}
                         where thread_id = ?1 and delivery_mode = ?2"
                    ),
                    params![thread_id, PROMPT_DELIVERY_MODE_PERSISTENT],
                )?;
                Ok(())
            }
        }
    }

    fn stop_decision_with_context(
        &self,
        thread_id: &str,
        cwd: Option<&str>,
    ) -> MobileSessionResult<Option<MobileStopDecision>> {
        Ok(self.stop_outcome_with_context(thread_id, cwd)?.decision)
    }

    fn stop_outcome_with_context(
        &self,
        thread_id: &str,
        cwd: Option<&str>,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        self.initialize()?;
        let state = self.state()?;
        let Some(override_state) = state.sessions.get(&thread_id) else {
            return self.stop_outcome_for_preset(
                &thread_id,
                &state,
                state.global_preset.as_deref(),
                cwd,
            );
        };
        if override_state.archived.unwrap_or(false) || override_state.deleted {
            self.clear_runtime(&thread_id)?;
            return Ok(MobileHookOutcome::default());
        }

        self.stop_outcome_for_preset(
            &thread_id,
            &state,
            override_state
                .preset
                .as_deref()
                .or(state.global_preset.as_deref()),
            cwd,
        )
    }

    fn stop_outcome_for_preset(
        &self,
        thread_id: &str,
        state: &MobileSessionState,
        preset: Option<&str>,
        cwd: Option<&str>,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let Some(preset) = preset else {
            self.clear_runtime(thread_id)?;
            return Ok(MobileHookOutcome::default());
        };
        match preset {
            "infinite" => self.continue_with_prompt(thread_id, &state.default_prompt, None),
            "await-reply" => self.continue_with_queued_prompt(thread_id),
            "completion-checks" => self.continue_with_completion_check(thread_id, state, cwd),
            _ => self.continue_with_max_turns(thread_id, &state.default_prompt, preset),
        }
    }

    fn continue_with_completion_check(
        &self,
        thread_id: &str,
        state: &MobileSessionState,
        cwd: Option<&str>,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let Some(completion_check) = active_completion_check(thread_id, state) else {
            return Ok(MobileHookOutcome::default());
        };
        let Some(cwd) = cwd.and_then(normalized_optional) else {
            return Ok(MobileHookOutcome::default());
        };
        if let Some(reason) = completion_check_failure_reason(&cwd, completion_check) {
            return Ok(MobileHookOutcome {
                decision: Some(MobileStopDecision {
                    decision: "block".to_owned(),
                    reason,
                }),
                delivered_prompt_id: None,
            });
        }
        if active_completion_check_wait_for_reply(thread_id, state) {
            return self.continue_with_queued_prompt(thread_id);
        }
        Ok(MobileHookOutcome::default())
    }

    fn continue_with_prompt(
        &self,
        thread_id: &str,
        default_prompt: &str,
        remaining_turns: Option<i64>,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let consumed = self.consume_prompt(thread_id)?;
        let prompt = consumed
            .prompt
            .or_else(|| Some(render_prompt(default_prompt, remaining_turns)));
        let Some(prompt) = prompt.filter(|prompt| !prompt.is_empty()) else {
            return Ok(MobileHookOutcome::default());
        };
        Ok(MobileHookOutcome {
            decision: Some(MobileStopDecision {
                decision: "block".to_owned(),
                reason: prompt,
            }),
            delivered_prompt_id: consumed.prompt_id,
        })
    }

    fn continue_with_queued_prompt(
        &self,
        thread_id: &str,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let consumed = self.consume_prompt(thread_id)?;
        let Some(prompt) = consumed.prompt else {
            return Ok(MobileHookOutcome::default());
        };
        Ok(MobileHookOutcome {
            decision: Some(MobileStopDecision {
                decision: "block".to_owned(),
                reason: prompt,
            }),
            delivered_prompt_id: consumed.prompt_id,
        })
    }

    fn continue_with_max_turns(
        &self,
        thread_id: &str,
        default_prompt: &str,
        preset: &str,
    ) -> MobileSessionResult<MobileHookOutcome> {
        let Some(max_turns) = max_turns_for_preset(preset) else {
            self.clear_runtime(thread_id)?;
            return Ok(MobileHookOutcome::default());
        };
        let remaining_turns = self.remaining_turns(thread_id)?.unwrap_or(max_turns);
        if remaining_turns <= 0 {
            self.clear_runtime(thread_id)?;
            return Ok(MobileHookOutcome::default());
        }
        let next_remaining_turns = remaining_turns - 1;
        self.set_remaining_turns(thread_id, next_remaining_turns)?;
        self.continue_with_prompt(thread_id, default_prompt, Some(next_remaining_turns))
    }

    fn consume_prompt(&self, thread_id: &str) -> MobileSessionResult<ConsumedPrompt> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        if let Some((prompt_id, prompt)) = prompt_for_mode(
            &connection,
            thread_id,
            PROMPT_DELIVERY_MODE_PERSISTENT,
            false,
        )? {
            return Ok(ConsumedPrompt {
                prompt: Some(prompt),
                prompt_id: Some(prompt_id),
            });
        }
        let once = prompt_for_mode(&connection, thread_id, PROMPT_DELIVERY_MODE_ONCE, true)?;
        Ok(ConsumedPrompt {
            prompt: once.as_ref().map(|(_, prompt)| prompt.clone()),
            prompt_id: once.map(|(prompt_id, _)| prompt_id),
        })
    }

    fn remaining_turns(&self, thread_id: &str) -> MobileSessionResult<Option<i64>> {
        self.initialize()?;
        Connection::open(&self.store_path)?
            .query_row(
                "select remaining_turns from mobile_session_runtime where thread_id = ?1",
                [thread_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(MobileSessionError::Store)
    }

    fn set_remaining_turns(
        &self,
        thread_id: &str,
        remaining_turns: i64,
    ) -> MobileSessionResult<()> {
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_session_runtime (thread_id, remaining_turns, updated_at)
             values (?1, ?2, ?3)
             on conflict(thread_id) do update set
                remaining_turns = excluded.remaining_turns,
                updated_at = excluded.updated_at",
            params![thread_id, remaining_turns, now_iso_string()?],
        )?;
        Ok(())
    }
}

fn hook_lifecycle_status(hook_event_name: &str, did_continue: bool) -> Option<&'static str> {
    session_fsm::hook_lifecycle_status(hook_event_name, did_continue)
}

use anyhow::Result;

use crate::automations::read_automations;
use crate::control_plane::ControlPlane;
use crate::events::{AutomationRunInput, AutomationRunRecord};
use crate::mobile::prompt_delivery::{PromptDispatch, send_non_acp_session_prompt};

const AUTOMATION_DELIVERY_MODE: &str = "local-prompt-dispatch";
const AUTOMATION_RESULT_PENDING: &str = "pending";
const AUTOMATION_RESULT_QUEUED: &str = "queued";
const AUTOMATION_RESULT_RESUMED: &str = "resumed";
const AUTOMATION_RESULT_DELIVERED: &str = "delivered";
const AUTOMATION_RESULT_FAILED: &str = "failed";

pub struct AutomationRunner {
    control_plane: ControlPlane,
}

impl AutomationRunner {
    pub fn new(control_plane: ControlPlane) -> Self {
        Self { control_plane }
    }

    pub fn tick(&mut self, now_ms: i64) -> Result<Vec<AutomationRunRecord>> {
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

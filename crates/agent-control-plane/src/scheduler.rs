use anyhow::Result;

use crate::automations::read_automations;
use crate::control_plane::ControlPlane;
use crate::events::AutomationRunRecord;

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
            if let Some(record) = self.control_plane.record_automation_fire(
                &automation.id,
                automation.target_thread_id.as_deref(),
                scheduled_at_ms,
                now_ms,
                Some("mirrored Codex automation queued for local delivery"),
            )? {
                fired.push(record);
            }
        }
        Ok(fired)
    }
}

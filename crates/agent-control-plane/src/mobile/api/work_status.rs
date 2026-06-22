use serde_json::{Value, json};

use crate::automations::{AutomationStatus, AutomationSummary};
use crate::goals::GoalSummary;

const MAX_VISIBLE_WORK_ITEMS: usize = 5;

pub(super) fn mobile_work_status(
    goals: &[GoalSummary],
    automations: &[AutomationSummary],
) -> Value {
    let running_goals = goals.iter().filter(|goal| goal.running).collect::<Vec<_>>();
    let active_automations = automations
        .iter()
        .filter(|automation| automation.status == AutomationStatus::Active)
        .collect::<Vec<_>>();
    let covered_automation_count = active_automations
        .iter()
        .filter(|automation| automation.control_plane_covered)
        .count();

    json!({
        "goalCount": goals.len(),
        "runningGoalCount": running_goals.len(),
        "automationCount": automations.len(),
        "activeAutomationCount": active_automations.len(),
        "coveredAutomationCount": covered_automation_count,
        "runningGoals": running_goals
            .into_iter()
            .take(MAX_VISIBLE_WORK_ITEMS)
            .map(mobile_goal_summary)
            .collect::<Vec<_>>(),
        "activeAutomations": active_automations
            .into_iter()
            .take(MAX_VISIBLE_WORK_ITEMS)
            .map(mobile_automation_summary)
            .collect::<Vec<_>>(),
    })
}

fn mobile_goal_summary(goal: &GoalSummary) -> Value {
    json!({
        "id": &goal.id,
        "title": &goal.title,
        "status": &goal.status,
        "targetThreadId": &goal.target_thread_id,
        "targetKnown": goal.target_known,
        "updatedAtMs": goal.updated_at_ms,
        "tokensUsed": goal.tokens_used,
        "tokenBudget": goal.token_budget,
        "timeUsedSeconds": goal.time_used_seconds,
    })
}

fn mobile_automation_summary(automation: &AutomationSummary) -> Value {
    json!({
        "id": &automation.id,
        "kind": &automation.kind,
        "name": &automation.name,
        "status": &automation.status,
        "scheduleSummary": &automation.schedule_summary,
        "targetThreadId": &automation.target_thread_id,
        "targetKnown": automation.target_known,
        "controlPlaneCovered": automation.control_plane_covered,
    })
}

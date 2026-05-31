use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const AUTOMATIONS_DIRECTORY_NAME: &str = "automations";
const AUTOMATION_FILE_NAME: &str = "automation.toml";
const MILLIS_PER_MINUTE: i64 = 60_000;
const MILLIS_PER_HOUR: i64 = 60 * MILLIS_PER_MINUTE;
const MILLIS_PER_DAY: i64 = 24 * MILLIS_PER_HOUR;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutomationKind {
    Heartbeat,
    Cron,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum AutomationStatus {
    Active,
    Paused,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationDefinition {
    pub id: String,
    pub kind: AutomationKind,
    pub name: String,
    pub prompt: String,
    pub status: AutomationStatus,
    pub rrule: String,
    pub target_thread_id: Option<String>,
    pub source_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationSummary {
    pub id: String,
    pub kind: AutomationKind,
    pub name: String,
    pub status: AutomationStatus,
    pub rrule: String,
    pub schedule_summary: String,
    pub target_thread_id: Option<String>,
    pub target_known: bool,
    pub control_plane_covered: bool,
    pub source_path: String,
}

impl AutomationDefinition {
    pub fn to_summary(&self, known_thread_ids: &BTreeSet<String>) -> AutomationSummary {
        let target_known = self
            .target_thread_id
            .as_ref()
            .map(|thread_id| known_thread_ids.contains(thread_id))
            .unwrap_or(false);
        AutomationSummary {
            id: self.id.clone(),
            kind: self.kind.clone(),
            name: self.name.clone(),
            status: self.status.clone(),
            rrule: self.rrule.clone(),
            schedule_summary: summarize_rrule(&self.rrule),
            target_thread_id: self.target_thread_id.clone(),
            target_known,
            control_plane_covered: target_known && self.status == AutomationStatus::Active,
            source_path: self.source_path.display().to_string(),
        }
    }

    pub fn due_scheduled_at_ms(&self, now_ms: i64) -> Option<i64> {
        if self.status != AutomationStatus::Active {
            return None;
        }
        let interval_ms = rrule_interval_ms(&self.rrule)?;
        Some((now_ms / interval_ms) * interval_ms)
    }
}

#[derive(Debug, Deserialize)]
struct AutomationToml {
    id: Option<String>,
    kind: Option<String>,
    name: Option<String>,
    prompt: Option<String>,
    status: Option<String>,
    rrule: Option<String>,
    target_thread_id: Option<String>,
}

pub fn read_automations(codex_home: &Path) -> Result<Vec<AutomationDefinition>> {
    let automations_path = codex_home.join(AUTOMATIONS_DIRECTORY_NAME);
    if !automations_path.exists() {
        return Ok(Vec::new());
    }

    let mut automations = Vec::new();
    for entry in std::fs::read_dir(&automations_path)
        .with_context(|| format!("read {}", automations_path.display()))?
    {
        let entry = entry?;
        let path = entry.path().join(AUTOMATION_FILE_NAME);
        if path.is_file()
            && let Ok(automation) = read_automation_file(&path)
        {
            automations.push(automation);
        }
    }
    automations.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(automations)
}

fn read_automation_file(path: &Path) -> Result<AutomationDefinition> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read automation {}", path.display()))?;
    let parsed: AutomationToml =
        toml::from_str(&content).with_context(|| format!("parse automation {}", path.display()))?;
    let id = parsed
        .id
        .or_else(|| {
            path.parent()
                .and_then(|parent| parent.file_name())
                .map(|name| name.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(AutomationDefinition {
        id,
        kind: parse_kind(parsed.kind.as_deref()),
        name: parsed
            .name
            .unwrap_or_else(|| "Unnamed automation".to_owned()),
        prompt: parsed.prompt.unwrap_or_default(),
        status: parse_status(parsed.status.as_deref()),
        rrule: parsed.rrule.unwrap_or_default(),
        target_thread_id: parsed.target_thread_id,
        source_path: path.to_path_buf(),
    })
}

fn parse_kind(value: Option<&str>) -> AutomationKind {
    match value.unwrap_or_default().to_ascii_lowercase().as_str() {
        "heartbeat" => AutomationKind::Heartbeat,
        "cron" => AutomationKind::Cron,
        _ => AutomationKind::Unknown,
    }
}

fn parse_status(value: Option<&str>) -> AutomationStatus {
    match value.unwrap_or("ACTIVE").to_ascii_uppercase().as_str() {
        "PAUSED" => AutomationStatus::Paused,
        "DISABLED" => AutomationStatus::Disabled,
        _ => AutomationStatus::Active,
    }
}

fn summarize_rrule(rrule: &str) -> String {
    let frequency = rrule_part(rrule, "FREQ").unwrap_or("UNKNOWN");
    let interval = rrule_part(rrule, "INTERVAL").unwrap_or("1");
    format!("{frequency} every {interval}")
}

fn rrule_interval_ms(rrule: &str) -> Option<i64> {
    let interval = rrule_part(rrule, "INTERVAL")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1)
        .max(1);
    let frequency = rrule_part(rrule, "FREQ")?.to_ascii_uppercase();
    match frequency.as_str() {
        "MINUTELY" => Some(interval * MILLIS_PER_MINUTE),
        "HOURLY" => Some(interval * MILLIS_PER_HOUR),
        "DAILY" => Some(interval * MILLIS_PER_DAY),
        _ => None,
    }
}

fn rrule_part<'a>(rrule: &'a str, key: &str) -> Option<&'a str> {
    rrule.split(';').find_map(|part| {
        let (left, right) = part.split_once('=')?;
        if left.eq_ignore_ascii_case(key) {
            Some(right)
        } else {
            None
        }
    })
}

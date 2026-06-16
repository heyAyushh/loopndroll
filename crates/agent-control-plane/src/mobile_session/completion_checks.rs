use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};

use super::normalization::{
    bool_to_flag, normalized_commands, normalized_optional, normalized_required, now_iso_string,
};
use super::session_overrides::SessionOverrideUpdate;
use super::{
    MobileCompletionCheck, MobileSessionError, MobileSessionResult, MobileSessionService,
    MobileSessionState,
};

const SHELL_PATH: &str = "/bin/sh";
const SHELL_COMMAND_FLAG: &str = "-lc";
const COMPLETION_CHECK_OUTPUT_LINE_LIMIT: usize = 8;
const DEFAULT_COMPLETION_CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const COMPLETION_CHECK_POLL_INTERVAL: Duration = Duration::from_millis(50);

impl MobileSessionService {
    pub fn upsert_completion_check(
        &self,
        id: &str,
        label: &str,
        commands: &[String],
    ) -> MobileSessionResult<MobileCompletionCheck> {
        let check = MobileCompletionCheck {
            id: normalized_required(id).ok_or(MobileSessionError::CompletionCheckNotFound)?,
            label: normalized_required(label).ok_or(MobileSessionError::CompletionCheckNotFound)?,
            commands: normalized_commands(commands)?,
        };
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_completion_checks (id, label, commands_json, created_at, updated_at)
             values (?1, ?2, ?3, ?4, ?4)
             on conflict(id) do update set
                label = excluded.label,
                commands_json = excluded.commands_json,
                updated_at = excluded.updated_at",
            params![
                &check.id,
                &check.label,
                serde_json::to_string(&check.commands)
                    .map_err(|_| MobileSessionError::InvalidCompletionCheck)?,
                now_iso_string()?,
            ],
        )?;
        Ok(check)
    }

    pub fn delete_completion_check(&self, id: &str) -> MobileSessionResult<()> {
        let id = normalized_required(id).ok_or(MobileSessionError::CompletionCheckNotFound)?;
        self.initialize()?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "delete from mobile_completion_checks where id = ?1",
            params![&id],
        )?;
        if removed == 0 {
            return Err(MobileSessionError::CompletionCheckNotFound);
        }
        transaction.execute(
            "update mobile_settings
             set global_completion_check_id = null,
                 global_completion_check_wait_for_reply = 0,
                 updated_at = ?2
             where global_completion_check_id = ?1",
            params![&id, now_iso_string()?],
        )?;
        transaction.execute(
            "update mobile_session_overrides
             set completion_check_id = null,
                 completion_check_wait_for_reply = 0,
                 updated_at = ?2
             where completion_check_id = ?1",
            params![&id, now_iso_string()?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_global_completion_check(
        &self,
        completion_check_id: Option<&str>,
        wait_for_reply: bool,
    ) -> MobileSessionResult<()> {
        let completion_check_id = self.valid_completion_check_id(completion_check_id)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "update mobile_settings
             set global_completion_check_id = ?1,
                 global_completion_check_wait_for_reply = ?2,
                 updated_at = ?3
             where id = 1",
            params![
                completion_check_id,
                bool_to_flag(wait_for_reply),
                now_iso_string()?
            ],
        )?;
        Ok(())
    }

    pub fn set_session_completion_check(
        &self,
        thread_id: &str,
        completion_check_id: Option<&str>,
        wait_for_reply: bool,
    ) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        let completion_check_id = self.valid_completion_check_id(completion_check_id)?;
        self.upsert_session_override(
            &thread_id,
            SessionOverrideUpdate {
                completion_check_id: Some(completion_check_id),
                completion_check_wait_for_reply: Some(wait_for_reply),
                ..SessionOverrideUpdate::default()
            },
        )
    }

    fn valid_completion_check_id(
        &self,
        completion_check_id: Option<&str>,
    ) -> MobileSessionResult<Option<String>> {
        let Some(completion_check_id) = completion_check_id.and_then(normalized_optional) else {
            return Ok(None);
        };
        let known_completion_check_ids = self
            .state()?
            .completion_checks
            .into_iter()
            .map(|completion_check| completion_check.id)
            .collect::<std::collections::BTreeSet<_>>();
        if known_completion_check_ids.contains(&completion_check_id) {
            return Ok(Some(completion_check_id));
        }
        Err(MobileSessionError::CompletionCheckNotFound)
    }
}

pub(super) fn completion_check_failure_reason(
    cwd: &str,
    completion_check: &MobileCompletionCheck,
) -> Option<String> {
    completion_check_failure_reason_with_timeout(
        cwd,
        completion_check,
        DEFAULT_COMPLETION_CHECK_TIMEOUT,
    )
}

fn completion_check_failure_reason_with_timeout(
    cwd: &str,
    completion_check: &MobileCompletionCheck,
    timeout: Duration,
) -> Option<String> {
    for command in &completion_check.commands {
        let output = match run_completion_check_command(cwd, command, timeout) {
            Ok(output) => output,
            Err(CompletionCheckRunError::Spawn(error)) => {
                return Some(completion_check_spawn_failure_reason(command, &error));
            }
            Err(CompletionCheckRunError::Timeout) => {
                return Some(completion_check_timeout_failure_reason(command, timeout));
            }
        };
        if output.status.success() {
            continue;
        }
        return Some(completion_check_exit_failure_reason(command, &output));
    }
    None
}

enum CompletionCheckRunError {
    Spawn(std::io::Error),
    Timeout,
}

fn run_completion_check_command(
    cwd: &str,
    command: &str,
    timeout: Duration,
) -> Result<Output, CompletionCheckRunError> {
    let mut child = Command::new(SHELL_PATH)
        .arg(SHELL_COMMAND_FLAG)
        .arg(command)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(CompletionCheckRunError::Spawn)?;
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                return child
                    .wait_with_output()
                    .map_err(CompletionCheckRunError::Spawn);
            }
            Ok(None) if started_at.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CompletionCheckRunError::Timeout);
            }
            Ok(None) => std::thread::sleep(COMPLETION_CHECK_POLL_INTERVAL),
            Err(error) => return Err(CompletionCheckRunError::Spawn(error)),
        }
    }
}

fn completion_check_spawn_failure_reason(command: &str, error: &std::io::Error) -> String {
    [
        "Completion check failed while running:".to_owned(),
        command.to_owned(),
        format!("The command could not start: {error}"),
        String::new(),
        "Fix issues.".to_owned(),
    ]
    .join("\n")
}

fn completion_check_timeout_failure_reason(command: &str, timeout: Duration) -> String {
    [
        "Completion check failed while running:".to_owned(),
        command.to_owned(),
        format!("The command timed out after {} seconds.", timeout.as_secs()),
        String::new(),
        "Fix issues.".to_owned(),
    ]
    .join("\n")
}

fn completion_check_exit_failure_reason(command: &str, output: &std::process::Output) -> String {
    let mut segments = vec![
        "Completion check failed while running:".to_owned(),
        command.to_owned(),
    ];
    if let Some(code) = output.status.code() {
        segments.push(format!("Exit code: {code}"));
    } else {
        segments.push("The command exited before completion.".to_owned());
    }
    if let Some(output_summary) = completion_check_output_summary(output) {
        segments.push(format!("Recent output:\n{output_summary}"));
    }
    segments.push(String::new());
    segments.push("Fix issues.".to_owned());
    segments.join("\n")
}

fn completion_check_output_summary(output: &std::process::Output) -> Option<String> {
    let combined_output = [
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ]
    .into_iter()
    .filter(|value| !value.trim().is_empty())
    .collect::<Vec<_>>()
    .join("\n");
    let lines = combined_output
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let start = lines
        .len()
        .saturating_sub(COMPLETION_CHECK_OUTPUT_LINE_LIMIT);
    Some(lines[start..].join("\n"))
}

#[cfg(test)]
mod tests {
    use super::completion_check_failure_reason_with_timeout;
    use crate::mobile_session::MobileCompletionCheck;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn completion_check_times_out_hanging_command() {
        let tempdir = tempdir().expect("tempdir");
        let check = MobileCompletionCheck {
            id: "hang".to_owned(),
            label: "Hang".to_owned(),
            commands: vec!["sleep 5".to_owned()],
        };

        let reason = completion_check_failure_reason_with_timeout(
            tempdir.path().to_str().expect("tempdir path"),
            &check,
            Duration::from_millis(100),
        )
        .expect("timeout failure");

        assert!(reason.contains("timed out"));
    }
}

pub(super) fn active_completion_check<'a>(
    thread_id: &str,
    state: &'a MobileSessionState,
) -> Option<&'a MobileCompletionCheck> {
    let completion_check_id = state
        .sessions
        .get(thread_id)
        .and_then(|override_state| override_state.completion_check_id.as_deref())
        .or(state.global_completion_check_id.as_deref())?;
    state
        .completion_checks
        .iter()
        .find(|completion_check| completion_check.id == completion_check_id)
}

pub(super) fn active_completion_check_wait_for_reply(
    thread_id: &str,
    state: &MobileSessionState,
) -> bool {
    if let Some(override_state) = state.sessions.get(thread_id)
        && override_state.completion_check_id.is_some()
    {
        return override_state.completion_check_wait_for_reply;
    }
    state.global_completion_check_wait_for_reply
}

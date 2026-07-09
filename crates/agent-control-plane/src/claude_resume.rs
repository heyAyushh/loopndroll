use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

const CLAUDE_EXECUTABLE_ENV: &str = "LOOPER_CLAUDE_EXECUTABLE";
const DEFAULT_CLAUDE_CLI: &str = "claude";
const PRINT_FLAG: &str = "-p";
const RESUME_FLAG: &str = "--resume";
const OUTPUT_FORMAT_FLAG: &str = "--output-format";
const JSON_OUTPUT_FORMAT: &str = "json";
const COMPLETION_WATCH_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeResumeRequest {
    pub session_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub claude_executable: Option<String>,
}

pub fn spawn_session_resume(request: &ClaudeResumeRequest) -> Result<()> {
    let executable = resolve_claude_executable(request.claude_executable.as_deref());
    let child = spawn_claude_resume(&executable, request)?;
    watch_resume_completion(child, request.session_id.clone());
    Ok(())
}

pub fn resolve_claude_executable(configured_executable: Option<&str>) -> String {
    if let Some(executable) = normalized_executable(configured_executable) {
        return executable;
    }
    if let Ok(executable) = std::env::var(CLAUDE_EXECUTABLE_ENV)
        && let Some(executable) = normalized_executable(Some(&executable))
    {
        return executable;
    }
    DEFAULT_CLAUDE_CLI.to_owned()
}

fn spawn_claude_resume(executable: &str, request: &ClaudeResumeRequest) -> Result<Child> {
    let mut command = Command::new(executable);
    command
        .args(claude_resume_args(request))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = resumable_cwd(request.cwd.as_deref()) {
        command.current_dir(cwd);
    }
    command
        .spawn()
        .with_context(|| format!("spawn claude resume using {executable}"))
}

fn claude_resume_args(request: &ClaudeResumeRequest) -> Vec<String> {
    vec![
        PRINT_FLAG.to_owned(),
        RESUME_FLAG.to_owned(),
        request.session_id.clone(),
        OUTPUT_FORMAT_FLAG.to_owned(),
        JSON_OUTPUT_FORMAT.to_owned(),
        request.prompt.clone(),
    ]
}

fn normalized_executable(executable: Option<&str>) -> Option<String> {
    executable
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn resumable_cwd(cwd: Option<&str>) -> Option<PathBuf> {
    let path = normalized_executable(cwd).map(PathBuf::from)?;
    path.is_dir().then_some(path)
}

fn watch_resume_completion(mut child: Child, session_id: String) {
    thread::spawn(move || {
        let deadline = Instant::now() + COMPLETION_WATCH_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => break,
                Ok(None) if Instant::now() >= deadline => {
                    eprintln!("claude resume timed out for session {session_id}");
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                Ok(None) => thread::sleep(COMPLETION_POLL_INTERVAL),
                Err(error) => {
                    eprintln!("claude resume wait failed for session {session_id}: {error}");
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_executable_wins_over_discovery() {
        assert_eq!(
            resolve_claude_executable(Some("/tmp/custom-claude")),
            "/tmp/custom-claude"
        );
    }

    #[test]
    fn claude_resume_args_use_headless_resume_shape() {
        let request = ClaudeResumeRequest {
            session_id: "session-1".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some("/tmp/project".to_owned()),
            claude_executable: None,
        };

        assert_eq!(
            claude_resume_args(&request),
            vec![
                PRINT_FLAG.to_owned(),
                RESUME_FLAG.to_owned(),
                "session-1".to_owned(),
                OUTPUT_FORMAT_FLAG.to_owned(),
                JSON_OUTPUT_FORMAT.to_owned(),
                "hello phone".to_owned()
            ]
        );
    }
}

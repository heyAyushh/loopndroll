use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

const GROK_HOME_ENV: &str = "GROK_HOME";
const DEFAULT_GROK_CLI: &str = "grok";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokContinueRequest {
    pub session_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub grok_executable: Option<String>,
    pub grok_home: Option<PathBuf>,
}

pub fn spawn_session_continue(request: &GrokContinueRequest) -> Result<()> {
    let executable = request
        .grok_executable
        .clone()
        .unwrap_or_else(|| resolve_grok_executable(request.grok_home.as_deref()));
    let mut command = Command::new(&executable);
    command
        .arg("-p")
        .arg(&request.prompt)
        .arg("-r")
        .arg(&request.session_id);
    if let Some(cwd) = request
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        command.current_dir(cwd);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().with_context(|| {
        format!(
            "spawn grok continue for session {} using {}",
            request.session_id, executable
        )
    })?;
    Ok(())
}

pub fn resolve_grok_executable(grok_home: Option<&Path>) -> String {
    let candidates = grok_executable_candidates(grok_home);
    candidates
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .unwrap_or_else(|| DEFAULT_GROK_CLI.to_owned())
}

fn grok_executable_candidates(grok_home: Option<&Path>) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Ok(home) = std::env::var(GROK_HOME_ENV) {
        candidates.push(PathBuf::from(home).join("bin").join(DEFAULT_GROK_CLI));
    }
    if let Some(home) = grok_home {
        candidates.push(home.join("bin").join(DEFAULT_GROK_CLI));
    }
    candidates
        .into_iter()
        .map(|path| path.display().to_string())
        .chain(std::iter::once(DEFAULT_GROK_CLI.to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_grok_home_bin_executable() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let grok_home = temp_dir.path();
        let executable = grok_home.join("bin").join(DEFAULT_GROK_CLI);
        std::fs::create_dir_all(executable.parent().expect("bin parent")).expect("create bin");
        std::fs::write(&executable, b"#!/bin/sh\n").expect("write grok stub");

        assert_eq!(
            resolve_grok_executable(Some(grok_home)),
            executable.display().to_string()
        );
    }

    #[test]
    fn falls_back_to_grok_on_path_when_missing() {
        assert_eq!(
            resolve_grok_executable(Some(Path::new("/tmp/missing-grok-home"))),
            DEFAULT_GROK_CLI
        );
    }
}

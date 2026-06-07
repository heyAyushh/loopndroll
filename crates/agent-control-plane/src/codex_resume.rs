use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

const CODEX_EXECUTABLE_ENV: &str = "LOOPER_CODEX_EXECUTABLE";
const BUNDLED_CODEX_APP_CLI: &str = "/Applications/Codex.app/Contents/Resources/codex";
const DEFAULT_CODEX_CLI: &str = "codex";
const NO_ALT_SCREEN_FLAG: &str = "--no-alt-screen";
const RESUME_COMMAND: &str = "resume";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexResumeRequest {
    pub thread_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub codex_executable: Option<String>,
}

pub fn spawn_thread_resume(request: &CodexResumeRequest) -> Result<()> {
    let executable = resolve_codex_executable(request.codex_executable.as_deref());
    let mut command = Command::new(&executable);
    command
        .arg(NO_ALT_SCREEN_FLAG)
        .arg(RESUME_COMMAND)
        .arg(&request.thread_id)
        .arg(&request.prompt);
    if let Some(cwd) = resumable_cwd(request.cwd.as_deref()) {
        command.current_dir(cwd);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().with_context(|| {
        format!(
            "spawn codex resume for thread {} using {}",
            request.thread_id, executable
        )
    })?;
    Ok(())
}

pub fn resolve_codex_executable(configured_executable: Option<&str>) -> String {
    if let Some(executable) = normalized_executable(configured_executable) {
        return executable;
    }
    if let Ok(executable) = std::env::var(CODEX_EXECUTABLE_ENV) {
        if let Some(executable) = normalized_executable(Some(&executable)) {
            return executable;
        }
    }
    codex_executable_candidates()
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .unwrap_or_else(|| DEFAULT_CODEX_CLI.to_owned())
}

fn codex_executable_candidates() -> Vec<String> {
    vec![
        BUNDLED_CODEX_APP_CLI.to_owned(),
        DEFAULT_CODEX_CLI.to_owned(),
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const STUB_WAIT_ATTEMPTS: usize = 100;
    const STUB_WAIT_INTERVAL_MS: u64 = 50;

    #[test]
    fn configured_executable_wins_over_discovery() {
        assert_eq!(
            resolve_codex_executable(Some("/tmp/custom-codex")),
            "/tmp/custom-codex"
        );
    }

    #[test]
    fn spawn_thread_resume_passes_resume_arguments() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let args_path = temp_dir.path().join("args.txt");
        let executable_path = temp_dir.path().join("codex-stub");
        fs::write(
            &executable_path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                args_path.display()
            ),
        )
        .expect("write stub");
        let mut permissions = fs::metadata(&executable_path)
            .expect("stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("chmod stub");

        spawn_thread_resume(&CodexResumeRequest {
            thread_id: "thread-1".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some(temp_dir.path().display().to_string()),
            codex_executable: Some(executable_path.display().to_string()),
        })
        .expect("spawn resume");

        let args = wait_for_stub_args(&args_path);
        assert_eq!(
            args,
            vec![
                NO_ALT_SCREEN_FLAG.to_owned(),
                RESUME_COMMAND.to_owned(),
                "thread-1".to_owned(),
                "hello phone".to_owned()
            ]
        );
    }

    fn wait_for_stub_args(args_path: &Path) -> Vec<String> {
        for _ in 0..STUB_WAIT_ATTEMPTS {
            if let Ok(contents) = fs::read_to_string(args_path) {
                return contents.lines().map(str::to_owned).collect();
            }
            thread::sleep(Duration::from_millis(STUB_WAIT_INTERVAL_MS));
        }
        panic!("stub did not write arguments");
    }
}

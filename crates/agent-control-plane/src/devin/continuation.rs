use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

const DEVIN_EXECUTABLE_ENV: &str = "LOOPER_DEVIN_EXECUTABLE";
const DEFAULT_DEVIN_CLI: &str = "devin";
const DEVIN_NEXT_BUNDLED_CLI: &str =
    "/Applications/Devin - Next.app/Contents/Resources/app/extensions/windsurf/devin/bin/devin";
const DEVIN_STABLE_BUNDLED_CLI: &str =
    "/Applications/Devin.app/Contents/Resources/app/extensions/windsurf/devin/bin/devin";
const PRINT_FLAG: &str = "-p";
const RESUME_FLAG: &str = "--resume";
const START_FAILURE_GRACE_PERIOD: Duration = Duration::from_secs(3);
const START_FAILURE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FAILURE_OUTPUT_LIMIT: usize = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevinContinueRequest {
    pub session_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub devin_executable: Option<String>,
}

pub fn spawn_session_continue(request: &DevinContinueRequest) -> Result<()> {
    let executable = resolve_devin_executable(request.devin_executable.as_deref());
    let mut command = Command::new(&executable);
    command
        .arg(RESUME_FLAG)
        .arg(&request.session_id)
        .arg(PRINT_FLAG)
        .arg(&request.prompt);
    if let Some(cwd) = resumable_cwd(request.cwd.as_deref()) {
        command.current_dir(cwd);
    }
    let output_path = devin_continue_output_path();
    let output_file = File::create(&output_path)
        .with_context(|| format!("create devin continue log {}", output_path.display()))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_file.try_clone().with_context(|| {
            format!("clone devin continue log {}", output_path.display())
        })?))
        .stderr(Stdio::from(output_file));
    let mut child = command.spawn().with_context(|| {
        format!(
            "spawn devin continue for session {} using {}",
            request.session_id, executable
        )
    })?;
    let deadline = Instant::now() + START_FAILURE_GRACE_PERIOD;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().with_context(|| {
            format!(
                "inspect devin continue launch for session {}",
                request.session_id
            )
        })? {
            if status.success() {
                let _ = fs::remove_file(&output_path);
                return Ok(());
            }
            let failure_output = devin_continue_failure_output(&output_path);
            let _ = fs::remove_file(&output_path);
            bail!(
                "devin continue for session {} exited during launch with status {}{}",
                request.session_id,
                status,
                failure_output
            );
        }
        thread::sleep(START_FAILURE_POLL_INTERVAL);
    }
    let _ = fs::remove_file(&output_path);
    Ok(())
}

fn devin_continue_output_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "looper-devin-continue-{}.log",
        uuid::Uuid::new_v4()
    ))
}

fn devin_continue_failure_output(path: &Path) -> String {
    let Ok(output) = fs::read_to_string(path) else {
        return String::new();
    };
    let output = output.trim();
    if output.is_empty() {
        return String::new();
    }
    let truncated = output
        .chars()
        .take(FAILURE_OUTPUT_LIMIT)
        .collect::<String>();
    format!(": {truncated}")
}

pub fn resolve_devin_executable(configured_executable: Option<&str>) -> String {
    if let Some(executable) = normalized_executable(configured_executable) {
        return executable;
    }
    if let Ok(executable) = std::env::var(DEVIN_EXECUTABLE_ENV) {
        if let Some(executable) = normalized_executable(Some(&executable)) {
            return executable;
        }
    }
    devin_executable_candidates()
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .unwrap_or_else(|| DEFAULT_DEVIN_CLI.to_owned())
}

fn devin_executable_candidates() -> Vec<String> {
    vec![
        DEVIN_NEXT_BUNDLED_CLI.to_owned(),
        DEVIN_STABLE_BUNDLED_CLI.to_owned(),
        DEFAULT_DEVIN_CLI.to_owned(),
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
            resolve_devin_executable(Some("/tmp/custom-devin")),
            "/tmp/custom-devin"
        );
    }

    #[test]
    fn spawn_session_continue_resumes_session_with_prompt() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let args_path = temp_dir.path().join("args.txt");
        let executable_path = temp_dir.path().join("devin-stub");
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

        spawn_session_continue(&DevinContinueRequest {
            session_id: "shadow-canidae".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some(temp_dir.path().display().to_string()),
            devin_executable: Some(executable_path.display().to_string()),
        })
        .expect("spawn continue");

        assert_eq!(
            wait_for_stub_args(&args_path),
            vec![
                RESUME_FLAG.to_owned(),
                "shadow-canidae".to_owned(),
                PRINT_FLAG.to_owned(),
                "hello phone".to_owned(),
            ]
        );
    }

    #[test]
    fn spawn_session_continue_reports_fast_launch_failure() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let executable_path = temp_dir.path().join("devin-failure-stub");
        fs::write(&executable_path, "#!/bin/sh\nexit 42\n").expect("write stub");
        let mut permissions = fs::metadata(&executable_path)
            .expect("stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("chmod stub");

        let error = spawn_session_continue(&DevinContinueRequest {
            session_id: "shadow-canidae".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some(temp_dir.path().display().to_string()),
            devin_executable: Some(executable_path.display().to_string()),
        })
        .expect_err("fast launch failure");

        assert!(
            error
                .to_string()
                .contains("exited during launch with status")
        );
    }

    #[test]
    fn spawn_session_continue_reports_fast_launch_output() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let executable_path = temp_dir.path().join("devin-output-stub");
        fs::write(
            &executable_path,
            "#!/bin/sh\nprintf '%s\\n' 'session already open' >&2\nexit 101\n",
        )
        .expect("write stub");
        let mut permissions = fs::metadata(&executable_path)
            .expect("stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("chmod stub");

        let error = spawn_session_continue(&DevinContinueRequest {
            session_id: "shadow-canidae".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some(temp_dir.path().display().to_string()),
            devin_executable: Some(executable_path.display().to_string()),
        })
        .expect_err("fast launch failure");

        assert!(error.to_string().contains("session already open"));
    }

    fn wait_for_stub_args(path: &Path) -> Vec<String> {
        for _ in 0..STUB_WAIT_ATTEMPTS {
            if let Ok(args) = fs::read_to_string(path) {
                return args.lines().map(str::to_owned).collect();
            }
            thread::sleep(Duration::from_millis(STUB_WAIT_INTERVAL_MS));
        }
        panic!("timed out waiting for stub args at {}", path.display());
    }
}

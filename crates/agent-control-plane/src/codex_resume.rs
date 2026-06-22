// allow: SIZE_OK — Codex resume launcher keeps process I/O protocol and resume request parsing in one failure boundary.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const CODEX_EXECUTABLE_ENV: &str = "LOOPER_CODEX_EXECUTABLE";
const BUNDLED_CODEX_APP_CLI: &str = "/Applications/Codex.app/Contents/Resources/codex";
const DEFAULT_CODEX_CLI: &str = "codex";
const APP_SERVER_COMMAND: &str = "app-server";
const LISTEN_FLAG: &str = "--listen";
const STDIO_LISTENER: &str = "stdio://";
const INITIALIZE_REQUEST_ID: &str = "looper-initialize";
const THREAD_RESUME_REQUEST_ID: &str = "looper-thread-resume";
const TURN_START_REQUEST_ID: &str = "looper-turn-start";
const INITIALIZE_METHOD: &str = "initialize";
const THREAD_RESUME_METHOD: &str = "thread/resume";
const TURN_START_METHOD: &str = "turn/start";
const TURN_COMPLETED_METHOD: &str = "turn/completed";
const LOOPER_CLIENT_NAME: &str = "looper";
const LOOPER_CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const START_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const COMPLETION_WATCH_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexResumeRequest {
    pub thread_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub codex_executable: Option<String>,
}

pub fn spawn_thread_resume(request: &CodexResumeRequest) -> Result<()> {
    let executable = resolve_codex_executable(request.codex_executable.as_deref());
    let mut child = spawn_codex_app_server(&executable, request.cwd.as_deref())?;
    let mut stdin = child
        .stdin
        .take()
        .context("codex app-server stdin unavailable")?;
    let stdout = child
        .stdout
        .take()
        .context("codex app-server stdout unavailable")?;
    let responses = read_app_server_messages(stdout);

    write_json_line(&mut stdin, initialize_request())?;
    wait_for_success(&responses, INITIALIZE_REQUEST_ID)?;

    write_json_line(&mut stdin, thread_resume_request(request))?;
    wait_for_success(&responses, THREAD_RESUME_REQUEST_ID)?;

    write_json_line(&mut stdin, turn_start_request(request))?;
    let turn_start = wait_for_success(&responses, TURN_START_REQUEST_ID)?;
    let turn_id = turn_start["result"]["turn"]["id"]
        .as_str()
        .map(str::to_owned);

    watch_turn_completion(child, stdin, responses, request.thread_id.clone(), turn_id);
    Ok(())
}

pub fn resolve_codex_executable(configured_executable: Option<&str>) -> String {
    if let Some(executable) = normalized_executable(configured_executable) {
        return executable;
    }
    if let Ok(executable) = std::env::var(CODEX_EXECUTABLE_ENV)
        && let Some(executable) = normalized_executable(Some(&executable))
    {
        return executable;
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

fn spawn_codex_app_server(executable: &str, cwd: Option<&str>) -> Result<Child> {
    let mut command = Command::new(executable);
    command
        .arg(APP_SERVER_COMMAND)
        .arg(LISTEN_FLAG)
        .arg(STDIO_LISTENER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(cwd) = resumable_cwd(cwd) {
        command.current_dir(cwd);
    }
    command
        .spawn()
        .with_context(|| format!("spawn codex app-server using {executable}"))
}

fn initialize_request() -> Value {
    json!({
        "id": INITIALIZE_REQUEST_ID,
        "method": INITIALIZE_METHOD,
        "params": {
            "clientInfo": {
                "name": LOOPER_CLIENT_NAME,
                "version": LOOPER_CLIENT_VERSION
            },
            "capabilities": {
                "experimentalApi": true
            }
        }
    })
}

fn thread_resume_request(request: &CodexResumeRequest) -> Value {
    json!({
        "id": THREAD_RESUME_REQUEST_ID,
        "method": THREAD_RESUME_METHOD,
        "params": {
            "threadId": request.thread_id,
            "cwd": request.cwd
        }
    })
}

fn turn_start_request(request: &CodexResumeRequest) -> Value {
    json!({
        "id": TURN_START_REQUEST_ID,
        "method": TURN_START_METHOD,
        "params": {
            "threadId": request.thread_id,
            "clientUserMessageId": format!("looper-{}", uuid::Uuid::new_v4()),
            "input": [{
                "type": "text",
                "text": request.prompt,
                "text_elements": []
            }],
            "cwd": request.cwd
        }
    })
}

fn write_json_line(stdin: &mut ChildStdin, value: Value) -> Result<()> {
    let line = serde_json::to_string(&value).context("serialize codex app-server request")?;
    stdin
        .write_all(line.as_bytes())
        .context("write codex app-server request")?;
    stdin
        .write_all(b"\n")
        .context("terminate codex app-server request")?;
    stdin.flush().context("flush codex app-server request")
}

fn read_app_server_messages(stdout: std::process::ChildStdout) -> Receiver<Value> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if sender.send(value).is_err() {
                break;
            }
        }
    });
    receiver
}

fn wait_for_success(receiver: &Receiver<Value>, request_id: &str) -> Result<Value> {
    let deadline = Instant::now() + START_RESPONSE_TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("codex app-server timed out waiting for {request_id}");
        }
        match receiver.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(message) if message["id"] == request_id => {
                if let Some(error) = message.get("error") {
                    bail!(
                        "codex app-server {request_id} failed: {}",
                        codex_error_message(error)
                    );
                }
                return Ok(message);
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                bail!("codex app-server timed out waiting for {request_id}");
            }
            Err(RecvTimeoutError::Disconnected) => {
                bail!("codex app-server exited before {request_id}");
            }
        }
    }
}

fn codex_error_message(error: &Value) -> String {
    error["message"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string())
}

fn watch_turn_completion(
    mut child: Child,
    stdin: ChildStdin,
    receiver: Receiver<Value>,
    thread_id: String,
    turn_id: Option<String>,
) {
    thread::spawn(move || {
        let _stdin_guard = stdin;
        let deadline = Instant::now() + COMPLETION_WATCH_TIMEOUT;
        while Instant::now() < deadline {
            match receiver.recv_timeout(COMPLETION_POLL_INTERVAL) {
                Ok(message) if is_turn_completed(&message, &thread_id, turn_id.as_deref()) => {
                    break;
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        drop(_stdin_guard);
        let _ = child.kill();
        let _ = child.wait();
    });
}

fn is_turn_completed(message: &Value, thread_id: &str, turn_id: Option<&str>) -> bool {
    if message["method"] != TURN_COMPLETED_METHOD {
        return false;
    }
    if message["params"]["threadId"].as_str() != Some(thread_id) {
        return false;
    }
    match turn_id {
        Some(turn_id) => message["params"]["turn"]["id"].as_str() == Some(turn_id),
        None => true,
    }
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
    fn spawn_thread_resume_starts_turn_through_app_server() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let args_path = temp_dir.path().join("args.txt");
        let input_path = temp_dir.path().join("input.jsonl");
        let executable_path = temp_dir.path().join("codex-stub");
        fs::write(
            &executable_path,
            format!(
                r#"#!/bin/sh
printf '%s\n' "$@" > '{}'
while IFS= read -r line; do
  printf '%s\n' "$line" >> '{}'
  case "$line" in
    *'"id":"looper-initialize"'*) printf '%s\n' '{{"id":"looper-initialize","result":{{}}}}' ;;
    *'"id":"looper-thread-resume"'*) printf '%s\n' '{{"id":"looper-thread-resume","result":{{"thread":{{"id":"thread-1"}}}}}}' ;;
    *'"id":"looper-turn-start"'*)
      printf '%s\n' '{{"id":"looper-turn-start","result":{{"turn":{{"id":"turn-1","status":"inProgress"}}}}}}'
      printf '%s\n' '{{"method":"turn/completed","params":{{"threadId":"thread-1","turn":{{"id":"turn-1","status":"completed"}}}}}}'
      ;;
  esac
done
"#,
                args_path.display(),
                input_path.display()
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
                APP_SERVER_COMMAND.to_owned(),
                LISTEN_FLAG.to_owned(),
                STDIO_LISTENER.to_owned()
            ]
        );
        let requests = wait_for_stub_requests(&input_path);
        assert_eq!(requests[0]["method"], INITIALIZE_METHOD);
        assert_eq!(requests[1]["method"], THREAD_RESUME_METHOD);
        assert_eq!(requests[1]["params"]["threadId"], "thread-1");
        assert_eq!(requests[2]["method"], TURN_START_METHOD);
        assert_eq!(requests[2]["params"]["threadId"], "thread-1");
        assert_eq!(requests[2]["params"]["input"][0]["text"], "hello phone");
        assert_eq!(
            requests[2]["params"]["input"][0]["text_elements"],
            json!([])
        );
    }

    #[test]
    fn spawn_thread_resume_reports_app_server_errors() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let executable_path = temp_dir.path().join("codex-error-stub");
        fs::write(
            &executable_path,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"id":"looper-initialize"'*) printf '%s\n' '{"id":"looper-initialize","result":{}}' ;;
    *'"id":"looper-thread-resume"'*) printf '%s\n' '{"id":"looper-thread-resume","error":{"message":"thread missing"}}' ;;
  esac
done
"#,
        )
        .expect("write stub");
        let mut permissions = fs::metadata(&executable_path)
            .expect("stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("chmod stub");

        let error = spawn_thread_resume(&CodexResumeRequest {
            thread_id: "missing-thread".to_owned(),
            prompt: "hello phone".to_owned(),
            cwd: Some(temp_dir.path().display().to_string()),
            codex_executable: Some(executable_path.display().to_string()),
        })
        .expect_err("resume should fail");

        assert!(error.to_string().contains("thread missing"));
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

    fn wait_for_stub_requests(input_path: &Path) -> Vec<Value> {
        for _ in 0..STUB_WAIT_ATTEMPTS {
            if let Ok(contents) = fs::read_to_string(input_path) {
                let requests = contents
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).expect("json request"))
                    .collect::<Vec<_>>();
                if requests.len() >= 3 {
                    return requests;
                }
            }
            thread::sleep(Duration::from_millis(STUB_WAIT_INTERVAL_MS));
        }
        panic!("stub did not write requests");
    }
}

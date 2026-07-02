use std::fs;

use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig, HostEnvironment};
use rusqlite::Connection;
use tempfile::TempDir;

pub struct TestControlPlaneFixture {
    temp_dir: TempDir,
    codex_home: std::path::PathBuf,
}

impl TestControlPlaneFixture {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("temp dir");
        let codex_home = temp_dir.path().join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).expect("codex dirs");
        fs::create_dir_all(temp_dir.path().join(".grok/sessions")).expect("grok dirs");
        Self {
            temp_dir,
            codex_home,
        }
    }

    pub fn write_state_db(&self) {
        let connection = Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
        connection
            .execute_batch(
                r#"
create table threads (
  thread_id text primary key,
  title text,
  cwd text,
  source text,
  model text,
  reasoning_effort text,
  created_at_ms integer,
  updated_at_ms integer,
  archived integer
);
insert into threads values
  ('thread-main', 'Main task', '/tmp/project', 'desktop', 'gpt-5.5', 'high', 1000, 2000, 0);
"#,
            )
            .expect("seed state");
        Connection::open(self.codex_home.join("logs_1.sqlite")).expect("logs");
    }

    pub fn control_plane(&self) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: self.codex_home.clone(),
            codex_executable: Some("/usr/bin/false".to_owned()),
            store_path: self.temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
            host_environment: HostEnvironment::hermetic(self.temp_dir.path().to_path_buf()),
        })
    }
}

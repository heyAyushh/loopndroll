use std::time::Duration;

use anyhow::{Result, bail};
use reqwest::Client;
use serde_json::Value;

use super::output::{OutputFormat, print_value, scalar_text};
use super::transport::url;

const DOCTOR_SERVER_TIMEOUT: Duration = Duration::from_secs(2);
const DOCTOR_COMMAND_NAME: &str = "doctor";
const DOCTOR_CHECK_SERVER: &str = "looper-server";
const DOCTOR_CHECK_MOBILE_API: &str = "mobile-api";
const DOCTOR_STATUS_OK: &str = "ok";
const DOCTOR_STATUS_UNREACHABLE: &str = "unreachable";
const DOCTOR_STATUS_UNHEALTHY: &str = "unhealthy";
const CONTROL_PLANE_HEALTH_PATH: &str = "/health";
const MOBILE_HEALTH_PATH: &str = "/api/mobile/health";
const HEALTHY_STATUS_VALUE: &str = "healthy";

pub(crate) async fn run_doctor_command(args: &[String], format: OutputFormat) -> Result<()> {
    if !args.is_empty() {
        bail!("usage: looper doctor");
    }
    let mut checks = Vec::new();
    let server_check = doctor_server_check().await;
    let server_ok = is_doctor_check_ok(&server_check);
    checks.push(server_check);
    if server_ok {
        checks.push(doctor_mobile_api_check().await);
    }
    let issue_count = checks
        .iter()
        .filter(|check| check.get("ok").and_then(Value::as_bool) != Some(true))
        .count();
    let report = serde_json::json!({
        "ok": issue_count == 0,
        "checks": checks,
    });
    print_value(&report, format)?;
    if issue_count > 0 {
        bail!("{DOCTOR_COMMAND_NAME} found {issue_count} issue(s)");
    }
    Ok(())
}

async fn doctor_server_check() -> Value {
    match get_doctor_json(CONTROL_PLANE_HEALTH_PATH).await {
        Ok(health) => doctor_control_plane_health_check(&health),
        Err(error) => doctor_check(DOCTOR_CHECK_SERVER, false, DOCTOR_STATUS_UNREACHABLE, error),
    }
}

async fn doctor_mobile_api_check() -> Value {
    match get_doctor_json(MOBILE_HEALTH_PATH).await {
        Ok(health) => doctor_mobile_health_check(&health),
        Err(error) => doctor_check(
            DOCTOR_CHECK_MOBILE_API,
            false,
            DOCTOR_STATUS_UNREACHABLE,
            error,
        ),
    }
}

async fn get_doctor_json(path: &str) -> std::result::Result<Value, String> {
    let request_url = url(path);
    let client = match Client::builder().timeout(DOCTOR_SERVER_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => return Err(error.to_string()),
    };
    match client.get(&request_url).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<Value>()
            .await
            .map_err(|error| format!("{request_url}: {error}")),
        Ok(response) => Err(format!("{} returned {}", request_url, response.status())),
        Err(error) => Err(error.to_string()),
    }
}

fn doctor_control_plane_health_check(health: &Value) -> Value {
    let source_health = health_text(health, &["source", "health"]);
    let hook_health = health_text(health, &["hooks", "health"]);
    let grok_hook_health = health_text(health, &["grok_hooks", "health"]);
    let claude_hook_health = health_text(health, &["claude_hooks", "health"]);
    let grok_hooks_ok = grok_hook_health
        .as_deref()
        .is_none_or(|health| health == HEALTHY_STATUS_VALUE || health == "missing");
    let claude_hooks_ok = claude_hook_health
        .as_deref()
        .is_none_or(|health| health == HEALTHY_STATUS_VALUE || health == "missing");
    let is_healthy = health.get("ok").and_then(Value::as_bool) == Some(true)
        && source_health.as_deref() == Some(HEALTHY_STATUS_VALUE)
        && hook_health.as_deref() == Some(HEALTHY_STATUS_VALUE)
        && grok_hooks_ok
        && claude_hooks_ok;
    let detail = format!(
        "source={} hooks={} grok_hooks={} claude_hooks={}",
        source_health.unwrap_or_else(|| "missing".to_owned()),
        hook_health.unwrap_or_else(|| "missing".to_owned()),
        grok_hook_health.unwrap_or_else(|| "missing".to_owned()),
        claude_hook_health.unwrap_or_else(|| "missing".to_owned())
    );
    doctor_check(
        DOCTOR_CHECK_SERVER,
        is_healthy,
        if is_healthy {
            DOCTOR_STATUS_OK
        } else {
            DOCTOR_STATUS_UNHEALTHY
        },
        detail,
    )
}

fn doctor_mobile_health_check(health: &Value) -> Value {
    let is_healthy = health.get("ok").and_then(Value::as_bool) == Some(true)
        && health
            .get("requiresAuthentication")
            .and_then(Value::as_bool)
            == Some(true);
    doctor_check(
        DOCTOR_CHECK_MOBILE_API,
        is_healthy,
        if is_healthy {
            DOCTOR_STATUS_OK
        } else {
            DOCTOR_STATUS_UNHEALTHY
        },
        format!(
            "ok={} requiresAuthentication={}",
            scalar_text(health.get("ok").unwrap_or(&Value::Null)),
            scalar_text(health.get("requiresAuthentication").unwrap_or(&Value::Null))
        ),
    )
}

fn is_doctor_check_ok(check: &Value) -> bool {
    check.get("ok").and_then(Value::as_bool) == Some(true)
}

fn health_text(health: &Value, path: &[&str]) -> Option<String> {
    path.iter()
        .try_fold(health, |value, key| value.get(*key))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn doctor_check(name: &str, ok: bool, status: &str, detail: impl Into<String>) -> Value {
    serde_json::json!({
        "name": name,
        "ok": ok,
        "status": status,
        "detail": detail.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_table_prints_checks() {
        let check = doctor_check("looper-server", false, DOCTOR_STATUS_UNREACHABLE, "offline");
        assert_eq!(check["name"], "looper-server");
        assert_eq!(check["ok"], serde_json::json!(false));
        assert_eq!(check["status"], DOCTOR_STATUS_UNREACHABLE);
    }

    #[test]
    fn doctor_control_plane_health_requires_source_and_hooks() {
        let health = serde_json::json!({
            "ok": true,
            "source": { "health": "healthy" },
            "hooks": { "health": "degraded" },
            "grok_hooks": { "health": "healthy" },
        });
        let check = doctor_control_plane_health_check(&health);
        assert_eq!(check["ok"], serde_json::json!(false));
        assert_eq!(check["status"], DOCTOR_STATUS_UNHEALTHY);
    }

    #[test]
    fn doctor_control_plane_health_allows_missing_grok_hooks() {
        let health = serde_json::json!({
            "ok": true,
            "source": { "health": "healthy" },
            "hooks": { "health": "healthy" },
            "grok_hooks": { "health": "missing" },
            "claude_hooks": { "health": "missing" },
        });
        let check = doctor_control_plane_health_check(&health);
        assert_eq!(check["ok"], serde_json::json!(true));
        assert_eq!(check["status"], DOCTOR_STATUS_OK);
    }

    #[test]
    fn doctor_control_plane_health_rejects_foreign_grok_hooks() {
        let health = serde_json::json!({
            "ok": true,
            "source": { "health": "healthy" },
            "hooks": { "health": "healthy" },
            "grok_hooks": { "health": "configured" },
            "claude_hooks": { "health": "healthy" },
        });
        let check = doctor_control_plane_health_check(&health);
        assert_eq!(check["ok"], serde_json::json!(false));
        assert_eq!(check["status"], DOCTOR_STATUS_UNHEALTHY);
    }

    #[test]
    fn doctor_control_plane_health_rejects_foreign_claude_hooks() {
        let health = serde_json::json!({
            "ok": true,
            "source": { "health": "healthy" },
            "hooks": { "health": "healthy" },
            "grok_hooks": { "health": "healthy" },
            "claude_hooks": { "health": "configured" },
        });
        let check = doctor_control_plane_health_check(&health);
        assert_eq!(check["ok"], serde_json::json!(false));
        assert_eq!(check["status"], DOCTOR_STATUS_UNHEALTHY);
    }

    #[test]
    fn doctor_mobile_health_requires_auth_contract() {
        let health = serde_json::json!({
            "ok": true,
            "requiresAuthentication": false,
        });
        let check = doctor_mobile_health_check(&health);
        assert_eq!(check["ok"], serde_json::json!(false));
        assert_eq!(check["status"], DOCTOR_STATUS_UNHEALTHY);
    }
}

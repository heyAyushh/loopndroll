use serde_json::Value;

use super::actions;
use super::json_value::{archived_label, json_array_len, json_path, value_array, value_array_path};
use super::state::TuiState;
use super::tabs::TuiTab;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PanelModel {
    pub(crate) title: &'static str,
    pub(crate) rows: Vec<RenderRow>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RenderRow {
    pub(crate) text: String,
    pub(crate) selected: bool,
    pub(crate) hovered: bool,
    pub(crate) tone: RowTone,
    pub(crate) target: Option<RowTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowTone {
    Default,
    Section,
    Muted,
    Good,
    Warning,
    Danger,
    Accent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowTarget {
    Session(usize),
    Connection(usize),
    Command(&'static str),
}

impl RenderRow {
    pub(super) fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            selected: false,
            hovered: false,
            tone: RowTone::Default,
            target: None,
        }
    }

    pub(super) fn selectable(selected: bool, hovered: bool, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            selected,
            hovered,
            tone: RowTone::Default,
            target: None,
        }
    }

    pub(super) fn section(text: impl Into<String>) -> Self {
        Self::plain(text).with_tone(RowTone::Section)
    }

    pub(super) fn muted(text: impl Into<String>) -> Self {
        Self::plain(text).with_tone(RowTone::Muted)
    }

    pub(super) fn accent(text: impl Into<String>) -> Self {
        Self::plain(text).with_tone(RowTone::Accent)
    }

    pub(super) fn warning(text: impl Into<String>) -> Self {
        Self::plain(text).with_tone(RowTone::Warning)
    }

    pub(super) fn good(text: impl Into<String>) -> Self {
        Self::plain(text).with_tone(RowTone::Good)
    }

    pub(super) fn with_tone(mut self, tone: RowTone) -> Self {
        self.tone = tone;
        self
    }

    pub(super) fn with_target(mut self, target: RowTarget) -> Self {
        self.target = Some(target);
        self
    }

    pub(super) fn action(
        hovered_command: Option<&'static str>,
        command: &'static str,
        text: impl Into<String>,
    ) -> Self {
        Self {
            text: text.into(),
            selected: false,
            hovered: hovered_command == Some(command),
            tone: RowTone::Accent,
            target: Some(RowTarget::Command(command)),
        }
    }
}

pub(super) fn panel_model(app: &TuiState) -> PanelModel {
    let rows = match app.tab {
        TuiTab::Dashboard => dashboard_rows(app),
        TuiTab::Sessions => session_rows(app),
        TuiTab::Connections => connection_rows(app),
        TuiTab::Settings => settings_rows(app),
        TuiTab::Logs => log_rows(app),
    };
    PanelModel {
        title: app.tab.title(),
        rows,
    }
}

fn dashboard_rows(app: &TuiState) -> Vec<RenderRow> {
    let server_health = json_path(&app.status, &["source", "health"]);
    let hooks_health = json_path(&app.status, &["hooks", "health"]);
    let bridge_summary = json_path(&app.snapshot, &["devin_desktop", "acp_bridge", "summary"]);
    vec![
        RenderRow::section("SYSTEM"),
        health_row("server", &server_health),
        health_row("hooks", &hooks_health),
        RenderRow::section("WORK"),
        RenderRow::accent(format!(
            "active sessions      {}",
            json_path(&app.snapshot, &["active_thread_count"])
        )),
        RenderRow::plain(format!(
            "automations          {}",
            json_array_len(&app.snapshot, "automations")
        )),
        RenderRow::plain(format!(
            "goals                {}",
            json_array_len(&app.snapshot, "goals")
        )),
        RenderRow::section("CONNECTIONS"),
        RenderRow::plain(format!(
            "iPhone               {}",
            connections_by_kind(app, "mobile")
        )),
        RenderRow::plain(format!(
            "Devin Desktop        {}",
            connections_by_kind(app, "devin")
        )),
        RenderRow::plain(format!(
            "Devin ACP agents     {}",
            value_array_path(&app.snapshot, &["devin_desktop", "acp_bridge", "agents"]).len()
        )),
        bridge_row(&bridge_summary),
    ]
}

fn session_rows(app: &TuiState) -> Vec<RenderRow> {
    let mut rows = vec![RenderRow::section("SESSIONS")];
    rows.extend(
        value_array(&app.snapshot, "threads")
            .iter()
            .enumerate()
            .map(|(index, thread)| {
                RenderRow::selectable(
                    index == app.selected_session,
                    Some(index) == app.hovered_session,
                    format!(
                        "{}  {}  {}",
                        json_path(thread, &["thread_id"]),
                        archived_label(thread),
                        json_path(thread, &["title"])
                    ),
                )
                .with_target(RowTarget::Session(index))
            })
            .collect::<Vec<_>>(),
    );
    if value_array(&app.snapshot, "threads").is_empty() {
        rows.push(RenderRow::muted("no sessions found"));
    }
    rows.extend(selected_session_rows(app));
    rows
}

fn connection_rows(app: &TuiState) -> Vec<RenderRow> {
    let mut rows = vec![RenderRow::section("CONNECTIONS")];
    rows.extend(
        value_array(&app.connections, "connections")
            .iter()
            .enumerate()
            .map(|(index, connection)| {
                RenderRow::selectable(
                    index == app.selected_connection,
                    Some(index) == app.hovered_connection,
                    format!(
                        "{}  {}  {}  {}",
                        json_path(connection, &["kind"]),
                        json_path(connection, &["status"]),
                        json_path(connection, &["label"]),
                        json_path(connection, &["detail"])
                    ),
                )
                .with_tone(connection_tone(connection))
                .with_target(RowTarget::Connection(index))
            })
            .collect::<Vec<_>>(),
    );
    if value_array(&app.connections, "connections").is_empty() {
        rows.push(RenderRow::muted("no iPhones paired yet"));
    }
    rows.extend(selected_connection_rows(app));
    rows.extend(devin_bridge_rows(app));
    rows.extend(pairing_rows(app));
    rows.extend(push_device_rows(app));
    rows
}

fn selected_connection_rows(app: &TuiState) -> Vec<RenderRow> {
    let Some(connection) =
        value_array(&app.connections, "connections").get(app.selected_connection)
    else {
        return vec![
            RenderRow::plain(""),
            RenderRow::muted("selected connection: none"),
        ];
    };
    let mut rows = vec![
        RenderRow::plain(""),
        RenderRow::section("SELECTED CONNECTION"),
        RenderRow::plain(format!("  id: {}", json_path(connection, &["id"]))),
        RenderRow::plain(format!("  kind: {}", json_path(connection, &["kind"]))),
        RenderRow::plain(format!("  status: {}", json_path(connection, &["status"]))),
        RenderRow::plain(format!("  label: {}", json_path(connection, &["label"]))),
        RenderRow::plain(format!(
            "  subtitle: {}",
            json_path(connection, &["subtitle"])
        )),
        RenderRow::plain(format!("  detail: {}", json_path(connection, &["detail"]))),
        RenderRow::plain(format!(
            "  last used: {}",
            json_path(connection, &["last_used_at"])
        )),
        RenderRow::plain(format!(
            "  actions: {}",
            json_path(connection, &["action_hint"])
        )),
    ];
    if app.selected_connection_can_rename() {
        rows.push(RenderRow::action(
            app.hovered_command,
            actions::RENAME_CONNECTION,
            "  action: rename selected connection",
        ));
    }
    if app.selected_connection_can_revoke() {
        rows.push(RenderRow::action(
            app.hovered_command,
            actions::REVOKE_CONNECTION,
            "  action: revoke selected connection",
        ));
    }
    rows
}

fn devin_bridge_rows(app: &TuiState) -> Vec<RenderRow> {
    let bridge_agents = value_array_path(&app.snapshot, &["devin_desktop", "acp_bridge", "agents"]);
    if bridge_agents.is_empty() {
        return Vec::new();
    }

    let mut rows = vec![
        RenderRow::plain(""),
        RenderRow::section("DEVIN ACP BRIDGE"),
        bridge_row(&json_path(
            &app.snapshot,
            &["devin_desktop", "acp_bridge", "summary"],
        )),
    ];
    rows.extend(bridge_agents.iter().map(|agent| {
        let control_level = json_path(agent, &["control_level"]);
        let tone = if control_level == "client-capable" {
            RowTone::Good
        } else {
            RowTone::Warning
        };
        RenderRow::plain(format!(
            "  {}  enabled:{}  preferred:{}  control:{}  launch:{}",
            json_path(agent, &["name"]),
            json_path(agent, &["enabled"]),
            json_path(agent, &["preferred"]),
            control_level,
            json_path(agent, &["launch_configured"])
        ))
        .with_tone(tone)
    }));
    rows.push(RenderRow::warning(
        "  boundary: explicit probe only; no Devin-native lifecycle control",
    ));
    rows
}

fn pairing_rows(app: &TuiState) -> Vec<RenderRow> {
    let primary_base_url = value_array(&app.pairing, "baseURLs")
        .first()
        .map(super::json_value::scalar_text)
        .unwrap_or_else(|| json_path(&app.pairing, &["baseURL"]));
    vec![
        RenderRow::plain(""),
        RenderRow::section("PAIR IPHONE"),
        RenderRow::plain(format!("  orb id: {}", json_path(&app.pairing, &["orbId"]))),
        RenderRow::plain(format!(
            "  orb image: {}",
            json_path(&app.pairing, &["orbImageURL"])
        )),
        RenderRow::plain(format!("  reachable url: {primary_base_url}")),
        RenderRow::plain(format!(
            "  manual code: {}",
            json_path(&app.pairing, &["code"])
        )),
        RenderRow::muted("  iPhone: scan the orb image or paste the manual code"),
        RenderRow::muted("  after pairing, the iPhone appears above as a mobile connection"),
        RenderRow::action(
            app.hovered_command,
            actions::NEW_PAIRING,
            "  action: create fresh pairing code",
        ),
    ]
}

fn push_device_rows(app: &TuiState) -> Vec<RenderRow> {
    let push_devices = value_array(&app.push_devices, "devices");
    let mut rows = vec![
        RenderRow::plain(""),
        RenderRow::section("IPHONE PUSH DEVICES"),
    ];
    if push_devices.is_empty() {
        rows.push(RenderRow::muted("  none registered yet"));
        return rows;
    }
    rows.extend(push_devices.iter().map(|device| {
        RenderRow::plain(format!(
            "  {}  {}  {}  {}  test:{}",
            json_path(device, &["installationId"]),
            json_path(device, &["state"]),
            json_path(device, &["deviceName"]),
            json_path(device, &["environment"]),
            json_path(device, &["canTest"])
        ))
    }));
    rows.push(RenderRow::accent("  command: :test-push <installation-id>"));
    rows
}

fn settings_rows(app: &TuiState) -> Vec<RenderRow> {
    let mut rows = vec![
        RenderRow::section("DEFAULTS"),
        RenderRow::plain(format!(
            "default prompt: {}",
            json_path(&app.mobile_state, &["defaultPrompt"])
        )),
        RenderRow::plain(format!(
            "scope: {}",
            json_path(&app.mobile_state, &["scope"])
        )),
        RenderRow::plain(format!(
            "global preset: {}",
            json_path(&app.mobile_state, &["globalPreset"])
        )),
        RenderRow::plain(format!(
            "global notification: {}",
            json_path(&app.mobile_state, &["globalNotificationId"])
        )),
        RenderRow::plain(format!(
            "global check: {}",
            json_path(&app.mobile_state, &["globalCompletionCheckId"])
        )),
        RenderRow::section("NOTIFICATIONS"),
    ];
    rows.extend(
        value_array(&app.mobile_state, "notifications")
            .iter()
            .map(|route| {
                RenderRow::plain(format!(
                    "  {}  {}  {}",
                    json_path(route, &["id"]),
                    json_path(route, &["channel"]),
                    json_path(route, &["label"])
                ))
            }),
    );
    if value_array(&app.mobile_state, "notifications").is_empty() {
        rows.push(RenderRow::muted("  no notification routes configured"));
    }
    rows.push(RenderRow::section("COMPLETION CHECKS"));
    rows.extend(
        value_array(&app.mobile_state, "completionChecks")
            .iter()
            .map(|check| {
                RenderRow::plain(format!(
                    "  {}  {}",
                    json_path(check, &["id"]),
                    json_path(check, &["label"])
                ))
            }),
    );
    if value_array(&app.mobile_state, "completionChecks").is_empty() {
        rows.push(RenderRow::muted("  no completion checks configured"));
    }
    rows.push(RenderRow::section("PUSH DEVICES"));
    let push_devices = value_array(&app.push_devices, "devices");
    if push_devices.is_empty() {
        rows.push(RenderRow::muted("  none registered from iPhone yet"));
    } else {
        rows.extend(push_devices.iter().map(|device| {
            RenderRow::plain(format!(
                "  {}  {}  {}  test:{}",
                json_path(device, &["state"]),
                json_path(device, &["deviceName"]),
                json_path(device, &["environment"]),
                json_path(device, &["canTest"])
            ))
        }));
    }
    rows.push(RenderRow::accent(
        "iPhone-compatible settings: default-prompt; push register/test is phone-initiated",
    ));
    rows.push(RenderRow::action(
        app.hovered_command,
        actions::SET_DEFAULT_PROMPT,
        "action: edit default prompt",
    ));
    rows.push(RenderRow::action(
        app.hovered_command,
        actions::SET_GLOBAL_PRESET,
        "action: set global preset",
    ));
    rows.push(RenderRow::action(
        app.hovered_command,
        actions::SET_GLOBAL_NOTIFICATION,
        "action: set global notification route",
    ));
    rows.push(RenderRow::action(
        app.hovered_command,
        actions::SET_GLOBAL_CHECK,
        "action: set global completion check",
    ));
    rows
}

fn selected_session_rows(app: &TuiState) -> Vec<RenderRow> {
    let Some(thread_id) = app.selected_thread_id() else {
        return vec![
            RenderRow::plain(""),
            RenderRow::muted("prompt composer: no selected session"),
        ];
    };
    let selected_thread = value_array(&app.snapshot, "threads")
        .get(app.selected_session)
        .unwrap_or(&Value::Null);
    let session_override = app
        .mobile_state
        .get("sessions")
        .and_then(|sessions| sessions.get(&thread_id))
        .cloned()
        .unwrap_or(Value::Null);
    vec![
        RenderRow::plain(""),
        RenderRow::section("SELECTED SESSION"),
        RenderRow::accent(format!("selected: {thread_id}")),
        RenderRow::plain(format!(
            "  title: {}",
            json_path(selected_thread, &["title"])
        )),
        RenderRow::plain(format!("  cwd: {}", json_path(selected_thread, &["cwd"]))),
        RenderRow::plain(format!(
            "  model: {}",
            json_path(selected_thread, &["model"])
        )),
        RenderRow::plain(format!(
            "  mode override: {}",
            json_path(&session_override, &["preset"])
        )),
        RenderRow::plain(format!(
            "  archived override: {}",
            json_path(&session_override, &["archived"])
        )),
        RenderRow::plain(format!(
            "  muted: {}",
            json_path(&session_override, &["muted"])
        )),
        RenderRow::plain(format!(
            "  completion check: {}",
            json_path(&session_override, &["completionCheckId"])
        )),
        RenderRow::plain(format!(
            "  notifications: {}",
            json_path(&session_override, &["notificationIds"])
        )),
        RenderRow::plain(""),
        RenderRow::section("PROMPT COMPOSER"),
        RenderRow::plain(format!("  selected target: {thread_id}")),
        RenderRow::plain(format!(
            "  active targets: {}",
            app.active_thread_ids().len()
        )),
        RenderRow::muted("  send selected: :prompt <text>"),
        RenderRow::muted("  set mode + send selected: :prompt-mode <preset> <text>"),
        RenderRow::muted("  send all active: :prompt-active <text>"),
        RenderRow::muted("  set mode + send all active: :prompt-active-mode <preset> <text>"),
        RenderRow::action(
            app.hovered_command,
            actions::PROMPT_SELECTED,
            "  action: prompt selected session",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::PROMPT_ACTIVE,
            "  action: prompt all active sessions",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::PROMPT_SELECTED_WITH_MODE,
            "  action: set selected mode and prompt",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::PROMPT_ACTIVE_WITH_MODE,
            "  action: set active mode and prompt",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::SET_SESSION_MODE,
            "  action: set selected session mode",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::ARCHIVE_SESSION,
            "  action: archive selected session",
        ),
        RenderRow::action(
            app.hovered_command,
            actions::MUTE_SESSION,
            "  action: mute selected session",
        ),
    ]
}

fn log_rows(app: &TuiState) -> Vec<RenderRow> {
    if app.events.is_empty() {
        return vec![RenderRow::muted("waiting for /events/tail")];
    }
    app.events
        .iter()
        .rev()
        .skip(app.log_scroll)
        .map(|event| RenderRow::plain(event.clone()))
        .collect()
}

fn connections_by_kind(app: &TuiState, kind: &str) -> usize {
    value_array(&app.connections, "connections")
        .iter()
        .filter(|connection| connection.get("kind").and_then(Value::as_str) == Some(kind))
        .count()
}

fn health_row(label: &str, health: &str) -> RenderRow {
    let tone = match health {
        "healthy" | "connected" | "active" => RowTone::Good,
        "degraded" | "configured" | "installed" => RowTone::Warning,
        "missing" | "unhealthy" | "error" => RowTone::Danger,
        _ => RowTone::Muted,
    };
    RenderRow::plain(format!("{label:<20} {health}")).with_tone(tone)
}

fn bridge_row(summary: &str) -> RenderRow {
    if summary.contains("can attach") {
        RenderRow::good(format!("Devin ACP bridge    {summary}"))
    } else if summary.contains("disabled") || summary.contains("no registry") {
        RenderRow::warning(format!("Devin ACP bridge    {summary}"))
    } else {
        RenderRow::muted(format!("Devin ACP bridge    {summary}"))
    }
}

fn connection_tone(connection: &Value) -> RowTone {
    match connection.get("status").and_then(Value::as_str) {
        Some("connected" | "active" | "healthy") => RowTone::Good,
        Some("configured" | "installed") => RowTone::Warning,
        Some("missing" | "revoked" | "error") => RowTone::Danger,
        _ => RowTone::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::state::ServerData;

    #[test]
    fn panel_model_is_pure_data() {
        let app = TuiState::with_server_data(ServerData {
            status: serde_json::json!({
                "source": { "health": "healthy" },
                "hooks": { "health": "healthy" }
            }),
            snapshot: serde_json::Value::Null,
            mobile_state: serde_json::Value::Null,
            connections: serde_json::Value::Null,
            push_devices: serde_json::Value::Null,
            pairing: None,
        });
        let panel = panel_model(&app);

        assert_eq!(panel.title, "Dashboard");
        assert_eq!(panel.rows[0].text, "SYSTEM");
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text == "server               healthy")
        );
    }

    #[test]
    fn sessions_panel_exposes_prompt_composer() {
        let mut app = TuiState::with_server_data(ServerData {
            status: serde_json::Value::Null,
            snapshot: serde_json::json!({
                "threads": [
                    {
                        "thread_id": "thread-1",
                        "archived": false,
                        "title": "one",
                        "cwd": "/workspace",
                        "model": "codex"
                    },
                    {
                        "thread_id": "thread-2",
                        "archived": true,
                        "title": "two"
                    }
                ]
            }),
            mobile_state: serde_json::json!({
                "sessions": {
                    "thread-1": {
                        "preset": "max-turns-1",
                        "archived": null,
                        "muted": false,
                        "completionCheckId": null,
                        "notificationIds": []
                    }
                }
            }),
            connections: serde_json::Value::Null,
            push_devices: serde_json::Value::Null,
            pairing: None,
        });
        app.next_tab();
        let panel = panel_model(&app);

        assert!(panel.rows.iter().any(|row| row.text == "PROMPT COMPOSER"));
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text == "  active targets: 1")
        );
        assert!(panel.rows.iter().any(|row| {
            row.target == Some(RowTarget::Command(actions::PROMPT_SELECTED))
                && row.text == "  action: prompt selected session"
        }));
    }

    #[test]
    fn connections_panel_exposes_pairing_and_push_cockpit() {
        let mut app = TuiState::with_server_data(ServerData {
            status: serde_json::Value::Null,
            snapshot: serde_json::json!({
                "devin_desktop": {
                    "acp_bridge": {
                        "summary": "Devin ACP bridge can attach to configured agent transports after explicit approval",
                        "agents": [
                            {
                                "name": "Codex",
                                "enabled": true,
                                "preferred": true,
                                "control_level": "client-capable",
                                "launch_configured": true
                            }
                        ]
                    }
                }
            }),
            mobile_state: serde_json::Value::Null,
            connections: serde_json::json!({
                "connections": [
                    {
                        "id": "pairing-1",
                        "kind": "mobile",
                        "status": "active",
                        "label": "iPhone",
                        "subtitle": "passkey",
                        "detail": "paired",
                        "last_used_at": "2026-06-04T00:00:00Z",
                        "action_hint": "rename or revoke",
                        "can_rename": true,
                        "can_revoke": true
                    }
                ]
            }),
            push_devices: serde_json::json!({
                "devices": [
                    {
                        "installationId": "install-1",
                        "state": "stored-awaiting-provider",
                        "deviceName": "iPhone",
                        "environment": "development",
                        "canTest": false
                    }
                ]
            }),
            pairing: Some(serde_json::json!({
                "orbId": "orb-1",
                "orbImageURL": "http://127.0.0.1/orb",
                "baseURLs": ["http://127.0.0.1:8765"],
                "code": "123456"
            })),
        });
        app.next_tab();
        app.next_tab();
        let panel = panel_model(&app);

        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text == "SELECTED CONNECTION")
        );
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text == "IPHONE PUSH DEVICES")
        );
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text == "  command: :test-push <installation-id>")
        );
        assert!(panel.rows.iter().any(|row| {
            row.target == Some(RowTarget::Command(actions::NEW_PAIRING))
                && row.text == "  action: create fresh pairing code"
        }));
        assert!(panel.rows.iter().any(|row| {
            row.target == Some(RowTarget::Command(actions::RENAME_CONNECTION))
                && row.text == "  action: rename selected connection"
        }));
        assert!(panel.rows.iter().any(|row| {
            row.target == Some(RowTarget::Command(actions::REVOKE_CONNECTION))
                && row.text == "  action: revoke selected connection"
        }));
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text.starts_with("Devin ACP bridge"))
        );
        assert!(
            panel
                .rows
                .iter()
                .any(|row| row.text.contains("control:client-capable"))
        );
    }
}

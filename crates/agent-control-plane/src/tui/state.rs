use ratatui::layout::Rect;
use serde_json::Value;

use super::actions;
use super::geometry::{PointerState, UiGeometry, inner_rect, rect_contains};
use super::json_value::{string_field, value_array};
use super::model::{PanelModel, RowTarget, panel_model};
use super::tabs::TuiTab;

pub(crate) const COMMAND_PREFIX: char = ':';

#[derive(Debug)]
pub(crate) struct ServerData {
    pub(crate) status: Value,
    pub(crate) snapshot: Value,
    pub(crate) mobile_state: Value,
    pub(crate) connections: Value,
    pub(crate) push_devices: Value,
    pub(crate) pairing: Option<Value>,
}

#[derive(Debug)]
pub(crate) struct TuiState {
    pub(super) tab: TuiTab,
    pub(super) selected_session: usize,
    pub(super) selected_connection: usize,
    pub(super) status: Value,
    pub(super) snapshot: Value,
    pub(super) mobile_state: Value,
    pub(super) connections: Value,
    pub(super) push_devices: Value,
    pub(super) pairing: Value,
    command_mode: bool,
    command_buffer: String,
    status_line: String,
    pointer: Option<PointerState>,
    hovered_tab: Option<TuiTab>,
    pub(super) hovered_session: Option<usize>,
    pub(super) hovered_connection: Option<usize>,
    pub(super) hovered_command: Option<&'static str>,
    ui_geometry: UiGeometry,
}

impl Default for TuiState {
    fn default() -> Self {
        Self {
            tab: TuiTab::Dashboard,
            selected_session: 0,
            selected_connection: 0,
            status: Value::Null,
            snapshot: Value::Null,
            mobile_state: Value::Null,
            connections: Value::Null,
            push_devices: Value::Null,
            pairing: Value::Null,
            command_mode: false,
            command_buffer: String::new(),
            status_line:
                "left/right tabs  up/down select  mouse click/scroll  : command  r refresh  q quit"
                    .to_owned(),
            pointer: None,
            hovered_tab: None,
            hovered_session: None,
            hovered_connection: None,
            hovered_command: None,
            ui_geometry: UiGeometry::default(),
        }
    }
}

impl TuiState {
    pub(crate) fn with_server_data(data: ServerData) -> Self {
        let mut state = Self::default();
        state.replace_server_data(data);
        state
    }

    pub(crate) fn replace_server_data(&mut self, data: ServerData) {
        self.status = data.status;
        self.snapshot = data.snapshot;
        self.mobile_state = data.mobile_state;
        self.connections = data.connections;
        self.push_devices = data.push_devices;
        if let Some(pairing) = data.pairing {
            self.pairing = pairing;
        }
        self.clamp_selection();
    }

    pub(crate) fn replace_pairing(&mut self, pairing: Value) {
        self.pairing = pairing;
    }

    pub(crate) fn tab(&self) -> TuiTab {
        self.tab
    }

    pub(crate) fn tab_index(&self) -> usize {
        self.tab.index()
    }

    pub(crate) fn command_mode(&self) -> bool {
        self.command_mode
    }

    pub(crate) fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    pub(crate) fn set_status_line(&mut self, value: impl Into<String>) {
        self.status_line = value.into();
    }

    pub(crate) fn next_tab(&mut self) {
        self.tab = self.tab.next();
    }

    pub(crate) fn previous_tab(&mut self) {
        self.tab = self.tab.previous();
    }

    pub(crate) fn next_row(&mut self) {
        match self.tab {
            TuiTab::Sessions => {
                self.selected_session = next_index(self.selected_session, self.session_count())
            }
            TuiTab::Connections => {
                self.selected_connection =
                    next_index(self.selected_connection, self.connection_count());
            }
            TuiTab::Dashboard | TuiTab::Settings => {}
        }
    }

    pub(crate) fn previous_row(&mut self) {
        match self.tab {
            TuiTab::Sessions => {
                self.selected_session = previous_index(self.selected_session, self.session_count())
            }
            TuiTab::Connections => {
                self.selected_connection =
                    previous_index(self.selected_connection, self.connection_count());
            }
            TuiTab::Dashboard | TuiTab::Settings => {}
        }
    }

    pub(crate) fn start_command(&mut self) {
        self.command_mode = true;
        self.command_buffer.clear();
        self.status_line = self.command_help();
    }

    pub(crate) fn start_command_with(&mut self, command: &'static str) {
        self.command_mode = true;
        self.command_buffer = command.to_owned();
        self.status_line = self.command_help();
    }

    pub(crate) fn start_context_command(&mut self) {
        match self.pointer.and_then(|pointer| self.row_target_at(pointer)) {
            Some(RowTarget::Session(index)) => {
                self.selected_session = index;
                self.start_command_with(actions::PROMPT_SELECTED);
            }
            Some(RowTarget::Connection(index)) => {
                self.selected_connection = index;
                if self.selected_connection_can_rename() {
                    self.start_command_with(actions::RENAME_CONNECTION);
                } else {
                    self.start_command();
                }
            }
            Some(RowTarget::Command(command)) => self.start_command_with(command),
            None => self.start_command(),
        }
    }

    pub(crate) fn cancel_command(&mut self) {
        self.command_mode = false;
        self.command_buffer.clear();
    }

    pub(crate) fn push_command_char(&mut self, value: char) {
        self.command_buffer.push(value);
    }

    pub(crate) fn pop_command_char(&mut self) {
        self.command_buffer.pop();
    }

    pub(crate) fn panel_model(&self) -> PanelModel {
        panel_model(self)
    }

    pub(crate) fn footer_text(&self) -> String {
        if self.command_mode {
            return format!("{}{}_", COMMAND_PREFIX, self.command_buffer);
        }
        self.status_line.clone()
    }

    pub(crate) fn selected_thread_id(&self) -> Option<String> {
        value_array(&self.snapshot, "threads")
            .get(self.selected_session)
            .and_then(|thread| string_field(thread, "thread_id"))
    }

    pub(crate) fn focus_thread(&mut self, thread_id: &str) -> bool {
        let Some(index) = value_array(&self.snapshot, "threads")
            .iter()
            .position(|thread| string_field(thread, "thread_id").as_deref() == Some(thread_id))
        else {
            self.tab = TuiTab::Sessions;
            self.status_line = format!("session not found: {thread_id}");
            return false;
        };
        self.tab = TuiTab::Sessions;
        self.selected_session = index;
        self.status_line = self.selected_session_status();
        true
    }

    pub(crate) fn active_thread_ids(&self) -> Vec<String> {
        value_array(&self.snapshot, "threads")
            .iter()
            .filter(|thread| {
                !thread
                    .get("archived")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .filter_map(|thread| string_field(thread, "thread_id"))
            .collect()
    }

    pub(crate) fn selected_connection_id(&self) -> Option<String> {
        value_array(&self.connections, "connections")
            .get(self.selected_connection)
            .and_then(|connection| string_field(connection, "id"))
    }

    pub(crate) fn selected_connection_can_revoke(&self) -> bool {
        selected_connection(self)
            .and_then(|connection| connection.get("can_revoke"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub(crate) fn selected_connection_can_rename(&self) -> bool {
        selected_connection(self)
            .and_then(|connection| connection.get("can_rename"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    pub(crate) fn remember_geometry(&mut self, tabs: Rect, content: Rect, footer: Rect) {
        self.ui_geometry = UiGeometry {
            tabs,
            content,
            footer,
        };
        self.refresh_hover();
    }

    pub(crate) fn update_pointer(&mut self, column: u16, row: u16) {
        self.pointer = Some(PointerState { column, row });
        self.refresh_hover();
    }

    pub(crate) fn activate_pointer_target(&mut self) {
        if let Some(tab) = self.hovered_tab {
            self.tab = tab;
            self.clamp_selection();
            self.status_line = format!("tab: {}", tab.title());
            return;
        }

        match self.pointer.and_then(|pointer| self.row_target_at(pointer)) {
            Some(RowTarget::Session(index)) => {
                self.selected_session = index;
                self.status_line = self.selected_session_status();
            }
            Some(RowTarget::Connection(index)) => {
                self.selected_connection = index;
                self.status_line = self.selected_connection_status();
            }
            Some(RowTarget::Command(command)) => self.start_command_with(command),
            None => {}
        }
    }

    pub(crate) fn drag_pointer_target(&mut self) {
        match self.tab {
            TuiTab::Sessions => {
                if let Some(index) = self.hovered_session {
                    self.selected_session = index;
                }
            }
            TuiTab::Connections => {
                if let Some(index) = self.hovered_connection {
                    self.selected_connection = index;
                }
            }
            TuiTab::Dashboard | TuiTab::Settings => {}
        }
    }

    pub(crate) fn scroll_pointer_target_up(&mut self) {
        if self.hovered_tab.is_some() {
            self.previous_tab();
            return;
        }
        self.previous_row();
    }

    pub(crate) fn scroll_pointer_target_down(&mut self) {
        if self.hovered_tab.is_some() {
            self.next_tab();
            return;
        }
        self.next_row();
    }

    fn session_count(&self) -> usize {
        value_array(&self.snapshot, "threads").len()
    }

    fn connection_count(&self) -> usize {
        value_array(&self.connections, "connections").len()
    }

    fn clamp_selection(&mut self) {
        self.selected_session = clamp_index(self.selected_session, self.session_count());
        self.selected_connection = clamp_index(self.selected_connection, self.connection_count());
        self.refresh_hover();
    }

    fn command_help(&self) -> String {
        match self.tab {
            TuiTab::Sessions => {
                "prompt <text> | prompt-mode <preset> <text> | prompt-active <text> | prompt-active-mode <preset> <text> | mode <preset|off>".to_owned()
            }
            TuiTab::Connections => "new-pairing | rename <label> | revoke | test-push <installation-id>".to_owned(),
            TuiTab::Settings => "default-prompt <text> | scope <scope> | global-preset <preset|off> | notify-slack <label> <webhook> | test-push <installation-id>".to_owned(),
            TuiTab::Dashboard => {
                "refresh commands available on Sessions, Connections, Settings".to_owned()
            }
        }
    }

    fn refresh_hover(&mut self) {
        self.hovered_tab = self.pointer.and_then(|pointer| self.tab_at(pointer));
        self.hovered_session = None;
        self.hovered_connection = None;
        self.hovered_command = None;
        let hovered_target = self.pointer.and_then(|pointer| self.row_target_at(pointer));
        match hovered_target {
            Some(RowTarget::Session(index)) => self.hovered_session = Some(index),
            Some(RowTarget::Connection(index)) => self.hovered_connection = Some(index),
            Some(RowTarget::Command(command)) => self.hovered_command = Some(command),
            None => {}
        }
    }

    fn tab_at(&self, pointer: PointerState) -> Option<TuiTab> {
        let tab_area = inner_rect(self.ui_geometry.tabs)?;
        if !rect_contains(tab_area, pointer) {
            return None;
        }
        let tab_count = TuiTab::ALL.len();
        let relative_column = usize::from(pointer.column.saturating_sub(tab_area.x));
        let inner_width = usize::from(tab_area.width.max(1));
        let tab_index =
            (relative_column * tab_count / inner_width).min(tab_count.saturating_sub(1));
        Some(TuiTab::from_index(tab_index))
    }

    fn row_target_at(&self, pointer: PointerState) -> Option<RowTarget> {
        let content_area = inner_rect(self.ui_geometry.content)?;
        if !rect_contains(content_area, pointer) {
            return None;
        }
        let row_index = usize::from(pointer.row.saturating_sub(content_area.y));
        self.panel_model().rows.get(row_index)?.target
    }

    fn selected_session_status(&self) -> String {
        self.selected_thread_id()
            .map(|thread_id| format!("selected session: {thread_id}"))
            .unwrap_or_else(|| "no session selected".to_owned())
    }

    fn selected_connection_status(&self) -> String {
        self.selected_connection_id()
            .map(|connection_id| format!("selected connection: {connection_id}"))
            .unwrap_or_else(|| "no connection selected".to_owned())
    }
}

fn selected_connection(app: &TuiState) -> Option<&Value> {
    value_array(&app.connections, "connections").get(app.selected_connection)
}

fn next_index(current: usize, len: usize) -> usize {
    if len == 0 { 0 } else { (current + 1) % len }
}

fn previous_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else if current == 0 {
        len - 1
    } else {
        current - 1
    }
}

fn clamp_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        current.min(len.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    use crate::tui::actions;
    use crate::tui::geometry::{BLOCK_BORDER_WIDTH, FOOTER_HEIGHT, TAB_BAR_HEIGHT};

    #[test]
    fn tab_navigation_uses_typed_tabs() {
        let mut app = TuiState::default();
        app.next_tab();
        assert_eq!(app.tab(), TuiTab::Sessions);
        app.previous_tab();
        assert_eq!(app.tab(), TuiTab::Dashboard);
    }

    #[test]
    fn row_selection_wraps_without_panics() {
        assert_eq!(next_index(0, 0), 0);
        assert_eq!(next_index(1, 2), 0);
        assert_eq!(previous_index(0, 2), 1);
    }

    #[test]
    fn pointer_maps_to_equal_width_tabs() {
        let mut app = TuiState::default();
        app.remember_geometry(
            Rect::new(0, 0, 50, TAB_BAR_HEIGHT),
            Rect::new(0, TAB_BAR_HEIGHT, 50, 10),
            Rect::new(0, 13, 50, FOOTER_HEIGHT),
        );
        app.update_pointer(13, 1);
        app.activate_pointer_target();

        assert_eq!(app.tab(), TuiTab::Sessions);
    }

    #[test]
    fn pointer_selects_session_row_inside_content() {
        let mut app = TuiState {
            snapshot: serde_json::json!({
                "threads": [
                    { "thread_id": "thread-1", "archived": false, "title": "one" },
                    { "thread_id": "thread-2", "archived": false, "title": "two" }
                ]
            }),
            tab: TuiTab::Sessions,
            ..TuiState::default()
        };
        app.remember_geometry(
            Rect::new(0, 0, 50, TAB_BAR_HEIGHT),
            Rect::new(0, TAB_BAR_HEIGHT, 50, 10),
            Rect::new(0, 13, 50, FOOTER_HEIGHT),
        );
        app.update_pointer(2, TAB_BAR_HEIGHT + BLOCK_BORDER_WIDTH + 2);
        app.activate_pointer_target();

        assert_eq!(app.selected_session, 1);
        assert_eq!(app.selected_thread_id().as_deref(), Some("thread-2"));
    }

    #[test]
    fn context_click_on_session_prefills_prompt_command() {
        let mut app = TuiState {
            snapshot: serde_json::json!({
                "threads": [
                    { "thread_id": "thread-1", "archived": false, "title": "one" }
                ]
            }),
            tab: TuiTab::Sessions,
            ..TuiState::default()
        };
        app.remember_geometry(
            Rect::new(0, 0, 50, TAB_BAR_HEIGHT),
            Rect::new(0, TAB_BAR_HEIGHT, 50, 24),
            Rect::new(0, 27, 50, FOOTER_HEIGHT),
        );
        app.update_pointer(2, TAB_BAR_HEIGHT + BLOCK_BORDER_WIDTH + 1);
        app.start_context_command();

        assert!(app.command_mode());
        assert_eq!(app.command_buffer(), actions::PROMPT_SELECTED);
        assert_eq!(app.selected_thread_id().as_deref(), Some("thread-1"));
    }

    #[test]
    fn action_row_click_prefills_command() {
        let mut app = TuiState {
            snapshot: serde_json::json!({
                "threads": [
                    {
                        "thread_id": "thread-1",
                        "archived": false,
                        "title": "one",
                        "cwd": "/workspace",
                        "model": "codex"
                    }
                ]
            }),
            tab: TuiTab::Sessions,
            ..TuiState::default()
        };
        app.remember_geometry(
            Rect::new(0, 0, 50, TAB_BAR_HEIGHT),
            Rect::new(0, TAB_BAR_HEIGHT, 50, 32),
            Rect::new(0, 35, 50, FOOTER_HEIGHT),
        );
        let action_row_index = app
            .panel_model()
            .rows
            .iter()
            .position(|row| row.target == Some(RowTarget::Command(actions::PROMPT_SELECTED)))
            .expect("prompt action row");
        app.update_pointer(
            2,
            TAB_BAR_HEIGHT + BLOCK_BORDER_WIDTH + u16::try_from(action_row_index).unwrap(),
        );
        app.activate_pointer_target();

        assert!(app.command_mode());
        assert_eq!(app.command_buffer(), actions::PROMPT_SELECTED);
    }

    #[test]
    fn pointer_ignores_connection_section_header() {
        let mut app = TuiState {
            connections: serde_json::json!({
                "connections": [
                    { "id": "mobile-1", "kind": "mobile", "status": "active", "label": "iPhone" },
                    { "id": "codex-hooks", "kind": "codex", "status": "healthy", "label": "Hooks" }
                ]
            }),
            tab: TuiTab::Connections,
            ..TuiState::default()
        };
        app.remember_geometry(
            Rect::new(0, 0, 50, TAB_BAR_HEIGHT),
            Rect::new(0, TAB_BAR_HEIGHT, 50, 10),
            Rect::new(0, 13, 50, FOOTER_HEIGHT),
        );
        app.update_pointer(2, TAB_BAR_HEIGHT + BLOCK_BORDER_WIDTH);
        app.activate_pointer_target();
        assert_eq!(app.selected_connection, 0);

        app.update_pointer(2, TAB_BAR_HEIGHT + BLOCK_BORDER_WIDTH + 2);
        app.activate_pointer_target();
        assert_eq!(app.selected_connection_id().as_deref(), Some("codex-hooks"));
    }

    #[test]
    fn active_thread_ids_skip_archived_sessions() {
        let app = TuiState {
            snapshot: serde_json::json!({
                "threads": [
                    { "thread_id": "thread-1", "archived": false },
                    { "thread_id": "thread-2", "archived": true },
                    { "thread_id": "thread-3", "archived": false }
                ]
            }),
            ..TuiState::default()
        };

        assert_eq!(
            app.active_thread_ids(),
            vec!["thread-1".to_owned(), "thread-3".to_owned()]
        );
    }

    #[test]
    fn focus_thread_selects_session_tab_and_row() {
        let mut app = TuiState {
            snapshot: serde_json::json!({
                "threads": [
                    { "thread_id": "thread-1", "archived": false },
                    { "thread_id": "thread-2", "archived": false }
                ]
            }),
            ..TuiState::default()
        };

        assert!(app.focus_thread("thread-2"));
        assert_eq!(app.tab(), TuiTab::Sessions);
        assert_eq!(app.selected_thread_id().as_deref(), Some("thread-2"));
    }
}

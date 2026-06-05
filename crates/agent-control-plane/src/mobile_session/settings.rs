use rusqlite::{Connection, params};

use super::normalization::{
    DEFAULT_ASSISTANT_SURFACE, DEFAULT_SCOPE, normalize_prompt_or_default,
    normalized_assistant_surface, normalized_preset, normalized_scope, now_iso_string,
};
use super::{DEFAULT_REMOTE_PROMPT, MobileSessionResult, MobileSessionService};

pub(super) struct MobileSettingsRow {
    pub(super) default_prompt: String,
    pub(super) scope: String,
    pub(super) global_preset: Option<String>,
    pub(super) global_notification_id: Option<String>,
    pub(super) global_completion_check_id: Option<String>,
    pub(super) global_completion_check_wait_for_reply: bool,
    pub(super) assistant_surface: String,
}

impl Default for MobileSettingsRow {
    fn default() -> Self {
        Self {
            default_prompt: DEFAULT_REMOTE_PROMPT.to_owned(),
            scope: DEFAULT_SCOPE.to_owned(),
            global_preset: None,
            global_notification_id: None,
            global_completion_check_id: None,
            global_completion_check_wait_for_reply: false,
            assistant_surface: DEFAULT_ASSISTANT_SURFACE.to_owned(),
        }
    }
}

impl MobileSessionService {
    pub fn save_default_prompt(&self, prompt: &str) -> MobileSessionResult<()> {
        let prompt = normalize_prompt_or_default(prompt);
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "update mobile_settings set default_prompt = ?1, updated_at = ?2 where id = 1",
            params![prompt, now_iso_string()?],
        )?;
        Ok(())
    }

    pub fn set_scope(&self, scope: &str) -> MobileSessionResult<()> {
        let scope = normalized_scope(scope)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "update mobile_settings set scope = ?1, updated_at = ?2 where id = 1",
            params![scope, now_iso_string()?],
        )?;
        Ok(())
    }

    pub fn set_assistant_surface(&self, surface: &str) -> MobileSessionResult<()> {
        let surface = normalized_assistant_surface(surface)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "update mobile_settings set assistant_surface = ?1, updated_at = ?2 where id = 1",
            params![surface, now_iso_string()?],
        )?;
        Ok(())
    }

    pub fn set_global_preset(&self, preset: Option<&str>) -> MobileSessionResult<()> {
        let preset = normalized_preset(preset)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "update mobile_settings set global_preset = ?1, updated_at = ?2 where id = 1",
            params![preset, now_iso_string()?],
        )?;
        Ok(())
    }
}

use rusqlite::{Connection, OptionalExtension, params};

use super::normalization::{
    ENABLED_FLAG, bool_to_flag, normalized_preset, normalized_required, now_iso_string,
};
use super::{MobileSessionError, MobileSessionOverride, MobileSessionResult, MobileSessionService};

#[derive(Default)]
pub(super) struct SessionOverrideUpdate {
    pub(super) preset: Option<Option<String>>,
    pub(super) archived: Option<Option<bool>>,
    pub(super) muted: Option<bool>,
    pub(super) deleted_at: Option<Option<String>>,
    pub(super) completion_check_id: Option<Option<String>>,
    pub(super) completion_check_wait_for_reply: Option<bool>,
}

impl MobileSessionService {
    pub fn set_session_preset(
        &self,
        thread_id: &str,
        preset: Option<&str>,
    ) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        let preset = normalized_preset(preset)?;
        self.upsert_session_override(
            &thread_id,
            SessionOverrideUpdate {
                preset: Some(preset.clone()),
                ..SessionOverrideUpdate::default()
            },
        )?;
        self.clear_runtime(&thread_id)?;
        self.clear_prompts_for_mode_change(&thread_id, preset.as_deref())?;
        Ok(())
    }

    pub fn set_session_archived(&self, thread_id: &str, archived: bool) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        self.upsert_session_override(
            &thread_id,
            SessionOverrideUpdate {
                archived: Some(Some(archived)),
                ..SessionOverrideUpdate::default()
            },
        )?;
        if archived {
            self.clear_runtime(&thread_id)?;
            self.clear_prompts(&thread_id)?;
        }
        Ok(())
    }

    pub fn mute_session(&self, thread_id: &str) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        self.upsert_session_override(
            &thread_id,
            SessionOverrideUpdate {
                muted: Some(true),
                ..SessionOverrideUpdate::default()
            },
        )
    }

    pub fn delete_session(&self, thread_id: &str) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        self.upsert_session_override(
            &thread_id,
            SessionOverrideUpdate {
                deleted_at: Some(Some(now_iso_string()?)),
                ..SessionOverrideUpdate::default()
            },
        )?;
        self.clear_runtime(&thread_id)?;
        self.clear_prompts(&thread_id)?;
        Ok(())
    }

    pub(super) fn upsert_session_override(
        &self,
        thread_id: &str,
        update: SessionOverrideUpdate,
    ) -> MobileSessionResult<()> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        let current = self.session_override(thread_id)?;
        let next_preset = update.preset.unwrap_or(current.preset);
        let next_archived = update.archived.unwrap_or(current.archived);
        let next_muted = update.muted.unwrap_or(current.muted);
        let next_deleted_at = update.deleted_at.unwrap_or(current.deleted_at);
        let next_completion_check_id = update
            .completion_check_id
            .unwrap_or(current.completion_check_id);
        let next_completion_check_wait_for_reply = update
            .completion_check_wait_for_reply
            .unwrap_or(current.completion_check_wait_for_reply);
        connection.execute(
            "insert into mobile_session_overrides (
                thread_id,
                preset,
                archived,
                muted,
                deleted_at,
                completion_check_id,
                completion_check_wait_for_reply,
                updated_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             on conflict(thread_id) do update set
                preset = excluded.preset,
                archived = excluded.archived,
                muted = excluded.muted,
                deleted_at = excluded.deleted_at,
                completion_check_id = excluded.completion_check_id,
                completion_check_wait_for_reply = excluded.completion_check_wait_for_reply,
                updated_at = excluded.updated_at",
            params![
                thread_id,
                next_preset,
                next_archived.map(bool_to_flag),
                bool_to_flag(next_muted),
                next_deleted_at,
                next_completion_check_id,
                bool_to_flag(next_completion_check_wait_for_reply),
                now_iso_string()?,
            ],
        )?;
        Ok(())
    }

    pub(super) fn session_override(
        &self,
        thread_id: &str,
    ) -> MobileSessionResult<MobileSessionOverride> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        let override_state = connection
            .query_row(
                "select
                    preset,
                    archived,
                    muted,
                    deleted_at,
                    completion_check_id,
                    completion_check_wait_for_reply
                 from mobile_session_overrides
                 where thread_id = ?1",
                [thread_id],
                |row| {
                    let archived = row
                        .get::<_, Option<i64>>(1)?
                        .map(|value| value == ENABLED_FLAG);
                    let deleted_at = row.get::<_, Option<String>>(3)?;
                    Ok(MobileSessionOverride {
                        preset: row.get(0)?,
                        archived,
                        muted: row.get::<_, i64>(2)? == ENABLED_FLAG,
                        deleted: deleted_at.is_some(),
                        deleted_at,
                        notification_ids: Vec::new(),
                        completion_check_id: row.get(4)?,
                        completion_check_wait_for_reply: row.get::<_, i64>(5)? == ENABLED_FLAG,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        Ok(override_state)
    }
}

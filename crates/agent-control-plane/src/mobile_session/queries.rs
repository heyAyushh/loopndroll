use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, params};

use super::normalization::{
    normalized_option, normalized_optional, now_iso_string, parse_commands_json,
};
use super::notifications::telegram_token_from_bot_url;
use super::{
    MobileCompletionCheck, MobileNotificationRoute, MobileSessionError, MobileSessionLifecycle,
    MobileSessionResult,
};

pub(super) const REMOTE_PROMPT_STATUS_QUEUED: &str = "queued";
pub(super) const REMOTE_PROMPT_STATUS_DELIVERED: &str = "delivered";

pub(super) fn prompt_for_mode(
    connection: &Connection,
    thread_id: &str,
    delivery_mode: &str,
    mark_delivered: bool,
) -> MobileSessionResult<Option<(String, String)>> {
    let row = connection
        .query_row(
            "select id, prompt
             from mobile_remote_prompts
             where thread_id = ?1
               and delivery_mode = ?2
               and status = ?3
             order by created_at desc
             limit 1",
            params![thread_id, delivery_mode, REMOTE_PROMPT_STATUS_QUEUED],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((prompt_id, prompt)) = row else {
        return Ok(None);
    };
    let Some(prompt) = normalized_optional(&prompt) else {
        return Ok(None);
    };
    if mark_delivered {
        connection.execute(
            "update mobile_remote_prompts
             set status = ?1, delivered_at = ?2
             where id = ?3",
            params![REMOTE_PROMPT_STATUS_DELIVERED, now_iso_string()?, prompt_id],
        )?;
    }
    Ok(Some((prompt_id, prompt)))
}

pub(super) fn read_notifications(
    connection: &Connection,
) -> MobileSessionResult<Vec<MobileNotificationRoute>> {
    let mut statement = connection.prepare(
        "select
            id,
            label,
            channel,
            webhook_url,
            chat_id,
            bot_token,
            bot_url,
            chat_username,
            chat_display_name
         from mobile_notification_routes
         order by created_at asc, id asc",
    )?;
    let rows = statement.query_map([], |row| {
        let bot_url = normalized_option(&row.get::<_, Option<String>>(6)?);
        Ok(MobileNotificationRoute {
            id: row.get(0)?,
            label: row.get(1)?,
            channel: row.get(2)?,
            webhook_url: normalized_option(&row.get::<_, Option<String>>(3)?),
            chat_id: normalized_option(&row.get::<_, Option<String>>(4)?),
            bot_token: normalized_option(&row.get::<_, Option<String>>(5)?)
                .or_else(|| telegram_token_from_bot_url(bot_url.as_deref())),
            bot_url,
            chat_username: normalized_option(&row.get::<_, Option<String>>(7)?),
            chat_display_name: normalized_option(&row.get::<_, Option<String>>(8)?),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(MobileSessionError::Store)
}

pub(super) fn read_completion_checks(
    connection: &Connection,
) -> MobileSessionResult<Vec<MobileCompletionCheck>> {
    let mut statement = connection.prepare(
        "select id, label, commands_json
         from mobile_completion_checks
         order by created_at asc, id asc",
    )?;
    let rows = statement.query_map([], |row| {
        let commands_json = row.get::<_, String>(2)?;
        Ok(MobileCompletionCheck {
            id: row.get(0)?,
            label: row.get(1)?,
            commands: parse_commands_json(&commands_json),
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(MobileSessionError::Store)
}

pub(super) fn read_session_notifications(
    connection: &Connection,
    known_notification_ids: &BTreeSet<String>,
) -> MobileSessionResult<BTreeMap<String, Vec<String>>> {
    let mut statement = connection.prepare(
        "select thread_id, notification_id
         from mobile_session_notifications
         order by thread_id asc, notification_id asc",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut notification_ids_by_thread = BTreeMap::new();
    for row in rows {
        let (thread_id, notification_id) = row?;
        if !known_notification_ids.contains(&notification_id) {
            continue;
        }
        notification_ids_by_thread
            .entry(thread_id)
            .or_insert_with(Vec::new)
            .push(notification_id);
    }
    Ok(notification_ids_by_thread)
}

pub(super) fn read_session_lifecycle(
    connection: &Connection,
) -> MobileSessionResult<BTreeMap<String, MobileSessionLifecycle>> {
    let mut statement = connection.prepare(
        "select thread_id, status, updated_at
         from mobile_session_lifecycle
         order by thread_id asc",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            MobileSessionLifecycle {
                status: row.get(1)?,
                updated_at: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(MobileSessionError::Store)
}

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use super::normalization::{
    normalized_option, normalized_optional, normalized_required, now_iso_string,
};
use super::schema::{
    MOBILE_DEFAULT_NOTIFICATION_TARGETS_TABLE, MOBILE_SESSION_NOTIFICATIONS_TABLE,
};
use super::{
    MobileNotificationRoute, MobileSessionError, MobileSessionResult, MobileSessionService,
    MobileSessionState, UpsertMobileNotificationRoute,
};

pub(super) const NOTIFICATION_CHANNEL_SLACK: &str = "slack";
pub(super) const NOTIFICATION_CHANNEL_TELEGRAM: &str = "telegram";
pub const NOTIFICATION_TARGET_IPHONE: &str = "iphone";
pub const NOTIFICATION_TARGET_MACOS: &str = "macos";

const TELEGRAM_API_ORIGIN: &str = "https://api.telegram.org";
const TELEGRAM_SEND_MESSAGE_METHOD: &str = "sendMessage";

impl MobileSessionService {
    pub fn upsert_notification_route(
        &self,
        input: UpsertMobileNotificationRoute,
    ) -> MobileSessionResult<MobileNotificationRoute> {
        let route = validated_notification_route(input)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_notification_routes (
                id,
                label,
                channel,
                webhook_url,
                chat_id,
                bot_token,
                bot_url,
                chat_username,
                chat_display_name,
                created_at,
                updated_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
             on conflict(id) do update set
                label = excluded.label,
                channel = excluded.channel,
                webhook_url = excluded.webhook_url,
                chat_id = excluded.chat_id,
                bot_token = excluded.bot_token,
                bot_url = excluded.bot_url,
                chat_username = excluded.chat_username,
                chat_display_name = excluded.chat_display_name,
                updated_at = excluded.updated_at",
            params![
                &route.id,
                &route.label,
                &route.channel,
                &route.webhook_url,
                &route.chat_id,
                &route.bot_token,
                &route.bot_url,
                &route.chat_username,
                &route.chat_display_name,
                now_iso_string()?,
            ],
        )?;
        Ok(route)
    }

    pub fn delete_notification_route(&self, id: &str) -> MobileSessionResult<()> {
        let id = normalized_required(id).ok_or(MobileSessionError::NotificationNotFound)?;
        self.initialize()?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        let removed = transaction.execute(
            "delete from mobile_notification_routes where id = ?1",
            params![&id],
        )?;
        if removed == 0 {
            return Err(MobileSessionError::NotificationNotFound);
        }
        transaction.execute(
            "delete from mobile_session_notifications where notification_id = ?1",
            params![&id],
        )?;
        transaction.execute(
            &format!(
                "delete from {MOBILE_DEFAULT_NOTIFICATION_TARGETS_TABLE} where target_id = ?1"
            ),
            params![&id],
        )?;
        transaction.execute(
            "update mobile_settings
             set global_notification_id = null,
                 updated_at = ?2
             where global_notification_id = ?1",
            params![&id, now_iso_string()?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_global_notification(
        &self,
        notification_id: Option<&str>,
    ) -> MobileSessionResult<()> {
        let notification_id = self.valid_notification_id(notification_id)?;
        self.initialize()?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "update mobile_settings set global_notification_id = ?1, updated_at = ?2 where id = 1",
            params![&notification_id, now_iso_string()?],
        )?;
        write_default_notification_targets(
            &transaction,
            legacy_global_notification_targets(notification_id.as_deref()),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_default_notification_targets(
        &self,
        target_ids: &[String],
    ) -> MobileSessionResult<()> {
        let target_ids = self.valid_notification_target_ids(target_ids)?;
        self.initialize()?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        write_default_notification_targets(&transaction, target_ids.iter().map(String::as_str))?;
        let global_notification_id = target_ids
            .iter()
            .find(|target_id| !is_builtin_notification_target(target_id))
            .cloned();
        transaction.execute(
            "update mobile_settings set global_notification_id = ?1, updated_at = ?2 where id = 1",
            params![global_notification_id, now_iso_string()?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_session_notifications(
        &self,
        thread_id: &str,
        notification_ids: &[String],
    ) -> MobileSessionResult<()> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        let notification_ids = self.valid_notification_ids(notification_ids)?;
        self.initialize()?;
        self.upsert_session_override(&thread_id, Default::default())?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            &format!("delete from {MOBILE_SESSION_NOTIFICATIONS_TABLE} where thread_id = ?1"),
            [&thread_id],
        )?;
        for notification_id in notification_ids {
            transaction.execute(
                "insert into mobile_session_notifications (thread_id, notification_id)
                 values (?1, ?2)",
                params![&thread_id, notification_id],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn notification_routes_for_thread(
        &self,
        thread_id: &str,
    ) -> MobileSessionResult<Vec<MobileNotificationRoute>> {
        let state = self.state()?;
        let selected_ids = route_ids_for_thread(thread_id, &state);
        Ok(state
            .notifications
            .into_iter()
            .filter(|route| selected_ids.contains(&route.id))
            .collect())
    }

    pub fn notification_target_ids_for_thread(
        &self,
        thread_id: &str,
    ) -> MobileSessionResult<Vec<String>> {
        let state = self.state()?;
        Ok(notification_target_ids_for_thread(thread_id, &state))
    }

    pub fn telegram_bot_tokens(&self) -> MobileSessionResult<Vec<String>> {
        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        let mut statement = connection.prepare(
            "select distinct bot_token
             from mobile_notification_routes
             where channel = ?1
               and bot_token is not null
               and trim(bot_token) != ''
             order by bot_token asc",
        )?;
        let rows = statement.query_map([NOTIFICATION_CHANNEL_TELEGRAM], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MobileSessionError::Store)
    }

    pub fn has_telegram_route(&self, bot_token: &str, chat_id: &str) -> MobileSessionResult<bool> {
        let Some(bot_token) = normalized_optional(bot_token) else {
            return Ok(false);
        };
        let Some(chat_id) = normalized_optional(chat_id) else {
            return Ok(false);
        };
        self.initialize()?;
        Connection::open(&self.store_path)?
            .query_row(
                "select 1
                 from mobile_notification_routes
                 where channel = ?1
                   and bot_token = ?2
                   and chat_id = ?3
                 limit 1",
                params![NOTIFICATION_CHANNEL_TELEGRAM, bot_token, chat_id],
                |_row| Ok(()),
            )
            .optional()
            .map(|row| row.is_some())
            .map_err(MobileSessionError::Store)
    }

    pub fn record_telegram_delivery_receipt(
        &self,
        notification_id: &str,
        thread_id: &str,
        bot_token: &str,
        chat_id: &str,
        telegram_message_id: i64,
    ) -> MobileSessionResult<()> {
        let notification_id =
            normalized_required(notification_id).ok_or(MobileSessionError::NotificationNotFound)?;
        let thread_id =
            normalized_required(thread_id).ok_or(MobileSessionError::SessionNotFound)?;
        let bot_token =
            normalized_required(bot_token).ok_or(MobileSessionError::MissingNotificationConfig)?;
        let chat_id =
            normalized_required(chat_id).ok_or(MobileSessionError::MissingNotificationConfig)?;
        self.initialize()?;
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_telegram_delivery_receipts (
                id,
                notification_id,
                thread_id,
                bot_token,
                chat_id,
                telegram_message_id,
                created_at
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                format!("telegram-receipt-{}", uuid::Uuid::new_v4()),
                notification_id,
                thread_id,
                bot_token,
                chat_id,
                telegram_message_id,
                now_iso_string()?,
            ],
        )?;
        Ok(())
    }

    pub fn telegram_receipt_thread_id(
        &self,
        bot_token: &str,
        chat_id: &str,
        telegram_message_id: i64,
    ) -> MobileSessionResult<Option<String>> {
        self.initialize()?;
        Connection::open(&self.store_path)?
            .query_row(
                "select thread_id
                 from mobile_telegram_delivery_receipts
                 where bot_token = ?1
                   and chat_id = ?2
                   and telegram_message_id = ?3
                 order by created_at desc
                 limit 1",
                params![bot_token, chat_id, telegram_message_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(MobileSessionError::Store)
    }

    pub(super) fn valid_notification_id(
        &self,
        notification_id: Option<&str>,
    ) -> MobileSessionResult<Option<String>> {
        let Some(notification_id) = notification_id.and_then(normalized_optional) else {
            return Ok(None);
        };
        let known_notification_ids = self
            .state()?
            .notifications
            .into_iter()
            .map(|notification| notification.id)
            .collect::<BTreeSet<_>>();
        if known_notification_ids.contains(&notification_id) {
            return Ok(Some(notification_id));
        }
        Err(MobileSessionError::NotificationNotFound)
    }

    fn valid_notification_ids(
        &self,
        notification_ids: &[String],
    ) -> MobileSessionResult<Vec<String>> {
        let known_notification_ids = self
            .state()?
            .notifications
            .into_iter()
            .map(|notification| notification.id)
            .collect::<BTreeSet<_>>();
        let mut valid_ids = Vec::new();
        for notification_id in notification_ids {
            let Some(notification_id) = normalized_optional(notification_id) else {
                continue;
            };
            if !known_notification_ids.contains(&notification_id) {
                return Err(MobileSessionError::NotificationNotFound);
            }
            if !valid_ids.contains(&notification_id) {
                valid_ids.push(notification_id);
            }
        }
        Ok(valid_ids)
    }

    fn valid_notification_target_ids(
        &self,
        target_ids: &[String],
    ) -> MobileSessionResult<Vec<String>> {
        let known_notification_ids = self
            .state()?
            .notifications
            .into_iter()
            .map(|notification| notification.id)
            .collect::<BTreeSet<_>>();
        let mut valid_ids = Vec::new();
        for target_id in target_ids {
            let Some(target_id) = normalized_optional(target_id) else {
                continue;
            };
            if !is_builtin_notification_target(&target_id)
                && !known_notification_ids.contains(&target_id)
            {
                return Err(MobileSessionError::NotificationNotFound);
            }
            if !valid_ids.contains(&target_id) {
                valid_ids.push(target_id);
            }
        }
        if valid_ids.is_empty() {
            valid_ids.push(NOTIFICATION_TARGET_MACOS.to_owned());
        }
        Ok(valid_ids)
    }
}

pub fn build_telegram_bot_url(bot_token: &str) -> String {
    format!("{TELEGRAM_API_ORIGIN}/bot{bot_token}/{TELEGRAM_SEND_MESSAGE_METHOD}")
}

pub(super) fn telegram_token_from_bot_url(bot_url: Option<&str>) -> Option<String> {
    let bot_url = bot_url.and_then(normalized_optional)?;
    bot_url
        .strip_prefix(&format!("{TELEGRAM_API_ORIGIN}/bot"))
        .and_then(|value| value.strip_suffix(&format!("/{TELEGRAM_SEND_MESSAGE_METHOD}")))
        .and_then(normalized_optional)
}

fn route_ids_for_thread(thread_id: &str, state: &MobileSessionState) -> Vec<String> {
    let known_route_ids = state
        .notifications
        .iter()
        .map(|notification| notification.id.clone())
        .collect::<BTreeSet<_>>();
    state
        .sessions
        .get(thread_id)
        .map(|override_state| override_state.notification_ids.clone())
        .filter(|ids| !ids.is_empty())
        .or_else(|| {
            let route_ids = state
                .default_notification_target_ids
                .iter()
                .filter(|target_id| known_route_ids.contains(*target_id))
                .cloned()
                .collect::<Vec<_>>();
            (!route_ids.is_empty()).then_some(route_ids)
        })
        .or_else(|| state.global_notification_id.clone().map(|id| vec![id]))
        .unwrap_or_default()
}

fn notification_target_ids_for_thread(thread_id: &str, state: &MobileSessionState) -> Vec<String> {
    let Some(override_ids) = state
        .sessions
        .get(thread_id)
        .map(|override_state| override_state.notification_ids.clone())
        .filter(|ids| !ids.is_empty())
    else {
        return state.default_notification_target_ids.clone();
    };

    let mut target_ids = state
        .default_notification_target_ids
        .iter()
        .filter(|target_id| is_builtin_notification_target(target_id))
        .cloned()
        .collect::<Vec<_>>();
    target_ids.extend(override_ids);
    dedupe_preserving_order(target_ids)
}

pub(super) fn is_builtin_notification_target(target_id: &str) -> bool {
    matches!(
        target_id,
        NOTIFICATION_TARGET_IPHONE | NOTIFICATION_TARGET_MACOS
    )
}

fn dedupe_preserving_order(target_ids: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    target_ids
        .into_iter()
        .filter(|target_id| seen.insert(target_id.clone()))
        .collect()
}

fn legacy_global_notification_targets(notification_id: Option<&str>) -> impl Iterator<Item = &str> {
    std::iter::once(NOTIFICATION_TARGET_MACOS).chain(notification_id.into_iter())
}

fn write_default_notification_targets<'a>(
    connection: &Connection,
    target_ids: impl IntoIterator<Item = &'a str>,
) -> MobileSessionResult<()> {
    connection.execute(
        &format!("delete from {MOBILE_DEFAULT_NOTIFICATION_TARGETS_TABLE}"),
        [],
    )?;
    for target_id in target_ids {
        connection.execute(
            &format!(
                "insert or ignore into {MOBILE_DEFAULT_NOTIFICATION_TARGETS_TABLE} (target_id)
                 values (?1)"
            ),
            [target_id],
        )?;
    }
    Ok(())
}

fn normalized_notification_channel(channel: &str) -> MobileSessionResult<String> {
    let Some(channel) = normalized_optional(channel) else {
        return Err(MobileSessionError::InvalidNotificationChannel);
    };
    match channel.as_str() {
        NOTIFICATION_CHANNEL_SLACK | NOTIFICATION_CHANNEL_TELEGRAM => Ok(channel),
        _ => Err(MobileSessionError::InvalidNotificationChannel),
    }
}

fn validated_notification_route(
    input: UpsertMobileNotificationRoute,
) -> MobileSessionResult<MobileNotificationRoute> {
    let channel = normalized_notification_channel(&input.channel)?;
    let id = input
        .id
        .as_deref()
        .and_then(normalized_optional)
        .unwrap_or_else(|| format!("notification-{}", uuid::Uuid::new_v4()));
    let label = normalized_option(&input.label)
        .unwrap_or_else(|| default_notification_label(&channel, &input));

    if channel == NOTIFICATION_CHANNEL_SLACK {
        let webhook_url = normalized_option(&input.webhook_url)
            .ok_or(MobileSessionError::MissingNotificationConfig)?;
        return Ok(MobileNotificationRoute {
            id,
            label,
            channel,
            webhook_url: Some(webhook_url),
            chat_id: None,
            bot_token: None,
            bot_url: None,
            chat_username: None,
            chat_display_name: None,
        });
    }

    let bot_token =
        normalized_option(&input.bot_token).ok_or(MobileSessionError::MissingNotificationConfig)?;
    let chat_id =
        normalized_option(&input.chat_id).ok_or(MobileSessionError::MissingNotificationConfig)?;
    Ok(MobileNotificationRoute {
        id,
        label,
        channel,
        webhook_url: None,
        chat_id: Some(chat_id),
        bot_url: Some(build_telegram_bot_url(&bot_token)),
        bot_token: Some(bot_token),
        chat_username: normalized_option(&input.chat_username),
        chat_display_name: normalized_option(&input.chat_display_name),
    })
}

fn default_notification_label(channel: &str, input: &UpsertMobileNotificationRoute) -> String {
    if channel == NOTIFICATION_CHANNEL_SLACK {
        return "Slack".to_owned();
    }
    if let Some(username) = normalized_option(&input.chat_username) {
        return format!("@{username}");
    }
    normalized_option(&input.chat_display_name).unwrap_or_else(|| "Telegram".to_owned())
}

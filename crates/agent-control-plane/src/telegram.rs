use std::path::PathBuf;

use reqwest::Client;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const TELEGRAM_API_ORIGIN: &str = "https://api.telegram.org";
const TELEGRAM_GET_UPDATES_METHOD: &str = "getUpdates";
const TELEGRAM_ALLOWED_UPDATES: &[&str] =
    &["message", "channel_post", "my_chat_member", "chat_member"];
const TELEGRAM_CHAT_KIND_CHANNEL: &str = "channel";
const TELEGRAM_CHAT_KIND_DM: &str = "dm";
const TELEGRAM_CHAT_KIND_GROUP: &str = "group";
const TELEGRAM_PRIVATE_CHAT_TYPE: &str = "private";
const TELEGRAM_CHANNEL_CHAT_TYPE: &str = "channel";
const TELEGRAM_UPDATES_TIMEOUT_SECONDS: u64 = 1;

#[derive(Clone, Debug)]
pub struct TelegramService {
    store_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramChatOption {
    pub chat_id: String,
    pub kind: String,
    pub username: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("telegram store failed: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("telegram filesystem failed: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("telegram timestamp failed: {0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("telegram request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("telegram bot token is required")]
    BotTokenRequired,
    #[error("telegram rejected request: {0}")]
    Api(String),
}

type TelegramResult<T> = Result<T, TelegramError>;

impl TelegramService {
    pub fn new(store_path: PathBuf) -> Self {
        Self { store_path }
    }

    pub async fn chats(
        &self,
        bot_token: &str,
        wait_for_updates: bool,
    ) -> TelegramResult<Vec<TelegramChatOption>> {
        let bot_token = normalized_required(bot_token).ok_or(TelegramError::BotTokenRequired)?;
        self.initialize()?;
        if wait_for_updates {
            let updates = fetch_telegram_updates(&bot_token, None).await?;
            let chats = collect_telegram_chats_from_updates(&updates);
            self.upsert_known_chats(&bot_token, &chats)?;
        }
        self.known_chats(&bot_token)
    }

    fn initialize(&self) -> TelegramResult<()> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Connection::open(&self.store_path)?.execute_batch(
            r#"
create table if not exists mobile_telegram_known_chats (
  bot_token text not null,
  chat_id text not null,
  kind text not null,
  username text,
  display_name text not null,
  updated_at text not null,
  primary key (bot_token, chat_id)
);

create table if not exists mobile_telegram_update_cursors (
  bot_token text primary key,
  last_update_id integer not null,
  updated_at text not null
);
"#,
        )?;
        Ok(())
    }

    fn known_chats(&self, bot_token: &str) -> TelegramResult<Vec<TelegramChatOption>> {
        let connection = Connection::open(&self.store_path)?;
        let mut statement = connection.prepare(
            "select chat_id, kind, username, display_name
             from mobile_telegram_known_chats
             where bot_token = ?1
             order by display_name asc, chat_id asc",
        )?;
        let rows = statement.query_map([bot_token], |row| {
            Ok(TelegramChatOption {
                chat_id: row.get(0)?,
                kind: normalized_chat_kind(row.get::<_, String>(1)?.as_str()).to_owned(),
                username: normalized_option(&row.get::<_, Option<String>>(2)?),
                display_name: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(TelegramError::Store)
    }

    fn upsert_known_chats(
        &self,
        bot_token: &str,
        chats: &[TelegramChatOption],
    ) -> TelegramResult<()> {
        if chats.is_empty() {
            return Ok(());
        }
        let timestamp = now_iso_string()?;
        let mut connection = Connection::open(&self.store_path)?;
        let transaction = connection.transaction()?;
        for chat in chats {
            transaction.execute(
                "insert into mobile_telegram_known_chats (
                    bot_token,
                    chat_id,
                    kind,
                    username,
                    display_name,
                    updated_at
                 ) values (?1, ?2, ?3, ?4, ?5, ?6)
                 on conflict(bot_token, chat_id) do update set
                    kind = excluded.kind,
                    username = excluded.username,
                    display_name = excluded.display_name,
                    updated_at = excluded.updated_at",
                params![
                    bot_token,
                    &chat.chat_id,
                    &chat.kind,
                    &chat.username,
                    &chat.display_name,
                    &timestamp,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub async fn send_message(
        &self,
        bot_token: &str,
        chat_id: &str,
        text: &str,
    ) -> TelegramResult<Option<i64>> {
        let bot_token = normalized_required(bot_token).ok_or(TelegramError::BotTokenRequired)?;
        let chat_id = normalized_required(chat_id).ok_or(TelegramError::BotTokenRequired)?;
        let text = normalized_required(text).ok_or(TelegramError::BotTokenRequired)?;
        let payload = Client::new()
            .post(format!("{TELEGRAM_API_ORIGIN}/bot{bot_token}/sendMessage"))
            .form(&[("chat_id", chat_id), ("text", text)])
            .send()
            .await?
            .json::<TelegramSendMessagePayload>()
            .await?;
        if payload.ok.unwrap_or(false) {
            return Ok(payload.result.and_then(|result| result.message_id));
        }
        Err(TelegramError::Api(
            payload
                .description
                .unwrap_or_else(|| "unknown Telegram API error".to_owned()),
        ))
    }

    pub async fn poll_updates(&self, bot_token: &str) -> TelegramResult<Vec<TelegramUpdate>> {
        let bot_token = normalized_required(bot_token).ok_or(TelegramError::BotTokenRequired)?;
        self.initialize()?;
        let offset = self.update_cursor(&bot_token)?.map(|cursor| cursor + 1);
        let updates = fetch_telegram_updates(&bot_token, offset).await?;
        let chats = collect_telegram_chats_from_updates(&updates);
        self.upsert_known_chats(&bot_token, &chats)?;
        if let Some(last_update_id) = updates.iter().filter_map(|update| update.update_id).max() {
            self.set_update_cursor(&bot_token, last_update_id)?;
        }
        Ok(updates)
    }

    fn update_cursor(&self, bot_token: &str) -> TelegramResult<Option<i64>> {
        Connection::open(&self.store_path)?
            .query_row(
                "select last_update_id
                 from mobile_telegram_update_cursors
                 where bot_token = ?1",
                [bot_token],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(TelegramError::Store)
    }

    fn set_update_cursor(&self, bot_token: &str, last_update_id: i64) -> TelegramResult<()> {
        Connection::open(&self.store_path)?.execute(
            "insert into mobile_telegram_update_cursors (bot_token, last_update_id, updated_at)
             values (?1, ?2, ?3)
             on conflict(bot_token) do update set
                last_update_id = excluded.last_update_id,
                updated_at = excluded.updated_at",
            params![bot_token, last_update_id, now_iso_string()?],
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
struct TelegramUpdatePayload {
    ok: Option<bool>,
    result: Option<Vec<TelegramUpdate>>,
    description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: Option<i64>,
    pub message: Option<TelegramInboundMessage>,
    pub channel_post: Option<TelegramInboundMessage>,
    pub my_chat_member: Option<TelegramChatMemberUpdate>,
    pub chat_member: Option<TelegramChatMemberUpdate>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramInboundMessage {
    pub message_id: Option<i64>,
    pub text: Option<String>,
    pub chat: Option<TelegramChat>,
    pub from: Option<TelegramUser>,
    pub reply_to_message: Option<TelegramReplyMessage>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramReplyMessage {
    pub message_id: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramUser {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramChatMemberUpdate {
    pub chat: Option<TelegramChat>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramChat {
    pub id: serde_json::Value,
    #[serde(rename = "type")]
    pub chat_type: Option<String>,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TelegramSendMessagePayload {
    ok: Option<bool>,
    result: Option<TelegramSendMessageResult>,
    description: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TelegramSendMessageResult {
    message_id: Option<i64>,
}

async fn fetch_telegram_updates(
    bot_token: &str,
    offset: Option<i64>,
) -> TelegramResult<Vec<TelegramUpdate>> {
    let mut query = vec![
        ("timeout", TELEGRAM_UPDATES_TIMEOUT_SECONDS.to_string()),
        (
            "allowed_updates",
            json!(TELEGRAM_ALLOWED_UPDATES).to_string(),
        ),
    ];
    if let Some(offset) = offset {
        query.push(("offset", offset.to_string()));
    }
    let payload = Client::new()
        .get(format!(
            "{TELEGRAM_API_ORIGIN}/bot{bot_token}/{TELEGRAM_GET_UPDATES_METHOD}"
        ))
        .query(&query)
        .send()
        .await?
        .json::<TelegramUpdatePayload>()
        .await?;
    if payload.ok.unwrap_or(false) {
        return Ok(payload.result.unwrap_or_default());
    }
    Err(TelegramError::Api(
        payload
            .description
            .unwrap_or_else(|| "unknown Telegram API error".to_owned()),
    ))
}

pub fn collect_telegram_chats_from_updates(updates: &[TelegramUpdate]) -> Vec<TelegramChatOption> {
    let mut chats = std::collections::BTreeMap::<String, TelegramChatOption>::new();
    for update in updates.iter().rev() {
        let Some(extracted) = update_chat(update) else {
            continue;
        };
        let Some(chat_id) = chat_id_string(&extracted.chat.id) else {
            continue;
        };
        chats.entry(chat_id.clone()).or_insert(TelegramChatOption {
            chat_id,
            kind: normalized_chat_kind(
                extracted
                    .chat
                    .chat_type
                    .as_deref()
                    .unwrap_or(TELEGRAM_CHAT_KIND_GROUP),
            )
            .to_owned(),
            username: normalized_option(&extracted.chat.username),
            display_name: telegram_chat_display_name(
                extracted.chat,
                extracted.from_first_name.map(String::as_str),
                extracted.from_last_name.map(String::as_str),
            ),
        });
    }
    chats.into_values().collect()
}

struct ExtractedTelegramChat<'a> {
    chat: &'a TelegramChat,
    from_first_name: Option<&'a String>,
    from_last_name: Option<&'a String>,
}

fn update_chat(update: &TelegramUpdate) -> Option<ExtractedTelegramChat<'_>> {
    if let Some(message) = &update.message
        && let Some(chat) = &message.chat
    {
        return Some(ExtractedTelegramChat {
            chat,
            from_first_name: message
                .from
                .as_ref()
                .and_then(|from| from.first_name.as_ref()),
            from_last_name: message
                .from
                .as_ref()
                .and_then(|from| from.last_name.as_ref()),
        });
    }
    if let Some(message) = &update.channel_post
        && let Some(chat) = &message.chat
    {
        return Some(ExtractedTelegramChat {
            chat,
            from_first_name: message
                .from
                .as_ref()
                .and_then(|from| from.first_name.as_ref()),
            from_last_name: message
                .from
                .as_ref()
                .and_then(|from| from.last_name.as_ref()),
        });
    }
    update
        .my_chat_member
        .as_ref()
        .or(update.chat_member.as_ref())
        .and_then(|member_update| member_update.chat.as_ref())
        .map(|chat| ExtractedTelegramChat {
            chat,
            from_first_name: None,
            from_last_name: None,
        })
}

fn chat_id_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(value) => normalized_optional(value),
        _ => None,
    }
}

fn normalized_chat_kind(chat_type: &str) -> &'static str {
    match chat_type {
        TELEGRAM_CHANNEL_CHAT_TYPE => TELEGRAM_CHAT_KIND_CHANNEL,
        TELEGRAM_PRIVATE_CHAT_TYPE => TELEGRAM_CHAT_KIND_DM,
        _ => TELEGRAM_CHAT_KIND_GROUP,
    }
}

fn telegram_chat_display_name(
    chat: &TelegramChat,
    from_first_name: Option<&str>,
    from_last_name: Option<&str>,
) -> String {
    let name_parts = [
        normalized_option(&chat.first_name)
            .or_else(|| from_first_name.and_then(normalized_optional)),
        normalized_option(&chat.last_name).or_else(|| from_last_name.and_then(normalized_optional)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if !name_parts.is_empty() {
        return name_parts.join(" ");
    }
    normalized_option(&chat.title)
        .or_else(|| normalized_option(&chat.username).map(|username| format!("@{username}")))
        .unwrap_or_else(|| "Unknown chat".to_owned())
}

fn normalized_required(value: &str) -> Option<String> {
    normalized_optional(value)
}

fn normalized_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn normalized_option(value: &Option<String>) -> Option<String> {
    value.as_deref().and_then(normalized_optional)
}

fn now_iso_string() -> TelegramResult<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_chat_collection_prefers_latest_unique_chats() {
        let updates = serde_json::from_value::<Vec<TelegramUpdate>>(json!([
            {
                "message": {
                    "chat": {
                        "id": 7,
                        "type": "private",
                        "username": "old",
                        "first_name": "Old"
                    }
                }
            },
            {
                "message": {
                    "chat": {
                        "id": 7,
                        "type": "private",
                        "username": "ada",
                        "first_name": "Ada",
                        "last_name": "Lovelace"
                    }
                }
            },
            {
                "channel_post": {
                    "chat": {
                        "id": "-1001",
                        "type": "channel",
                        "title": "Release Notes"
                    }
                }
            }
        ]))
        .expect("updates");

        let chats = collect_telegram_chats_from_updates(&updates);
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].chat_id, "-1001");
        assert_eq!(chats[0].kind, TELEGRAM_CHAT_KIND_CHANNEL);
        assert_eq!(chats[0].display_name, "Release Notes");
        assert_eq!(chats[1].chat_id, "7");
        assert_eq!(chats[1].kind, TELEGRAM_CHAT_KIND_DM);
        assert_eq!(chats[1].display_name, "Ada Lovelace");
    }
}

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use p256::pkcs8::DecodePrivateKey;
use reqwest::Client;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const APNS_ALERT_PRIORITY: &str = "10";
const APNS_CONFIG_FILE_NAME: &str = "apns.json";
const APNS_CONFIG_PATH_ENV: &str = "LOOPER_APNS_CONFIG_PATH";
const APNS_DEVELOPMENT_ORIGIN: &str = "https://api.sandbox.push.apple.com";
const APNS_PRODUCTION_ORIGIN: &str = "https://api.push.apple.com";
const APNS_PUSH_TYPE_ALERT: &str = "alert";
const APNS_SOUND_DEFAULT: &str = "default";
const APNS_SUCCESS_STATUS: u16 = 200;
const APNS_TEST_BODY: &str = "Remote notifications are working.";
const APNS_TEST_SUBTITLE: &str = "TestFlight push ready";
const APNS_TEST_TITLE: &str = "Looper";
const APNS_CATEGORY_SESSION_STOP: &str = "looper-session-stop";
const APNS_SESSION_STOP_KIND: &str = "session-stop";
const APNS_SESSION_STOP_SUBTITLE: &str = "Session stopped";
const APNS_SESSION_STOP_TITLE: &str = "Looper";
const APNS_TOPIC_HEADER: &str = "apns-topic";
const APNS_PRIORITY_HEADER: &str = "apns-priority";
const APNS_PUSH_TYPE_HEADER: &str = "apns-push-type";
const AUTHORIZATION_HEADER: &str = "authorization";
const ENABLED_FLAG: i64 = 1;
const DISABLED_FLAG: i64 = 0;
const MOBILE_PUSH_DISABLED_STATE: &str = "disabled";
const MOBILE_PUSH_ENABLED_STATE: &str = "enabled";
const STORED_AWAITING_PROVIDER_STATE: &str = "stored-awaiting-provider";
const PROVIDER_READY_MESSAGE: &str = "Remote push is ready on this Mac.";
const PUSH_STORED_MESSAGE: &str =
    "This iPhone is registered. Add APNs provider credentials on the Mac to deliver remote pushes.";
const PUSH_NOT_REGISTERED_MESSAGE: &str =
    "This iPhone has not completed remote push registration on the Mac yet.";
const PUSH_PROVIDER_MISSING_MESSAGE: &str =
    "The iPhone is registered, but APNs provider credentials are still missing on the Mac.";
const PUSH_TEST_SENT_MESSAGE: &str = "Test push sent.";
const APNS_DISABLE_REASONS: &[&str] = &["BadDeviceToken", "DeviceTokenNotForTopic", "Unregistered"];

#[derive(Clone, Debug)]
pub struct MobilePushService {
    store_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePushRegistrationRequest {
    pub installation_id: String,
    pub device_token: String,
    pub bundle_id: String,
    pub environment: MobilePushEnvironment,
    pub device_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MobilePushEnvironment {
    Development,
    Production,
}

impl MobilePushEnvironment {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }

    fn apns_origin(&self) -> &'static str {
        match self {
            Self::Development => APNS_DEVELOPMENT_ORIGIN,
            Self::Production => APNS_PRODUCTION_ORIGIN,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePushRegistrationResponse {
    pub state: String,
    pub environment: MobilePushEnvironment,
    pub registered_at: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePushTestResponse {
    pub delivered: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePushDeviceSummary {
    pub installation_id: String,
    pub device_name: Option<String>,
    pub bundle_id: String,
    pub environment: MobilePushEnvironment,
    pub state: String,
    pub registered_at: String,
    pub last_delivered_at: Option<String>,
    pub last_delivery_error: Option<String>,
    pub can_test: bool,
}

#[derive(Debug, Error)]
pub enum MobilePushError {
    #[error("mobile push store failed: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("mobile push filesystem failed: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("mobile push timestamp failed: {0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("mobile push provider request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("mobile push provider configuration failed: {0}")]
    Config(String),
    #[error("mobile push JWT failed: {0}")]
    Jwt(String),
    #[error("push registration request is missing required values")]
    MissingRequiredValues,
}

type MobilePushResult<T> = Result<T, MobilePushError>;

impl MobilePushService {
    pub fn new(store_path: PathBuf) -> Self {
        Self { store_path }
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn initialize(&self) -> MobilePushResult<()> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.store_path)?;
        connection.execute_batch(
            r#"
create table if not exists mobile_push_devices (
  installation_id text primary key,
  device_token text not null,
  bundle_id text not null,
  environment text not null check (environment in ('development', 'production')),
  device_name text,
  push_enabled integer not null default 1 check (push_enabled in (0, 1)),
  created_at text not null,
  registered_at text not null,
  updated_at text not null,
  last_delivered_at text,
  last_delivery_error text
);

create index if not exists mobile_push_devices_token_idx
  on mobile_push_devices(device_token, bundle_id, environment);
"#,
        )?;
        ensure_column(
            &connection,
            "mobile_push_devices",
            "push_enabled",
            "integer not null default 1 check (push_enabled in (0, 1))",
        )?;
        ensure_column(
            &connection,
            "mobile_push_devices",
            "created_at",
            "text not null default ''",
        )?;
        ensure_column(
            &connection,
            "mobile_push_devices",
            "last_delivered_at",
            "text",
        )?;
        ensure_column(
            &connection,
            "mobile_push_devices",
            "last_delivery_error",
            "text",
        )?;
        Ok(())
    }

    pub fn register_device(
        &self,
        request: MobilePushRegistrationRequest,
    ) -> MobilePushResult<MobilePushRegistrationResponse> {
        let installation_id = normalized_required(&request.installation_id)
            .ok_or(MobilePushError::MissingRequiredValues)?;
        let device_token = normalized_required(&request.device_token)
            .ok_or(MobilePushError::MissingRequiredValues)?;
        let bundle_id = normalized_required(&request.bundle_id)
            .ok_or(MobilePushError::MissingRequiredValues)?;
        let device_name = request.device_name.as_deref().and_then(normalized_optional);
        let registered_at = now_iso_string()?;
        let provider_config = self.load_apns_provider_config()?;
        let provider_ready = provider_is_ready_for_device(
            provider_config.as_ref(),
            &bundle_id,
            &request.environment,
        );

        self.initialize()?;
        let connection = Connection::open(&self.store_path)?;
        connection.execute(
            "delete from mobile_push_devices
             where installation_id = ?1
                or (device_token = ?2 and bundle_id = ?3 and environment = ?4)",
            params![
                &installation_id,
                &device_token,
                &bundle_id,
                request.environment.as_str(),
            ],
        )?;
        connection.execute(
            "insert into mobile_push_devices (
                installation_id,
                device_token,
                bundle_id,
                environment,
                device_name,
                push_enabled,
                created_at,
                registered_at,
                updated_at,
                last_delivered_at,
                last_delivery_error
             ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?7, null, null)",
            params![
                installation_id,
                device_token,
                bundle_id,
                request.environment.as_str(),
                device_name,
                ENABLED_FLAG,
                registered_at,
            ],
        )?;

        Ok(MobilePushRegistrationResponse {
            state: if provider_ready {
                MOBILE_PUSH_ENABLED_STATE
            } else {
                STORED_AWAITING_PROVIDER_STATE
            }
            .to_owned(),
            environment: request.environment,
            registered_at,
            message: if provider_ready {
                PROVIDER_READY_MESSAGE
            } else {
                PUSH_STORED_MESSAGE
            }
            .to_owned(),
        })
    }

    pub async fn send_test_push(
        &self,
        installation_id: &str,
    ) -> MobilePushResult<MobilePushTestResponse> {
        let installation_id =
            normalized_required(installation_id).ok_or(MobilePushError::MissingRequiredValues)?;
        self.initialize()?;
        let Some(device) = self.registered_device(&installation_id)? else {
            return Ok(MobilePushTestResponse {
                delivered: false,
                message: PUSH_NOT_REGISTERED_MESSAGE.to_owned(),
            });
        };
        if !device.push_enabled {
            return Ok(MobilePushTestResponse {
                delivered: false,
                message: PUSH_NOT_REGISTERED_MESSAGE.to_owned(),
            });
        }

        let Some(provider_config) = self.load_apns_provider_config()? else {
            return Ok(MobilePushTestResponse {
                delivered: false,
                message: PUSH_PROVIDER_MISSING_MESSAGE.to_owned(),
            });
        };
        if !provider_is_ready_for_device(
            Some(&provider_config),
            &device.bundle_id,
            &device.environment,
        ) {
            return Ok(MobilePushTestResponse {
                delivered: false,
                message: provider_mismatch_message(&device),
            });
        }

        let response = send_apns_alert(
            &provider_config,
            &device.device_token,
            &test_push_message(&device),
        )
        .await?;
        self.update_delivery_result(&device.installation_id, &response)?;

        Ok(MobilePushTestResponse {
            delivered: response.ok,
            message: if response.ok {
                PUSH_TEST_SENT_MESSAGE.to_owned()
            } else {
                format!(
                    "APNs rejected the test push{}",
                    response
                        .reason
                        .as_deref()
                        .map(|reason| format!(": {reason}"))
                        .unwrap_or_else(|| ".".to_owned())
                )
            },
        })
    }

    pub async fn send_session_stop_pushes(
        &self,
        thread_id: &str,
        message: &str,
    ) -> MobilePushResult<usize> {
        let thread_id =
            normalized_required(thread_id).ok_or(MobilePushError::MissingRequiredValues)?;
        let message = normalized_required(message).ok_or(MobilePushError::MissingRequiredValues)?;
        self.initialize()?;
        let Some(provider_config) = self.load_apns_provider_config()? else {
            return Ok(0);
        };

        let mut delivered_count = 0;
        for device in self.enabled_devices()? {
            if !provider_is_ready_for_device(
                Some(&provider_config),
                &device.bundle_id,
                &device.environment,
            ) {
                continue;
            }
            let response = send_apns_alert(
                &provider_config,
                &device.device_token,
                &session_stop_push_message(&thread_id, &message),
            )
            .await?;
            self.update_delivery_result(&device.installation_id, &response)?;
            if response.ok {
                delivered_count += 1;
            }
        }
        Ok(delivered_count)
    }

    pub fn registered_devices(&self) -> MobilePushResult<Vec<MobilePushDeviceSummary>> {
        self.initialize()?;
        let provider_config = self.load_apns_provider_config()?;
        let connection = Connection::open(&self.store_path)?;
        let mut statement = connection.prepare(
            "select
                installation_id,
                bundle_id,
                environment,
                device_name,
                push_enabled,
                registered_at,
                last_delivered_at,
                last_delivery_error
             from mobile_push_devices
             order by registered_at desc, installation_id asc",
        )?;
        let rows = statement.query_map([], |row| {
            let environment = environment_from_str(&row.get::<_, String>(2)?)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            Ok(StoredPushDeviceSummary {
                installation_id: row.get(0)?,
                bundle_id: row.get(1)?,
                environment,
                device_name: row.get(3)?,
                push_enabled: row.get::<_, i64>(4)? == ENABLED_FLAG,
                registered_at: row.get(5)?,
                last_delivered_at: row.get(6)?,
                last_delivery_error: row.get(7)?,
            })
        })?;
        rows.map(|row| row.map(|device| device.summary(provider_config.as_ref())))
            .collect::<Result<Vec<_>, _>>()
            .map_err(MobilePushError::Store)
    }

    fn load_apns_provider_config(&self) -> MobilePushResult<Option<ApnsProviderConfig>> {
        let config_path = self.apns_config_path();
        let file_config = match std::fs::read_to_string(&config_path) {
            Ok(content) => serde_json::from_str::<ApnsProviderConfigFile>(&content)
                .map_err(|error| MobilePushError::Config(error.to_string()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ApnsProviderConfigFile::default()
            }
            Err(error) => return Err(MobilePushError::Filesystem(error)),
        };

        apns_provider_config_from_sources(&file_config)
    }

    fn apns_config_path(&self) -> PathBuf {
        std::env::var(APNS_CONFIG_PATH_ENV)
            .map(PathBuf::from)
            .ok()
            .or_else(|| {
                self.store_path
                    .parent()
                    .map(|parent| parent.join(APNS_CONFIG_FILE_NAME))
            })
            .unwrap_or_else(|| PathBuf::from(APNS_CONFIG_FILE_NAME))
    }

    fn registered_device(
        &self,
        installation_id: &str,
    ) -> MobilePushResult<Option<StoredPushDevice>> {
        let connection = Connection::open(&self.store_path)?;
        connection
            .query_row(
                "select installation_id, device_token, bundle_id, environment, push_enabled
                 from mobile_push_devices
                 where installation_id = ?1",
                [installation_id],
                |row| {
                    let environment = environment_from_str(&row.get::<_, String>(3)?)
                        .ok_or(rusqlite::Error::InvalidQuery)?;
                    Ok(StoredPushDevice {
                        installation_id: row.get(0)?,
                        device_token: row.get(1)?,
                        bundle_id: row.get(2)?,
                        environment,
                        push_enabled: row.get::<_, i64>(4)? == ENABLED_FLAG,
                    })
                },
            )
            .optional()
            .map_err(MobilePushError::Store)
    }

    fn enabled_devices(&self) -> MobilePushResult<Vec<StoredPushDevice>> {
        let connection = Connection::open(&self.store_path)?;
        let mut statement = connection.prepare(
            "select installation_id, device_token, bundle_id, environment, push_enabled
             from mobile_push_devices
             where push_enabled = ?1
             order by registered_at desc, installation_id asc",
        )?;
        let rows = statement.query_map([ENABLED_FLAG], |row| {
            let environment = environment_from_str(&row.get::<_, String>(3)?)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            Ok(StoredPushDevice {
                installation_id: row.get(0)?,
                device_token: row.get(1)?,
                bundle_id: row.get(2)?,
                environment,
                push_enabled: row.get::<_, i64>(4)? == ENABLED_FLAG,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MobilePushError::Store)
    }

    fn update_delivery_result(
        &self,
        installation_id: &str,
        response: &ApnsDeliveryResponse,
    ) -> MobilePushResult<()> {
        let timestamp = now_iso_string()?;
        let should_disable_device = response
            .reason
            .as_deref()
            .map(should_disable_apns_device)
            .unwrap_or(false);
        Connection::open(&self.store_path)?.execute(
            "update mobile_push_devices
             set push_enabled = ?1,
                 updated_at = ?2,
                 last_delivered_at = ?3,
                 last_delivery_error = ?4
             where installation_id = ?5",
            params![
                if should_disable_device {
                    DISABLED_FLAG
                } else {
                    ENABLED_FLAG
                },
                &timestamp,
                response.ok.then_some(timestamp.clone()),
                if response.ok {
                    None
                } else {
                    Some(
                        response
                            .reason
                            .clone()
                            .unwrap_or_else(|| "Unknown APNs error".to_owned()),
                    )
                },
                installation_id,
            ],
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct ApnsProviderConfigFile {
    key_id: Option<String>,
    team_id: Option<String>,
    bundle_id: Option<String>,
    environment: Option<String>,
    private_key_path: Option<String>,
    private_key_base64: Option<String>,
    private_key_pem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ApnsProviderConfig {
    bundle_id: String,
    environment: MobilePushEnvironment,
    key_id: String,
    team_id: String,
    private_key_pem: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredPushDevice {
    installation_id: String,
    device_token: String,
    bundle_id: String,
    environment: MobilePushEnvironment,
    push_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredPushDeviceSummary {
    installation_id: String,
    bundle_id: String,
    environment: MobilePushEnvironment,
    device_name: Option<String>,
    push_enabled: bool,
    registered_at: String,
    last_delivered_at: Option<String>,
    last_delivery_error: Option<String>,
}

impl StoredPushDeviceSummary {
    fn summary(self, provider_config: Option<&ApnsProviderConfig>) -> MobilePushDeviceSummary {
        let provider_ready =
            provider_is_ready_for_device(provider_config, &self.bundle_id, &self.environment);
        MobilePushDeviceSummary {
            installation_id: self.installation_id,
            device_name: self.device_name,
            bundle_id: self.bundle_id,
            environment: self.environment,
            state: push_device_state(self.push_enabled, provider_ready),
            registered_at: self.registered_at,
            last_delivered_at: self.last_delivered_at,
            last_delivery_error: self.last_delivery_error,
            can_test: self.push_enabled && provider_ready,
        }
    }
}

#[derive(Clone, Debug)]
struct ApnsAlertMessage {
    body: String,
    category: Option<String>,
    subtitle: String,
    title: String,
    thread_id: String,
    user_info: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ApnsDeliveryResponse {
    ok: bool,
    reason: Option<String>,
}

fn apns_provider_config_from_sources(
    file_config: &ApnsProviderConfigFile,
) -> MobilePushResult<Option<ApnsProviderConfig>> {
    let key_id = env_value("LOOPER_APNS_KEY_ID").or_else(|| normalized_option(&file_config.key_id));
    let team_id =
        env_value("LOOPER_APNS_TEAM_ID").or_else(|| normalized_option(&file_config.team_id));
    let bundle_id =
        env_value("LOOPER_APNS_BUNDLE_ID").or_else(|| normalized_option(&file_config.bundle_id));
    let environment = env_value("LOOPER_APNS_ENVIRONMENT")
        .or_else(|| normalized_option(&file_config.environment))
        .and_then(|value| environment_from_str(&value))
        .unwrap_or(MobilePushEnvironment::Production);
    let private_key_pem = load_private_key_pem(file_config)?;

    let Some((key_id, team_id, bundle_id, private_key_pem)) =
        key_id.zip(team_id).zip(bundle_id).zip(private_key_pem).map(
            |(((key_id, team_id), bundle_id), private_key_pem)| {
                (key_id, team_id, bundle_id, private_key_pem)
            },
        )
    else {
        return Ok(None);
    };

    Ok(Some(ApnsProviderConfig {
        bundle_id,
        environment,
        key_id,
        team_id,
        private_key_pem,
    }))
}

fn load_private_key_pem(file_config: &ApnsProviderConfigFile) -> MobilePushResult<Option<String>> {
    if let Some(value) = env_value("LOOPER_APNS_PRIVATE_KEY_PEM")
        .or_else(|| normalized_option(&file_config.private_key_pem))
    {
        return Ok(Some(value));
    }

    if let Some(value) = env_value("LOOPER_APNS_PRIVATE_KEY_BASE64")
        .or_else(|| normalized_option(&file_config.private_key_base64))
    {
        let bytes = STANDARD
            .decode(value)
            .map_err(|error| MobilePushError::Config(error.to_string()))?;
        return String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| MobilePushError::Config(error.to_string()));
    }

    let Some(path) = env_value("LOOPER_APNS_PRIVATE_KEY_PATH")
        .or_else(|| normalized_option(&file_config.private_key_path))
    else {
        return Ok(None);
    };

    Ok(Some(std::fs::read_to_string(path)?.trim().to_owned()))
}

async fn send_apns_alert(
    config: &ApnsProviderConfig,
    device_token: &str,
    message: &ApnsAlertMessage,
) -> MobilePushResult<ApnsDeliveryResponse> {
    let token = create_apns_jwt(config, OffsetDateTime::now_utc().unix_timestamp())?;
    let response = Client::builder()
        .http2_adaptive_window(true)
        .build()?
        .post(format!(
            "{}/3/device/{}",
            config.environment.apns_origin(),
            device_token
        ))
        .header(AUTHORIZATION_HEADER, format!("bearer {token}"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(APNS_PRIORITY_HEADER, APNS_ALERT_PRIORITY)
        .header(APNS_PUSH_TYPE_HEADER, APNS_PUSH_TYPE_ALERT)
        .header(APNS_TOPIC_HEADER, &config.bundle_id)
        .json(&apns_body(message))
        .send()
        .await?;
    let status = response.status().as_u16();
    let reason = apns_rejection_reason(response.text().await?);

    Ok(ApnsDeliveryResponse {
        ok: status == APNS_SUCCESS_STATUS,
        reason: if status == APNS_SUCCESS_STATUS {
            None
        } else {
            reason.or_else(|| Some(format!("APNs status {status}")))
        },
    })
}

fn create_apns_jwt(config: &ApnsProviderConfig, issued_at: i64) -> MobilePushResult<String> {
    let header =
        URL_SAFE_NO_PAD.encode(json!({ "alg": "ES256", "kid": config.key_id }).to_string());
    let claims =
        URL_SAFE_NO_PAD.encode(json!({ "iss": config.team_id, "iat": issued_at }).to_string());
    let unsigned_token = format!("{header}.{claims}");
    let signing_key = SigningKey::from_pkcs8_pem(&config.private_key_pem)
        .map_err(|error| MobilePushError::Jwt(error.to_string()))?;
    let signature: Signature = signing_key.sign(unsigned_token.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    Ok(format!("{unsigned_token}.{signature}"))
}

fn apns_body(message: &ApnsAlertMessage) -> Value {
    let mut aps = json!({
        "alert": {
            "title": message.title,
            "subtitle": message.subtitle,
            "body": message.body,
        },
        "sound": APNS_SOUND_DEFAULT,
        "thread-id": message.thread_id,
    });
    if let Some(category) = message.category.as_deref().and_then(normalized_optional)
        && let Some(aps_object) = aps.as_object_mut()
    {
        aps_object.insert("category".to_owned(), json!(category));
    }
    let mut body = json!({
        "aps": aps
    });
    merge_user_info(&mut body, &message.user_info);
    body
}

fn merge_user_info(body: &mut Value, user_info: &Value) {
    let Some(body_object) = body.as_object_mut() else {
        return;
    };
    let Some(user_info_object) = user_info.as_object() else {
        return;
    };
    for (key, value) in user_info_object {
        body_object.insert(key.clone(), value.clone());
    }
}

fn apns_rejection_reason(response_body: String) -> Option<String> {
    let response_body = response_body.trim();
    if response_body.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(response_body)
        .ok()
        .and_then(|value| {
            value
                .get("reason")
                .and_then(Value::as_str)
                .and_then(normalized_optional)
        })
        .or_else(|| Some(response_body.to_owned()))
}

fn test_push_message(device: &StoredPushDevice) -> ApnsAlertMessage {
    ApnsAlertMessage {
        title: APNS_TEST_TITLE.to_owned(),
        category: None,
        subtitle: APNS_TEST_SUBTITLE.to_owned(),
        body: APNS_TEST_BODY.to_owned(),
        thread_id: device.installation_id.clone(),
        user_info: json!({ "notificationKind": "test" }),
    }
}

fn session_stop_push_message(thread_id: &str, message: &str) -> ApnsAlertMessage {
    ApnsAlertMessage {
        title: APNS_SESSION_STOP_TITLE.to_owned(),
        category: Some(APNS_CATEGORY_SESSION_STOP.to_owned()),
        subtitle: APNS_SESSION_STOP_SUBTITLE.to_owned(),
        body: message.to_owned(),
        thread_id: thread_id.to_owned(),
        user_info: json!({
            "notificationKind": APNS_SESSION_STOP_KIND,
            "sessionId": thread_id,
            "sessionRef": thread_id,
        }),
    }
}

fn provider_is_ready_for_device(
    config: Option<&ApnsProviderConfig>,
    bundle_id: &str,
    environment: &MobilePushEnvironment,
) -> bool {
    config
        .map(|config| config.bundle_id == bundle_id && config.environment == *environment)
        .unwrap_or(false)
}

fn push_device_state(push_enabled: bool, provider_ready: bool) -> String {
    if !push_enabled {
        return MOBILE_PUSH_DISABLED_STATE.to_owned();
    }
    if provider_ready {
        return MOBILE_PUSH_ENABLED_STATE.to_owned();
    }
    STORED_AWAITING_PROVIDER_STATE.to_owned()
}

fn provider_mismatch_message(device: &StoredPushDevice) -> String {
    format!(
        "APNs provider credentials are present, but they do not match this device registration. Expected topic {} in the {} environment.",
        device.bundle_id,
        device.environment.as_str()
    )
}

fn should_disable_apns_device(reason: &str) -> bool {
    APNS_DISABLE_REASONS.contains(&reason)
}

fn environment_from_str(value: &str) -> Option<MobilePushEnvironment> {
    match value {
        "development" => Some(MobilePushEnvironment::Development),
        "production" => Some(MobilePushEnvironment::Production),
        _ => None,
    }
}

fn ensure_column(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    column_definition: &str,
) -> MobilePushResult<()> {
    let escaped_table_name = table_name.replace('\'', "''");
    let mut statement =
        connection.prepare(&format!("pragma table_info('{escaped_table_name}')"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.iter().any(|column| column == column_name) {
        return Ok(());
    }

    connection.execute(
        &format!("alter table {table_name} add column {column_name} {column_definition}"),
        [],
    )?;
    Ok(())
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .and_then(|value| normalized_optional(&value))
}

fn normalized_option(value: &Option<String>) -> Option<String> {
    value.as_deref().and_then(normalized_optional)
}

fn normalized_required(value: &str) -> Option<String> {
    normalized_optional(value)
}

fn normalized_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn now_iso_string() -> MobilePushResult<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mobile_push_registration_is_persisted_as_awaiting_provider() {
        let temp_dir = TempDir::new().expect("temp dir");
        let service = MobilePushService::new(temp_dir.path().join("control-plane.sqlite"));

        let response = service
            .register_device(MobilePushRegistrationRequest {
                installation_id: "install-1".to_owned(),
                device_token: "token-1".to_owned(),
                bundle_id: "dev.looper.app.ios".to_owned(),
                environment: MobilePushEnvironment::Development,
                device_name: Some("Test iPhone".to_owned()),
            })
            .expect("register");

        assert_eq!(response.state, STORED_AWAITING_PROVIDER_STATE);
        assert_eq!(response.environment, MobilePushEnvironment::Development);
        assert!(!response.registered_at.is_empty());

        let devices = service.registered_devices().expect("registered devices");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].installation_id, "install-1");
        assert_eq!(devices[0].state, STORED_AWAITING_PROVIDER_STATE);
        assert!(!devices[0].can_test);
    }

    #[test]
    fn mobile_push_registration_reports_enabled_when_provider_matches() {
        let temp_dir = TempDir::new().expect("temp dir");
        let store_path = temp_dir.path().join("control-plane.sqlite");
        std::fs::write(
            temp_dir.path().join(APNS_CONFIG_FILE_NAME),
            serde_json::to_string(&json!({
                "keyId": "key",
                "teamId": "team",
                "bundleId": "dev.looper.app.ios",
                "environment": "development",
                "privateKeyPem": "test-pem"
            }))
            .expect("json"),
        )
        .expect("write config");
        let service = MobilePushService::new(store_path);

        let response = service
            .register_device(MobilePushRegistrationRequest {
                installation_id: "install-1".to_owned(),
                device_token: "token-1".to_owned(),
                bundle_id: "dev.looper.app.ios".to_owned(),
                environment: MobilePushEnvironment::Development,
                device_name: None,
            })
            .expect("register");

        assert_eq!(response.state, MOBILE_PUSH_ENABLED_STATE);
        assert_eq!(response.message, PROVIDER_READY_MESSAGE);
    }

    #[tokio::test]
    async fn mobile_push_test_reports_missing_provider_without_network() {
        let temp_dir = TempDir::new().expect("temp dir");
        let service = MobilePushService::new(temp_dir.path().join("control-plane.sqlite"));
        service
            .register_device(MobilePushRegistrationRequest {
                installation_id: "install-1".to_owned(),
                device_token: "token-1".to_owned(),
                bundle_id: "dev.looper.app.ios".to_owned(),
                environment: MobilePushEnvironment::Development,
                device_name: None,
            })
            .expect("register");

        let response = service
            .send_test_push("install-1")
            .await
            .expect("test push");

        assert!(!response.delivered);
        assert_eq!(response.message, PUSH_PROVIDER_MISSING_MESSAGE);
    }

    #[tokio::test]
    async fn session_stop_push_reports_zero_without_provider() {
        let temp_dir = TempDir::new().expect("temp dir");
        let service = MobilePushService::new(temp_dir.path().join("control-plane.sqlite"));
        service
            .register_device(MobilePushRegistrationRequest {
                installation_id: "install-1".to_owned(),
                device_token: "token-1".to_owned(),
                bundle_id: "dev.looper.app.ios".to_owned(),
                environment: MobilePushEnvironment::Development,
                device_name: None,
            })
            .expect("register");

        let delivered = service
            .send_session_stop_pushes("thread-1", "ready")
            .await
            .expect("session stop pushes");

        assert_eq!(delivered, 0);
    }

    #[test]
    fn apns_body_preserves_alert_and_user_info() {
        let body = apns_body(&ApnsAlertMessage {
            title: "Title".to_owned(),
            category: Some("category-1".to_owned()),
            subtitle: "Subtitle".to_owned(),
            body: "Body".to_owned(),
            thread_id: "thread-1".to_owned(),
            user_info: json!({
                "notificationKind": "test",
                "sessionId": "thread-1"
            }),
        });

        assert_eq!(body["aps"]["alert"]["title"], "Title");
        assert_eq!(body["aps"]["category"], "category-1");
        assert_eq!(body["aps"]["sound"], APNS_SOUND_DEFAULT);
        assert_eq!(body["notificationKind"], "test");
        assert_eq!(body["sessionId"], "thread-1");
    }

    #[test]
    fn session_stop_push_body_preserves_session_context() {
        let body = apns_body(&session_stop_push_message("thread-1", "Done."));

        assert_eq!(body["aps"]["alert"]["title"], APNS_SESSION_STOP_TITLE);
        assert_eq!(body["aps"]["alert"]["subtitle"], APNS_SESSION_STOP_SUBTITLE);
        assert_eq!(body["aps"]["alert"]["body"], "Done.");
        assert_eq!(body["aps"]["thread-id"], "thread-1");
        assert_eq!(body["aps"]["category"], APNS_CATEGORY_SESSION_STOP);
        assert_eq!(body["notificationKind"], APNS_SESSION_STOP_KIND);
        assert_eq!(body["sessionId"], "thread-1");
        assert_eq!(body["sessionRef"], "thread-1");
    }
}

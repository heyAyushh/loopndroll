use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;

const JSON_CONTENT_TYPE: &str = "application/json";
const EVENT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub(crate) async fn get_json(client: &Client, path: &str) -> Result<Value> {
    Ok(client
        .get(url(path))
        .send()
        .await
        .with_context(|| format!("GET {path}"))?
        .json::<Value>()
        .await?)
}

pub(crate) async fn post_json(client: &Client, path: &str, body: Value) -> Result<()> {
    send_json(client.post(url(path)), path, body).await
}

pub(crate) async fn patch_json(client: &Client, path: &str, body: Value) -> Result<()> {
    send_json(client.patch(url(path)), path, body).await
}

pub(crate) async fn delete_json(client: &Client, path: &str) -> Result<()> {
    let response = client
        .delete(url(path))
        .send()
        .await
        .with_context(|| format!("DELETE {path}"))?;
    ensure_success(path, response).await
}

pub(crate) async fn tail_events(client: Client, sender: mpsc::Sender<String>) {
    loop {
        let response = client
            .get(url("/events/tail"))
            .send()
            .await
            .map_err(|error| error.to_string());
        let Ok(response) = response else {
            tokio::time::sleep(EVENT_RECONNECT_DELAY).await;
            continue;
        };
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else {
                break;
            };
            for line in String::from_utf8_lossy(&chunk).lines() {
                let line = line.trim();
                if let Some(data) = line.strip_prefix("data:") {
                    let _ = sender.send(data.trim().to_owned()).await;
                }
            }
        }
        tokio::time::sleep(EVENT_RECONNECT_DELAY).await;
    }
}

async fn send_json(request: reqwest::RequestBuilder, path: &str, body: Value) -> Result<()> {
    let response = request
        .header(reqwest::header::CONTENT_TYPE, JSON_CONTENT_TYPE)
        .body(body_payload(body))
        .send()
        .await
        .with_context(|| format!("send {path}"))?;
    ensure_success(path, response).await
}

async fn ensure_success(path: &str, response: reqwest::Response) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    bail!("{path} failed with {status}: {}", response.text().await?)
}

fn body_payload(body: Value) -> String {
    if body.is_null() {
        "{}".to_owned()
    } else {
        body.to_string()
    }
}

fn url(path: &str) -> String {
    format!("{}{}", crate::runtime::default_server_base_url(), path)
}

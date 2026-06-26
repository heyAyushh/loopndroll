use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;

const JSON_CONTENT_TYPE: &str = "application/json";

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

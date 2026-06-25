use std::sync::OnceLock;

use anyhow::{Context, Result};
use reqwest::{Client, Method};
use serde_json::Value;

use super::output::{OutputFormat, print_response};

const JSON_CONTENT_TYPE: &str = "application/json";
static BASE_URL_OVERRIDE: OnceLock<String> = OnceLock::new();

pub(crate) async fn print_get(path: &str, format: OutputFormat) -> Result<()> {
    print_request(Method::GET, path, None, format).await
}

pub(crate) async fn fetch_json(path: &str) -> Result<Value> {
    Ok(Client::new()
        .get(url(path))
        .send()
        .await
        .with_context(|| format!("GET {path}"))?
        .json::<Value>()
        .await?)
}

pub(crate) async fn post_json(path: &str, body: Value, format: OutputFormat) -> Result<()> {
    print_request(Method::POST, path, Some(body), format).await
}

pub(crate) async fn patch_json(path: &str, body: Value, format: OutputFormat) -> Result<()> {
    print_request(Method::PATCH, path, Some(body), format).await
}

pub(crate) async fn delete_json(path: &str, format: OutputFormat) -> Result<()> {
    print_request(Method::DELETE, path, None, format).await
}

pub(crate) fn url(path: &str) -> String {
    format!("{}{}", base_url(), path)
}

pub(crate) fn set_base_url(base_url: String) {
    let _ = BASE_URL_OVERRIDE.set(base_url);
}

pub(crate) fn base_url() -> String {
    BASE_URL_OVERRIDE
        .get()
        .cloned()
        .unwrap_or_else(crate::runtime::default_server_base_url)
}

async fn print_request(
    method: Method,
    path: &str,
    body: Option<Value>,
    format: OutputFormat,
) -> Result<()> {
    let method_label = method.as_str().to_owned();
    let mut request = Client::new().request(method, url(path));
    if let Some(body) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, JSON_CONTENT_TYPE)
            .body(body_payload(body));
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("{method_label} {path}"))?;
    print_response(response, format).await
}

fn body_payload(body: Value) -> String {
    if body.is_null() {
        "{}".to_owned()
    } else {
        body.to_string()
    }
}

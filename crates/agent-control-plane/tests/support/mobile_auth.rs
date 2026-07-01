use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

pub async fn issue_mobile_authorization_header(router: &Router) -> String {
    let response = request_json(
        router,
        Method::GET,
        "/api/mobile/connection-code",
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let token_id = response["pairingTokenId"]
        .as_str()
        .expect("pairing token id");
    let token = response["pairingToken"].as_str().expect("pairing token");
    format!("Bearer {token_id}.{token}")
}

async fn request_json(
    router: &Router,
    method: Method,
    path: &str,
    remote_address: Option<SocketAddr>,
) -> serde_json::Value {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    if let Some(remote_address) = remote_address {
        request.extensions_mut().insert(ConnectInfo(remote_address));
    }
    let response = router.clone().oneshot(request).await.expect("response");
    assert!(
        response.status().is_success(),
        "response status: {}",
        response.status()
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json response")
}

use super::*;

#[tokio::test]
async fn mobile_event_stream_refreshes_after_session_state_content_change() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set existing override");
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/api/mobile/events")
        .header(axum::http::header::AUTHORIZATION, authorization.as_str())
        .header(axum::http::header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body();
    let mut buffer = String::new();
    wait_for_sse_buffer(
        &mut body,
        &mut buffer,
        "event: connected",
        "connected event",
    )
    .await;
    let connected_revision = sse_revision_values(&buffer)
        .into_iter()
        .next()
        .expect("connected revision");

    request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/archive",
        serde_json::json!({ "archived": true }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    wait_for_sse_buffer(
        &mut body,
        &mut buffer,
        "\"eventType\":\"session-changed\"",
        "session state content change",
    )
    .await;
    let changed_revision = sse_revision_values(&buffer)
        .into_iter()
        .last()
        .expect("changed revision");

    assert_ne!(changed_revision, connected_revision);
    assert!(buffer.contains("\"threadId\":\"thread-main\""));
}

fn sse_revision_values(buffer: &str) -> Vec<String> {
    buffer
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|payload| {
            payload
                .get("revision")
                .and_then(serde_json::Value::as_str)
                .filter(|revision| !revision.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

#[tokio::test]
async fn grpc_mobile_events_streams_authenticated_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let service = control_plane.mobile_session_service();
    service
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set mode");
    record_thread_active(&control_plane, "thread-main");

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut stream_request = tonic::Request::new(SubscribeEventsRequest {});
    stream_request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );
    let mut event_stream = client
        .subscribe_mobile_events(stream_request)
        .await
        .expect("mobile event stream")
        .into_inner();

    let connected_event = event_stream
        .message()
        .await
        .expect("connected stream message")
        .expect("connected event");
    assert_eq!(connected_event.event_name, "connected");
    assert!(!connected_event.revision.is_empty());

    let mut prompt_request = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Keep going from authenticated gRPC stream.".to_owned(),
        assistant_surface: String::new(),
    });
    prompt_request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );

    let prompt_response = client
        .send_session_prompt(prompt_request)
        .await
        .expect("send session prompt")
        .into_inner();
    assert!(prompt_response.accepted);
    assert_eq!(prompt_response.dispatch_kind, "resumed");

    let resumed_event = event_stream
        .message()
        .await
        .expect("resumed stream message")
        .expect("resumed event");
    assert_eq!(resumed_event.event_name, "session.changed");
    assert_eq!(resumed_event.thread_id, "thread-main");
    assert_eq!(resumed_event.detail, "prompt-resumed");
    assert!(!resumed_event.revision.is_empty());
}

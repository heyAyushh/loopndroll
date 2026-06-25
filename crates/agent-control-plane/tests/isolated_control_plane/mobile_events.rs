use super::*;
use agent_control_plane::events::{
    EventStore, MobileCommandAckInput, MobileCommandAckResult, MobileCommandReservationResult,
    MobileSessionMiniProjectionInput, MobileStateEventGap, MobileStateEventInput,
};
use agent_control_plane::mobile::events::mobile_event_now;

const COMMAND_KIND_SET_SESSION_MODE: &str = "SetSessionMode";
const COMMAND_KIND_SEND_SESSION_PROMPT: &str = "SendSessionPrompt";
const CLIENT_MUTATION_ID: &str = "mutation-send-session-prompt-1";
const REQUEST_HASH: &str = "sha256:send-session-prompt-a";
const CONFLICTING_REQUEST_HASH: &str = "sha256:send-session-prompt-b";
const SESSION_FRAME_TIMEOUT_MILLIS: u64 = 1_500;
const SESSION_FRAME_SCAN_LIMIT: usize = 32;

#[test]
fn mobile_state_event_log_replays_after_seq_and_detects_gap() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = EventStore::new(temp_dir.path().join("events.sqlite"));

    let first = store
        .record_mobile_state_event(state_delta_input("thread-main", "revision-1", "first"))
        .expect("record first delta");
    let second = store
        .record_mobile_state_event(state_delta_input("thread-main", "revision-2", "second"))
        .expect("record second delta");
    let third = store
        .record_mobile_state_event(state_delta_input("thread-main", "revision-3", "third"))
        .expect("record third delta");

    assert!(first.seq > 0);
    assert!(second.seq > first.seq);
    assert!(third.seq > second.seq);
    for (record, expected_revision, expected_delta) in [
        (&first, "revision-1", "first"),
        (&second, "revision-2", "second"),
        (&third, "revision-3", "third"),
    ] {
        assert!(record.seq > 0);
        assert_eq!(record.entity_id, "thread-main");
        assert_eq!(record.kind, MobileEventKind::SessionChanged);
        assert_eq!(record.revision, expected_revision);
        assert!(!record.server_time.is_empty());
        assert!(record.client_mutation_id.is_none());
        assert!(record.command_kind.is_none());
        let payload: serde_json::Value =
            serde_json::from_str(&record.payload_json).expect("payload json");
        assert_eq!(payload["delta"], expected_delta);
    }

    let replay = store
        .mobile_state_events_after_seq(first.seq, 10)
        .expect("replay after seq");
    assert_eq!(
        replay.iter().map(|record| record.seq).collect::<Vec<_>>(),
        vec![second.seq, third.seq]
    );

    let error = store
        .mobile_state_events_after_seq(third.seq + 1, 10)
        .expect_err("gap should be explicit");
    let gap = error
        .downcast_ref::<MobileStateEventGap>()
        .expect("typed mobile state event gap");
    assert_eq!(gap.requested_after_seq, third.seq + 1);
    assert_eq!(gap.latest_seq, third.seq);
}

#[test]
fn mobile_command_log_dedupes_client_mutation_id() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = EventStore::new(temp_dir.path().join("events.sqlite"));

    let first = store
        .record_mobile_command_ack(command_ack_input(REQUEST_HASH, "resumed"))
        .expect("record command ack");
    let first_record = match first {
        MobileCommandAckResult::Recorded(record) => record,
        other => panic!("expected recorded ack, got {other:?}"),
    };
    assert_eq!(first_record.ack_seq, 1);
    assert_eq!(first_record.command_kind, COMMAND_KIND_SEND_SESSION_PROMPT);
    assert_eq!(first_record.client_mutation_id, CLIENT_MUTATION_ID);

    let duplicate = store
        .record_mobile_command_ack(command_ack_input(REQUEST_HASH, "ignored-duplicate"))
        .expect("dedupe command ack");
    let duplicate_record = match duplicate {
        MobileCommandAckResult::Duplicate(record) => record,
        other => panic!("expected duplicate ack, got {other:?}"),
    };
    assert_eq!(duplicate_record.ack_seq, first_record.ack_seq);
    assert_eq!(duplicate_record.response_json, first_record.response_json);

    let events_after_duplicate = store
        .mobile_state_events_after_seq(0, 10)
        .expect("events after duplicate");
    assert_eq!(events_after_duplicate.len(), 1);
    assert_eq!(
        events_after_duplicate[0].client_mutation_id.as_deref(),
        Some(CLIENT_MUTATION_ID)
    );
    assert_eq!(
        events_after_duplicate[0].command_kind.as_deref(),
        Some(COMMAND_KIND_SEND_SESSION_PROMPT)
    );

    let conflict = store
        .record_mobile_command_ack(command_ack_input(CONFLICTING_REQUEST_HASH, "conflict"))
        .expect("conflicting command ack");
    let conflict_record = match conflict {
        MobileCommandAckResult::Conflict(record) => record,
        other => panic!("expected conflict ack, got {other:?}"),
    };
    assert_eq!(conflict_record.ack_seq, first_record.ack_seq);
    assert_eq!(conflict_record.response_json, first_record.response_json);

    let events_after_conflict = store
        .mobile_state_events_after_seq(0, 10)
        .expect("events after conflict");
    assert_eq!(events_after_conflict.len(), 1);
    let stored_ack = store
        .mobile_command_ack(COMMAND_KIND_SEND_SESSION_PROMPT, CLIENT_MUTATION_ID)
        .expect("stored command ack")
        .expect("stored command ack should exist");
    assert_eq!(stored_ack, first_record);
}

#[test]
fn mobile_command_log_reserves_before_ack_and_reports_inflight() {
    let temp_dir = TempDir::new().expect("temp dir");
    let store = EventStore::new(temp_dir.path().join("events.sqlite"));

    let reserved = store
        .reserve_mobile_command_ack(
            COMMAND_KIND_SEND_SESSION_PROMPT,
            CLIENT_MUTATION_ID,
            REQUEST_HASH,
        )
        .expect("reserve command");
    let reserved_record = match reserved {
        MobileCommandReservationResult::Reserved(record) => record,
        other => panic!("expected reserved command, got {other:?}"),
    };
    assert_eq!(reserved_record.ack_seq, 0);
    assert_eq!(reserved_record.request_hash, REQUEST_HASH);

    let in_flight = store
        .reserve_mobile_command_ack(
            COMMAND_KIND_SEND_SESSION_PROMPT,
            CLIENT_MUTATION_ID,
            REQUEST_HASH,
        )
        .expect("detect in-flight command");
    match in_flight {
        MobileCommandReservationResult::InFlight(record) => {
            assert_eq!(record.ack_seq, 0);
            assert_eq!(record.request_hash, REQUEST_HASH);
        }
        other => panic!("expected in-flight command, got {other:?}"),
    }
    assert!(
        store
            .mobile_state_events_after_seq(0, 10)
            .expect("events before ack")
            .is_empty(),
        "reservation alone must not publish a state delta"
    );

    let first = store
        .record_mobile_command_ack(command_ack_input(REQUEST_HASH, "resumed"))
        .expect("finalize command ack");
    let first_record = match first {
        MobileCommandAckResult::Recorded(record) => record,
        other => panic!("expected finalized ack, got {other:?}"),
    };
    assert!(first_record.ack_seq > 0);

    let duplicate = store
        .reserve_mobile_command_ack(
            COMMAND_KIND_SEND_SESSION_PROMPT,
            CLIENT_MUTATION_ID,
            REQUEST_HASH,
        )
        .expect("duplicate after finalized ack");
    match duplicate {
        MobileCommandReservationResult::Duplicate(record) => {
            assert_eq!(record.ack_seq, first_record.ack_seq);
            assert_eq!(record.response_json, first_record.response_json);
        }
        other => panic!("expected duplicate command, got {other:?}"),
    }

    let conflict = store
        .reserve_mobile_command_ack(
            COMMAND_KIND_SEND_SESSION_PROMPT,
            CLIENT_MUTATION_ID,
            CONFLICTING_REQUEST_HASH,
        )
        .expect("conflicting reserved command");
    match conflict {
        MobileCommandReservationResult::Conflict(record) => {
            assert_eq!(record.ack_seq, first_record.ack_seq);
            assert_eq!(record.request_hash, REQUEST_HASH);
        }
        other => panic!("expected conflicting command, got {other:?}"),
    }
}

fn state_delta_input(entity_id: &str, revision: &str, delta: &str) -> MobileStateEventInput {
    MobileStateEventInput {
        entity_id: entity_id.to_owned(),
        kind: MobileEventKind::SessionChanged,
        revision: revision.to_owned(),
        server_time: mobile_event_now(),
        payload_json: serde_json::json!({
            "threadId": entity_id,
            "detail": "state-delta",
            "delta": delta,
        }),
        client_mutation_id: None,
        command_kind: None,
        command_request_hash: None,
        command_response_json: None,
    }
}

fn command_ack_input(request_hash: &str, dispatch_kind: &str) -> MobileCommandAckInput {
    MobileCommandAckInput {
        command_kind: COMMAND_KIND_SEND_SESSION_PROMPT.to_owned(),
        client_mutation_id: CLIENT_MUTATION_ID.to_owned(),
        request_hash: request_hash.to_owned(),
        response_json: serde_json::json!({
            "accepted": true,
            "dispatchKind": dispatch_kind,
        }),
        state_event: MobileStateEventInput {
            entity_id: "thread-main".to_owned(),
            kind: MobileEventKind::SessionChanged,
            revision: "revision-command-ack".to_owned(),
            server_time: mobile_event_now(),
            payload_json: serde_json::json!({
                "threadId": "thread-main",
                "detail": "prompt-resumed",
            }),
            client_mutation_id: None,
            command_kind: None,
            command_request_hash: None,
            command_response_json: None,
        },
    }
}

#[tokio::test]
async fn mobile_event_stream_refreshes_after_session_state_content_change() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane =
        fixture.control_plane_with_codex_executable(fixture.slow_codex_resume_stub());
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
    record_thread_active(&control_plane, "thread-main");

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    prime_state_mini_cache(&control_plane);

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
        client_mutation_id: "grpc-authenticated-prompt-1".to_owned(),
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
    assert_eq!(prompt_response.dispatch_kind, "accepted");

    let ack_event = event_stream
        .message()
        .await
        .expect("command ack stream message")
        .expect("command ack event");
    assert_eq!(ack_event.event_name, "session.changed");
    assert_eq!(ack_event.thread_id, "thread-main");
    assert_eq!(ack_event.detail, "command-ack");

    for _ in 0..20 {
        let event = event_stream
            .message()
            .await
            .expect("post-ack stream message")
            .expect("post-ack event");
        if event.detail == "prompt-resumed" {
            assert_eq!(event.event_name, "session.changed");
            assert_eq!(event.thread_id, "thread-main");
            assert!(!event.revision.is_empty());
            return;
        }
    }
    panic!("timed out waiting for prompt-resumed stream event");
}

#[tokio::test]
async fn grpc_prompt_ack_returns_before_codex_resume_delivery_completes() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane =
        fixture.control_plane_with_codex_executable(fixture.slow_codex_resume_stub());
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    prime_state_mini_cache(&control_plane);

    let mut prompt_request = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Continue without waiting for slow resume delivery.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: "g011-ack-first-slow-resume".to_owned(),
    });
    add_mobile_grpc_authorization(&mut prompt_request, &authorization);

    let response = tokio::time::timeout(
        tokio::time::Duration::from_millis(1_500),
        client.send_session_prompt(prompt_request),
    )
    .await
    .expect("prompt ACK must not wait for slow resume delivery")
    .expect("send session prompt")
    .into_inner();

    assert!(response.accepted);
    assert_eq!(response.dispatch_kind, "accepted");
    assert_command_ack(
        response.accepted,
        &response.client_mutation_id,
        response.ack_seq,
        &response.entity_id,
        &response.revision,
        &response.server_time,
        response.idempotent_replay,
        "g011-ack-first-slow-resume",
    );
    wait_for_mobile_event_detail(&control_plane, "thread-main", "prompt-resumed").await;
}

#[tokio::test]
async fn grpc_set_session_mode_returns_ack_and_streams_mode_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
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

    let mut mode_request = tonic::Request::new(SetSessionModeRequest {
        thread_id: "thread-main".to_owned(),
        preset: "await-reply".to_owned(),
        client_mutation_id: "grpc-mode-stream-1".to_owned(),
    });
    mode_request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );

    let mode_response = client
        .set_session_mode(mode_request)
        .await
        .expect("set session mode")
        .into_inner();
    assert!(mode_response.accepted);
    assert_eq!(mode_response.thread_id, "thread-main");
    assert_eq!(mode_response.preset, "await-reply");
    assert!(!mode_response.server_time.is_empty());

    let lifecycle_event = event_stream
        .message()
        .await
        .expect("mode lifecycle stream message")
        .expect("mode lifecycle event");
    assert_eq!(lifecycle_event.event_name, "lifecycle.changed");
    assert_eq!(lifecycle_event.thread_id, "thread-main");
    assert_eq!(lifecycle_event.detail, "await-reply");

    let session_event = event_stream
        .message()
        .await
        .expect("mode session stream message")
        .expect("mode session event");
    assert_eq!(session_event.event_name, "session.changed");
    assert_eq!(session_event.thread_id, "thread-main");
    assert_eq!(session_event.detail, "mode-updated");
}

#[tokio::test]
async fn grpc_commands_return_idempotent_ack_seq() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    seed_replyable_session_mini(&control_plane, "mini-revision-g004", 4);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut blank_mode = tonic::Request::new(SetSessionModeRequest {
        thread_id: "thread-main".to_owned(),
        preset: "await-reply".to_owned(),
        client_mutation_id: String::new(),
    });
    add_mobile_grpc_authorization(&mut blank_mode, &authorization);
    assert_eq!(
        client
            .set_session_mode(blank_mode)
            .await
            .expect_err("blank mode mutation id")
            .code(),
        tonic::Code::InvalidArgument
    );

    let first_mode =
        set_mode_grpc(&mut client, &authorization, "await-reply", "g004-c002-mode").await;
    assert_command_ack(
        first_mode.accepted,
        &first_mode.client_mutation_id,
        first_mode.ack_seq,
        &first_mode.entity_id,
        &first_mode.revision,
        &first_mode.server_time,
        first_mode.idempotent_replay,
        "g004-c002-mode",
    );
    let mode_event_count = mobile_state_event_count(&control_plane);
    let replayed_mode =
        set_mode_grpc(&mut client, &authorization, "await-reply", "g004-c002-mode").await;
    assert!(replayed_mode.idempotent_replay);
    assert_eq!(replayed_mode.ack_seq, first_mode.ack_seq);
    assert_eq!(replayed_mode.revision, first_mode.revision);
    assert_eq!(mobile_state_event_count(&control_plane), mode_event_count);

    let mut blank_prompt = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Continue from blank rejection check.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: String::new(),
    });
    add_mobile_grpc_authorization(&mut blank_prompt, &authorization);
    assert_eq!(
        client
            .send_session_prompt(blank_prompt)
            .await
            .expect_err("blank prompt mutation id")
            .code(),
        tonic::Code::InvalidArgument
    );

    let first_prompt = send_prompt_grpc(&mut client, &authorization, "g004-c002-prompt").await;
    assert_eq!(first_prompt.dispatch_kind, "accepted");
    assert_command_ack(
        first_prompt.accepted,
        &first_prompt.client_mutation_id,
        first_prompt.ack_seq,
        &first_prompt.entity_id,
        &first_prompt.revision,
        &first_prompt.server_time,
        first_prompt.idempotent_replay,
        "g004-c002-prompt",
    );
    wait_for_mobile_event_detail(&control_plane, "thread-main", "prompt-queued").await;
    let prompt_event_count = mobile_state_event_count(&control_plane);
    let replayed_prompt = send_prompt_grpc(&mut client, &authorization, "g004-c002-prompt").await;
    assert!(replayed_prompt.idempotent_replay);
    assert_eq!(replayed_prompt.ack_seq, first_prompt.ack_seq);
    assert_eq!(replayed_prompt.revision, first_prompt.revision);
    assert_eq!(mobile_state_event_count(&control_plane), prompt_event_count);

    control_plane
        .store()
        .reserve_mobile_command_ack(
            COMMAND_KIND_SEND_SESSION_PROMPT,
            "g004-c002-inflight-prompt",
            &command_request_hash_for_test(
                COMMAND_KIND_SEND_SESSION_PROMPT,
                serde_json::json!({
                    "threadId": "thread-main",
                    "prompt": "Continue from G004.",
                    "assistantSurface": Option::<&str>::None,
                }),
            ),
        )
        .expect("reserve in-flight prompt command");
    let event_count_before_inflight_retry = mobile_state_event_count(&control_plane);
    let mut in_flight_prompt = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Continue from G004.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: "g004-c002-inflight-prompt".to_owned(),
    });
    add_mobile_grpc_authorization(&mut in_flight_prompt, &authorization);
    assert_eq!(
        client
            .send_session_prompt(in_flight_prompt)
            .await
            .expect_err("in-flight prompt mutation id")
            .code(),
        tonic::Code::Aborted
    );
    assert_eq!(
        mobile_state_event_count(&control_plane),
        event_count_before_inflight_retry,
        "in-flight retry must not dispatch a second prompt"
    );

    let connection = Connection::open(control_plane.store().path()).expect("open event store");
    connection
        .execute(
            "update mobile_command_log
             set created_at_ms = 0
             where command_kind = ?1 and client_mutation_id = ?2",
            rusqlite::params![
                COMMAND_KIND_SEND_SESSION_PROMPT,
                "g004-c002-inflight-prompt"
            ],
        )
        .expect("backdate in-flight prompt reservation");
    let recovered_prompt =
        send_prompt_grpc(&mut client, &authorization, "g004-c002-inflight-prompt").await;
    assert_command_ack(
        recovered_prompt.accepted,
        &recovered_prompt.client_mutation_id,
        recovered_prompt.ack_seq,
        &recovered_prompt.entity_id,
        &recovered_prompt.revision,
        &recovered_prompt.server_time,
        recovered_prompt.idempotent_replay,
        "g004-c002-inflight-prompt",
    );
    assert!(
        recovered_prompt.ack_seq > 0,
        "stale in-flight reservation must be reclaimed into a durable ACK"
    );

    let mut blank_reply = tonic::Request::new(SubmitNotificationReplyRequest {
        notification_id: "notification-1".to_owned(),
        thread_id: "thread-main".to_owned(),
        prompt: "Continue from notification.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: String::new(),
    });
    add_mobile_grpc_authorization(&mut blank_reply, &authorization);
    assert_eq!(
        client
            .submit_notification_reply(blank_reply)
            .await
            .expect_err("blank notification reply mutation id")
            .code(),
        tonic::Code::InvalidArgument
    );

    let first_reply =
        submit_notification_reply_grpc(&mut client, &authorization, "g004-c002-reply").await;
    assert_eq!(first_reply.notification_id, "notification-1");
    assert_eq!(first_reply.dispatch_kind, "accepted");
    assert_command_ack(
        first_reply.accepted,
        &first_reply.client_mutation_id,
        first_reply.ack_seq,
        &first_reply.entity_id,
        &first_reply.revision,
        &first_reply.server_time,
        first_reply.idempotent_replay,
        "g004-c002-reply",
    );
    wait_for_queued_prompt_count(&control_plane, "thread-main", 1).await;
    let reply_event_count = mobile_state_event_count(&control_plane);
    let replayed_reply =
        submit_notification_reply_grpc(&mut client, &authorization, "g004-c002-reply").await;
    assert!(replayed_reply.idempotent_replay);
    assert_eq!(replayed_reply.ack_seq, first_reply.ack_seq);
    assert_eq!(replayed_reply.revision, first_reply.revision);
    assert_eq!(mobile_state_event_count(&control_plane), reply_event_count);
}

#[tokio::test]
async fn grpc_session_commands_use_session_mini_visibility_before_snapshot() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    control_plane
        .store()
        .replace_mobile_session_minis(
            vec![MobileSessionMiniProjectionInput {
                session_id: "other-thread".to_owned(),
                assistant_surface: "codex".to_owned(),
                body_json: serde_json::json!({
                    "sessionId": "other-thread",
                    "assistantSurface": "codex",
                }),
            }],
            42,
            "mini-revision-42",
        )
        .expect("seed state mini projection");
    let event_count_before = mobile_state_event_count(&control_plane);

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    let mutation_id = "g010-mini-visibility-mode";
    let mut mode = tonic::Request::new(SetSessionModeRequest {
        thread_id: "thread-main".to_owned(),
        preset: "await-reply".to_owned(),
        client_mutation_id: mutation_id.to_owned(),
    });
    add_mobile_grpc_authorization(&mut mode, &authorization);

    assert_eq!(
        client
            .set_session_mode(mode)
            .await
            .expect_err("mini projection hides thread-main")
            .code(),
        tonic::Code::NotFound
    );
    assert_eq!(mobile_state_event_count(&control_plane), event_count_before);
    assert!(
        control_plane
            .store()
            .mobile_command_ack(COMMAND_KIND_SET_SESSION_MODE, mutation_id)
            .expect("command ack lookup")
            .is_none(),
        "failed fast-path validation must clear the reservation"
    );
}

#[tokio::test]
async fn grpc_prompt_requires_hot_state_mini_cache_without_snapshot_fallback() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    let event_count_before = mobile_state_event_count(&control_plane);

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    let mutation_id = "g011-cold-cache-prompt";
    let mut prompt = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "This must not trigger a desktop snapshot fallback.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: mutation_id.to_owned(),
    });
    add_mobile_grpc_authorization(&mut prompt, &authorization);

    assert_eq!(
        client
            .send_session_prompt(prompt)
            .await
            .expect_err("cold mini cache should fail fast")
            .code(),
        tonic::Code::FailedPrecondition
    );
    assert_eq!(mobile_state_event_count(&control_plane), event_count_before);
    assert!(
        control_plane
            .store()
            .mobile_command_ack(COMMAND_KIND_SEND_SESSION_PROMPT, mutation_id)
            .expect("command ack lookup")
            .is_none(),
        "failed cold-cache prompt must clear the reservation"
    );
}

#[tokio::test]
async fn grpc_prompt_in_mode_queues_from_session_mini_without_desktop_snapshot() {
    let fixture = IsolatedCodexFixture::new();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set prompt mode");
    control_plane
        .store()
        .replace_mobile_session_minis(
            vec![MobileSessionMiniProjectionInput {
                session_id: "thread-main".to_owned(),
                assistant_surface: "codex".to_owned(),
                body_json: serde_json::json!({
                    "sessionId": "thread-main",
                    "assistantSurface": "codex",
                    "effectiveMode": "await-reply",
                    "replyable": true,
                    "canSendPrompt": true,
                }),
            }],
            44,
            "mini-revision-44",
        )
        .expect("seed state mini projection");

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    let response = send_prompt_grpc(&mut client, &authorization, "g010-mini-prompt").await;

    assert_eq!(response.dispatch_kind, "accepted");
    assert_eq!(response.revision, "mini-revision-44");
    assert_command_ack(
        response.accepted,
        &response.client_mutation_id,
        response.ack_seq,
        &response.entity_id,
        &response.revision,
        &response.server_time,
        response.idempotent_replay,
        "g010-mini-prompt",
    );
    wait_for_queued_prompt_count(&control_plane, "thread-main", 1).await;
}

#[tokio::test]
async fn grpc_session_stream_acks_mode_command_before_state_deltas() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    seed_replyable_session_mini(&control_plane, "mini-revision-session-mode", 4);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    let mutation_id = "session-stream-mode-ack-first";

    let mut stream = open_session_stream(
        &mut client,
        &authorization,
        vec![set_mode_session_frame("await-reply", mutation_id)],
    )
    .await;

    let ack = next_session_ack_frame(&mut stream, "first mode command frame").await;
    assert_command_ack(
        ack.accepted,
        &ack.client_mutation_id,
        ack.ack_seq,
        &ack.entity_id,
        &ack.revision,
        &ack.server_time,
        ack.idempotent_replay,
        mutation_id,
    );
    assert_eq!(ack.error_code, "");
    assert_eq!(ack.reject_reason, "");

    let delta = next_session_state_delta_matching(&mut stream, "mode command ack delta", |delta| {
        delta.seq == ack.ack_seq
    })
    .await;
    assert_eq!(delta.entity_id, "thread-main");
    assert_eq!(state_delta_detail(&delta).as_deref(), Some("command-ack"));
}

#[tokio::test]
async fn grpc_session_stream_replays_state_deltas_after_resume_seq() {
    let fixture = IsolatedCodexFixture::new();
    let control_plane = fixture.control_plane();
    let first = control_plane
        .store()
        .record_mobile_state_event(state_delta_input("thread-main", "revision-one", "first"))
        .expect("record first state delta");
    let second = control_plane
        .store()
        .record_mobile_state_event(state_delta_input("thread-main", "revision-two", "second"))
        .expect("record second state delta");
    let third = control_plane
        .store()
        .record_mobile_state_event(state_delta_input("thread-main", "revision-three", "third"))
        .expect("record third state delta");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut stream = open_session_stream(
        &mut client,
        &authorization,
        vec![resume_session_frame(first.seq)],
    )
    .await;

    let replayed_second =
        next_session_state_delta(&mut stream, "second replayed state delta").await;
    assert_eq!(replayed_second.seq, second.seq);
    assert_eq!(replayed_second.revision, second.revision);
    assert_eq!(
        state_delta_payload_delta(&replayed_second).as_deref(),
        Some("second")
    );

    let replayed_third = next_session_state_delta(&mut stream, "third replayed state delta").await;
    assert_eq!(replayed_third.seq, third.seq);
    assert_eq!(replayed_third.revision, third.revision);
    assert_eq!(
        state_delta_payload_delta(&replayed_third).as_deref(),
        Some("third")
    );
}

#[tokio::test]
async fn grpc_session_stream_rejects_invalid_command_as_ack_frame() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    seed_replyable_session_mini(&control_plane, "mini-revision-invalid-session-command", 4);
    let event_count_before = mobile_state_event_count(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut stream = open_session_stream(
        &mut client,
        &authorization,
        vec![set_mode_session_frame("await-reply", "")],
    )
    .await;

    let ack = next_session_ack_frame(&mut stream, "first invalid mode command frame").await;
    assert!(!ack.accepted);
    assert_eq!(ack.client_mutation_id, "");
    assert_eq!(ack.entity_id, "thread-main");
    assert_eq!(ack.ack_seq, 0);
    assert_eq!(ack.error_code, "invalid_argument");
    assert!(ack.reject_reason.contains("client_mutation_id"));
    assert_eq!(mobile_state_event_count(&control_plane), event_count_before);
}

#[tokio::test]
async fn grpc_session_stream_mode_requires_hot_state_mini_cache_without_snapshot() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    let event_count_before = mobile_state_event_count(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut stream = open_session_stream(
        &mut client,
        &authorization,
        vec![set_mode_session_frame(
            "await-reply",
            "session-stream-mode-cold-mini",
        )],
    )
    .await;

    let ack = next_session_ack_frame(&mut stream, "first cold mini mode command frame").await;
    assert!(!ack.accepted);
    assert_eq!(ack.client_mutation_id, "session-stream-mode-cold-mini");
    assert_eq!(ack.entity_id, "thread-main");
    assert_eq!(ack.ack_seq, 0);
    assert_eq!(ack.error_code, "failed_precondition");
    assert!(ack.reject_reason.contains("state mini cache"));
    assert_eq!(mobile_state_event_count(&control_plane), event_count_before);
}

#[tokio::test]
async fn grpc_session_stream_replays_duplicate_command_ack() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    seed_replyable_session_mini(&control_plane, "mini-revision-session-duplicate", 4);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    let mutation_id = "session-stream-duplicate-mode";

    let mut stream = open_session_stream(
        &mut client,
        &authorization,
        vec![
            set_mode_session_frame("await-reply", mutation_id),
            set_mode_session_frame("await-reply", mutation_id),
        ],
    )
    .await;

    let first_ack = next_session_ack_frame(&mut stream, "first duplicate command frame").await;
    assert_command_ack(
        first_ack.accepted,
        &first_ack.client_mutation_id,
        first_ack.ack_seq,
        &first_ack.entity_id,
        &first_ack.revision,
        &first_ack.server_time,
        first_ack.idempotent_replay,
        mutation_id,
    );

    let replayed_ack =
        next_session_ack_matching(&mut stream, "idempotent replay command ack", |ack| {
            ack.client_mutation_id == mutation_id && ack.idempotent_replay
        })
        .await;
    assert!(replayed_ack.accepted);
    assert_eq!(replayed_ack.ack_seq, first_ack.ack_seq);
    assert_eq!(replayed_ack.revision, first_ack.revision);
    assert_eq!(replayed_ack.entity_id, first_ack.entity_id);
    assert_eq!(replayed_ack.error_code, "");
    assert_eq!(replayed_ack.reject_reason, "");
}

fn add_mobile_grpc_authorization<T>(request: &mut tonic::Request<T>, authorization: &str) {
    request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );
}

async fn open_session_stream(
    client: &mut LooperRealtimeClient<tonic::transport::Channel>,
    authorization: &str,
    frames: Vec<ClientFrame>,
) -> tonic::codec::Streaming<ServerFrame> {
    let mut request = tonic::Request::new(tokio_stream::iter(frames));
    add_mobile_grpc_authorization(&mut request, authorization);
    client
        .session(request)
        .await
        .expect("open session stream")
        .into_inner()
}

fn set_mode_session_frame(preset: &str, client_mutation_id: &str) -> ClientFrame {
    ClientFrame {
        frame: Some(client_frame::Frame::Command(Command {
            command: Some(command::Command::SetSessionMode(SetSessionModeRequest {
                thread_id: "thread-main".to_owned(),
                preset: preset.to_owned(),
                client_mutation_id: client_mutation_id.to_owned(),
            })),
        })),
    }
}

fn resume_session_frame(after_seq: i64) -> ClientFrame {
    ClientFrame {
        frame: Some(client_frame::Frame::Resume(Resume { after_seq })),
    }
}

async fn next_session_frame(
    stream: &mut tonic::codec::Streaming<ServerFrame>,
    label: &str,
) -> ServerFrame {
    tokio::time::timeout(
        tokio::time::Duration::from_millis(SESSION_FRAME_TIMEOUT_MILLIS),
        stream.message(),
    )
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
    .expect("session stream frame result")
    .unwrap_or_else(|| panic!("session stream closed before {label}"))
}

async fn next_session_ack_frame(
    stream: &mut tonic::codec::Streaming<ServerFrame>,
    label: &str,
) -> agent_control_plane::grpc::proto::CommandAck {
    let frame = next_session_frame(stream, label).await;
    match frame.frame {
        Some(server_frame::Frame::Ack(ack)) => ack,
        other => panic!("expected ACK as {label}, got {other:?}"),
    }
}

async fn next_session_ack_matching(
    stream: &mut tonic::codec::Streaming<ServerFrame>,
    label: &str,
    mut matches: impl FnMut(&agent_control_plane::grpc::proto::CommandAck) -> bool,
) -> agent_control_plane::grpc::proto::CommandAck {
    for _ in 0..SESSION_FRAME_SCAN_LIMIT {
        let frame = next_session_frame(stream, label).await;
        if let Some(server_frame::Frame::Ack(ack)) = frame.frame {
            if matches(&ack) {
                return ack;
            }
        }
    }
    panic!("timed out scanning session stream for {label}");
}

async fn next_session_state_delta(
    stream: &mut tonic::codec::Streaming<ServerFrame>,
    label: &str,
) -> agent_control_plane::grpc::proto::StateMiniDelta {
    next_session_state_delta_matching(stream, label, |_| true).await
}

async fn next_session_state_delta_matching(
    stream: &mut tonic::codec::Streaming<ServerFrame>,
    label: &str,
    mut matches: impl FnMut(&agent_control_plane::grpc::proto::StateMiniDelta) -> bool,
) -> agent_control_plane::grpc::proto::StateMiniDelta {
    for _ in 0..SESSION_FRAME_SCAN_LIMIT {
        let frame = next_session_frame(stream, label).await;
        if let Some(server_frame::Frame::StateDelta(delta)) = frame.frame {
            if matches(&delta) {
                return delta;
            }
        }
    }
    panic!("timed out scanning session stream for {label}");
}

fn state_delta_detail(delta: &agent_control_plane::grpc::proto::StateMiniDelta) -> Option<String> {
    state_delta_payload_string(delta, "detail")
}

fn state_delta_payload_delta(
    delta: &agent_control_plane::grpc::proto::StateMiniDelta,
) -> Option<String> {
    state_delta_payload_string(delta, "delta")
}

fn state_delta_payload_string(
    delta: &agent_control_plane::grpc::proto::StateMiniDelta,
    key: &str,
) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&delta.payload_json)
        .ok()
        .and_then(|payload| {
            payload
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
}

async fn set_mode_grpc(
    client: &mut LooperRealtimeClient<tonic::transport::Channel>,
    authorization: &str,
    preset: &str,
    client_mutation_id: &str,
) -> agent_control_plane::grpc::proto::SetSessionModeResponse {
    let mut request = tonic::Request::new(SetSessionModeRequest {
        thread_id: "thread-main".to_owned(),
        preset: preset.to_owned(),
        client_mutation_id: client_mutation_id.to_owned(),
    });
    add_mobile_grpc_authorization(&mut request, authorization);
    client
        .set_session_mode(request)
        .await
        .expect("set session mode")
        .into_inner()
}

async fn send_prompt_grpc(
    client: &mut LooperRealtimeClient<tonic::transport::Channel>,
    authorization: &str,
    client_mutation_id: &str,
) -> agent_control_plane::grpc::proto::SendSessionPromptResponse {
    let mut request = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Continue from G004.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: client_mutation_id.to_owned(),
    });
    add_mobile_grpc_authorization(&mut request, authorization);
    client
        .send_session_prompt(request)
        .await
        .expect("send session prompt")
        .into_inner()
}

async fn submit_notification_reply_grpc(
    client: &mut LooperRealtimeClient<tonic::transport::Channel>,
    authorization: &str,
    client_mutation_id: &str,
) -> agent_control_plane::grpc::proto::SubmitNotificationReplyResponse {
    let mut request = tonic::Request::new(SubmitNotificationReplyRequest {
        notification_id: "notification-1".to_owned(),
        thread_id: "thread-main".to_owned(),
        prompt: "Continue from notification.".to_owned(),
        assistant_surface: String::new(),
        client_mutation_id: client_mutation_id.to_owned(),
    });
    add_mobile_grpc_authorization(&mut request, authorization);
    client
        .submit_notification_reply(request)
        .await
        .expect("submit notification reply")
        .into_inner()
}

fn assert_command_ack(
    accepted: bool,
    client_mutation_id: &str,
    ack_seq: i64,
    entity_id: &str,
    revision: &str,
    server_time: &str,
    idempotent_replay: bool,
    expected_client_mutation_id: &str,
) {
    assert!(accepted);
    assert_eq!(client_mutation_id, expected_client_mutation_id);
    assert!(ack_seq > 0);
    assert_eq!(entity_id, "thread-main");
    assert!(!revision.is_empty());
    assert!(!server_time.is_empty());
    assert!(!idempotent_replay);
}

fn mobile_state_event_count(control_plane: &ControlPlane) -> usize {
    control_plane
        .store()
        .mobile_state_events_after_seq(0, 1_000)
        .expect("mobile state events")
        .len()
}

async fn wait_for_mobile_event_detail(control_plane: &ControlPlane, thread_id: &str, detail: &str) {
    for _ in 0..80 {
        let found = control_plane
            .store()
            .mobile_state_events_after_seq(0, 1_000)
            .expect("mobile state events")
            .iter()
            .any(|event| {
                event.entity_id == thread_id
                    && serde_json::from_str::<serde_json::Value>(&event.payload_json)
                        .ok()
                        .and_then(|payload| {
                            payload
                                .get("detail")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        })
                        .as_deref()
                        == Some(detail)
            });
        if found {
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {detail} event for {thread_id}");
}

async fn wait_for_queued_prompt_count(
    control_plane: &ControlPlane,
    thread_id: &str,
    expected_count: i64,
) {
    let mut observed_count = None;
    for _ in 0..80 {
        let queued_counts = control_plane
            .mobile_session_service()
            .queued_prompt_counts()
            .expect("queued prompt counts");
        observed_count = queued_counts.get(thread_id).copied();
        if queued_counts.get(thread_id) == Some(&expected_count) {
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    panic!(
        "timed out waiting for {expected_count} queued prompts for {thread_id}; observed {observed_count:?}"
    );
}

fn seed_replyable_session_mini(control_plane: &ControlPlane, revision: &str, seq: i64) {
    control_plane
        .store()
        .replace_mobile_session_minis(
            vec![MobileSessionMiniProjectionInput {
                session_id: "thread-main".to_owned(),
                assistant_surface: "codex".to_owned(),
                body_json: serde_json::json!({
                    "sessionId": "thread-main",
                    "assistantSurface": "codex",
                    "effectiveMode": "await-reply",
                    "replyable": true,
                    "canSendPrompt": true,
                }),
            }],
            seq,
            revision,
        )
        .expect("seed replyable session mini");
}

fn command_request_hash_for_test(command_kind: &str, payload: serde_json::Value) -> String {
    use sha2::{Digest, Sha256};

    let body = serde_json::json!({
        "commandKind": command_kind,
        "payload": payload,
    });
    let encoded = serde_json::to_vec(&body).expect("command hash json");
    let digest = Sha256::digest(encoded);
    format!("sha256:{digest:x}")
}

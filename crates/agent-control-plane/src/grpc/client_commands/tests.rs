use std::time::Duration;

use tokio::sync::oneshot;

use super::transport::{local_session_transport_endpoints, open_local_session_command_stream};
use super::*;

mod fixtures;

use fixtures::*;

#[tokio::test]
async fn local_session_command_h3() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let grpc_address = reserve_local_tcp_address().await;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let h3 = crate::grpc::spawn_h3_server(control_plane.clone(), grpc_address, async {
        let _ = shutdown_receiver.await;
    })
    .await
    .expect("spawn H3 server");

    let client_mutation_id = "local-h3-mode";
    let command = set_session_mode_command("thread-main", client_mutation_id);
    let result = submit_local_session_command_with_h3_certificate_sha256(
        &http_base_url_for_grpc_address(grpc_address),
        command,
        client_mutation_id,
        Some(h3.certificate_sha256.clone()),
    )
    .await
    .expect("local Session command should use H3 when H2 is absent");

    let ack = result.ack;
    assert!(ack.accepted, "H3 local command ACK should be accepted");
    assert_eq!(result.transport, LocalSessionTransport::H3);
    assert!(result.fallback_reason.is_empty());
    assert_eq!(ack.client_mutation_id, client_mutation_id);
    assert!(
        control_plane
            .store()
            .mobile_command_ack("SetSessionMode", client_mutation_id)
            .expect("stored command ACK lookup")
            .is_some(),
        "local helper must use the Session command ACK path"
    );
    println!(
        "local_session_command_h3 transport=h3 endpoint={} ack_mutation_id={} ack_seq={} udp_addr={}",
        result.endpoint_url, ack.client_mutation_id, ack.ack_seq, h3.listen_address
    );

    let _ = shutdown_sender.send(());
    h3.server_task
        .await
        .expect("H3 server task should not panic")
        .expect("H3 server should shut down");
}

#[tokio::test]
async fn local_session_command_h3_keeps_stream_open_for_followup_frame() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let grpc_address = reserve_local_tcp_address().await;
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let h3 = crate::grpc::spawn_h3_server(control_plane.clone(), grpc_address, async {
        let _ = shutdown_receiver.await;
    })
    .await
    .expect("spawn H3 server");

    let first_mutation_id = "local-h3-followup-mode-1";
    let first_frame = proto::ClientFrame {
        frame: Some(proto::client_frame::Frame::Command(
            set_session_mode_command("thread-main", first_mutation_id),
        )),
    };
    let mut opened = open_local_session_command_stream(
        local_session_transport_endpoints(
            &http_base_url_for_grpc_address(grpc_address),
            Some(h3.certificate_sha256.clone()),
        )
        .expect("local endpoints"),
        first_frame,
    )
    .await
    .expect("open H3 Session command stream");

    let first_ack = read_command_ack(&mut opened.stream, first_mutation_id).await;
    assert!(
        first_ack.accepted,
        "first H3 command ACK should be accepted"
    );

    let second_mutation_id = "local-h3-followup-mode-2";
    opened
        .request_sender
        .send(proto::ClientFrame {
            frame: Some(proto::client_frame::Frame::Command(
                set_session_mode_command("thread-main", second_mutation_id),
            )),
        })
        .await
        .expect("send follow-up H3 Session command frame");
    let second_ack = read_command_ack(&mut opened.stream, second_mutation_id).await;
    assert!(
        second_ack.accepted,
        "follow-up H3 command ACK should be accepted"
    );
    println!(
        "local_session_command_h3_followup transport=h3 endpoint={} first_ack_seq={} second_ack_seq={}",
        opened.endpoint_url, first_ack.ack_seq, second_ack.ack_seq
    );

    drop(opened);
    let _ = shutdown_sender.send(());
    h3.server_task
        .await
        .expect("H3 server task should not panic")
        .expect("H3 server should shut down");
}

#[tokio::test]
async fn local_session_command_h3_falls_back_to_h2() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let certificate =
        crate::grpc::load_or_create_h3_certificate(&control_plane).expect("local H3 certificate");
    let h2 = spawn_h2(control_plane.clone()).await;

    let client_mutation_id = "local-h2-fallback-mode";
    let command = set_session_mode_command("thread-main", client_mutation_id);
    let result = submit_local_session_command_with_h3_certificate_sha256(
        &http_base_url_for_grpc_address(h2.address),
        command,
        client_mutation_id,
        Some(certificate.certificate_sha256),
    )
    .await
    .expect("local Session command should fall back to H2 when H3 is absent");

    assert!(result.ack.accepted, "fallback H2 ACK should be accepted");
    assert_eq!(result.transport, LocalSessionTransport::H2);
    assert!(
        result.fallback_reason.contains("h3 pre-stream failure"),
        "fallback should record H3 failure reason, got {:?}",
        result.fallback_reason
    );
    assert!(
        control_plane
            .store()
            .mobile_command_ack("SetSessionMode", client_mutation_id)
            .expect("stored command ACK lookup")
            .is_some(),
        "fallback must still use the Session command ACK path"
    );
    println!(
        "local_session_command_h3_falls_back_to_h2 transport=h2 endpoint={} fallback_reason={} ack_mutation_id={} ack_seq={}",
        result.endpoint_url,
        result.fallback_reason,
        result.ack.client_mutation_id,
        result.ack.ack_seq
    );

    h2.shutdown().await;
}

#[tokio::test]
async fn local_session_command_ack_timeout_remains_two_seconds() {
    let h2 = spawn_no_ack_h2().await;
    let started = tokio::time::Instant::now();
    let error = submit_local_session_command_with_h3_certificate_sha256(
        &http_base_url_for_grpc_address(h2.address),
        set_session_mode_command("thread-main", "timeout-command"),
        "timeout-command",
        None,
    )
    .await
    .expect_err("local Session command should time out waiting for ACK");
    let elapsed = started.elapsed();

    assert!(
        elapsed >= COMMAND_ACK_TIMEOUT && elapsed < COMMAND_ACK_TIMEOUT + Duration::from_secs(2),
        "ACK timeout should stay near 2s, elapsed={elapsed:?}"
    );
    assert!(
        error
            .to_string()
            .contains("local Session command ACK timed out"),
        "unexpected timeout error: {error:#}"
    );
    println!(
        "local_session_command_ack_timeout timeout_ms={} error={}",
        elapsed.as_millis(),
        error
    );

    h2.shutdown().await;
}

#[test]
fn local_session_transport_candidates_require_pin_for_h3() {
    let endpoints = local_session_transport_endpoints("http://127.0.0.1:8765", None)
        .expect("local transport endpoints");
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].transport, LocalSessionTransport::H2);
    assert_eq!(endpoints[0].url, "http://127.0.0.1:8766/");
}

async fn read_command_ack(
    stream: &mut tonic::Streaming<proto::ServerFrame>,
    client_mutation_id: &str,
) -> proto::CommandAck {
    tokio::time::timeout(COMMAND_ACK_TIMEOUT, async {
        while let Some(frame) = stream.message().await.expect("Session frame result") {
            let Some(proto::server_frame::Frame::Ack(ack)) = frame.frame else {
                continue;
            };
            if ack.client_mutation_id == client_mutation_id {
                return ack;
            }
        }
        panic!("Session stream ended before ACK {client_mutation_id}");
    })
    .await
    .expect("timed out waiting for command ACK")
}

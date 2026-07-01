mod support;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use agent_control_plane::grpc::proto::{ClientFrame, HealthRequest, server_frame};
use agent_control_plane::http::build_router;
use support::control_plane::TestControlPlaneFixture;
use support::h3::{H3_TEST_TIMEOUT, h3_client_tls_config, spawn_h2_client, spawn_h3};
use support::mobile_auth::issue_mobile_authorization_header;
use tonic::metadata::MetadataValue;

#[tokio::test]
async fn grpc_h3_listener_binds_shutdowns_and_coexists_with_h2() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let authorization =
        issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
    let h2 = spawn_h2_client(control_plane.clone()).await;

    let listen_address = agent_control_plane::grpc::default_h3_listen_address(h2.address)
        .expect("H3 listen address");
    let h3 = spawn_h3(control_plane.clone(), listen_address).await;
    assert!(
        h3.certificate_sha256.starts_with("sha256:"),
        "H3 cert pin should be exposed as sha256-prefixed hex"
    );
    let reloaded = agent_control_plane::grpc::load_or_create_h3_certificate(&control_plane)
        .expect("reload persisted H3 certificate");
    assert_eq!(
        h3.certificate_sha256, reloaded.certificate_sha256,
        "H3 certificate should persist in control-plane-owned state"
    );

    let (mut h3_client, health) = h3.ready_client().await;
    assert!(health.ok, "H3 health should report ok");

    let mut h3_request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    h3_request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(authorization.as_str()).expect("authorization metadata"),
    );
    let mut h3_stream = tokio::time::timeout(H3_TEST_TIMEOUT, h3_client.session(h3_request))
        .await
        .expect("H3 session open timed out")
        .expect("H3 session should open")
        .into_inner();
    let h3_frame = tokio::time::timeout(H3_TEST_TIMEOUT, h3_stream.message())
        .await
        .expect("H3 frame timed out")
        .expect("H3 frame result")
        .expect("H3 should produce a Session frame");
    let h3_ack = match h3_frame.frame {
        Some(server_frame::Frame::Ack(ack)) => ack,
        other => panic!("expected H3 Session ack, got {other:?}"),
    };
    assert_eq!(h3_ack.error_code, "empty_client_frame");
    println!(
        "h3_listener_session_ok udp_addr={} pin={}",
        h3.address, h3.certificate_sha256
    );

    let mut h2_client = h2.client.clone();
    let h2_health = tokio::time::timeout(H3_TEST_TIMEOUT, h2_client.health(HealthRequest {}))
        .await
        .expect("H2 health timed out")
        .expect("H2 health response")
        .into_inner();
    assert!(h2_health.ok, "H2 listener should remain live");
    println!("h2_coexistence_ok tcp_addr={}", h2.address);

    drop(h3_stream);
    drop(h3_client);
    let h3_address = h3.shutdown().await;
    println!("h3_shutdown_joined udp_addr={h3_address}");
    h2.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_listener_allows_loopback_without_mobile_auth() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )
    .await;
    let (mut client, _) = h3.ready_client().await;
    let request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    let mut stream = tokio::time::timeout(H3_TEST_TIMEOUT, client.session(request))
        .await
        .expect("loopback H3 session timed out")
        .expect("loopback H3 should bypass auth")
        .into_inner();
    let frame = tokio::time::timeout(H3_TEST_TIMEOUT, stream.message())
        .await
        .expect("loopback H3 frame timed out")
        .expect("loopback H3 frame result")
        .expect("loopback H3 should produce a frame");
    assert!(matches!(frame.frame, Some(server_frame::Frame::Ack(_))));
    println!("h3_loopback_bypass_ok udp_addr={}", h3.address);
    h3.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_auth_rejects_missing_pairing_token() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    )
    .await;
    h3.wait_until_ready().await;
    let mut client = h3.client_for_host(Ipv4Addr::LOCALHOST.into());
    let request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    let status = tokio::time::timeout(H3_TEST_TIMEOUT, client.session(request))
        .await
        .expect("non-loopback H3 auth rejection timed out")
        .expect_err("non-loopback H3 should reject missing pairing token");
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
    assert!(
        status.message().contains("pairing token required"),
        "unexpected auth error: {status}"
    );
    println!(
        "h3_missing_token_rejected code={:?} message={}",
        status.code(),
        status.message()
    );
    h3.shutdown().await;
}

#[test]
fn grpc_h3_test_client_disables_0rtt_early_data() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let certificate =
        agent_control_plane::grpc::load_or_create_h3_certificate(&fixture.control_plane())
            .expect("H3 test certificate");
    let tls_config = h3_client_tls_config(&certificate.certificate_der);
    assert!(
        !tls_config.enable_early_data,
        "H3 Session test client must not send replayable 0-RTT early data"
    );
}

mod support;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use agent_control_plane::grpc::proto::{ClientFrame, HealthRequest, server_frame};
use agent_control_plane::http::build_router;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use support::control_plane::TestControlPlaneFixture;
use support::h3::{
    H3_TEST_TIMEOUT, configured_client_endpoint, h3_uri, quinn_h3_channel,
    reserve_dead_udp_address, spawn_h3,
};
use support::mobile_auth::issue_mobile_authorization_header;
use tonic::metadata::MetadataValue;

#[tokio::test]
async fn grpc_h3_quinn_smoke_health_and_session() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let authorization =
        issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )
    .await;
    let (mut client, health) = h3.ready_client().await;

    assert!(health.ok, "H3 health should report ok");
    assert_eq!(health.service, "looper-realtime");
    println!(
        "h3_health_ok service={} udp_addr={}",
        health.service, h3.address
    );

    let mut request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(authorization.as_str()).expect("authorization metadata"),
    );
    let mut stream = tokio::time::timeout(H3_TEST_TIMEOUT, client.session(request))
        .await
        .expect("H3 Session open timed out")
        .expect("H3 Session should open")
        .into_inner();
    let frame = tokio::time::timeout(H3_TEST_TIMEOUT, stream.message())
        .await
        .expect("H3 Session frame timed out")
        .expect("H3 Session frame result")
        .expect("H3 Session should yield an observable frame");
    let ack = match frame.frame {
        Some(server_frame::Frame::Ack(ack)) => ack,
        other => panic!("expected Session ack over H3, got {other:?}"),
    };
    assert_eq!(ack.error_code, "empty_client_frame");
    assert!(
        !ack.accepted,
        "empty client frame should be rejected but prove Session round trip"
    );
    println!(
        "h3_session_open ack_error={} udp_addr={}",
        ack.error_code, h3.address
    );

    h3.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_quinn_smoke_dead_udp_returns_error() {
    let certificate = h3_certificate();
    let client_endpoint = configured_client_endpoint(certificate.cert.der().as_ref()).await;
    let dead_address = reserve_dead_udp_address().await;
    let channel = quinn_h3_channel(h3_uri(dead_address), client_endpoint);
    let mut client =
        agent_control_plane::grpc::proto::looper_realtime_client::LooperRealtimeClient::new(
            channel,
        );

    let result = tokio::time::timeout(H3_TEST_TIMEOUT, client.health(HealthRequest {})).await;
    match result {
        Ok(Err(error)) => {
            println!("dead_udp_handled_error addr={dead_address} error={error}");
        }
        Err(_) => {
            println!("dead_udp_handled_error addr={dead_address} error=connect timed out");
        }
        Ok(Ok(response)) => panic!(
            "dead UDP endpoint unexpectedly returned health over H3: {:?}",
            response.into_inner()
        ),
    }
}

fn h3_certificate() -> CertifiedKey {
    generate_simple_self_signed(vec!["localhost".to_owned()]).expect("self-signed localhost cert")
}

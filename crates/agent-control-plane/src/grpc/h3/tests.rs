use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::certificate::generate_h3_certificate;
use super::client::pinned_h3_client_endpoint;
use super::server::{
    h3_listen_address_from_env_value, h3_server_transport_config, h3_tls_server_config,
};

#[test]
fn grpc_h3_invalid_listen_address_is_rejected() {
    let h2_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8766);
    let error = h3_listen_address_from_env_value(h2_address, Some("not-a-socket"))
        .expect_err("invalid H3 listen override should be rejected");
    assert!(
        error
            .to_string()
            .contains("invalid AGENT_CONTROL_PLANE_GRPC_H3_LISTEN value"),
        "unexpected parse error: {error:#}"
    );
}

#[test]
fn grpc_h3_server_tls_disables_0rtt_early_data() {
    let certificate = generate_h3_certificate().expect("H3 test certificate");
    let tls_config = h3_tls_server_config(&certificate).expect("H3 TLS config");
    assert_eq!(
        tls_config.max_early_data_size, 0,
        "H3 Session transport must not accept replayable 0-RTT early data"
    );
}

#[test]
fn grpc_h3_server_transport_config_sets_liveness_window() {
    let config = h3_server_transport_config().expect("H3 server transport config");
    let debug = format!("{config:?}");

    assert!(
        debug.contains("keep_alive_interval: Some(10s)"),
        "H3 server keepalive should stay below Session heartbeat interval: {debug}"
    );
    assert!(
        debug.contains("max_idle_timeout: Some(40000)"),
        "H3 server idle timeout should exceed client read deadline: {debug}"
    );
}

#[test]
fn grpc_h3_pinned_client_rejects_malformed_certificate_pin() {
    let error = pinned_h3_client_endpoint("sha256:not-hex")
        .expect_err("malformed H3 certificate pin should be rejected");
    assert!(
        error.to_string().contains("H3 certificate pin"),
        "unexpected malformed pin error: {error:#}"
    );
}

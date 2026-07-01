use super::*;

#[tokio::test]
async fn grpc_finality_commands_accept_fresh_visible_session_state() {
    let commands = [
        (
            "session-siri-current",
            command::Command::SetSiriCurrentSession(
                agent_control_plane::grpc::proto::SetSiriCurrentSessionRequest {
                    thread_id: "thread-main".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    client_mutation_id: "session-siri-current".to_owned(),
                },
            ),
        ),
        (
            "session-siri-default",
            command::Command::SetSiriDefaultSession(SetSiriDefaultSessionRequest {
                thread_id: "thread-main".to_owned(),
                assistant_surface: "codex".to_owned(),
                client_mutation_id: "session-siri-default".to_owned(),
            }),
        ),
        (
            "session-mute",
            command::Command::MuteSession(MuteSessionRequest {
                thread_id: "thread-main".to_owned(),
                client_mutation_id: "session-mute".to_owned(),
            }),
        ),
        (
            "session-archive",
            command::Command::SetSessionArchived(SetSessionArchivedRequest {
                thread_id: "thread-main".to_owned(),
                archived: true,
                client_mutation_id: "session-archive".to_owned(),
            }),
        ),
        (
            "session-delete",
            command::Command::DeleteSession(DeleteSessionRequest {
                thread_id: "thread-main".to_owned(),
                client_mutation_id: "session-delete".to_owned(),
            }),
        ),
    ];

    for (mutation_id, command) in commands {
        let ack = finality_command_ack_for_fresh_visible_session(mutation_id, command).await;
        assert_accepted_finality_certificate(&ack, mutation_id);
    }
}

async fn finality_command_ack_for_fresh_visible_session(
    mutation_id: &str,
    command: command::Command,
) -> agent_control_plane::grpc::proto::CommandAck {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    seed_replyable_session_mini_for_thread(
        &control_plane,
        "thread-main",
        "codex",
        &format!("mini-revision-{mutation_id}"),
        4,
    );
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    submit_grpc_session_command(control_plane, &authorization, command).await
}

fn assert_accepted_finality_certificate(
    ack: &agent_control_plane::grpc::proto::CommandAck,
    expected_client_mutation_id: &str,
) {
    assert!(
        ack.accepted,
        "{expected_client_mutation_id}: {}",
        ack.reject_reason
    );
    assert_eq!(ack.client_mutation_id, expected_client_mutation_id);
    assert!(!ack.account_id.is_empty());
    assert!(!ack.node_id.is_empty());
    assert!(ack.ack_seq > 0);
    assert!(!ack.entity_id.is_empty());
    assert!(!ack.revision.is_empty());
    assert!(!ack.server_time.is_empty());
    assert_eq!(ack.error_code, "");
    assert_eq!(ack.reject_reason, "");
    assert_eq!(ack.current_state, "");
}

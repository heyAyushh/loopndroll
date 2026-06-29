use crate::error::ClientCoreError;
use crate::model::{
    ClientCommandAck, ClientCommandAckEnvelope, ClientCommandBatchResponse, ClientCommandKind,
    ClientCommandMetadata,
};

const REJECTED_DISPATCH_KIND: &str = "rejected";

pub fn build_command_batch_response(
    commands: Vec<ClientCommandMetadata>,
    acks: Vec<ClientCommandAck>,
) -> Result<ClientCommandBatchResponse, ClientCoreError> {
    validate_command_metadata(&commands)?;

    if commands.is_empty() {
        return Ok(ClientCommandBatchResponse {
            accepted: true,
            command_acks: Vec::new(),
        });
    }

    let mut envelopes = Vec::with_capacity(commands.len());
    let mut acknowledged_mutations = Vec::with_capacity(commands.len());

    for ack in acks {
        let Some(command) = commands
            .iter()
            .find(|command| command.client_mutation_id == ack.client_mutation_id)
        else {
            continue;
        };
        if ack.ack_seq < 0 {
            return Err(ClientCoreError::InvalidSequence);
        }
        if acknowledged_mutations
            .iter()
            .any(|client_mutation_id| client_mutation_id == &ack.client_mutation_id)
        {
            continue;
        }

        acknowledged_mutations.push(ack.client_mutation_id.clone());
        envelopes.push(ClientCommandAckEnvelope {
            command_kind: command.command_kind,
            preset: command.preset.clone(),
            dispatch_kind: command_dispatch_kind(command, &ack),
            prompt_id: String::new(),
            notification_id: command.notification_id.clone(),
            ack,
        });

        if envelopes.len() == commands.len() {
            break;
        }
    }

    Ok(ClientCommandBatchResponse {
        accepted: envelopes.len() == commands.len()
            && envelopes.iter().all(|envelope| envelope.ack.accepted),
        command_acks: envelopes,
    })
}

pub fn reduce_expected_command_ack(
    response: ClientCommandBatchResponse,
    command_kind: ClientCommandKind,
    expected_client_mutation_id: String,
) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
    require_mutation_id(&expected_client_mutation_id)?;

    let envelope = response
        .command_acks
        .into_iter()
        .find(|envelope| {
            envelope.command_kind == command_kind
                && envelope.ack.client_mutation_id == expected_client_mutation_id
        })
        .ok_or(ClientCoreError::MissingCommandAcknowledgement)?;

    Ok(envelope)
}

fn validate_command_metadata(commands: &[ClientCommandMetadata]) -> Result<(), ClientCoreError> {
    for command in commands {
        require_mutation_id(&command.client_mutation_id)?;
    }
    Ok(())
}

fn require_mutation_id(client_mutation_id: &str) -> Result<(), ClientCoreError> {
    if client_mutation_id.trim().is_empty() {
        Err(ClientCoreError::EmptyMutationId)
    } else {
        Ok(())
    }
}

fn command_dispatch_kind(command: &ClientCommandMetadata, ack: &ClientCommandAck) -> String {
    if ack.accepted {
        command.dispatch_kind.clone()
    } else {
        REJECTED_DISPATCH_KIND.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUTATION_MODE: &str = "mutation-mode";
    const MUTATION_PROMPT: &str = "mutation-prompt";
    const THREAD_ID: &str = "thread-1";

    #[test]
    fn empty_batch_is_accepted() {
        let response = build_command_batch_response(vec![], vec![]).expect("batch response");

        assert!(response.accepted);
        assert!(response.command_acks.is_empty());
    }

    #[test]
    fn batch_matches_acks_by_mutation_and_preserves_command_metadata() {
        let response = build_command_batch_response(
            vec![
                command_metadata(
                    ClientCommandKind::SetSessionMode,
                    MUTATION_MODE,
                    "await-reply",
                    "",
                    "",
                ),
                command_metadata(
                    ClientCommandKind::SendSessionPrompt,
                    MUTATION_PROMPT,
                    "",
                    "accepted",
                    "",
                ),
            ],
            vec![
                ack("", true, 39),
                ack("unknown", true, 40),
                ack(MUTATION_PROMPT, true, 42),
                ack(MUTATION_MODE, true, 41),
            ],
        )
        .expect("batch response");

        assert!(response.accepted);
        assert_eq!(response.command_acks.len(), 2);
        assert_eq!(
            response.command_acks[0].command_kind,
            ClientCommandKind::SendSessionPrompt
        );
        assert_eq!(response.command_acks[0].dispatch_kind, "accepted");
        assert_eq!(
            response.command_acks[1].command_kind,
            ClientCommandKind::SetSessionMode
        );
        assert_eq!(response.command_acks[1].preset, "await-reply");
    }

    #[test]
    fn rejected_ack_marks_batch_rejected_and_overrides_dispatch_kind() {
        let response = build_command_batch_response(
            vec![command_metadata(
                ClientCommandKind::SubmitNotificationReply,
                MUTATION_PROMPT,
                "",
                "accepted",
                "notification-1",
            )],
            vec![ack(MUTATION_PROMPT, false, 42)],
        )
        .expect("batch response");

        assert!(!response.accepted);
        assert_eq!(response.command_acks[0].dispatch_kind, "rejected");
        assert_eq!(response.command_acks[0].notification_id, "notification-1");
    }

    #[test]
    fn duplicate_acks_are_ignored_after_first_match() {
        let response = build_command_batch_response(
            vec![command_metadata(
                ClientCommandKind::SendSessionPrompt,
                MUTATION_PROMPT,
                "",
                "accepted",
                "",
            )],
            vec![
                ack(MUTATION_PROMPT, true, 42),
                ack(MUTATION_PROMPT, false, 43),
            ],
        )
        .expect("batch response");

        assert!(response.accepted);
        assert_eq!(response.command_acks.len(), 1);
        assert_eq!(response.command_acks[0].ack.ack_seq, 42);
    }

    #[test]
    fn missing_ack_keeps_batch_unaccepted() {
        let response = build_command_batch_response(
            vec![command_metadata(
                ClientCommandKind::SendSessionPrompt,
                MUTATION_PROMPT,
                "",
                "accepted",
                "",
            )],
            vec![],
        )
        .expect("batch response");

        assert!(!response.accepted);
        assert!(response.command_acks.is_empty());
    }

    #[test]
    fn expected_command_ack_returns_matching_envelope() {
        let response = ClientCommandBatchResponse {
            accepted: true,
            command_acks: vec![
                ClientCommandAckEnvelope {
                    command_kind: ClientCommandKind::SetSessionMode,
                    ack: ack(MUTATION_MODE, true, 41),
                    preset: "await-reply".to_owned(),
                    dispatch_kind: String::new(),
                    prompt_id: String::new(),
                    notification_id: String::new(),
                },
                ClientCommandAckEnvelope {
                    command_kind: ClientCommandKind::SendSessionPrompt,
                    ack: ack(MUTATION_PROMPT, true, 42),
                    preset: String::new(),
                    dispatch_kind: "accepted".to_owned(),
                    prompt_id: "prompt-1".to_owned(),
                    notification_id: String::new(),
                },
            ],
        };

        let envelope = reduce_expected_command_ack(
            response,
            ClientCommandKind::SendSessionPrompt,
            MUTATION_PROMPT.to_owned(),
        )
        .expect("expected ack");

        assert_eq!(envelope.command_kind, ClientCommandKind::SendSessionPrompt);
        assert_eq!(envelope.ack.client_mutation_id, MUTATION_PROMPT);
        assert_eq!(envelope.dispatch_kind, "accepted");
    }

    #[test]
    fn expected_command_ack_rejects_missing_ack() {
        let error = reduce_expected_command_ack(
            ClientCommandBatchResponse {
                accepted: false,
                command_acks: vec![ClientCommandAckEnvelope {
                    command_kind: ClientCommandKind::SetSessionMode,
                    ack: ack(MUTATION_MODE, true, 41),
                    preset: String::new(),
                    dispatch_kind: String::new(),
                    prompt_id: String::new(),
                    notification_id: String::new(),
                }],
            },
            ClientCommandKind::SendSessionPrompt,
            MUTATION_PROMPT.to_owned(),
        )
        .expect_err("missing ack");

        assert_eq!(error, ClientCoreError::MissingCommandAcknowledgement);
    }

    fn command_metadata(
        command_kind: ClientCommandKind,
        client_mutation_id: &str,
        preset: &str,
        dispatch_kind: &str,
        notification_id: &str,
    ) -> ClientCommandMetadata {
        ClientCommandMetadata {
            command_kind,
            client_mutation_id: client_mutation_id.to_owned(),
            preset: preset.to_owned(),
            dispatch_kind: dispatch_kind.to_owned(),
            notification_id: notification_id.to_owned(),
        }
    }

    fn ack(client_mutation_id: &str, accepted: bool, ack_seq: i64) -> ClientCommandAck {
        ClientCommandAck {
            accepted,
            account_id: "local-account".to_owned(),
            node_id: "local-node".to_owned(),
            client_mutation_id: client_mutation_id.to_owned(),
            ack_seq,
            entity_id: THREAD_ID.to_owned(),
            revision: format!("rev-{ack_seq}"),
            server_time: "2026-06-25T00:00:00Z".to_owned(),
            idempotent_replay: false,
            error_code: String::new(),
            reject_reason: String::new(),
            current_state: String::new(),
        }
    }
}

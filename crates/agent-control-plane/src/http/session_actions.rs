use crate::control_plane::ControlPlane;
use crate::grpc::{delete_session_command, mute_session_command, set_session_archived_command};
use tonic::Status;

const HTTP_ARCHIVE_MUTATION_PREFIX: &str = "http-session-archive";
const HTTP_DELETE_MUTATION_PREFIX: &str = "http-session-delete";
const HTTP_MUTE_MUTATION_PREFIX: &str = "http-session-mute";

pub(super) fn set_session_archived(
    control_plane: &ControlPlane,
    thread_id: &str,
    archived: bool,
) -> Result<(), Status> {
    set_session_archived_command(
        control_plane,
        thread_id.to_owned(),
        archived,
        &http_session_mutation_id(HTTP_ARCHIVE_MUTATION_PREFIX),
    )
    .map(|_| ())
}

pub(super) fn mute_session(control_plane: &ControlPlane, thread_id: &str) -> Result<(), Status> {
    mute_session_command(
        control_plane,
        thread_id.to_owned(),
        &http_session_mutation_id(HTTP_MUTE_MUTATION_PREFIX),
    )
    .map(|_| ())
}

pub(super) fn delete_session(control_plane: &ControlPlane, thread_id: &str) -> Result<(), Status> {
    delete_session_command(
        control_plane,
        thread_id.to_owned(),
        &http_session_mutation_id(HTTP_DELETE_MUTATION_PREFIX),
    )
    .map(|_| ())
}

fn http_session_mutation_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

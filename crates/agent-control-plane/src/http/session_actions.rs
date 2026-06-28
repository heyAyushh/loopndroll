use crate::control_plane::ControlPlane;
use crate::mobile::realtime_commands::{
    delete_completion_check_command, delete_notification_route_command, delete_session_command,
    mute_session_command, save_default_prompt_command, set_assistant_surface_command,
    set_default_notification_targets_command, set_global_completion_check_command,
    set_global_notification_command, set_global_preset_command, set_scope_command,
    set_session_archived_command, set_session_completion_check_command,
    set_session_notifications_command, upsert_completion_check_command,
    upsert_notification_route_command,
};
use crate::mobile::session::UpsertMobileNotificationRoute;
use tonic::Status;

const HTTP_ARCHIVE_MUTATION_PREFIX: &str = "http-session-archive";
const HTTP_DELETE_MUTATION_PREFIX: &str = "http-session-delete";
const HTTP_MUTE_MUTATION_PREFIX: &str = "http-session-mute";
const HTTP_DEFAULT_PROMPT_MUTATION_PREFIX: &str = "http-default-prompt";
const HTTP_SCOPE_MUTATION_PREFIX: &str = "http-scope";
const HTTP_ASSISTANT_SURFACE_MUTATION_PREFIX: &str = "http-assistant-surface";
const HTTP_GLOBAL_PRESET_MUTATION_PREFIX: &str = "http-global-preset";
const HTTP_GLOBAL_NOTIFICATION_MUTATION_PREFIX: &str = "http-global-notification";
const HTTP_DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX: &str = "http-default-notification-targets";
const HTTP_GLOBAL_COMPLETION_CHECK_MUTATION_PREFIX: &str = "http-global-completion-check";
const HTTP_UPSERT_NOTIFICATION_ROUTE_MUTATION_PREFIX: &str = "http-upsert-notification-route";
const HTTP_DELETE_NOTIFICATION_ROUTE_MUTATION_PREFIX: &str = "http-delete-notification-route";
const HTTP_UPSERT_COMPLETION_CHECK_MUTATION_PREFIX: &str = "http-upsert-completion-check";
const HTTP_DELETE_COMPLETION_CHECK_MUTATION_PREFIX: &str = "http-delete-completion-check";
const HTTP_SESSION_NOTIFICATIONS_MUTATION_PREFIX: &str = "http-session-notifications";
const HTTP_SESSION_COMPLETION_CHECK_MUTATION_PREFIX: &str = "http-session-completion-check";

pub(super) fn save_default_prompt(
    control_plane: &ControlPlane,
    prompt: String,
) -> Result<(), Status> {
    save_default_prompt_command(
        control_plane,
        prompt,
        &http_session_mutation_id(HTTP_DEFAULT_PROMPT_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_scope(control_plane: &ControlPlane, scope: String) -> Result<(), Status> {
    set_scope_command(
        control_plane,
        scope,
        &http_session_mutation_id(HTTP_SCOPE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_assistant_surface(
    control_plane: &ControlPlane,
    assistant_surface: String,
) -> Result<(), Status> {
    set_assistant_surface_command(
        control_plane,
        assistant_surface,
        &http_session_mutation_id(HTTP_ASSISTANT_SURFACE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_global_preset(
    control_plane: &ControlPlane,
    preset: Option<String>,
) -> Result<(), Status> {
    set_global_preset_command(
        control_plane,
        preset,
        &http_session_mutation_id(HTTP_GLOBAL_PRESET_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_global_notification(
    control_plane: &ControlPlane,
    notification_id: Option<String>,
) -> Result<(), Status> {
    set_global_notification_command(
        control_plane,
        notification_id,
        &http_session_mutation_id(HTTP_GLOBAL_NOTIFICATION_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_default_notification_targets(
    control_plane: &ControlPlane,
    notification_target_ids: Vec<String>,
) -> Result<(), Status> {
    set_default_notification_targets_command(
        control_plane,
        notification_target_ids,
        &http_session_mutation_id(HTTP_DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_global_completion_check(
    control_plane: &ControlPlane,
    completion_check_id: Option<String>,
    wait_for_reply_after_completion: bool,
) -> Result<(), Status> {
    set_global_completion_check_command(
        control_plane,
        completion_check_id,
        wait_for_reply_after_completion,
        &http_session_mutation_id(HTTP_GLOBAL_COMPLETION_CHECK_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn upsert_notification_route(
    control_plane: &ControlPlane,
    input: UpsertMobileNotificationRoute,
) -> Result<(), Status> {
    upsert_notification_route_command(
        control_plane,
        input,
        &http_session_mutation_id(HTTP_UPSERT_NOTIFICATION_ROUTE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn delete_notification_route(
    control_plane: &ControlPlane,
    notification_id: String,
) -> Result<(), Status> {
    delete_notification_route_command(
        control_plane,
        notification_id,
        &http_session_mutation_id(HTTP_DELETE_NOTIFICATION_ROUTE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn upsert_completion_check(
    control_plane: &ControlPlane,
    completion_check_id: String,
    label: String,
    commands: Vec<String>,
) -> Result<(), Status> {
    upsert_completion_check_command(
        control_plane,
        completion_check_id,
        label,
        commands,
        &http_session_mutation_id(HTTP_UPSERT_COMPLETION_CHECK_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn delete_completion_check(
    control_plane: &ControlPlane,
    completion_check_id: String,
) -> Result<(), Status> {
    delete_completion_check_command(
        control_plane,
        completion_check_id,
        &http_session_mutation_id(HTTP_DELETE_COMPLETION_CHECK_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_session_notifications(
    control_plane: &ControlPlane,
    thread_id: String,
    notification_ids: Vec<String>,
) -> Result<(), Status> {
    set_session_notifications_command(
        control_plane,
        thread_id,
        notification_ids,
        &http_session_mutation_id(HTTP_SESSION_NOTIFICATIONS_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn set_session_completion_check(
    control_plane: &ControlPlane,
    thread_id: String,
    completion_check_id: Option<String>,
    wait_for_reply_after_completion: bool,
) -> Result<(), Status> {
    set_session_completion_check_command(
        control_plane,
        thread_id,
        completion_check_id,
        wait_for_reply_after_completion,
        &http_session_mutation_id(HTTP_SESSION_COMPLETION_CHECK_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

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
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn mute_session(control_plane: &ControlPlane, thread_id: &str) -> Result<(), Status> {
    mute_session_command(
        control_plane,
        thread_id.to_owned(),
        &http_session_mutation_id(HTTP_MUTE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

pub(super) fn delete_session(control_plane: &ControlPlane, thread_id: &str) -> Result<(), Status> {
    delete_session_command(
        control_plane,
        thread_id.to_owned(),
        &http_session_mutation_id(HTTP_DELETE_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

fn http_session_mutation_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

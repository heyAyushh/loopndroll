use crate::control_plane::ControlPlane;
use crate::mobile::session::MobileSessionError;

use super::mobile_state::{emit_mobile_lifecycle_changed, emit_mobile_session_changed};

pub(super) fn set_session_mode(
    control_plane: &ControlPlane,
    thread_id: &str,
    preset: Option<&str>,
) -> Result<(), MobileSessionError> {
    control_plane
        .mobile_session_service()
        .set_session_preset(thread_id, preset)?;
    emit_mobile_lifecycle_changed(control_plane, thread_id, preset.or(Some("mode-cleared")));
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("mode-updated"));
    Ok(())
}

pub(super) fn set_session_archived(
    control_plane: &ControlPlane,
    thread_id: &str,
    archived: bool,
) -> Result<(), MobileSessionError> {
    control_plane
        .mobile_session_service()
        .set_session_archived(thread_id, archived)?;
    emit_mobile_session_changed(
        control_plane,
        Some(thread_id),
        Some(if archived { "archived" } else { "unarchived" }),
    );
    Ok(())
}

pub(super) fn mute_session(
    control_plane: &ControlPlane,
    thread_id: &str,
) -> Result<(), MobileSessionError> {
    control_plane
        .mobile_session_service()
        .mute_session(thread_id)?;
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("muted"));
    Ok(())
}

pub(super) fn delete_session(
    control_plane: &ControlPlane,
    thread_id: &str,
) -> Result<(), MobileSessionError> {
    control_plane
        .mobile_session_service()
        .delete_session(thread_id)?;
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("deleted"));
    Ok(())
}

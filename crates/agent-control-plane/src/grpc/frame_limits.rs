use prost::Message;
use tonic::Status;

use crate::grpc::proto;

const BYTES_PER_KIB: usize = 1024;
const SESSION_STATE_DELTA_FRAME_OVERHEAD_RESERVE_BYTES: usize = 4 * BYTES_PER_KIB;

pub(crate) const SESSION_CONTROL_FRAME_MAX_BYTES: usize = 512 * BYTES_PER_KIB;
pub(crate) const SESSION_STATE_DELTA_REPLACEMENT_CHUNK_MAX_BYTES: usize =
    SESSION_CONTROL_FRAME_MAX_BYTES - SESSION_STATE_DELTA_FRAME_OVERHEAD_RESERVE_BYTES;
pub(crate) const SESSION_COMMAND_TEXT_MAX_BYTES: usize = 64 * BYTES_PER_KIB;
pub(crate) const SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES: usize = 64 * BYTES_PER_KIB;
pub(crate) const SESSION_CONTROL_TEXT_MAX_CHARS: usize = 512;

pub(crate) fn ensure_client_frame_size(frame: &proto::ClientFrame) -> Result<(), Status> {
    ensure_encoded_message_size("ClientFrame", frame)
}

pub(crate) fn ensure_server_frame_size(frame: &proto::ServerFrame) -> Result<(), Status> {
    if let Some(proto::server_frame::Frame::TextChunk(text_chunk)) = &frame.frame {
        ensure_text_chunk_content_size(&text_chunk.content)?;
    }
    ensure_encoded_message_size("ServerFrame", frame)
}

pub(crate) fn ensure_command_text_size(field_name: &str, value: &str) -> Result<(), Status> {
    ensure_control_text_size(field_name, value, SESSION_COMMAND_TEXT_MAX_BYTES)
}

pub(crate) fn ensure_text_chunk_content_size(value: &str) -> Result<(), Status> {
    ensure_control_text_size(
        "text chunk content",
        value,
        SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES,
    )
}

fn ensure_control_text_size(field_name: &str, value: &str, max_bytes: usize) -> Result<(), Status> {
    let byte_count = value.len();
    if byte_count > max_bytes {
        return Err(Status::resource_exhausted(format!(
            "{field_name} exceeded {max_bytes} byte control-frame cap"
        )));
    }
    Ok(())
}

pub(crate) fn ensure_command_text_list_size(
    field_name: &str,
    values: &[String],
) -> Result<(), Status> {
    for value in values {
        ensure_command_text_size(field_name, value)?;
    }
    Ok(())
}

pub(crate) fn truncate_control_text(value: &str) -> String {
    truncate_chars(value, SESSION_CONTROL_TEXT_MAX_CHARS)
}

fn ensure_encoded_message_size<M: Message>(frame_name: &str, message: &M) -> Result<(), Status> {
    let byte_count = message.encoded_len();
    if byte_count > SESSION_CONTROL_FRAME_MAX_BYTES {
        return Err(Status::resource_exhausted(format!(
            "{frame_name} exceeded {SESSION_CONTROL_FRAME_MAX_BYTES} byte Session control-frame cap; recover through snapshot or data-plane fetch"
        )));
    }
    Ok(())
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.char_indices();
    let Some((end, _)) = chars.nth(max_chars) else {
        return value.to_owned();
    };
    let mut truncated = value[..end].to_owned();
    truncated.push_str("...");
    truncated
}

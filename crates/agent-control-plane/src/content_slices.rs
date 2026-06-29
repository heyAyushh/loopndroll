use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::codex::read_state;

pub const DEFAULT_CONTENT_CHUNK_BYTES: usize = 64 * 1_024;
pub const MAX_CONTENT_CHUNK_BYTES: usize = 512 * 1_024;

const LOCAL_ACCOUNT_ID: &str = "local-account";
const LOCAL_NODE_ID: &str = "local-node";
const CONTENT_TYPE_TRANSCRIPT: &str = "transcript";
const RANGE_TAIL: &str = "tail";
const RANGE_AFTER: &str = "after";
const CURSOR_AFTER_PREFIX: &str = "after";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionContentSliceRequest<'a> {
    pub session_id: &'a str,
    pub range: Option<&'a str>,
    pub limit: Option<usize>,
    pub cursor: Option<&'a str>,
    pub revision: Option<&'a str>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SessionContentSliceResponse {
    pub content_type: String,
    pub supported_ranges: Vec<String>,
    pub chunk: ContentChunk,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ContentChunk {
    pub account_id: String,
    pub node_id: String,
    pub session_id: String,
    pub revision: String,
    pub offset: u64,
    pub length: usize,
    pub sha256: String,
    pub next_cursor: String,
    pub merkle_root: Option<String>,
    pub merkle_proof: Option<Vec<String>>,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentSliceError {
    SessionNotFound,
    TranscriptUnavailable,
    InvalidLimit {
        requested: Option<usize>,
        max: usize,
    },
    InvalidCursor,
    UnsupportedRange(String),
    StaleRevision {
        requested: String,
        current: String,
    },
    Filesystem(String),
}

pub fn session_content_slice(
    codex_home: &Path,
    request: SessionContentSliceRequest<'_>,
) -> Result<SessionContentSliceResponse, ContentSliceError> {
    let thread = read_state(codex_home)
        .map_err(|error| ContentSliceError::Filesystem(error.to_string()))?
        .threads
        .into_iter()
        .find(|thread| thread.thread_id == request.session_id)
        .ok_or(ContentSliceError::SessionNotFound)?;
    let transcript_path = thread
        .transcript_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .ok_or(ContentSliceError::TranscriptUnavailable)?;

    transcript_content_slice(&transcript_path, request)
}

fn transcript_content_slice(
    transcript_path: &Path,
    request: SessionContentSliceRequest<'_>,
) -> Result<SessionContentSliceResponse, ContentSliceError> {
    let limit = bounded_limit(request.limit)?;
    let range = request.range.unwrap_or(RANGE_TAIL);
    let metadata =
        std::fs::metadata(transcript_path).map_err(|error| filesystem_error(error, "metadata"))?;
    let content_len = metadata.len();
    let revision = transcript_revision(content_len, modified_at_ms(&metadata));

    if let Some(requested_revision) = request.revision
        && requested_revision != revision
    {
        return Err(ContentSliceError::StaleRevision {
            requested: requested_revision.to_owned(),
            current: revision,
        });
    }

    let offset = match range {
        RANGE_TAIL => content_len.saturating_sub(limit as u64),
        RANGE_AFTER => {
            let cursor =
                parse_after_cursor(request.cursor.ok_or(ContentSliceError::InvalidCursor)?)
                    .ok_or(ContentSliceError::InvalidCursor)?;
            if cursor.revision != revision {
                return Err(ContentSliceError::StaleRevision {
                    requested: cursor.revision,
                    current: revision,
                });
            }
            if cursor.offset > content_len {
                return Err(ContentSliceError::InvalidCursor);
            }
            cursor.offset
        }
        other => return Err(ContentSliceError::UnsupportedRange(other.to_owned())),
    };
    let bytes = read_content_chunk(transcript_path, offset, limit)?;
    let next_offset = offset + bytes.len() as u64;
    let next_cursor = after_cursor(&revision, next_offset);
    let content = String::from_utf8(bytes.clone())
        .map_err(|error| ContentSliceError::Filesystem(error.to_string()))?;

    Ok(SessionContentSliceResponse {
        content_type: CONTENT_TYPE_TRANSCRIPT.to_owned(),
        supported_ranges: vec![RANGE_TAIL.to_owned(), RANGE_AFTER.to_owned()],
        chunk: ContentChunk {
            account_id: LOCAL_ACCOUNT_ID.to_owned(),
            node_id: LOCAL_NODE_ID.to_owned(),
            session_id: request.session_id.to_owned(),
            revision,
            offset,
            length: bytes.len(),
            sha256: sha256_hex(&bytes),
            next_cursor,
            merkle_root: None,
            merkle_proof: None,
            content,
        },
    })
}

fn bounded_limit(limit: Option<usize>) -> Result<usize, ContentSliceError> {
    let limit = limit.unwrap_or(DEFAULT_CONTENT_CHUNK_BYTES);
    if limit == 0 || limit > MAX_CONTENT_CHUNK_BYTES {
        return Err(ContentSliceError::InvalidLimit {
            requested: Some(limit),
            max: MAX_CONTENT_CHUNK_BYTES,
        });
    }
    Ok(limit)
}

fn read_content_chunk(
    transcript_path: &Path,
    offset: u64,
    limit: usize,
) -> Result<Vec<u8>, ContentSliceError> {
    let mut file =
        File::open(transcript_path).map_err(|error| filesystem_error(error, "open transcript"))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| filesystem_error(error, "seek transcript"))?;
    let mut bounded = file.take(limit as u64);
    let mut bytes = Vec::with_capacity(limit);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| filesystem_error(error, "read transcript"))?;
    Ok(bytes)
}

fn transcript_revision(content_len: u64, modified_at_ms: u128) -> String {
    format!("transcript:{content_len}:{modified_at_ms}")
}

fn modified_at_ms(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn after_cursor(revision: &str, offset: u64) -> String {
    format!("{CURSOR_AFTER_PREFIX}:{revision}:{offset}")
}

struct AfterCursor {
    revision: String,
    offset: u64,
}

fn parse_after_cursor(cursor: &str) -> Option<AfterCursor> {
    let (prefix, remainder) = cursor.split_once(':')?;
    if prefix != CURSOR_AFTER_PREFIX {
        return None;
    }
    let (revision_prefix, offset) = remainder.rsplit_once(':')?;
    Some(AfterCursor {
        revision: revision_prefix.to_owned(),
        offset: offset.parse().ok()?,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn filesystem_error(error: std::io::Error, action: &str) -> ContentSliceError {
    ContentSliceError::Filesystem(format!("{action}: {error}"))
}

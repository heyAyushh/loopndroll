use serde_json::{Value, json};

use crate::assistant::AssistantKind;
use crate::control_plane::DesktopThread;

const TRANSCRIPT_SOURCE_KIND: &str = "transcript";
const TRANSCRIPT_SOURCE_LABEL: &str = "Transcript";

pub(super) fn session_supports_subagents(thread: &DesktopThread) -> bool {
    matches!(thread.capabilities.assistant_kind, AssistantKind::Codex)
}

pub(super) fn project_name_from_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_owned()
}

pub(super) fn git_repository(thread: &DesktopThread) -> Value {
    let Some(repository_path) = thread.cwd.as_deref() else {
        return Value::Null;
    };
    if thread.git_branch.is_none() && thread.git_sha.is_none() {
        return Value::Null;
    }
    json!({
        "repositoryName": project_name_from_path(repository_path),
        "repositoryPath": repository_path,
        "remoteURL": null,
        "branch": thread.git_branch,
        "commit": thread.git_sha,
    })
}

pub(super) fn session_sources(thread: &DesktopThread) -> Vec<Value> {
    let mut sources = thread
        .cwd
        .as_deref()
        .map(|cwd| {
            vec![json!({
                "kind": "cwd",
                "label": "Working Directory",
                "value": cwd,
                "url": null,
            })]
        })
        .unwrap_or_default();
    if let Some(transcript_path) = thread.transcript_path.as_deref() {
        sources.push(json!({
            "kind": TRANSCRIPT_SOURCE_KIND,
            "label": TRANSCRIPT_SOURCE_LABEL,
            "value": transcript_path,
            "url": null,
        }));
    }
    sources
}

pub(super) fn session_tags(kind: &str, source_display_name: &str) -> Vec<String> {
    vec![kind.to_owned(), source_display_name.to_owned()]
}

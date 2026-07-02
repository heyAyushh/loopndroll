use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;

use crate::control_plane::{ControlPlane, DesktopThread};

use super::mobile_access::request_advertised_mobile_base_urls;

pub(super) async fn handoff_session_page(
    State(control_plane): State<ControlPlane>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let snapshot = match control_plane.desktop_handoff_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                )],
                error.to_string(),
            )
                .into_response();
        }
    };

    let Some(thread) = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id && !thread.archived)
        .or_else(|| {
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == thread_id)
        })
    else {
        return (
            StatusCode::NOT_FOUND,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            "Session not found".to_owned(),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        handoff_session_html(
            thread,
            request_advertised_mobile_base_urls(&headers)
                .first()
                .map(String::as_str),
        ),
    )
        .into_response()
}

fn handoff_session_html(thread: &DesktopThread, handoff_base_url: Option<&str>) -> String {
    let title = handoff_session_title(thread);
    let subtitle = thread
        .cwd
        .as_deref()
        .map(handoff_project_name)
        .unwrap_or_else(|| "Looper session".to_owned());
    let preview = thread
        .assistant_preview
        .as_deref()
        .map(str::trim)
        .filter(|preview| !preview.is_empty())
        .unwrap_or("Open this session in Looper on your iPhone.");
    let deep_link = handoff_deep_link(&thread.thread_id, handoff_base_url);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }}
main {{ width: min(34rem, calc(100vw - 2rem)); }}
h1 {{ font-size: 1.35rem; line-height: 1.2; margin: 0 0 .5rem; }}
p {{ color: color-mix(in srgb, CanvasText 72%, transparent); line-height: 1.45; }}
a {{ display: inline-block; margin-top: 1rem; padding: .7rem .95rem; border-radius: .75rem; background: LinkText; color: Canvas; text-decoration: none; font-weight: 650; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p>{subtitle}</p>
<p>{preview}</p>
<a href="{deep_link}">Open in looper</a>
</main>
</body>
</html>"#,
        title = html_escaped_text(&title),
        subtitle = html_escaped_text(&subtitle),
        preview = html_escaped_text(preview),
        deep_link = html_escaped_attribute(&deep_link)
    )
}

fn handoff_deep_link(thread_id: &str, handoff_base_url: Option<&str>) -> String {
    let encoded_thread_id = percent_encoded_path_segment(thread_id);
    let Some(handoff_base_url) = handoff_base_url else {
        return format!("looper://session/{encoded_thread_id}");
    };

    format!(
        "looper://session/{encoded_thread_id}?baseURL={}",
        percent_encoded_url_component(handoff_base_url)
    )
}

fn handoff_session_title(thread: &DesktopThread) -> String {
    thread
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(&thread.thread_id)
        .to_owned()
}

fn handoff_project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn html_escaped_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escaped_attribute(value: &str) -> String {
    html_escaped_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn percent_encoded_path_segment(value: &str) -> String {
    percent_encoded_url_component(value)
}

fn percent_encoded_url_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::AssistantKind;
    use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};

    fn thread_for_handoff() -> DesktopThread {
        DesktopThread {
            thread_id: "thread/main id".to_owned(),
            title: Some("A&B <handoff>".to_owned()),
            cwd: Some("/Users/test/looper".to_owned()),
            transcript_path: None,
            source: None,
            originator: None,
            model: None,
            reasoning_effort: None,
            git_sha: None,
            git_branch: None,
            cli_version: None,
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
            created_at_ms: None,
            updated_at_ms: None,
            latest_message_at_ms: None,
            assistant_preview: Some("Use \"quote\" & less <html>".to_owned()),
            first_user_prompt: None,
            runtime_status: None,
            goal: None,
            capabilities: ThreadCapabilities {
                thread_id: "thread/main id".to_owned(),
                assistant_kind: AssistantKind::Codex,
                tools: Vec::new(),
                mcp_tools: Vec::new(),
                app_tools: Vec::new(),
                automation_tools: Vec::new(),
                spawn: SpawnGraph {
                    parent_thread_id: None,
                    root_thread_id: "thread/main id".to_owned(),
                    children: Vec::new(),
                    launch_kind: LaunchKind::Main,
                },
                diff: DiffSummary {
                    git_branch: None,
                    git_sha: None,
                    produced_file_changes: false,
                    paths: Vec::new(),
                },
                agent_nickname: None,
                agent_role: None,
                agent_path: None,
            },
            archived: false,
        }
    }

    #[test]
    fn handoff_html_escapes_visible_text_and_deep_link() {
        let html = handoff_session_html(&thread_for_handoff(), Some("http://127.0.0.1:8765/a b"));

        assert!(html.contains("A&amp;B &lt;handoff&gt;"));
        assert!(html.contains("Use \"quote\" &amp; less &lt;html&gt;"));
        assert!(html.contains(
            "looper://session/thread%2Fmain%20id?baseURL=http%3A%2F%2F127.0.0.1%3A8765%2Fa%20b"
        ));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicThreadId {
    Claude {
        session_id: String,
    },
    Devin {
        agent_id: String,
        session_id: String,
    },
    Zed {
        agent_id: String,
        session_id: String,
    },
    Raw {
        session_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevinThreadIdentity {
    pub provider_id: String,
    pub session_id: String,
}

struct ZedPublicAgentMapping {
    client_id: &'static str,
    internal_agent_id: &'static str,
    public_agent_id: &'static str,
}

const CLAUDE_THREAD_PREFIX: &str = "claude:";
const DEVIN_THREAD_ID_PREFIX: &str = "devin";
const DEVIN_LOCAL_PROVIDER_ID: &str = "devin-cli";
const DEVIN_ACP_SESSION_PREFIX: &str = "acp/";
const LOOPER_ACP_AGENT_ID: &str = "looper";
const LOOPER_SESSION_PREFIX: &str = "acp/looper/";
const ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS: &str = "zed";
const ZED_CODEX_ACP_AGENT_ID: &str = "codex-acp";
const ZED_CODEX_DIRECT_AGENT_ID: &str = "codex";
const ZED_CODEX_DIRECT_PUBLIC_AGENT_ID: &str = "codex-direct";
const ZED_CODEX_PUBLIC_AGENT_ID: &str = "codex";

/// Public Zed thread IDs intentionally expose Codex-facing agent names instead of
/// the internal ACP target IDs used by the runtime.
const ZED_PUBLIC_AGENT_MAPPINGS: &[ZedPublicAgentMapping] = &[
    ZedPublicAgentMapping {
        client_id: ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS,
        internal_agent_id: ZED_CODEX_ACP_AGENT_ID,
        public_agent_id: ZED_CODEX_PUBLIC_AGENT_ID,
    },
    ZedPublicAgentMapping {
        client_id: ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS,
        internal_agent_id: ZED_CODEX_DIRECT_AGENT_ID,
        public_agent_id: ZED_CODEX_DIRECT_PUBLIC_AGENT_ID,
    },
];

pub fn parse_public_thread_id(thread_id: &str) -> PublicThreadId {
    let thread_id = thread_id.trim();
    if let Some(session_id) = thread_id.strip_prefix(CLAUDE_THREAD_PREFIX) {
        return PublicThreadId::Claude {
            session_id: session_id.replace(':', "/"),
        };
    }
    if let Some(identity) = devin_thread_identity_from_public_thread_id(thread_id) {
        return PublicThreadId::Devin {
            agent_id: identity.provider_id,
            session_id: identity.session_id,
        };
    }
    if let Some((agent_id, session_id)) = acp_agent_and_session_id_for_client_public_thread_id(
        ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS,
        thread_id,
    ) {
        return PublicThreadId::Zed {
            agent_id,
            session_id,
        };
    }
    PublicThreadId::Raw {
        session_id: thread_id.to_owned(),
    }
}

pub fn public_thread_id_for_claude_session(session_id: &str) -> String {
    let session_id = session_id.trim();
    if session_id.starts_with(CLAUDE_THREAD_PREFIX) {
        return session_id.to_owned();
    }
    format!("claude:{}", session_id.replace('/', ":"))
}

pub fn claude_session_id_from_public_thread_id(thread_id: &str) -> String {
    let thread_id = thread_id.trim();
    thread_id
        .strip_prefix(CLAUDE_THREAD_PREFIX)
        .unwrap_or(thread_id)
        .replace(':', "/")
}

pub fn public_thread_id_for_codex_session(session_id: &str) -> String {
    public_thread_id_for_raw_session(session_id)
}

pub fn codex_session_id_from_public_thread_id(thread_id: &str) -> String {
    public_thread_id_for_raw_session(thread_id)
}

pub fn public_thread_id_for_grok_session(session_id: &str) -> String {
    public_thread_id_for_raw_session(session_id)
}

pub fn grok_session_id_from_public_thread_id(thread_id: &str) -> String {
    public_thread_id_for_raw_session(thread_id)
}

pub fn public_thread_id_for_raw_session(session_id: &str) -> String {
    session_id.trim().to_owned()
}

pub fn public_thread_id_for_client_acp_session(client_id: &str, session_id: &str) -> String {
    public_thread_id_for_client_agent_acp_session(client_id, LOOPER_ACP_AGENT_ID, session_id)
}

pub fn public_thread_id_for_client_agent_acp_session(
    client_id: &str,
    agent_id: &str,
    session_id: &str,
) -> String {
    let zed_looper_control_session =
        client_id == ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS && agent_id == LOOPER_ACP_AGENT_ID;
    let public_agent_id = if zed_looper_control_session {
        ZED_CODEX_PUBLIC_AGENT_ID.to_owned()
    } else {
        public_agent_id_for_client_agent_id(client_id, agent_id)
    };
    let normalized_session_id = if zed_looper_control_session {
        session_id
            .strip_prefix(DEVIN_ACP_SESSION_PREFIX)
            .unwrap_or(session_id)
    } else if agent_id == LOOPER_ACP_AGENT_ID {
        session_id
            .strip_prefix(LOOPER_SESSION_PREFIX)
            .unwrap_or(session_id)
    } else {
        session_id
            .strip_prefix(DEVIN_ACP_SESSION_PREFIX)
            .unwrap_or(session_id)
    }
    .replace('/', ":");
    format!("{client_id}:{public_agent_id}:{normalized_session_id}")
}

pub fn public_agent_id_for_client_agent_id(client_id: &str, agent_id: &str) -> String {
    public_agent_mapping_for_internal_agent(client_id, agent_id)
        .map(|mapping| mapping.public_agent_id.to_owned())
        .unwrap_or_else(|| agent_id.to_owned())
}

fn internal_agent_id_for_client_public_agent_id(client_id: &str, agent_id: &str) -> String {
    public_agent_mapping_for_public_agent(client_id, agent_id)
        .map(|mapping| mapping.internal_agent_id.to_owned())
        .unwrap_or_else(|| agent_id.to_owned())
}

fn public_agent_mapping_for_internal_agent(
    client_id: &str,
    agent_id: &str,
) -> Option<&'static ZedPublicAgentMapping> {
    ZED_PUBLIC_AGENT_MAPPINGS
        .iter()
        .find(|mapping| mapping.client_id == client_id && mapping.internal_agent_id == agent_id)
}

fn public_agent_mapping_for_public_agent(
    client_id: &str,
    agent_id: &str,
) -> Option<&'static ZedPublicAgentMapping> {
    ZED_PUBLIC_AGENT_MAPPINGS
        .iter()
        .find(|mapping| mapping.client_id == client_id && mapping.public_agent_id == agent_id)
}

pub fn acp_session_id_for_client_public_thread_id(
    client_id: &str,
    thread_id: &str,
) -> Option<String> {
    let legacy_prefix = format!("{client_id}:{LOOPER_ACP_AGENT_ID}:");
    if let Some(remainder) = thread_id.strip_prefix(&legacy_prefix) {
        return Some(looper_acp_session_id_from_public_remainder(remainder));
    }

    if client_id == ZED_CLIENT_ID_FOR_PUBLIC_AGENT_ALIAS {
        let zed_codex_looper_prefix =
            format!("{client_id}:{ZED_CODEX_PUBLIC_AGENT_ID}:{LOOPER_ACP_AGENT_ID}:");
        if let Some(remainder) = thread_id.strip_prefix(&zed_codex_looper_prefix) {
            return Some(looper_acp_session_id_from_public_remainder(remainder));
        }
    }

    None
}

fn looper_acp_session_id_from_public_remainder(remainder: &str) -> String {
    format!("{LOOPER_SESSION_PREFIX}{}", remainder.replace(':', "/"))
}

pub fn acp_agent_and_session_id_for_client_public_thread_id(
    client_id: &str,
    thread_id: &str,
) -> Option<(String, String)> {
    let prefix = format!("{client_id}:");
    let remainder = thread_id.strip_prefix(&prefix)?;
    let (public_agent_id, normalized_session_id) = remainder.split_once(':')?;
    let agent_id = internal_agent_id_for_client_public_agent_id(client_id, public_agent_id);
    let session_id = if agent_id == LOOPER_ACP_AGENT_ID {
        format!(
            "{LOOPER_SESSION_PREFIX}{}",
            normalized_session_id.replace(':', "/")
        )
    } else if let Some(looper_session_id) = normalized_session_id.strip_prefix("looper:") {
        format!(
            "{LOOPER_SESSION_PREFIX}{}",
            looper_session_id.replace(':', "/")
        )
    } else {
        normalized_session_id.replace(':', "/")
    };
    Some((agent_id, session_id))
}

pub fn public_thread_id_for_devin_session(session_id: &str) -> String {
    let session_id = session_id.trim();
    if session_id.starts_with("devin:") {
        return session_id.to_owned();
    }
    if let Some(local_session_id) = session_id.strip_prefix(DEVIN_ACP_SESSION_PREFIX)
        && let Some((provider_id, provider_session_id)) = local_session_id.split_once('/')
    {
        return public_thread_id_for_devin_agent_session(provider_id, provider_session_id);
    }
    public_thread_id_for_devin_agent_session(DEVIN_LOCAL_PROVIDER_ID, session_id)
}

pub fn public_thread_id_for_devin_metadata_session(session_id: &str, provider_id: &str) -> String {
    if is_codex_acp_provider(provider_id) {
        return codex_thread_id_for_devin_acp_session(session_id, provider_id);
    }

    public_thread_id_for_devin_session_id(session_id)
}

pub fn public_thread_id_for_devin_agent_session(agent_id: &str, session_id: &str) -> String {
    format!(
        "{DEVIN_THREAD_ID_PREFIX}:{agent_id}:{}",
        session_id.trim().replace('/', ":")
    )
}

fn codex_thread_id_for_devin_acp_session(session_id: &str, provider_id: &str) -> String {
    let without_acp_prefix = session_id
        .strip_prefix(DEVIN_ACP_SESSION_PREFIX)
        .unwrap_or(session_id);
    let without_provider_prefix = without_acp_prefix
        .strip_prefix(provider_id)
        .and_then(|value| value.strip_prefix('/'))
        .unwrap_or(without_acp_prefix);
    public_thread_id_for_codex_session(&without_provider_prefix.replace('/', ":"))
}

fn public_thread_id_for_devin_session_id(session_id: &str) -> String {
    let normalized_session_id = session_id
        .strip_prefix(DEVIN_ACP_SESSION_PREFIX)
        .unwrap_or(session_id)
        .replace('/', ":");
    format!("{DEVIN_THREAD_ID_PREFIX}:{normalized_session_id}")
}

fn is_codex_acp_provider(provider_id: &str) -> bool {
    matches!(provider_id.trim(), "codex" | "codex-acp")
}

pub fn devin_thread_identity_from_public_thread_id(thread_id: &str) -> Option<DevinThreadIdentity> {
    let remainder = thread_id.strip_prefix("devin:")?;
    let (provider_id, session_id) = remainder.split_once(':')?;
    let provider_id = non_empty_identity_segment(provider_id)?;
    let session_id = non_empty_identity_segment(&session_id.replace(':', "/"))?;
    Some(DevinThreadIdentity {
        provider_id,
        session_id,
    })
}

fn non_empty_identity_segment(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_claude_public_thread_ids() {
        let thread_id = public_thread_id_for_claude_session("session/path");

        assert_eq!(thread_id, "claude:session:path");
        assert_eq!(
            claude_session_id_from_public_thread_id(&thread_id),
            "session/path"
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Claude {
                session_id: "session/path".to_owned()
            }
        );
        assert_eq!(
            public_thread_id_for_claude_session(&thread_id),
            "claude:session:path"
        );
    }

    #[test]
    fn round_trips_devin_hook_public_thread_ids() {
        let thread_id = public_thread_id_for_devin_session("acp/codex-acp/session/path");

        assert_eq!(thread_id, "devin:codex-acp:session:path");
        assert_eq!(
            devin_thread_identity_from_public_thread_id(&thread_id),
            Some(DevinThreadIdentity {
                provider_id: "codex-acp".to_owned(),
                session_id: "session/path".to_owned(),
            })
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Devin {
                agent_id: "codex-acp".to_owned(),
                session_id: "session/path".to_owned(),
            }
        );
    }

    #[test]
    fn round_trips_devin_metadata_public_thread_ids() {
        let thread_id =
            public_thread_id_for_devin_metadata_session("acp/devin-cli/session/path", "devin-cli");

        assert_eq!(thread_id, "devin:devin-cli:session:path");
        assert_eq!(
            devin_thread_identity_from_public_thread_id(&thread_id),
            Some(DevinThreadIdentity {
                provider_id: "devin-cli".to_owned(),
                session_id: "session/path".to_owned(),
            })
        );
    }

    #[test]
    fn round_trips_devin_codex_metadata_as_raw_codex_thread_ids() {
        let thread_id =
            public_thread_id_for_devin_metadata_session("acp/codex/session/path", "codex");

        assert_eq!(thread_id, "session:path");
        assert_eq!(
            codex_session_id_from_public_thread_id(&thread_id),
            "session:path"
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Raw {
                session_id: "session:path".to_owned()
            }
        );
    }

    #[test]
    fn round_trips_devin_acp_public_thread_ids() {
        let thread_id = public_thread_id_for_client_acp_session("devin", "acp/looper/session/path");

        assert_eq!(thread_id, "devin:looper:session:path");
        assert_eq!(
            acp_session_id_for_client_public_thread_id("devin", &thread_id).as_deref(),
            Some("acp/looper/session/path")
        );
        assert_eq!(
            acp_agent_and_session_id_for_client_public_thread_id("devin", &thread_id),
            Some(("looper".to_owned(), "acp/looper/session/path".to_owned()))
        );
    }

    #[test]
    fn round_trips_zed_public_thread_ids() {
        let thread_id = public_thread_id_for_client_acp_session("zed", "acp/looper/session/path");

        assert_eq!(thread_id, "zed:codex:looper:session:path");
        assert_eq!(
            acp_session_id_for_client_public_thread_id("zed", &thread_id).as_deref(),
            Some("acp/looper/session/path")
        );
        assert_eq!(
            acp_agent_and_session_id_for_client_public_thread_id("zed", &thread_id),
            Some(("codex-acp".to_owned(), "acp/looper/session/path".to_owned()))
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Zed {
                agent_id: "codex-acp".to_owned(),
                session_id: "acp/looper/session/path".to_owned(),
            }
        );
    }

    #[test]
    fn maps_zed_codex_acp_targets_through_documented_public_agent_table() {
        assert_eq!(
            public_agent_id_for_client_agent_id("zed", "codex-acp"),
            "codex"
        );
        assert_eq!(
            public_thread_id_for_client_agent_acp_session("zed", "codex-acp", "session-1"),
            "zed:codex:session-1"
        );
        assert_eq!(
            acp_agent_and_session_id_for_client_public_thread_id("zed", "zed:codex:session-1"),
            Some(("codex-acp".to_owned(), "session-1".to_owned()))
        );
        assert_eq!(
            public_agent_id_for_client_agent_id("zed", "codex"),
            "codex-direct"
        );
        assert_eq!(
            public_thread_id_for_client_agent_acp_session("zed", "codex", "session-1"),
            "zed:codex-direct:session-1"
        );
        assert_eq!(
            acp_agent_and_session_id_for_client_public_thread_id(
                "zed",
                "zed:codex-direct:session-1"
            ),
            Some(("codex".to_owned(), "session-1".to_owned()))
        );
        assert_eq!(
            public_agent_id_for_client_agent_id("devin", "codex-acp"),
            "codex-acp"
        );
    }

    #[test]
    fn round_trips_raw_codex_public_thread_ids() {
        let thread_id = public_thread_id_for_codex_session("thread/main");

        assert_eq!(thread_id, "thread/main");
        assert_eq!(
            codex_session_id_from_public_thread_id(&thread_id),
            "thread/main"
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Raw {
                session_id: "thread/main".to_owned()
            }
        );
    }

    #[test]
    fn round_trips_raw_grok_public_thread_ids() {
        let thread_id = public_thread_id_for_grok_session("grok-thread/main");

        assert_eq!(thread_id, "grok-thread/main");
        assert_eq!(
            grok_session_id_from_public_thread_id(&thread_id),
            "grok-thread/main"
        );
        assert_eq!(
            parse_public_thread_id(&thread_id),
            PublicThreadId::Raw {
                session_id: "grok-thread/main".to_owned()
            }
        );
    }
}

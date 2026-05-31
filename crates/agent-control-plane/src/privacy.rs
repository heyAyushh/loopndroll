const REDACTED_VALUE: &str = "<redacted>";

const SENSITIVE_KEY_NEEDLES: &[&str] = &[
    "api_key",
    "apikey",
    "auth",
    "bearer",
    "credential",
    "key",
    "password",
    "secret",
    "token",
];

pub fn redact_command_for_display(command: &str) -> String {
    command
        .split_whitespace()
        .map(redact_command_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_command_token(token: &str) -> String {
    if token.contains("://") && token.contains('?') {
        return redact_url_query(token);
    }

    let Some((key, _value)) = token.split_once('=') else {
        return token.to_owned();
    };

    if is_sensitive_key(key) {
        format!("{key}={REDACTED_VALUE}")
    } else {
        token.to_owned()
    }
}

fn redact_url_query(token: &str) -> String {
    let Some((base, query)) = token.split_once('?') else {
        return token.to_owned();
    };

    let redacted_query = query
        .split('&')
        .map(|part| {
            let Some((key, _value)) = part.split_once('=') else {
                return part.to_owned();
            };
            format!("{key}={REDACTED_VALUE}")
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{redacted_query}")
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    SENSITIVE_KEY_NEEDLES
        .iter()
        .any(|needle| normalized.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::redact_command_for_display;

    #[test]
    fn redacts_sensitive_flags_and_url_queries() {
        let command = "/opt/homebrew/bin/codex --api-key=secret -c server.url=http://localhost:1/mcp?token=abc&worktree=/tmp/private app-server";

        let redacted = redact_command_for_display(command);

        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("abc"));
        assert!(!redacted.contains("/tmp/private"));
        assert!(redacted.contains("--api-key=<redacted>"));
        assert!(redacted.contains("token=<redacted>"));
        assert!(redacted.contains("worktree=<redacted>"));
    }
}

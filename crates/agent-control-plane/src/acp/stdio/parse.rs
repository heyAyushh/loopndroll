use anyhow::{Result, bail};

const ACP_STDIO_PROXY_SEPARATOR: &str = "--";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProxyTarget {
    pub(super) agent_id: String,
    pub(super) command: String,
    pub(super) args: Vec<String>,
}

pub(super) fn parse_proxy_target(args: &[String]) -> Result<ProxyTarget> {
    let Some(agent_id) = args.first().filter(|value| !value.trim().is_empty()) else {
        bail!("usage: looper acp stdio zed <agent-id> -- <command> [args...]");
    };
    let Some(separator_index) = args
        .iter()
        .position(|value| value == ACP_STDIO_PROXY_SEPARATOR)
    else {
        bail!("usage: looper acp stdio zed <agent-id> -- <command> [args...]");
    };
    let Some(command) = args.get(separator_index + 1) else {
        bail!("usage: looper acp stdio zed <agent-id> -- <command> [args...]");
    };
    Ok(ProxyTarget {
        agent_id: agent_id.to_owned(),
        command: command.to_owned(),
        args: args[(separator_index + 2)..].to_vec(),
    })
}

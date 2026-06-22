use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::acp::runtime::LOOPER_ACP_AGENT_ID;

use super::settings::{read_json_object, selected_settings_path, write_json_document};
use super::{
    ZED_DEFAULT_CONTROLLED_AGENT_ID, ZED_DEFAULT_CONTROLLED_AGENT_NAME,
    ZED_EXTERNAL_AGENTS_REGISTRY_RELATIVE_PATH, ZED_LOOPER_ACP_COMMAND_ARGS,
    ZED_LOOPER_ACP_COMMAND_FALLBACK, ZED_PROXY_ARG_SEPARATOR, ZedAcpInstallResult,
};

pub fn install_looper_zed_acp_agent_for_home(home: &Path) -> Result<ZedAcpInstallResult> {
    let command = looper_command_path();
    install_looper_zed_acp_agent_for_home_with_command(home, command)
}

pub(super) fn install_looper_zed_acp_agent_for_home_with_command(
    home: &Path,
    command: String,
) -> Result<ZedAcpInstallResult> {
    let settings_path = selected_settings_path(home);
    let wrapped_command = installed_registry_agent_binary(home, ZED_DEFAULT_CONTROLLED_AGENT_ID)
        .unwrap_or_else(|| PathBuf::from(ZED_DEFAULT_CONTROLLED_AGENT_ID))
        .display()
        .to_string();
    let proxy_args = zed_looper_proxy_args(ZED_DEFAULT_CONTROLLED_AGENT_ID, &wrapped_command);
    upsert_looper_zed_acp_agent(
        &settings_path,
        ZED_DEFAULT_CONTROLLED_AGENT_ID,
        ZED_DEFAULT_CONTROLLED_AGENT_NAME,
        &command,
        &proxy_args,
    )?;
    Ok(ZedAcpInstallResult {
        installed_agent_id: ZED_DEFAULT_CONTROLLED_AGENT_ID.to_owned(),
        settings_path: settings_path.display().to_string(),
        command: command.clone(),
        args: proxy_args.clone(),
        wrapped_command: wrapped_command.clone(),
        command_line: format!("{} {}", command, proxy_args.join(" ")),
        preferred_agent: ZED_DEFAULT_CONTROLLED_AGENT_ID.to_owned(),
    })
}

fn upsert_looper_zed_acp_agent(
    path: &Path,
    agent_id: &str,
    agent_name: &str,
    command: &str,
    args: &[String],
) -> Result<()> {
    let mut settings = if path.exists() {
        read_json_object(path)
            .map_err(|error| anyhow!("cannot update Zed settings: {}", error.summary()))?
    } else {
        json!({})
    };
    let settings_object = settings
        .as_object_mut()
        .ok_or_else(|| anyhow!("Zed settings file is not a JSON object"))?;
    let agent_servers = settings_object
        .entry(super::ZED_AGENT_SERVERS_KEY.to_owned())
        .or_insert_with(|| json!({}));
    let agent_servers = agent_servers
        .as_object_mut()
        .ok_or_else(|| anyhow!("Zed agent_servers setting is not a JSON object"))?;
    agent_servers.remove(LOOPER_ACP_AGENT_ID);
    agent_servers.insert(
        agent_id.to_owned(),
        looper_zed_acp_agent(agent_name, command, args),
    );
    write_json_document(path, &settings)
}

fn looper_zed_acp_agent(agent_name: &str, command: &str, args: &[String]) -> Value {
    json!({
        "type": "custom",
        "name": agent_name,
        "command": command,
        "args": args,
        "env": {}
    })
}

fn zed_looper_proxy_args(agent_id: &str, wrapped_command: &str) -> Vec<String> {
    ZED_LOOPER_ACP_COMMAND_ARGS
        .iter()
        .map(|arg| (*arg).to_owned())
        .chain([
            agent_id.to_owned(),
            ZED_PROXY_ARG_SEPARATOR.to_owned(),
            wrapped_command.to_owned(),
        ])
        .collect()
}

fn installed_registry_agent_binary(home: &Path, agent_id: &str) -> Option<PathBuf> {
    let agent_dir = home
        .join(ZED_EXTERNAL_AGENTS_REGISTRY_RELATIVE_PATH)
        .join(agent_id);
    let mut candidates = fs::read_dir(agent_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join(agent_id))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn looper_command_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|current_exe| {
            let looper = current_exe.with_file_name(ZED_LOOPER_ACP_COMMAND_FALLBACK);
            looper.exists().then(|| looper.display().to_string())
        })
        .unwrap_or_else(|| ZED_LOOPER_ACP_COMMAND_FALLBACK.to_owned())
}

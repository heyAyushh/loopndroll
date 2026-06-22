use std::path::Path;

use serde_json::{Map, Value};

use crate::acp::runtime::{LOOPER_ACP_AGENT_ID, public_agent_id_for_client_agent_id};
use crate::acp::targets::{AcpTarget, inspect_launch_metadata};

use super::{
    ZED_ACP_TARGET_SOURCE, ZED_BLOCKED_STATUS, ZED_CLIENT_ID, ZED_CLIENT_NAME,
    ZED_LOOPER_ACP_COMMAND_ARGS, ZED_LOOPER_ACP_COMMAND_FALLBACK, ZED_LOOPER_MISSING_LAUNCH_DETAIL,
    ZED_MISSING_LAUNCH_DETAIL, ZED_NOT_RUNNING_DETAIL, ZED_PROXY_ARG_SEPARATOR,
    ZED_READ_ONLY_DETAIL, ZED_READ_ONLY_STATUS, ZED_READY_DETAIL, ZED_READY_STATUS, ZedAcpTarget,
    ZedStatus,
};

pub fn zed_acp_targets(status: &ZedStatus) -> Vec<AcpTarget> {
    status
        .acp_targets
        .iter()
        .map(|target| zed_acp_target(status, target))
        .collect()
}

pub(super) fn parse_zed_acp_target(id: &str, value: &Value) -> Option<ZedAcpTarget> {
    let object = value.as_object()?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_owned();
    let target_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let launch = inspect_launch_metadata(value);
    let looper_managed = zed_target_uses_looper_stdio(id, object);
    Some(ZedAcpTarget {
        id: id.to_owned(),
        name,
        target_type,
        launch_configured: launch.configured,
        looper_managed,
        launch,
    })
}

fn zed_acp_target(status: &ZedStatus, target: &ZedAcpTarget) -> AcpTarget {
    let runtime_ready = status.running && status.installed;
    let command_configured = zed_target_has_command_launch(target);
    let target_ready = target.looper_managed && command_configured && runtime_ready;
    let (ready, target_status, detail) = if target_ready {
        (true, ZED_READY_STATUS, ZED_READY_DETAIL)
    } else if target.looper_managed && !command_configured {
        (false, ZED_BLOCKED_STATUS, ZED_LOOPER_MISSING_LAUNCH_DETAIL)
    } else if target.looper_managed {
        (false, ZED_READ_ONLY_STATUS, ZED_NOT_RUNNING_DETAIL)
    } else if target.launch_configured {
        (false, ZED_READ_ONLY_STATUS, ZED_READ_ONLY_DETAIL)
    } else {
        (false, ZED_BLOCKED_STATUS, ZED_MISSING_LAUNCH_DETAIL)
    };
    let public_agent_id = public_agent_id_for_client_agent_id(ZED_CLIENT_ID, &target.id);
    AcpTarget {
        id: format!("{ZED_CLIENT_ID}:{public_agent_id}"),
        client: ZED_CLIENT_ID.to_owned(),
        client_name: ZED_CLIENT_NAME.to_owned(),
        agent_id: public_agent_id,
        name: target.name.clone(),
        source: ZED_ACP_TARGET_SOURCE.to_owned(),
        source_path: Some(status.settings_path.clone()),
        enabled: true,
        preferred: target.looper_managed,
        launch_configured: target.launch_configured,
        launch: target.launch.clone(),
        ready,
        status: target_status.to_owned(),
        detail: detail.to_owned(),
    }
}

fn zed_target_has_command_launch(target: &ZedAcpTarget) -> bool {
    target
        .launch
        .methods
        .iter()
        .any(|method| method == "command")
}

fn zed_target_uses_looper_stdio(agent_id: &str, object: &Map<String, Value>) -> bool {
    let command = object.get("command").and_then(Value::as_str);
    let args_use_looper_stdio = object
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|args| args_are_looper_zed_stdio(agent_id, args));
    args_use_looper_stdio && command.is_none_or(command_looks_like_looper)
}

fn command_looks_like_looper(command: &str) -> bool {
    if command == ZED_LOOPER_ACP_COMMAND_FALLBACK {
        return true;
    }
    Path::new(command)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| {
            matches!(
                file_name,
                ZED_LOOPER_ACP_COMMAND_FALLBACK | "agent-control-plane" | "looper-cli"
            )
        })
}

fn args_are_looper_zed_stdio(agent_id: &str, args: &[Value]) -> bool {
    if agent_id == LOOPER_ACP_AGENT_ID
        && args.len() == ZED_LOOPER_ACP_COMMAND_ARGS.len()
        && args
            .iter()
            .zip(ZED_LOOPER_ACP_COMMAND_ARGS)
            .all(|(value, expected)| value.as_str() == Some(*expected))
    {
        return true;
    }
    args.len() >= ZED_LOOPER_ACP_COMMAND_ARGS.len() + 3
        && args
            .iter()
            .take(ZED_LOOPER_ACP_COMMAND_ARGS.len())
            .zip(ZED_LOOPER_ACP_COMMAND_ARGS)
            .all(|(value, expected)| value.as_str() == Some(*expected))
        && args
            .get(ZED_LOOPER_ACP_COMMAND_ARGS.len())
            .and_then(Value::as_str)
            == Some(agent_id)
        && args
            .get(ZED_LOOPER_ACP_COMMAND_ARGS.len() + 1)
            .and_then(Value::as_str)
            == Some(ZED_PROXY_ARG_SEPARATOR)
}

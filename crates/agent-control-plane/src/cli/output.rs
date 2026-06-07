use anyhow::{Result, bail};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputFormat {
    Json,
    Table,
}

pub(crate) fn print_value(value: &Value, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(value)?),
        OutputFormat::Table => print_table(value)?,
    }
    Ok(())
}

pub(crate) async fn print_response(
    response: reqwest::Response,
    format: OutputFormat,
) -> Result<()> {
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        bail!("{status}: {text}");
    }
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    print_value(&parsed, format)
}

pub(crate) fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "-".to_owned(),
        value => value.to_string(),
    }
}

fn print_table(value: &Value) -> Result<()> {
    if let Some(checks) = value.get("checks").and_then(Value::as_array) {
        print_rows(
            "check\tstatus\tdetail",
            checks,
            &["name", "status", "detail"],
        );
        return Ok(());
    }
    if let Some(connections) = value.get("connections").and_then(Value::as_array) {
        print_rows(
            "id\tkind\tstatus\tlabel",
            connections,
            &["id", "kind", "status", "label"],
        );
        return Ok(());
    }
    if let Some(bridge) = value.get("bridge") {
        if let Some(agents) = bridge.get("agents").and_then(Value::as_array) {
            println!(
                "summary\t{}",
                scalar_text(bridge.get("summary").unwrap_or(&Value::Null))
            );
            print_rows(
                "id\tenabled\tpreferred\tcontrol_level\tlaunch_configured\tname",
                agents,
                &[
                    "id",
                    "enabled",
                    "preferred",
                    "control_level",
                    "launch_configured",
                    "name",
                ],
            );
            return Ok(());
        }
    }
    if let Some(probe) = value.get("probe") {
        println!("agent_id\tstatus\tready\tprobe_kind\tdetail");
        println!(
            "{}\t{}\t{}\t{}\t{}",
            scalar_text(probe.get("agent_id").unwrap_or(&Value::Null)),
            scalar_text(probe.get("status").unwrap_or(&Value::Null)),
            scalar_text(probe.get("ready").unwrap_or(&Value::Null)),
            scalar_text(probe.get("probe_kind").unwrap_or(&Value::Null)),
            scalar_text(probe.get("detail").unwrap_or(&Value::Null))
        );
        if let Some(blockers) = probe.get("blockers").and_then(Value::as_array) {
            if !blockers.is_empty() {
                println!(
                    "blockers\t{}",
                    blockers
                        .iter()
                        .map(scalar_text)
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
        return Ok(());
    }
    if let Some(notifications) = value.get("notifications").and_then(Value::as_array) {
        print_rows(
            "id\tchannel\tlabel",
            notifications,
            &["id", "channel", "label"],
        );
        return Ok(());
    }
    if let Some(checks) = value.get("completionChecks").and_then(Value::as_array) {
        print_rows(
            "id\tcommands\tlabel",
            checks,
            &["id", "commandCount", "label"],
        );
        return Ok(());
    }
    if let Some(chats) = value.get("chats").and_then(Value::as_array) {
        print_rows(
            "chat_id\tkind\tdisplay_name",
            chats,
            &["chatId", "kind", "displayName"],
        );
        return Ok(());
    }
    if let Some(devices) = value.get("devices").and_then(Value::as_array) {
        print_rows(
            "installation_id\tstate\tenvironment\tdevice",
            devices,
            &["installationId", "state", "environment", "deviceName"],
        );
        return Ok(());
    }
    if value.get("orbId").is_some() && value.get("orbImageURL").is_some() {
        println!("orb_id\torb_image_url\tbase_url");
        println!(
            "{}\t{}\t{}",
            scalar_text(value.get("orbId").unwrap_or(&Value::Null)),
            scalar_text(value.get("orbImageURL").unwrap_or(&Value::Null)),
            scalar_text(value.get("baseURL").unwrap_or(&Value::Null))
        );
        return Ok(());
    }
    if value.get("prompted").is_some() && value.get("threadIds").is_some() {
        println!("prompted\tthread_ids");
        println!(
            "{}\t{}",
            scalar_text(value.get("prompted").unwrap_or(&Value::Null)),
            value
                .get("threadIds")
                .and_then(Value::as_array)
                .map(|thread_ids| {
                    thread_ids
                        .iter()
                        .map(scalar_text)
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| "-".to_owned())
        );
        return Ok(());
    }
    if let Some(threads) = value.get("threads").and_then(Value::as_array) {
        print_rows(
            "thread_id\tarchived\ttitle",
            threads,
            &["thread_id", "archived", "title"],
        );
        return Ok(());
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_rows(header: &str, rows: &[Value], fields: &[&str]) {
    println!("{header}");
    for row in rows {
        let values = fields
            .iter()
            .map(|field| scalar_text(row.get(*field).unwrap_or(&Value::Null)))
            .collect::<Vec<_>>();
        println!("{}", values.join("\t"));
    }
}

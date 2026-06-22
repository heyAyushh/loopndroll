use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::zed::ZED_CLIENT_ID;

mod observe;
mod parse;

use observe::{
    ObservedSessionEvent, PendingNewSession, observe_agent_output, observe_zed_request,
    post_observed_sessions,
};
use parse::{ProxyTarget, parse_proxy_target};

const SESSION_CANCEL_METHOD: &str = "session/cancel";
const SESSION_NEW_METHOD: &str = "session/new";
const SESSION_PROMPT_METHOD: &str = "session/prompt";
const SESSION_UPDATE_METHOD: &str = "session/update";
const TEXT_CONTENT_TYPE: &str = "text";

pub async fn run_stdio_agent(client_id: &str, args: &[String]) -> Result<()> {
    let client_id = match client_id {
        ZED_CLIENT_ID => ZED_CLIENT_ID,
        value => bail!("unsupported ACP stdio client host: {value}"),
    };
    if args.is_empty() {
        return run_local_stdio_agent(client_id).await;
    }
    run_proxy_stdio_agent(client_id, parse_proxy_target(args)?).await
}

async fn run_local_stdio_agent(client_id: &'static str) -> Result<()> {
    crate::cli::server::ensure_server_ready().await?;
    let websocket_url = acp_client_host_websocket_url(client_id, None);
    relay_stdio_to_server_websocket(websocket_url).await
}

async fn run_proxy_stdio_agent(client_id: &'static str, target: ProxyTarget) -> Result<()> {
    crate::cli::server::ensure_server_ready().await?;
    let websocket_url = acp_client_host_websocket_url(client_id, Some(&target.agent_id));
    let (server_input_sender, server_input_receiver) = mpsc::unbounded_channel::<String>();
    let (server_output_sender, server_output_receiver) = std_mpsc::channel::<String>();
    let server_bridge = tokio::spawn(relay_proxy_to_server_websocket(
        websocket_url,
        server_input_receiver,
        server_output_sender,
    ));
    let (observer_sender, observer_receiver) = mpsc::unbounded_channel::<ObservedSessionEvent>();
    let observer = tokio::spawn(post_observed_sessions(
        client_id.to_owned(),
        observer_receiver,
    ));
    let mut child = Command::new(&target.command)
        .args(&target.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start Zed ACP target {}", target.agent_id))?;
    let child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Zed ACP target stdin was not available"))?;
    let child_stdin = Arc::new(Mutex::new(child_stdin));
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Zed ACP target stdout was not available"))?;
    let pending_new_sessions = Arc::new(Mutex::new(BTreeMap::<String, PendingNewSession>::new()));

    let stdin_thread = {
        let target = target.clone();
        let child_stdin = Arc::clone(&child_stdin);
        let pending_new_sessions = Arc::clone(&pending_new_sessions);
        let observer_sender = observer_sender.clone();
        let server_input_sender = server_input_sender.clone();
        std::thread::spawn(move || -> Result<()> {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                let line = line?;
                observe_zed_request(
                    &target.agent_id,
                    &line,
                    &pending_new_sessions,
                    &observer_sender,
                );
                let _ = server_input_sender.send(line.clone());
                let mut child_stdin = child_stdin
                    .lock()
                    .map_err(|_| anyhow!("Zed ACP target stdin lock poisoned"))?;
                writeln!(child_stdin, "{line}")?;
                child_stdin.flush()?;
            }
            Ok(())
        })
    };

    let stdout_thread = {
        let target = target.clone();
        let pending_new_sessions = Arc::clone(&pending_new_sessions);
        let observer_sender = observer_sender.clone();
        std::thread::spawn(move || -> Result<()> {
            let reader = BufReader::new(child_stdout);
            let mut stdout = io::stdout();
            for line in reader.lines() {
                let line = line?;
                observe_agent_output(
                    &target.agent_id,
                    &line,
                    &pending_new_sessions,
                    &observer_sender,
                );
                writeln!(stdout, "{line}")?;
                stdout.flush()?;
            }
            Ok(())
        })
    };

    let server_output_thread = {
        let child_stdin = Arc::clone(&child_stdin);
        std::thread::spawn(move || -> Result<()> {
            while let Ok(message) = server_output_receiver.recv() {
                let mut child_stdin = child_stdin
                    .lock()
                    .map_err(|_| anyhow!("Zed ACP target stdin lock poisoned"))?;
                writeln!(child_stdin, "{message}")?;
                child_stdin.flush()?;
            }
            Ok(())
        })
    };

    join_stdio_thread(stdin_thread, "Zed ACP stdin proxy")?;
    join_stdio_thread(stdout_thread, "Zed ACP stdout proxy")?;
    let _ = child.wait();
    drop(server_input_sender);
    drop(observer_sender);
    server_bridge.abort();
    let _ = server_bridge.await;
    join_stdio_thread(server_output_thread, "Zed ACP server output proxy")?;
    observer
        .await
        .map_err(|error| anyhow!("Zed ACP observation task failed: {error}"))?;
    Ok(())
}

async fn relay_stdio_to_server_websocket(websocket_url: String) -> Result<()> {
    let (stdin_sender, mut stdin_receiver) = mpsc::unbounded_channel::<String>();
    let stdin_thread = std::thread::spawn(move || -> Result<()> {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            if !line.trim().is_empty() {
                let _ = stdin_sender.send(line);
            }
        }
        Ok(())
    });

    let (socket, _) = connect_async(&websocket_url)
        .await
        .with_context(|| format!("connect Looper ACP websocket {websocket_url}"))?;
    let (mut socket_writer, mut socket_reader) = socket.split();
    let mut stdout = io::stdout();

    loop {
        tokio::select! {
            Some(line) = stdin_receiver.recv() => {
                socket_writer.send(Message::Text(line)).await?;
            }
            message = socket_reader.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        writeln!(stdout, "{text}")?;
                        stdout.flush()?;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                    Some(Err(error)) => return Err(error.into()),
                }
            }
        }
    }

    drop(stdin_thread);
    Ok(())
}

async fn relay_proxy_to_server_websocket(
    websocket_url: String,
    mut input_receiver: mpsc::UnboundedReceiver<String>,
    output_sender: std_mpsc::Sender<String>,
) -> Result<()> {
    let (socket, _) = connect_async(&websocket_url)
        .await
        .with_context(|| format!("connect Looper ACP websocket {websocket_url}"))?;
    let (mut socket_writer, mut socket_reader) = socket.split();

    loop {
        tokio::select! {
            Some(line) = input_receiver.recv() => {
                socket_writer.send(Message::Text(line)).await?;
            }
            message = socket_reader.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        let _ = output_sender.send(text);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                    Some(Err(error)) => return Err(error.into()),
                }
            }
        }
    }

    Ok(())
}

fn acp_client_host_websocket_url(client_id: &str, agent_id: Option<&str>) -> String {
    let path = match agent_id {
        Some(agent_id) => format!("/acp/client-hosts/{client_id}?agentId={agent_id}"),
        None => format!("/acp/client-hosts/{client_id}"),
    };
    crate::cli::transport::url(&path)
        .replacen("http://", "ws://", 1)
        .replacen("https://", "wss://", 1)
}

fn join_stdio_thread(
    thread: std::thread::JoinHandle<Result<()>>,
    label: &'static str,
) -> Result<()> {
    thread
        .join()
        .map_err(|_| anyhow!("{label} panicked"))?
        .with_context(|| label)
}

#[cfg(test)]
mod tests;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = std::env::args().nth(1);
    if matches!(command.as_deref(), Some("--hook") | Some("hook")) {
        return agent_control_plane::runtime::run_hook_mode().await;
    }
    if matches!(command.as_deref(), None | Some("serve")) {
        return agent_control_plane::runtime::run_server().await;
    }
    eprintln!("usage: looper-server [serve|hook|--hook]");
    std::process::exit(2);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|argument| argument == "--hook" || argument == "hook") {
        return agent_control_plane::runtime::run_hook_mode();
    }
    agent_control_plane::runtime::run_server().await
}

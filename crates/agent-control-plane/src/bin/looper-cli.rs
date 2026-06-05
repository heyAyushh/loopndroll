#[tokio::main]
async fn main() -> anyhow::Result<()> {
    agent_control_plane::cli::run().await
}

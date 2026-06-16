use tinyfusion_core::server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .init();

    server::run().await?;
    Ok(())
}

use tinyfusion_core::{config::Config, server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .init();

    let config = Config::load_default()?;

    server::run(config).await?;
    Ok(())
}

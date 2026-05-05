use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env variables implicitly if present
    dotenvy::dotenv().ok();

    // Setup tracing logs with RUST_LOG environment variable (default to info)
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = FmtSubscriber::builder().with_env_filter(filter).finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Setting default tracing subscriber failed");

    info!("Initializing Aegis AI Agent...");

    if let Err(e) = aegis_ai_agent::run().await {
        error!("Agent crashed with error: {:?}", e);
        return Err(e);
    }

    Ok(())
}

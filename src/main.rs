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

    // Check mandatory configuration to avoid silent failures later
    if std::env::var("DEPLOYMENT_TOKEN").is_err() {
        error!("Fatal: DEPLOYMENT_TOKEN is missing. Please set it to start the agent.");
        std::process::exit(1);
    }

    if std::env::var("GATEWAY_URL").is_err() {
        error!("Fatal: GATEWAY_URL is missing. Please set it to start the agent.");
        std::process::exit(1);
    }

    info!("Initializing Aegis AI Agent...");

    if let Err(e) = aegis_ai_agent::run().await {
        error!("Agent crashed with error: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}

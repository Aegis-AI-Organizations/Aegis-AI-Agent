use anyhow::Context;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};
use aegis_ai_agent::domain::SystemExtractor;

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

    // Initial system extraction for diagnostic/initialization purposes
    let extractor = aegis_ai_agent::extractor::SysinfoExtractor::new();
    let host = extractor
        .get_host_info()
        .await
        .context("Failed to collect initial host info")?;
    let processes = extractor
        .get_processes()
        .await
        .context("Failed to collect initial processes")?;

    let payload = aegis_ai_agent::domain::TopologyPayload { host, processes };

    // Pretty-print the topology payload to logs
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => info!("Initial System Topology:\n{}", json),
        Err(e) => error!("Failed to serialize topology payload: {}", e),
    }

    if let Err(e) = aegis_ai_agent::run().await {
        error!("Agent crashed with error: {:?}", e);
        return Err(e);
    }

    Ok(())
}

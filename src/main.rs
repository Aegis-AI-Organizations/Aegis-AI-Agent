use aegis_ai_agent::domain::SystemExtractor;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env variables implicitly if present
    dotenvy::dotenv().ok();

    // Setup tracing logs with RUST_LOG environment variable (default to info)
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_timer(tracing_subscriber::fmt::time::ChronoUtc::rfc_3339())
        .with_thread_ids(true)
        .with_line_number(true)
        .with_file(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("Setting default tracing subscriber failed");

    info!("Initializing Aegis AI Agent...");

    // Initial system extraction for diagnostic/initialization purposes
    // Make it best-effort to avoid crashing the agent if sysinfo fails
    let extractor = aegis_ai_agent::extractor::SysinfoExtractor::new();
    let topology_result = async {
        let host = extractor.get_host_info().await?;
        let processes = extractor.get_processes().await?;
        Ok::<aegis_ai_agent::domain::TopologyPayload, anyhow::Error>(
            aegis_ai_agent::domain::TopologyPayload {
                host,
                processes,
                containers: None,
                pods: None,
            },
        )
    }
    .await;

    match topology_result {
        Ok(payload) => {
            // Security: Only log full topology if explicitly requested via environment flag
            if std::env::var("AEGIS_DEBUG_TOPOLOGY")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                match serde_json::to_string_pretty(&payload) {
                    Ok(json) => info!(
                        "Initial System Topology (AEGIS_DEBUG_TOPOLOGY=true):\n{}",
                        json
                    ),
                    Err(e) => error!("Failed to serialize topology payload: {}", e),
                }
            } else {
                info!("System topology collected successfully (logging disabled, use AEGIS_DEBUG_TOPOLOGY=true to see details)");
            }
        }
        Err(e) => error!("Failed to collect initial system topology: {:?}", e),
    }

    if let Err(e) = aegis_ai_agent::run().await {
        error!("Agent crashed with error: {:?}", e);
        return Err(e);
    }

    Ok(())
}

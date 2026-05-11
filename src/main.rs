use aegis_ai_agent::domain::SystemExtractor;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env variables implicitly if present
    let dotenv_result = dotenvy::dotenv();

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

    // Now we can use info!/error!
    match dotenv_result {
        Ok(path) => info!("Loaded environment from {:?}", path),
        Err(_) => info!("No .env file found, using system environment variables"),
    }

    info!("Initializing Aegis AI Agent...");

    // Initial system extraction for diagnostic/initialization purposes
    // Make it best-effort to avoid crashing the agent if sysinfo fails
    let sys_extractor = aegis_ai_agent::extractor::SysinfoExtractor::new();
    let topology_result = async {
        let host = sys_extractor.get_host_info().await?;
        let processes = sys_extractor.get_processes().await?;

        // Detect if we are in Kubernetes
        let mut pods = None;
        #[cfg(feature = "k8s")]
        if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
            info!("Kubernetes detected, initializing K8s extractor...");
            let k8s_extraction = async {
                if let Ok(k8s_extractor) = aegis_ai_agent::extractor::K8sExtractor::new().await {
                    match k8s_extractor.get_pods().await {
                        Ok(p) => {
                            info!("Successfully discovered {} pods in cluster", p.len());
                            Some(p)
                        }
                        Err(e) => {
                            error!("Failed to extract Kubernetes pods: {:?}", e);
                            None
                        }
                    }
                } else {
                    error!("Failed to initialize K8s client (check ServiceAccount/RBAC)");
                    None
                }
            };

            pods = tokio::time::timeout(std::time::Duration::from_secs(5), k8s_extraction)
                .await
                .unwrap_or_else(|_| {
                    error!("Kubernetes extraction timed out after 5s");
                    None
                });
        }

        // Try to detect Docker containers
        let mut containers = None;
        #[cfg(feature = "docker")]
        if let Ok(docker_extractor) = aegis_ai_agent::extractor::DockerExtractor::new() {
            let docker_extraction = async {
                match docker_extractor.get_containers().await {
                    Ok(c) => {
                        if !c.is_empty() {
                            info!("Successfully discovered {} Docker containers", c.len());
                            Some(c)
                        } else {
                            None
                        }
                    }
                    Err(e) => {
                        info!(
                            "Docker extraction skipped or failed (daemon might not be reachable): {}",
                            e
                        );
                        None
                    }
                }
            };

            containers = tokio::time::timeout(std::time::Duration::from_secs(5), docker_extraction)
                .await
                .unwrap_or_else(|_| {
                    info!("Docker extraction timed out after 5s");
                    None
                });
        }

        Ok::<aegis_ai_agent::domain::TopologyPayload, anyhow::Error>(
            aegis_ai_agent::domain::TopologyPayload {
                host,
                processes,
                containers,
                pods,
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

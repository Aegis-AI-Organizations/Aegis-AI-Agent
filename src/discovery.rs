use crate::client::AegisClient;
use crate::config::AgentConfig;
use crate::domain::{SystemExtractor, TopologyPayload};
use crate::extractor::SysinfoExtractor;
use crate::redaction::Redactor;
use anyhow::Result;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

pub async fn start_discovery_loop(config: AgentConfig) -> Result<()> {
    let gateway_url = crate::config::get_gateway_url();
    let client = AegisClient::new(gateway_url);
    let redactor = Redactor::new();
    let sys_extractor = SysinfoExtractor::new();

    let mut interval = time::interval(Duration::from_secs(300)); // Every 5 minutes

    loop {
        interval.tick().await;
        info!("Starting topology discovery...");

        match collect_topology(&sys_extractor).await {
            Ok(mut payload) => {
                // Apply redaction to all text fields in the topology
                redact_payload(&mut payload, &redactor);

                let filename = format!("topology_{}.json", chrono::Utc::now().timestamp());
                match client.get_upload_url(&config, &filename).await {
                    Ok(url) => match serde_json::to_vec(&payload) {
                        Ok(data) => {
                            if let Err(e) = client.upload_payload(&url, data).await {
                                error!("Failed to upload topology payload: {}", e);
                            } else {
                                info!("Topology uploaded successfully as {}", filename);
                            }
                        }
                        Err(e) => error!("Failed to serialize topology: {}", e),
                    },
                    Err(e) => error!("Failed to get upload URL: {}", e),
                }
            }
            Err(e) => error!("Topology collection failed: {}", e),
        }
    }
}

async fn collect_topology(sys_extractor: &SysinfoExtractor) -> Result<TopologyPayload> {
    let host = sys_extractor.get_host_info().await?;
    let processes = sys_extractor.get_processes().await?;

    let mut pods = None;
    #[cfg(feature = "k8s")]
    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        if let Ok(k8s_extractor) = crate::extractor::K8sExtractor::new().await {
            pods = k8s_extractor.get_pods().await.ok();
        }
    }

    let mut containers = None;
    #[cfg(feature = "docker")]
    if let Ok(docker_extractor) = crate::extractor::DockerExtractor::new() {
        containers = docker_extractor.get_containers().await.ok();
    }

    Ok(TopologyPayload {
        host,
        processes,
        containers,
        pods,
    })
}

fn redact_payload(payload: &mut TopologyPayload, redactor: &Redactor) {
    // Redact host info
    payload.host.hostname = redactor.redact(&payload.host.hostname);

    // Redact process info
    for proc in &mut payload.processes {
        proc.name = redactor.redact(&proc.name);
        if let Some(args) = &mut proc.args {
            for arg in args {
                *arg = redactor.redact(arg);
            }
        }
    }

    // Redact containers
    if let Some(containers) = &mut payload.containers {
        for container in containers {
            container.name = redactor.redact(&container.name);
            for value in container.env.values_mut() {
                *value = redactor.redact(value);
            }
        }
    }

    // Redact pods
    if let Some(pods) = &mut payload.pods {
        for pod in pods {
            pod.name = redactor.redact(&pod.name);
            for value in pod.labels.values_mut() {
                *value = redactor.redact(value);
            }
            for container in &mut pod.containers {
                container.name = redactor.redact(&container.name);
                for value in container.env.values_mut() {
                    *value = redactor.redact(value);
                }
            }
        }
    }
}

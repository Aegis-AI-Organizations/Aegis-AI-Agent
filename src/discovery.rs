use crate::client::AegisClient;
use crate::config::AgentConfig;
use crate::domain::{
    ContainerNode, HostNode, NetworkTopologyPayload, PodNode, ProtoContainer, ProtoHost,
    ProtoProcess, SystemExtractor,
};
use crate::extractor::SysinfoExtractor;
#[cfg(any(feature = "docker", feature = "k8s"))]
use crate::extractor::TopologyExtractor;
use crate::redaction::Redactor;
use anyhow::Result;
use std::collections::BTreeMap;
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

pub async fn collect_topology(sys_extractor: &SysinfoExtractor) -> Result<NetworkTopologyPayload> {
    let host = sys_extractor.get_host_info().await?;
    let processes = sys_extractor.get_processes().await?;

    #[allow(unused_mut)]
    let mut containers = Vec::new();
    #[cfg(feature = "docker")]
    if let Ok(docker_extractor) = crate::extractor::DockerExtractor::new() {
        match tokio::time::timeout(Duration::from_secs(5), docker_extractor.list_active_containers()).await {
            Ok(Ok(discovered)) => containers.extend(discovered),
            Ok(Err(e)) => info!("Docker topology extraction skipped: {}", e),
            Err(_) => info!("Docker topology extraction timed out after 5s"),
        }
    }

    #[allow(unused_mut)]
    let mut pods = Vec::new();
    #[cfg(feature = "k8s")]
    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok() {
        match tokio::time::timeout(Duration::from_secs(5), crate::extractor::K8sExtractor::new()).await {
            Ok(Ok(k8s_extractor)) => {
                match tokio::time::timeout(Duration::from_secs(5), k8s_extractor.list_active_pods()).await {
                    Ok(Ok(discovered)) => pods.extend(discovered),
                    Ok(Err(e)) => info!("Kubernetes topology extraction skipped: {}", e),
                    Err(_) => info!("Kubernetes topology extraction timed out after 5s"),
                }
            }
            Ok(Err(e)) => info!("Kubernetes client initialization skipped: {}", e),
            Err(_) => info!("Kubernetes client initialization timed out after 5s"),
        }
    }

    Ok(build_network_topology(host, processes, containers, pods))
}

pub fn build_network_topology(
    host: HostNode,
    processes: Vec<crate::domain::ProcessNode>,
    containers: Vec<ContainerNode>,
    pods: Vec<PodNode>,
) -> NetworkTopologyPayload {
    let mut merged_containers = BTreeMap::new();

    for container in containers {
        insert_container(&mut merged_containers, proto_container_from_node(container));
    }

    for pod in pods {
        for container in pod.containers {
            insert_container(&mut merged_containers, proto_container_from_node(container));
        }
    }

    NetworkTopologyPayload {
        hosts: vec![ProtoHost {
            id: host.hostname.clone(),
            hostname: host.hostname,
            ip_addresses: Vec::new(),
            containers: merged_containers.into_values().collect(),
            processes: processes.into_iter().map(proto_process_from_node).collect(),
        }],
    }
}

pub fn redact_payload(payload: &mut NetworkTopologyPayload, redactor: &Redactor) {
    for host in &mut payload.hosts {
        let original_hostname = host.hostname.clone();
        let original_id = host.id.clone();
        
        let redacted_hostname = redactor.redact(&original_hostname);
        host.hostname = redacted_hostname.clone();
        
        // If ID matches hostname, keep them consistent. Otherwise redact ID separately.
        host.id = if original_id == original_hostname {
            redacted_hostname
        } else {
            redactor.redact(&original_id)
        };

        for proc in &mut host.processes {
            proc.name = redactor.redact(&proc.name);
            if let Some(command_line) = &mut proc.command_line {
                *command_line = redactor.redact(command_line);
            }
            if let Some(user) = &mut proc.user {
                *user = redactor.redact(user);
            }
        }

        for container in &mut host.containers {
            container.name = redactor.redact(&container.name);
            container.image = redactor.redact(&container.image);
            for proc in &mut container.processes {
                proc.name = redactor.redact(&proc.name);
            }
        }
    }
}

fn insert_container(containers: &mut BTreeMap<String, ProtoContainer>, container: ProtoContainer) {
    let key = container_key(&container);
    containers
        .entry(key)
        .and_modify(|existing| merge_container(existing, &container))
        .or_insert(container);
}

fn merge_container(existing: &mut ProtoContainer, incoming: &ProtoContainer) {
    if existing.id.is_empty() {
        existing.id = incoming.id.clone();
    }
    if existing.image == "unknown" && incoming.image != "unknown" {
        existing.image = incoming.image.clone();
    }
}

fn container_key(container: &ProtoContainer) -> String {
    let normalized_id = normalize_container_id(&container.id);
    if normalized_id.is_empty() {
        format!("{}:{}", container.name, container.image)
    } else {
        normalized_id
    }
}

fn normalize_container_id(id: &str) -> String {
    id.rsplit_once("://")
        .map(|(_, value)| value)
        .unwrap_or(id)
        .to_string()
}

fn proto_container_from_node(container: ContainerNode) -> ProtoContainer {
    ProtoContainer {
        id: normalize_container_id(&container.id),
        name: container.name,
        image: container.image,
        processes: Vec::new(),
        ports: Vec::new(),
    }
}

fn proto_process_from_node(process: crate::domain::ProcessNode) -> ProtoProcess {
    ProtoProcess {
        pid: process.pid.min(i32::MAX as u32) as i32,
        name: process.name,
        command_line: process.args.map(|args| args.join(" ")),
        user: Some(process.user),
    }
}

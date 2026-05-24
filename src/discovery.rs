use crate::client::AegisClient;
use crate::config::AgentConfig;
use crate::domain::{
    ActiveResource, ContainerNode, HostNode, NetworkTopologyPayload, PodNode, ProtoContainer,
    ProtoHost, ProtoProcess, SystemExtractor,
};
use crate::extractor::{SysinfoExtractor, TopologyExtractor};
use crate::redaction::Redactor;
use anyhow::Result;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{error, info};

static SCAN_TRIGGER: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);

/// Triggers a manual system topology discovery and upload.
/// Returns true if the scan signal was successfully sent.
pub fn trigger_topology_scan() -> bool {
    if let Ok(guard) = SCAN_TRIGGER.lock() {
        if let Some(ref tx) = *guard {
            return tx.try_send(()).is_ok();
        }
    }
    false
}

pub fn set_mock_scan_trigger(tx: mpsc::Sender<()>) {
    if let Ok(mut guard) = SCAN_TRIGGER.lock() {
        *guard = Some(tx);
    }
}

pub async fn start_discovery_loop(config: AgentConfig) -> Result<()> {
    let gateway_url = crate::config::get_gateway_url();
    let client = AegisClient::new(gateway_url);
    let redactor = Redactor::new();
    let sys_extractor = SysinfoExtractor::new();

    let mut interval = time::interval(Duration::from_secs(900)); // Every 15 minutes
    let (tx, mut rx) = mpsc::channel::<()>(10);

    if let Ok(mut guard) = SCAN_TRIGGER.lock() {
        *guard = Some(tx);
    }

    let mut last_payload: Option<NetworkTopologyPayload> = None;

    loop {
        let force_upload = tokio::select! {
            _ = interval.tick() => false,
            Some(_) = rx.recv() => true,
        };

        info!(
            "Starting topology discovery (force_upload={})...",
            force_upload
        );

        match collect_topology(&sys_extractor).await {
            Ok(mut payload) => {
                // Apply redaction to all text fields in the topology
                redact_payload(&mut payload, &redactor);

                // If this is a periodic tick (not forced), skip upload if unchanged
                if !force_upload {
                    if let Some(ref last) = last_payload {
                        if last == &payload {
                            info!("Topology unchanged, skipping upload.");
                            continue;
                        }
                    }
                }

                let filename = format!("topology_{}.json", chrono::Utc::now().timestamp());
                match client.get_upload_url(&config, &filename).await {
                    Ok((url, object_name)) => match serde_json::to_vec(&payload) {
                        Ok(data) => {
                            if let Err(e) = client.upload_payload(&url, data).await {
                                error!("Failed to upload topology payload: {}", e);
                            } else {
                                info!("Topology uploaded successfully as {}", filename);
                                last_payload = Some(payload);
                                if let Err(e) = client
                                    .update_status(&config, "UPLOAD_COMPLETE", Some(&object_name))
                                    .await
                                {
                                    error!("Failed to notify status update UPLOAD_COMPLETE: {}", e);
                                }
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
    let resources = collect_runtime_resources().await;

    Ok(build_network_topology_from_resources(
        host, processes, resources,
    ))
}

async fn collect_runtime_resources() -> Vec<ActiveResource> {
    let mut resources = Vec::new();

    #[cfg(feature = "docker")]
    match crate::extractor::DockerExtractor::new() {
        Ok(docker_extractor) => {
            collect_resources_from_extractor("Docker", &docker_extractor, &mut resources).await
        }
        Err(e) => info!("Docker extractor initialization skipped: {}", e),
    }

    #[cfg(feature = "k8s")]
    match crate::extractor::K8sExtractor::new().await {
        Ok(k8s_extractor) => {
            collect_resources_from_extractor("Kubernetes", &k8s_extractor, &mut resources).await
        }
        Err(e) => info!("Kubernetes client initialization skipped: {}", e),
    }

    resources
}

async fn collect_resources_from_extractor<T: TopologyExtractor>(
    extractor_name: &str,
    extractor: &T,
    resources: &mut Vec<ActiveResource>,
) {
    match extractor.list_active_resources().await {
        Ok(discovered) => resources.extend(discovered),
        Err(e) => info!("{} topology extraction skipped: {}", extractor_name, e),
    }
}

pub fn build_network_topology(
    host: HostNode,
    processes: Vec<crate::domain::ProcessNode>,
    containers: Vec<ContainerNode>,
    pods: Vec<PodNode>,
) -> NetworkTopologyPayload {
    let resources = containers
        .into_iter()
        .map(ActiveResource::Container)
        .chain(pods.into_iter().map(ActiveResource::Pod))
        .collect();

    build_network_topology_from_resources(host, processes, resources)
}

pub fn build_network_topology_from_resources(
    host: HostNode,
    processes: Vec<crate::domain::ProcessNode>,
    resources: Vec<ActiveResource>,
) -> NetworkTopologyPayload {
    let mut merged_containers = BTreeMap::new();

    for resource in resources {
        match resource {
            ActiveResource::Container(container) => {
                insert_container(&mut merged_containers, proto_container_from_node(container));
            }
            ActiveResource::Pod(pod) => {
                for container in pod.containers {
                    insert_container(&mut merged_containers, proto_container_from_node(container));
                }
            }
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
        host.hostname = redactor.redact(&host.hostname);

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

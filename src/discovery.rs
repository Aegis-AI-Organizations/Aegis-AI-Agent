use crate::client::AegisClient;
use crate::config::AgentConfig;
use crate::domain::{
    ActiveResource, ContainerNode, HostNode, IngressBackendNode, IngressNode,
    NetworkTopologyPayload, PodNode, PortBindingNode, ProtoContainer, ProtoHost, ProtoPort,
    ProtoProcess, ProtoRoute, ServiceNode, SystemExtractor,
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
    let mut routes = BTreeMap::new();

    for resource in resources {
        match resource {
            ActiveResource::Container(container) => {
                let proto = proto_container_from_node(container);
                insert_routes(&mut routes, container_routes_from_proto_container(&proto));
                insert_container(&mut merged_containers, proto);
            }
            ActiveResource::Pod(pod) => {
                for container in pod.containers {
                    let proto = proto_container_from_node(container);
                    insert_routes(&mut routes, container_routes_from_proto_container(&proto));
                    insert_container(&mut merged_containers, proto);
                }
            }
            ActiveResource::Service(service) => {
                insert_routes(&mut routes, service_routes_from_node(service));
            }
            ActiveResource::Ingress(ingress) => {
                insert_routes(&mut routes, ingress_routes_from_node(&ingress));
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
        routes: routes.into_values().collect(),
    }
}

pub fn redact_payload(payload: &mut NetworkTopologyPayload, redactor: &Redactor) {
    for host in &mut payload.hosts {
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
            for value in container.env.values_mut() {
                *value = redact_env_value(value.as_str(), redactor);
            }
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
    existing.env.extend(incoming.env.clone());

    if existing.id.is_empty() {
        existing.id = incoming.id.clone();
    }
    if existing.image == "unknown" && incoming.image != "unknown" {
        existing.image = incoming.image.clone();
    }
    if existing.image_sha256.is_none() {
        existing.image_sha256 = incoming.image_sha256.clone();
    }
    if existing.privileged.is_none() {
        existing.privileged = incoming.privileged;
    }
    if existing.run_as_root.is_none() {
        existing.run_as_root = incoming.run_as_root;
    }
    existing
        .exposed_ports
        .extend(incoming.exposed_ports.clone());
    if existing.sensitive_volumes.is_empty() {
        existing.sensitive_volumes = incoming.sensitive_volumes.clone();
    } else {
        for volume in &incoming.sensitive_volumes {
            if !existing.sensitive_volumes.contains(volume) {
                existing.sensitive_volumes.push(volume.clone());
            }
        }
    }

    normalize_proto_container(existing);
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
    let exposed_ports = dedupe_port_bindings(container.exposed_ports)
        .into_iter()
        .map(proto_port_from_binding)
        .collect::<Vec<_>>();
    let ports = exposed_ports
        .iter()
        .filter(|port| port.host_port.is_some())
        .cloned()
        .collect::<Vec<_>>();

    let mut proto = ProtoContainer {
        id: normalize_container_id(&container.id),
        name: container.name,
        image: container.image,
        image_sha256: container.image_sha256,
        env: container.env,
        processes: Vec::new(),
        ports,
        exposed_ports,
        privileged: container.privileged,
        run_as_root: container.run_as_root,
        sensitive_volumes: container.sensitive_volumes,
    };

    normalize_proto_container(&mut proto);
    proto
}

fn proto_port_from_binding(binding: PortBindingNode) -> ProtoPort {
    ProtoPort {
        number: binding.number,
        protocol: normalize_protocol(&binding.protocol),
        state: Some(if binding.host_port.is_some() {
            "published".to_string()
        } else {
            "exposed".to_string()
        }),
        host_ip: binding.host_ip,
        host_port: binding.host_port,
        source: binding.source,
    }
}

fn container_routes_from_proto_container(container: &ProtoContainer) -> Vec<ProtoRoute> {
    container
        .ports
        .iter()
        .map(|port| ProtoRoute {
            kind: "docker_port_binding".to_string(),
            source_kind: "container".to_string(),
            source_name: container.name.clone(),
            source_namespace: None,
            target_kind: Some("host".to_string()),
            target_name: route_target_name(port.host_ip.as_deref()),
            target_namespace: None,
            host: normalize_route_host(port.host_ip.as_deref()),
            path: None,
            path_type: None,
            protocol: Some(normalize_protocol(&port.protocol)),
            source_port: Some(port.number),
            target_port: port.host_port.map(|value| value.to_string()),
            published_port: port.host_port,
        })
        .collect()
}

fn service_routes_from_node(service: ServiceNode) -> Vec<ProtoRoute> {
    service
        .ports
        .into_iter()
        .map(|port| ProtoRoute {
            kind: "k8s_service".to_string(),
            source_kind: "service".to_string(),
            source_name: service.name.clone(),
            source_namespace: Some(service.namespace.clone()),
            target_kind: Some("service".to_string()),
            target_name: Some(service.name.clone()),
            target_namespace: Some(service.namespace.clone()),
            host: service.cluster_ip.clone(),
            path: None,
            path_type: None,
            protocol: Some(normalize_protocol(&port.protocol)),
            source_port: Some(port.port),
            target_port: port.target_port,
            published_port: port.node_port,
        })
        .collect()
}

fn ingress_routes_from_node(ingress: &IngressNode) -> Vec<ProtoRoute> {
    let mut routes = Vec::new();

    if let Some(default_backend) = &ingress.default_backend {
        routes.push(proto_route_from_ingress_backend(
            ingress,
            None,
            None,
            default_backend.clone(),
        ));
    }

    for rule in &ingress.rules {
        for path in &rule.paths {
            routes.push(proto_route_from_ingress_backend(
                ingress,
                rule.host.clone(),
                Some((path.path.clone(), path.path_type.clone())),
                path.backend.clone(),
            ));
        }
    }

    routes
}

fn proto_route_from_ingress_backend(
    ingress: &IngressNode,
    host: Option<String>,
    path: Option<(Option<String>, String)>,
    backend: IngressBackendNode,
) -> ProtoRoute {
    let (path, path_type) = path
        .map(|(path, path_type)| (path, Some(path_type)))
        .unwrap_or((None, None));

    ProtoRoute {
        kind: "k8s_ingress".to_string(),
        source_kind: "ingress".to_string(),
        source_name: ingress.name.clone(),
        source_namespace: Some(ingress.namespace.clone()),
        target_kind: Some("service".to_string()),
        target_name: Some(backend.service_name),
        target_namespace: Some(backend.namespace),
        host,
        path,
        path_type,
        protocol: None,
        source_port: None,
        target_port: backend
            .port_name
            .or_else(|| backend.port_number.map(|value| value.to_string())),
        published_port: backend.port_number,
    }
}

fn insert_routes(routes: &mut BTreeMap<String, ProtoRoute>, incoming: Vec<ProtoRoute>) {
    for route in incoming {
        let key = route_key(&route);
        routes.entry(key).or_insert(route);
    }
}

fn route_key(route: &ProtoRoute) -> String {
    let normalized_host = normalize_route_host(route.host.as_deref());
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        route.kind,
        route.source_kind,
        route.source_name,
        route.source_namespace.clone().unwrap_or_default(),
        route.target_kind.clone().unwrap_or_default(),
        route.target_name.clone().unwrap_or_default(),
        route.target_namespace.clone().unwrap_or_default(),
        normalized_host.unwrap_or_default(),
        route.path.clone().unwrap_or_default(),
        route.path_type.clone().unwrap_or_default(),
        route
            .protocol
            .as_deref()
            .map(normalize_protocol)
            .unwrap_or_default(),
        route
            .source_port
            .map(|value| value.to_string())
            .unwrap_or_default(),
        route.target_port.clone().unwrap_or_default(),
        route
            .published_port
            .map(|value| value.to_string())
            .unwrap_or_default()
    )
}

fn normalize_proto_container(container: &mut ProtoContainer) {
    container.exposed_ports = dedupe_proto_ports(std::mem::take(&mut container.exposed_ports));
    container.ports = container
        .exposed_ports
        .iter()
        .filter(|port| port.host_port.is_some())
        .cloned()
        .collect();
}

fn dedupe_port_bindings(bindings: Vec<PortBindingNode>) -> Vec<PortBindingNode> {
    let mut deduped: BTreeMap<PortBindingKey, (u8, PortBindingNode)> = BTreeMap::new();

    for binding in bindings {
        let normalized = normalize_port_binding(binding);
        let key = PortBindingKey::from_binding(&normalized);
        let priority = port_binding_priority(normalized.source.as_deref());

        match deduped.get_mut(&key) {
            Some((existing_priority, existing_binding)) => {
                if priority > *existing_priority {
                    *existing_priority = priority;
                    *existing_binding = normalized;
                }
            }
            None => {
                deduped.insert(key, (priority, normalized));
            }
        }
    }

    deduped.into_values().map(|(_, binding)| binding).collect()
}

fn dedupe_proto_ports(ports: Vec<ProtoPort>) -> Vec<ProtoPort> {
    let mut deduped: BTreeMap<ProtoPortKey, (u8, ProtoPort)> = BTreeMap::new();

    for port in ports {
        let normalized = normalize_proto_port(port);
        let key = ProtoPortKey::from_port(&normalized);
        let priority = port_binding_priority(normalized.source.as_deref());

        match deduped.get_mut(&key) {
            Some((existing_priority, existing_port)) => {
                if priority > *existing_priority {
                    *existing_priority = priority;
                    *existing_port = normalized;
                }
            }
            None => {
                deduped.insert(key, (priority, normalized));
            }
        }
    }

    deduped.into_values().map(|(_, port)| port).collect()
}

fn normalize_port_binding(mut binding: PortBindingNode) -> PortBindingNode {
    binding.protocol = normalize_protocol(&binding.protocol);
    binding.host_ip = normalize_route_host(binding.host_ip.as_deref());
    binding
}

fn normalize_proto_port(mut port: ProtoPort) -> ProtoPort {
    port.protocol = normalize_protocol(&port.protocol);
    port.host_ip = normalize_route_host(port.host_ip.as_deref());
    port
}

fn normalize_protocol(protocol: &str) -> String {
    protocol.trim().to_lowercase()
}

fn normalize_route_host(host: Option<&str>) -> Option<String> {
    host.and_then(|value| {
        let normalized = value.trim();
        if normalized.is_empty()
            || normalized == "0.0.0.0"
            || normalized == "::"
            || normalized == "localhost"
        {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

fn route_target_name(host: Option<&str>) -> Option<String> {
    normalize_route_host(host).or_else(|| Some("localhost".to_string()))
}

fn port_binding_priority(source: Option<&str>) -> u8 {
    match source {
        Some("docker_port_bindings") | Some("k8s_container") => 3,
        Some("docker_exposed_ports") => 2,
        Some("docker_summary") => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct PortBindingKey {
    number: i32,
    protocol: String,
    host_ip: Option<String>,
    host_port: Option<i32>,
}

impl PortBindingKey {
    fn from_binding(binding: &PortBindingNode) -> Self {
        Self {
            number: binding.number,
            protocol: binding.protocol.clone(),
            host_ip: binding.host_ip.clone(),
            host_port: binding.host_port,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct ProtoPortKey {
    number: i32,
    protocol: String,
    host_ip: Option<String>,
    host_port: Option<i32>,
}

impl ProtoPortKey {
    fn from_port(port: &ProtoPort) -> Self {
        Self {
            number: port.number,
            protocol: port.protocol.clone(),
            host_ip: port.host_ip.clone(),
            host_port: port.host_port,
        }
    }
}

fn redact_env_value(value: &str, redactor: &Redactor) -> String {
    let redacted = redactor.redact(value);
    if redacted == value {
        "REDACTED".to_string()
    } else {
        redacted
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

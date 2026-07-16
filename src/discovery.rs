use crate::client::AegisClient;
use crate::config::AgentConfig;
use crate::domain::{
    ActiveResource, ContainerNode, DatabaseSchema, HostNode, IngressBackendNode, IngressNode,
    NetworkTopologyPayload, PodNode, PortBindingNode, ProtoContainer, ProtoHost, ProtoPort,
    ProtoProcess, ProtoRoute, ServiceNode, SystemExtractor,
};
use crate::extractor::{
    redact_environment_value_with_redactor, SysinfoExtractor, TopologyExtractor,
};
use crate::redaction::Redactor;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::Command;
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
                if let Err(e) = attach_local_image_archives(&config, &client, &mut payload).await {
                    error!("Failed to export local image archive(s): {}", e);
                }

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

async fn attach_local_image_archives(
    config: &AgentConfig,
    client: &AegisClient,
    payload: &mut NetworkTopologyPayload,
) -> Result<()> {
    if std::env::var("AEGIS_EXPORT_LOCAL_IMAGES").unwrap_or_else(|_| "true".to_string()) == "false"
    {
        return Ok(());
    }

    let mut exported = std::collections::BTreeMap::<String, (String, String)>::new();
    let mut attempted_count = 0usize;
    for host in &mut payload.hosts {
        for container in &mut host.containers {
            let image = container.image.trim();
            if !should_export_local_image(image) {
                continue;
            }
            attempted_count += 1;
            if let Some((archive_ref, object_name)) = exported.get(image).cloned() {
                container.image_archive_ref = Some(archive_ref);
                container.image_archive_object = Some(object_name);
                continue;
            }

            let filename = format!(
                "image_{}_{}.tar",
                sanitize_archive_name(image),
                chrono::Utc::now().timestamp()
            );
            let (url, object_name) = match client.get_upload_url(config, &filename).await {
                Ok(value) => value,
                Err(e) => {
                    error!(
                        "Failed to get upload URL for local Docker image {}: {}",
                        image, e
                    );
                    continue;
                }
            };
            let archive = match docker_save_image(image).await {
                Ok(value) => value,
                Err(e) => {
                    error!("Failed to export local Docker image {}: {}", image, e);
                    continue;
                }
            };
            if let Err(e) = client.upload_payload(&url, archive).await {
                error!(
                    "Failed to upload local Docker image archive for {}: {}",
                    image, e
                );
                continue;
            }
            let archive_ref = format!("minio:{}", object_name);
            exported.insert(
                image.to_string(),
                (archive_ref.clone(), object_name.clone()),
            );
            container.image_archive_ref = Some(archive_ref);
            container.image_archive_object = Some(object_name);
            info!("Exported local Docker image archive for {}", image);
        }
    }
    if attempted_count > 0 {
        info!(
            "Local Docker image archive export summary: attempted_containers={}, exported_images={}",
            attempted_count,
            exported.len()
        );
    }
    Ok(())
}

async fn docker_save_image(image: &str) -> Result<Vec<u8>> {
    let output = match Command::new("docker").arg("save").arg(image).output().await {
        Ok(output) => output,
        Err(first_error) => Command::new("/usr/bin/docker")
            .arg("save")
            .arg(image)
            .output()
            .await
            .with_context(|| format!("failed to run docker save: {first_error}"))?,
    };
    if !output.status.success() {
        anyhow::bail!(
            "docker save {} failed: {}",
            image,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn should_export_local_image(image: &str) -> bool {
    let image = image.trim();
    if image.is_empty() || image.contains("@sha256:") {
        return false;
    }
    let repository = image.split(':').next().unwrap_or(image);
    if repository.contains('/') {
        return false;
    }
    !matches!(
        repository,
        "alpine"
            | "busybox"
            | "debian"
            | "ubuntu"
            | "nginx"
            | "postgres"
            | "mysql"
            | "mariadb"
            | "redis"
            | "mongo"
            | "node"
            | "python"
            | "golang"
            | "httpd"
    )
}

fn sanitize_archive_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub async fn collect_topology(sys_extractor: &SysinfoExtractor) -> Result<NetworkTopologyPayload> {
    let host = sys_extractor.get_host_info().await?;
    let processes = sys_extractor.get_processes().await?;
    let resources = collect_runtime_resources().await;
    let mut payload = build_network_topology_from_resources(host, processes, resources.clone());
    enrich_database_schemas(&mut payload, &resources).await;

    Ok(payload)
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
    let mut database_schemas = BTreeMap::new();

    for resource in resources {
        match resource {
            ActiveResource::Container(container) => {
                let proto = proto_container_from_node(container);
                if let Some(schema) = database_schema_from_container(&proto) {
                    database_schemas.insert(database_schema_key(&schema), schema);
                }
                insert_routes(&mut routes, container_routes_from_proto_container(&proto));
                insert_container(&mut merged_containers, proto);
            }
            ActiveResource::Pod(pod) => {
                for container in pod.containers {
                    let proto = proto_container_from_node(container);
                    if let Some(schema) = database_schema_from_container(&proto) {
                        database_schemas.insert(database_schema_key(&schema), schema);
                    }
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
        database_schemas: database_schemas.into_values().collect(),
    }
}

fn database_schema_from_container(container: &ProtoContainer) -> Option<DatabaseSchema> {
    let url = env_value(
        &container.env,
        &["DATABASE_URL", "POSTGRES_URL", "POSTGRESQL_URL"],
    );
    let parsed_url = url.and_then(parse_postgres_url);
    let has_postgres_env = container.env.keys().any(|key| {
        let normalized = key.to_ascii_uppercase();
        normalized.starts_with("POSTGRES_") || normalized.starts_with("PG")
    });
    let mysql_url = env_value(&container.env, &["MYSQL_URL", "MARIADB_URL"]);
    let parsed_mysql_url = mysql_url.and_then(parse_mysql_url);
    let has_mysql_env = container.env.keys().any(|key| {
        let normalized = key.to_ascii_uppercase();
        normalized.starts_with("MYSQL_") || normalized.starts_with("MARIADB_")
    });
    let has_postgres_image = container.image.to_ascii_lowercase().contains("postgres")
        || container.image.to_ascii_lowercase().contains("postgis");
    let has_mysql_image = container.image.to_ascii_lowercase().contains("mysql")
        || container.image.to_ascii_lowercase().contains("mariadb");

    if has_mysql_env || has_mysql_image || mysql_url.is_some() {
        let parsed = parsed_mysql_url.unwrap_or_default();
        return Some(DatabaseSchema {
            engine: if container.image.to_ascii_lowercase().contains("mariadb") {
                "mariadb".to_string()
            } else {
                "mysql".to_string()
            },
            host: env_value(&container.env, &["MYSQL_HOST", "DB_HOST"])
                .map(str::to_string)
                .or(parsed.host),
            port: env_value(&container.env, &["MYSQL_PORT", "DB_PORT"])
                .and_then(|value| value.parse::<i32>().ok())
                .or(parsed.port),
            database_name: env_value(&container.env, &["MYSQL_DATABASE", "DB_NAME"])
                .map(str::to_string)
                .or(parsed.database_name),
            username: env_value(
                &container.env,
                &["MYSQL_USER", "MYSQL_ROOT_USER", "DB_USER"],
            )
            .map(str::to_string)
            .or(parsed.username),
            source_container_id: container.id.clone(),
            source_container_name: container.name.clone(),
            tables: Vec::new(),
        });
    }

    if !has_postgres_env && !has_postgres_image && parsed_url.is_none() {
        return None;
    }

    let parsed = parsed_url.unwrap_or_default();
    Some(DatabaseSchema {
        engine: "postgresql".to_string(),
        host: env_value(&container.env, &["POSTGRES_HOST", "PGHOST", "DB_HOST"])
            .map(str::to_string)
            .or(parsed.host),
        port: env_value(&container.env, &["POSTGRES_PORT", "PGPORT", "DB_PORT"])
            .and_then(|value| value.parse::<i32>().ok())
            .or(parsed.port),
        database_name: env_value(&container.env, &["POSTGRES_DB", "PGDATABASE", "DB_NAME"])
            .map(str::to_string)
            .or(parsed.database_name),
        username: env_value(&container.env, &["POSTGRES_USER", "PGUSER", "DB_USER"])
            .map(str::to_string)
            .or(parsed.username),
        source_container_id: container.id.clone(),
        source_container_name: container.name.clone(),
        tables: Vec::new(),
    })
}

async fn enrich_database_schemas(
    payload: &mut NetworkTopologyPayload,
    resources: &[ActiveResource],
) {
    let mut enriched_schemas = BTreeMap::new();
    for resource in resources {
        match resource {
            ActiveResource::Container(container) => {
                enrich_database_schema_from_container(container, &mut enriched_schemas).await;
            }
            ActiveResource::Pod(pod) => {
                for container in &pod.containers {
                    enrich_database_schema_from_container(container, &mut enriched_schemas).await;
                }
            }
            ActiveResource::Service(_) | ActiveResource::Ingress(_) => {}
        }
    }

    for schema in &mut payload.database_schemas {
        if let Some(enriched_schema) = enriched_schemas.remove(&database_schema_key(schema)) {
            if !enriched_schema.tables.is_empty() {
                schema.tables = enriched_schema.tables;
            }
        }
    }
}

async fn enrich_database_schema_from_container(
    container: &ContainerNode,
    enriched_schemas: &mut BTreeMap<String, DatabaseSchema>,
) {
    let proto = proto_container_from_node(container.clone());
    let Some(mut schema) = database_schema_from_container(&proto) else {
        return;
    };
    let schema_key = database_schema_key(&schema);
    crate::database::enrich_database_schema(&mut schema, &container.raw_env).await;
    enriched_schemas.insert(schema_key, schema);
}

fn env_value<'a>(env: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| env.get(*key).map(String::as_str))
}

#[derive(Default)]
struct ParsedPostgresUrl {
    host: Option<String>,
    port: Option<i32>,
    database_name: Option<String>,
    username: Option<String>,
}

fn parse_postgres_url(value: &str) -> Option<ParsedPostgresUrl> {
    let rest = value
        .strip_prefix("postgres://")
        .or_else(|| value.strip_prefix("postgresql://"))?;
    let without_query = rest.split(['?', '#']).next().unwrap_or(rest);
    let (credentials, host_path) = without_query
        .rsplit_once('@')
        .map(|(credentials, host_path)| (Some(credentials), host_path))
        .unwrap_or((None, without_query));
    let (host_port, database_name) = host_path
        .split_once('/')
        .map(|(host_port, database)| (host_port, non_empty(database)))
        .unwrap_or((host_path, None));
    let (host, port) = host_port
        .rsplit_once(':')
        .map(|(host, port)| (non_empty(host), port.parse::<i32>().ok()))
        .unwrap_or((non_empty(host_port), None));
    let username =
        credentials.and_then(|value| value.split_once(':').map(|(user, _)| user).or(Some(value)));

    Some(ParsedPostgresUrl {
        host: host.map(str::to_string),
        port,
        database_name: database_name.map(str::to_string),
        username: username.and_then(non_empty).map(str::to_string),
    })
}

fn parse_mysql_url(value: &str) -> Option<ParsedPostgresUrl> {
    let rest = value
        .strip_prefix("mysql://")
        .or_else(|| value.strip_prefix("mariadb://"))?;
    parse_database_url_rest(rest)
}

fn parse_database_url_rest(rest: &str) -> Option<ParsedPostgresUrl> {
    let without_query = rest.split(['?', '#']).next().unwrap_or(rest);
    let (credentials, host_path) = without_query
        .rsplit_once('@')
        .map(|(credentials, host_path)| (Some(credentials), host_path))
        .unwrap_or((None, without_query));
    let (host_port, database_name) = host_path
        .split_once('/')
        .map(|(host_port, database)| (host_port, non_empty(database)))
        .unwrap_or((host_path, None));
    let (host, port) = host_port
        .rsplit_once(':')
        .map(|(host, port)| (non_empty(host), port.parse::<i32>().ok()))
        .unwrap_or((non_empty(host_port), None));
    let username =
        credentials.and_then(|value| value.split_once(':').map(|(user, _)| user).or(Some(value)));

    Some(ParsedPostgresUrl {
        host: host.map(str::to_string),
        port,
        database_name: database_name.map(str::to_string),
        username: username.and_then(non_empty).map(str::to_string),
    })
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn database_schema_key(schema: &DatabaseSchema) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        schema.engine,
        schema.source_container_id,
        schema.host.clone().unwrap_or_default(),
        schema
            .port
            .map(|value| value.to_string())
            .unwrap_or_default(),
        schema.database_name.clone().unwrap_or_default()
    )
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
            for (key, value) in container.env.iter_mut() {
                *value = redact_env_value(key, value.as_str(), redactor);
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
    existing.labels.extend(incoming.labels.clone());

    if existing.id.is_empty() {
        existing.id = incoming.id.clone();
    }
    if existing.image == "unknown" && incoming.image != "unknown" {
        existing.image = incoming.image.clone();
    }
    if existing.image_version.is_none() {
        existing.image_version = incoming.image_version.clone();
    }
    if existing.image_hash.is_none() {
        existing.image_hash = incoming
            .image_hash
            .clone()
            .or_else(|| incoming.image_sha256.clone());
    }
    if existing.image_sha256.is_none() {
        existing.image_sha256 = incoming
            .image_sha256
            .clone()
            .or_else(|| incoming.image_hash.clone());
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
    for network in &incoming.networks {
        if !existing.networks.contains(network) {
            existing.networks.push(network.clone());
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

    let image_hash = container
        .image_hash
        .or_else(|| container.image_sha256.clone());
    let image_sha256 = container.image_sha256.or_else(|| image_hash.clone());

    let mut proto = ProtoContainer {
        id: normalize_container_id(&container.id),
        name: container.name,
        image: container.image,
        image_version: container.image_version,
        image_hash,
        image_sha256,
        image_archive_ref: container.image_archive_ref,
        image_archive_object: container.image_archive_object,
        env: container.env,
        labels: container.labels,
        networks: container.networks,
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

fn redact_env_value(key: &str, value: &str, redactor: &Redactor) -> String {
    redact_environment_value_with_redactor(key, value, redactor)
}

fn proto_process_from_node(process: crate::domain::ProcessNode) -> ProtoProcess {
    ProtoProcess {
        pid: process.pid.min(i32::MAX as u32) as i32,
        name: process.name,
        command_line: process.args.map(|args| args.join(" ")),
        user: Some(process.user),
    }
}

use crate::domain::{
    ActiveResource, ContainerNode, HostNode, PodNode, PortBindingNode, ProcessNode,
    SystemExtractor, TopologyExtractor,
};
use crate::extractor::{
    is_root_user, looks_sensitive_volume, parse_port_key, redact_environment_value,
    should_include_runtime_container,
};
use bollard::container::ListContainersOptions;
use bollard::Docker;
use bollard::API_DEFAULT_VERSION;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// SystemExtractor implementation using `bollard` to query the local Docker daemon.
pub struct DockerExtractor {
    docker: Docker,
}

impl DockerExtractor {
    /// Connects to the local Docker socket.
    pub fn new() -> anyhow::Result<Self> {
        let docker = connect_to_docker_socket()
            .map_err(|e| anyhow::anyhow!("Failed to connect to Docker socket: {}", e))?;
        Ok(Self { docker })
    }
}

fn connect_to_docker_socket() -> Result<Docker, bollard::errors::Error> {
    let mut last_error = None;

    for candidate in docker_socket_candidates(
        std::env::var("DOCKER_HOST").ok().as_deref(),
        docker_home_dir().as_deref(),
        docker_runtime_dir().as_deref(),
        read_current_docker_context(docker_home_dir().as_deref()).as_deref(),
    ) {
        match Docker::connect_with_unix(&candidate, 120, API_DEFAULT_VERSION) {
            Ok(docker) => return Ok(docker),
            Err(err) => last_error = Some(err),
        }
    }

    Docker::connect_with_local_defaults().map_err(|err| last_error.unwrap_or(err))
}

fn docker_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn docker_runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(current_user_runtime_dir)
}

#[cfg(target_os = "linux")]
fn current_user_runtime_dir() -> Option<PathBuf> {
    current_uid().map(|uid| PathBuf::from(format!("/run/user/{}", uid)))
}

#[cfg(not(target_os = "linux"))]
fn current_user_runtime_dir() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn current_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let uid_line = status.lines().find(|line| line.starts_with("Uid:"))?;
    uid_line.split_whitespace().nth(1)?.parse::<u32>().ok()
}

fn read_current_docker_context(home_dir: Option<&Path>) -> Option<String> {
    let config_path = home_dir?.join(".docker/config.json");
    let config = fs::read_to_string(config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&config).ok()?;
    value
        .get("currentContext")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn docker_socket_candidates(
    docker_host: Option<&str>,
    home_dir: Option<&Path>,
    runtime_dir: Option<&Path>,
    current_context: Option<&str>,
) -> Vec<String> {
    let mut candidates = Vec::new();

    if let Some(host) = docker_host {
        if host.starts_with("unix://") {
            candidates.push(host.to_string());
        } else if host.starts_with('/') {
            candidates.push(format!("unix://{}", host));
        }
    }

    if let Some(home) = home_dir {
        if let Some(context) = current_context {
            match context {
                "orbstack" => candidates.push(format!(
                    "unix://{}",
                    home.join(".orbstack/run/docker.sock").display()
                )),
                "desktop-linux" => candidates.push(format!(
                    "unix://{}",
                    home.join(".docker/run/docker.sock").display()
                )),
                _ => {}
            }
        }

        candidates.push(format!(
            "unix://{}",
            home.join(".orbstack/run/docker.sock").display()
        ));
        candidates.push(format!(
            "unix://{}",
            home.join(".docker/run/docker.sock").display()
        ));
    }

    if let Some(runtime) = runtime_dir {
        candidates.push(format!("unix://{}", runtime.join("docker.sock").display()));
    }

    candidates.push("unix:///var/run/docker.sock".to_string());

    let mut deduped = Vec::new();
    for candidate in candidates {
        if !deduped.contains(&candidate) {
            deduped.push(candidate);
        }
    }

    deduped
}

impl SystemExtractor for DockerExtractor {
    async fn get_host_info(&self) -> anyhow::Result<HostNode> {
        anyhow::bail!("DockerExtractor does not support host info extraction")
    }

    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>> {
        anyhow::bail!("DockerExtractor does not support process info extraction")
    }

    async fn get_pods(&self) -> anyhow::Result<Vec<PodNode>> {
        Ok(vec![])
    }

    async fn get_containers(&self) -> anyhow::Result<Vec<ContainerNode>> {
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        let containers = self
            .docker
            .list_containers(options)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list Docker containers: {}", e))?;

        let mut nodes = Vec::new();
        for c in containers {
            let node = map_container_to_node(c, None);
            if should_include_runtime_container(&node.name, &node.image) {
                nodes.push(node);
            }
        }

        // Second pass: enrich with detailed info if possible
        for node in &mut nodes {
            if let Ok(inspect) = self.docker.inspect_container(&node.id, None).await {
                enrich_node_with_inspect(node, inspect);
            }
        }

        Ok(nodes)
    }
}

impl TopologyExtractor for DockerExtractor {
    async fn list_active_resources(&self) -> anyhow::Result<Vec<ActiveResource>> {
        let options = Some(ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        });

        let containers = self
            .docker
            .list_containers(options)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list active Docker containers: {}", e))?;

        let mut nodes = Vec::new();
        for c in containers {
            let node = map_container_to_node(c, None);
            if should_include_runtime_container(&node.name, &node.image) {
                nodes.push(node);
            }
        }

        for node in &mut nodes {
            if let Ok(inspect) = self.docker.inspect_container(&node.id, None).await {
                enrich_node_with_inspect(node, inspect);
            }
        }

        Ok(nodes.into_iter().map(ActiveResource::Container).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::current_user_runtime_dir;

    #[test]
    fn test_current_user_runtime_dir_uses_current_uid_on_linux() {
        let runtime_dir = current_user_runtime_dir();

        #[cfg(target_os = "linux")]
        {
            use super::current_uid;

            let uid = current_uid().expect("expected to read current uid on linux");
            assert_eq!(
                runtime_dir.as_deref().map(|path| path.to_string_lossy().to_string()),
                Some(format!("/run/user/{}", uid))
            );
        }

        #[cfg(not(target_os = "linux"))]
        {
            assert!(runtime_dir.is_none());
        }
    }
}

pub fn map_container_to_node(
    c: bollard::models::ContainerSummary,
    inspect: Option<bollard::models::ContainerInspectResponse>,
) -> ContainerNode {
    let id = c.id.unwrap_or_else(|| "unknown".to_string());
    let raw_name = c
        .names
        .as_ref()
        .and_then(|n| n.first())
        .cloned()
        .unwrap_or_else(|| id.clone());

    let name = normalize_container_name(&raw_name);
    let image = c.image.unwrap_or_else(|| "unknown".to_string());
    let state = c.state.unwrap_or_else(|| "unknown".to_string());
    let image_sha256 = c.image_id;

    let mut env = BTreeMap::new();
    if let Some(labels) = c.labels {
        for (k, v) in labels {
            env.insert(format!("label:{}", k), v);
        }
    }

    let mut exposed_ports = Vec::new();
    if let Some(ports) = c.ports {
        for port in ports {
            exposed_ports.push(PortBindingNode {
                number: port.private_port as i32,
                protocol: port
                    .typ
                    .as_ref()
                    .map(|value| value.as_ref().to_string())
                    .unwrap_or_else(|| "tcp".to_string()),
                host_ip: port.ip,
                host_port: port.public_port.map(|value| value as i32),
                source: Some("docker_summary".to_string()),
            });
        }
    }

    let mut node = ContainerNode {
        id,
        name,
        image,
        image_sha256,
        state,
        env,
        exposed_ports,
        privileged: None,
        run_as_root: None,
        sensitive_volumes: Vec::new(),
    };

    if let Some(ins) = inspect {
        enrich_node_with_inspect(&mut node, ins);
    }

    node
}

pub fn enrich_node_with_inspect(
    node: &mut ContainerNode,
    inspect: bollard::models::ContainerInspectResponse,
) {
    if let Some(config) = inspect.config {
        if let Some(envs) = config.env {
            for e in envs {
                let parts: Vec<&str> = e.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let key = parts[0].to_string();
                    node.env
                        .insert(key.clone(), redact_environment_value(parts[1]));
                }
            }

            node.run_as_root = is_root_user(config.user.as_deref());
            if let Some(image) = config.image {
                node.image = image;
            }

            if let Some(exposed_ports) = config.exposed_ports {
                for (port_key, _) in exposed_ports {
                    if let Some((number, protocol)) = parse_port_key(&port_key) {
                        node.exposed_ports.push(PortBindingNode {
                            number,
                            protocol,
                            host_ip: None,
                            host_port: None,
                            source: Some("docker_exposed_ports".to_string()),
                        });
                    }
                }
            }
        }
    }

    if let Some(host_config) = inspect.host_config {
        node.privileged = host_config.privileged;

        if let Some(port_bindings) = host_config.port_bindings {
            for (port_key, bindings) in port_bindings {
                if let Some((number, protocol)) = parse_port_key(&port_key) {
                    if let Some(bindings) = bindings {
                        for binding in bindings {
                            let host_port = binding
                                .host_port
                                .as_deref()
                                .and_then(|value| value.parse::<i32>().ok());
                            node.exposed_ports.push(PortBindingNode {
                                number,
                                protocol: protocol.clone(),
                                host_ip: binding.host_ip,
                                host_port,
                                source: Some("docker_port_bindings".to_string()),
                            });
                        }
                    }
                }
            }
        }

        if let Some(mounts) = host_config.mounts {
            for mount in mounts {
                let destination = mount.target.unwrap_or_default();
                let source = mount.source.unwrap_or_default();
                if looks_sensitive_volume(&destination) || looks_sensitive_volume(&source) {
                    node.sensitive_volumes
                        .push(format!("{}:{}", source, destination));
                }
            }
        }

        if let Some(binds) = host_config.binds {
            for bind in binds {
                let mut parts = bind.split(':');
                let source = parts.next().unwrap_or_default().to_string();
                let destination = parts.next().unwrap_or_default().to_string();
                if looks_sensitive_volume(&destination) || looks_sensitive_volume(&source) {
                    node.sensitive_volumes
                        .push(format!("{}:{}", source, destination));
                }
            }
        }
    }

    if node.image_sha256.is_none() {
        node.image_sha256 = inspect.image;
    }
}

pub fn normalize_container_name(name: &str) -> String {
    name.strip_prefix('/').unwrap_or(name).to_string()
}

pub fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_uppercase();
    k.contains("PASS")
        || k.contains("SECRET")
        || k.contains("TOKEN")
        || k.contains("KEY")
        || k.contains("AUTH")
}

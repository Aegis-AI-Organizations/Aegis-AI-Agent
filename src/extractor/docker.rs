use crate::domain::{
    ActiveResource, ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor,
    TopologyExtractor,
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
            nodes.push(map_container_to_node(c, None));
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

        Ok(containers
            .into_iter()
            .map(|container| ActiveResource::Container(map_container_to_node(container, None)))
            .collect())
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

    let mut env = BTreeMap::new();
    if let Some(labels) = c.labels {
        for (k, v) in labels {
            env.insert(format!("label:{}", k), v);
        }
    }

    let mut node = ContainerNode {
        id,
        name,
        image,
        state,
        env,
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
                    let val = if is_sensitive_key(&key) {
                        "<redacted>".to_string()
                    } else {
                        parts[1].to_string()
                    };
                    node.env.insert(key, val);
                }
            }
        }
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

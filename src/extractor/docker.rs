use crate::domain::{ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor};
use bollard::container::ListContainersOptions;
use bollard::Docker;
use std::collections::BTreeMap;

/// SystemExtractor implementation using `bollard` to query the local Docker daemon.
pub struct DockerExtractor {
    docker: Docker,
}

impl DockerExtractor {
    /// Connects to the local Docker socket.
    pub fn new() -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| anyhow::anyhow!("Failed to connect to Docker socket: {}", e))?;
        Ok(Self { docker })
    }
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
            let id = c.id.clone().unwrap_or_else(|| "unknown".to_string());
            let raw_name = c
                .names
                .as_ref()
                .and_then(|n| n.first())
                .cloned()
                .unwrap_or_else(|| id.clone());

            let name = normalize_container_name(&raw_name);
            let image = c.image.clone().unwrap_or_else(|| "unknown".to_string());
            let state = c.state.clone().unwrap_or_else(|| "unknown".to_string());

            let mut env = BTreeMap::new();

            // Detailed inspection to get real environment variables
            match self.docker.inspect_container(&id, None).await {
                Ok(inspect) => {
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
                                    env.insert(key, val);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to inspect container {}: {}. Falling back to labels.",
                        id,
                        e
                    );
                    if let Some(labels) = c.labels {
                        for (k, v) in labels {
                            env.insert(format!("label:{}", k), v);
                        }
                    }
                }
            }

            nodes.push(ContainerNode {
                id,
                name,
                image,
                state,
                env,
            });
        }

        Ok(nodes)
    }
}

fn normalize_container_name(name: &str) -> String {
    name.strip_prefix('/').unwrap_or(name).to_string()
}

fn is_sensitive_key(key: &str) -> bool {
    let k = key.to_uppercase();
    k.contains("PASSWORD")
        || k.contains("SECRET")
        || k.contains("TOKEN")
        || k.contains("KEY")
        || k.contains("AUTH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_container_name() {
        assert_eq!(normalize_container_name("/redis"), "redis");
        assert_eq!(normalize_container_name("mysql"), "mysql");
    }

    #[test]
    fn test_is_sensitive_key() {
        assert!(is_sensitive_key("DB_PASSWORD"));
        assert!(is_sensitive_key("API_TOKEN"));
        assert!(is_sensitive_key("APP_SECRET"));
        assert!(!is_sensitive_key("APP_NAME"));
        assert!(!is_sensitive_key("DB_HOST"));
    }
}

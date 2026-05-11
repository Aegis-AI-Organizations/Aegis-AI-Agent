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

        let containers = self.docker.list_containers(options).await
            .map_err(|e| anyhow::anyhow!("Failed to list Docker containers: {}", e))?;

        let mut nodes = Vec::new();
        for c in containers {
            let id = c.id.unwrap_or_else(|| "unknown".to_string());
            let name = c.names.unwrap_or_default().first().cloned().unwrap_or_else(|| id.clone());
            let image = c.image.unwrap_or_else(|| "unknown".to_string());
            let state = c.state.unwrap_or_else(|| "unknown".to_string());

            // Extract labels as environment/metadata for now
            let mut env = BTreeMap::new();
            if let Some(labels) = c.labels {
                for (k, v) in labels {
                    env.insert(k, v);
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

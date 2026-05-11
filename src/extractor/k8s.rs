use crate::domain::{ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor};
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client, api::ListParams};
use std::collections::BTreeMap;

/// SystemExtractor implementation using the `kube-rs` crate to query Kubernetes API.
pub struct K8sExtractor {
    client: Client,
}

impl K8sExtractor {
    /// Attempts to initialize the K8s client using in-cluster config or local kubeconfig.
    pub async fn new() -> anyhow::Result<Self> {
        let client = Client::try_default().await?;
        Ok(Self { client })
    }
}

impl SystemExtractor for K8sExtractor {
    async fn get_host_info(&self) -> anyhow::Result<HostNode> {
        // K8s API doesn't directly provide host-level info in the same way sysinfo does
        anyhow::bail!("K8sExtractor does not support host info extraction")
    }

    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>> {
        // Processes are usually hidden from the K8s API unless using specialized nodes/crun
        anyhow::bail!("K8sExtractor does not support process info extraction")
    }

    async fn get_pods(&self) -> anyhow::Result<Vec<PodNode>> {
        let pods: Api<Pod> = Api::all(self.client.clone());
        let lp = ListParams::default();
        let list = pods.list(&lp).await?;

        let mut nodes = Vec::new();

        for p in list.items {
            let metadata = p.metadata;
            let spec = p.spec.as_ref();
            let status = p.status.as_ref();

            let mut containers = Vec::new();
            
            // Extract containers from spec
            if let Some(s) = spec {
                for c in &s.containers {
                    let mut env_map = BTreeMap::new();
                    if let Some(envs) = &c.env {
                        for e in envs {
                            if let Some(val) = &e.value {
                                env_map.insert(e.name.clone(), val.clone());
                            }
                        }
                    }

                    containers.push(ContainerNode {
                        id: "".to_string(), // ID is only available in status.container_statuses
                        name: c.name.clone(),
                        image: c.image.clone().unwrap_or_else(|| "unknown".to_string()),
                        state: "Unknown".to_string(),
                        env: env_map,
                    });
                }
            }

            // Enrich container state and ID from status if available
            if let Some(s) = status {
                if let Some(c_statuses) = &s.container_statuses {
                    for cs in c_statuses {
                        if let Some(target) = containers.iter_mut().find(|c| c.name == cs.name) {
                            target.id = cs.container_id.clone().unwrap_or_default();
                            target.state = format!("{:?}", cs.state);
                        }
                    }
                }
            }

            nodes.push(PodNode {
                name: metadata.name.unwrap_or_else(|| "unknown".to_string()),
                namespace: metadata.namespace.unwrap_or_else(|| "default".to_string()),
                ip: status.and_then(|s| s.pod_ip.clone()),
                labels: metadata.labels.unwrap_or_default(),
                containers,
                connections: Vec::new(), // Neighborhood mapping logic can be added here
            });
        }

        Ok(nodes)
    }
}

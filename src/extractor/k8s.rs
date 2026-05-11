use crate::domain::{ContainerNode, HostNode, PodNode, ProcessNode, SystemExtractor};
use k8s_openapi::api::core::v1::Pod;
use kube::{api::ListParams, Api, Client};
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
            nodes.push(map_pod_to_node(p));
        }

        Ok(nodes)
    }

    async fn get_containers(&self) -> anyhow::Result<Vec<ContainerNode>> {
        // Pods already contain containers in K8sExtractor::get_pods
        Ok(vec![])
    }
}

fn map_pod_to_node(p: Pod) -> PodNode {
    let metadata = p.metadata;
    let spec = p.spec;
    let status = p.status;

    let mut containers = Vec::new();

    // Extract containers from spec
    if let Some(s) = spec.as_ref() {
        for c in &s.containers {
            let mut env_map = BTreeMap::new();
            if let Some(envs) = &c.env {
                for e in envs {
                    if e.value.is_some() {
                        // Security: Redact environment variable values by default to avoid leaking secrets
                        env_map.insert(e.name.clone(), "<redacted>".to_string());
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
    if let Some(s) = status.as_ref() {
        if let Some(c_statuses) = &s.container_statuses {
            for cs in c_statuses {
                if let Some(target) = containers.iter_mut().find(|c| c.name == cs.name) {
                    target.id = cs.container_id.clone().unwrap_or_default();
                    target.state = format!("{:?}", cs.state);
                }
            }
        }
    }

    PodNode {
        name: metadata.name.unwrap_or_else(|| "unknown".to_string()),
        namespace: metadata.namespace.unwrap_or_else(|| "default".to_string()),
        ip: status.as_ref().and_then(|s| s.pod_ip.clone()),
        labels: metadata.labels.unwrap_or_default(),
        containers,
        connections: Vec::new(), // Neighborhood mapping logic can be added here
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodStatus};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    #[test]
    fn test_map_pod_to_node_basic() {
        let pod = Pod {
            metadata: ObjectMeta {
                name: Some("test-pod".to_string()),
                namespace: Some("test-ns".to_string()),
                labels: Some(BTreeMap::from([("app".to_string(), "test".to_string())])),
                ..Default::default()
            },
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "test-container".to_string(),
                    image: Some("test-image".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            status: Some(PodStatus {
                pod_ip: Some("10.0.0.1".to_string()),
                ..Default::default()
            }),
        };

        let node = map_pod_to_node(pod);
        assert_eq!(node.name, "test-pod");
        assert_eq!(node.namespace, "test-ns");
        assert_eq!(node.ip, Some("10.0.0.1".to_string()));
        assert_eq!(node.labels.get("app").unwrap(), "test");
        assert_eq!(node.containers.len(), 1);
        assert_eq!(node.containers[0].name, "test-container");
    }

    #[test]
    fn test_map_pod_to_node_redaction() {
        let pod = Pod {
            spec: Some(PodSpec {
                containers: vec![Container {
                    name: "c1".to_string(),
                    env: Some(vec![k8s_openapi::api::core::v1::EnvVar {
                        name: "SECRET_KEY".to_string(),
                        value: Some("super-secret".to_string()),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let node = map_pod_to_node(pod);
        let env = &node.containers[0].env;
        assert_eq!(env.get("SECRET_KEY").unwrap(), "<redacted>");
    }
}

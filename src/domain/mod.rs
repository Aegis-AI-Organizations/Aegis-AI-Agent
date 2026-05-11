use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Represents the host infrastructure of the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNode {
    pub hostname: String,
    pub os: String,
    pub kernel: String,
    /// System uptime in seconds.
    pub uptime: u64,
    /// Total RAM available in bytes.
    pub total_ram: u64,
}

/// Represents a running process on the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessNode {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub args: Option<Vec<String>>,
}

/// Represents a Docker/Runtime container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNode {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    /// Environment variables (key-value pairs).
    pub env: BTreeMap<String, String>,
}

/// Represents a network connection between pods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodConnection {
    pub target_pod: String,
    pub target_namespace: String,
    pub port: Option<u16>,
    pub protocol: Option<String>,
}

/// Represents a Kubernetes Pod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodNode {
    pub name: String,
    pub namespace: String,
    pub ip: Option<String>,
    /// Kubernetes labels.
    pub labels: BTreeMap<String, String>,
    /// List of containers within the pod.
    pub containers: Vec<ContainerNode>,
    /// Network connections/relations discovered.
    pub connections: Vec<PodConnection>,
}

/// Represents the complete system topology at a given time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyPayload {
    pub host: HostNode,
    pub processes: Vec<ProcessNode>,
    /// Optional list of discovered containers (Docker).
    pub containers: Option<Vec<ContainerNode>>,
    /// Optional list of discovered pods (Kubernetes).
    pub pods: Option<Vec<PodNode>>,
}

/// JSON payload matching `aegis.v2.NetworkTopology` from topology.proto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkTopologyPayload {
    pub hosts: Vec<ProtoHost>,
}

/// JSON representation of `aegis.v2.Host`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtoHost {
    pub id: String,
    pub hostname: String,
    pub ip_addresses: Vec<String>,
    pub containers: Vec<ProtoContainer>,
    pub processes: Vec<ProtoProcess>,
}

/// JSON representation of `aegis.v2.Container`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtoContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub processes: Vec<ProtoProcess>,
    pub ports: Vec<ProtoPort>,
}

/// JSON representation of `aegis.v2.Process`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtoProcess {
    pub pid: i32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// JSON representation of `aegis.v2.Port`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtoPort {
    pub number: i32,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Runtime resource discovered by a topology extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "resource", rename_all = "snake_case")]
pub enum ActiveResource {
    Container(ContainerNode),
    Pod(PodNode),
}

/// Interface for system data extraction.
#[allow(async_fn_in_trait)]
pub trait SystemExtractor: Send + Sync {
    /// Retrieves information about the host system.
    async fn get_host_info(&self) -> anyhow::Result<HostNode>;
    /// Retrieves a list of currently running processes.
    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>>;
    /// Retrieves a list of pods in the cluster (if in K8s).
    async fn get_pods(&self) -> anyhow::Result<Vec<PodNode>>;
    /// Retrieves a list of discovered containers (Docker).
    async fn get_containers(&self) -> anyhow::Result<Vec<ContainerNode>>;
}

/// Interface for runtime topology extraction.
///
/// Implementors expose active runtime resources through a common model, allowing
/// Docker and Kubernetes extractors to be exchanged transparently.
#[allow(async_fn_in_trait)]
pub trait TopologyExtractor: Send + Sync {
    /// Lists active resources visible to the extractor.
    async fn list_active_resources(&self) -> anyhow::Result<Vec<ActiveResource>>;

    /// Lists active containers visible to the extractor.
    async fn list_active_containers(&self) -> anyhow::Result<Vec<ContainerNode>> {
        Ok(self
            .list_active_resources()
            .await?
            .into_iter()
            .filter_map(|resource| match resource {
                ActiveResource::Container(container) => Some(container),
                ActiveResource::Pod(_) => None,
            })
            .collect())
    }

    /// Lists active pods visible to the extractor.
    async fn list_active_pods(&self) -> anyhow::Result<Vec<PodNode>> {
        Ok(self
            .list_active_resources()
            .await?
            .into_iter()
            .filter_map(|resource| match resource {
                ActiveResource::Pod(pod) => Some(pod),
                ActiveResource::Container(_) => None,
            })
            .collect())
    }
}

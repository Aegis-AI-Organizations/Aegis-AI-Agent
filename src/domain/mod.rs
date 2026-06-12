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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerNode {
    pub id: String,
    pub name: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sha256: Option<String>,
    pub state: String,
    /// Environment variables (key-value pairs).
    pub env: BTreeMap<String, String>,
    /// Runtime labels attached to the container.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Runtime network names the container is connected to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<String>,
    /// Exposed container ports or host bindings.
    pub exposed_ports: Vec<PortBindingNode>,
    /// Whether the container runs with elevated privileges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privileged: Option<bool>,
    /// Whether the container is configured to run as UID 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_as_root: Option<bool>,
    /// Sensitive mounts or volumes that should be tracked in the graph.
    pub sensitive_volumes: Vec<String>,
}

/// Represents a port exposure discovered from Docker or Kubernetes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBindingNode {
    pub number: i32,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
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

/// Represents a Kubernetes Service discovered by the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceNode {
    pub name: String,
    pub namespace: String,
    pub service_type: Option<String>,
    pub cluster_ip: Option<String>,
    pub selectors: BTreeMap<String, String>,
    pub ports: Vec<ServicePortNode>,
}

/// Represents a service port exposed by a Kubernetes Service.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePortNode {
    pub port: i32,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_protocol: Option<String>,
}

/// Represents a Kubernetes Ingress discovered by the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressNode {
    pub name: String,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ingress_class_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_backend: Option<IngressBackendNode>,
    pub rules: Vec<IngressRuleNode>,
}

/// Represents an ingress routing rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressRuleNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub paths: Vec<IngressPathNode>,
}

/// Represents a single path -> service backend mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressPathNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub path_type: String,
    pub backend: IngressBackendNode,
}

/// Represents a service backend target in an ingress rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressBackendNode {
    pub service_name: String,
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_number: Option<i32>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<ProtoRoute>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtoContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<String>,
    pub processes: Vec<ProtoProcess>,
    pub ports: Vec<ProtoPort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exposed_ports: Vec<ProtoPort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privileged: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_as_root: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensitive_volumes: Vec<String>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtoPort {
    pub number: i32,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// JSON representation of a routing rule to persist into Neo4j.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtoRoute {
    pub kind: String,
    pub source_kind: String,
    pub source_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_port: Option<i32>,
}

/// Runtime resource discovered by a topology extractor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "resource", rename_all = "snake_case")]
pub enum ActiveResource {
    Container(ContainerNode),
    Pod(PodNode),
    Service(ServiceNode),
    Ingress(IngressNode),
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
                ActiveResource::Pod(_)
                | ActiveResource::Service(_)
                | ActiveResource::Ingress(_) => None,
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
                ActiveResource::Container(_)
                | ActiveResource::Service(_)
                | ActiveResource::Ingress(_) => None,
            })
            .collect())
    }
}

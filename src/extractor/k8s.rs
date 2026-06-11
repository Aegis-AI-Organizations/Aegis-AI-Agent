use crate::domain::{
    ActiveResource, ContainerNode, HostNode, IngressBackendNode, IngressNode, IngressPathNode,
    IngressRuleNode, PodNode, PortBindingNode, ProcessNode, ServiceNode, ServicePortNode,
    SystemExtractor, TopologyExtractor,
};
use crate::extractor::should_include_k8s_namespace;
use crate::extractor::{looks_sensitive_volume, redact_environment_entry};
use k8s_openapi::api::core::v1::{Container, Pod, Service, Volume};
use k8s_openapi::api::networking::v1::{Ingress, IngressBackend, IngressRule};
use kube::{api::ListParams, Api, Client};
use std::collections::BTreeMap;
use std::env;

/// SystemExtractor implementation using the `kube-rs` crate to query Kubernetes API.
pub struct K8sExtractor {
    client: Client,
}

impl K8sExtractor {
    /// Attempts to initialize the K8s client using in-cluster config or an explicit kubeconfig.
    pub async fn new() -> anyhow::Result<Self> {
        if !should_attempt_kubeclient_autodiscovery() {
            anyhow::bail!(
                "Kubernetes discovery is disabled outside a Kubernetes pod; set KUBECONFIG to enable local kubeconfig access"
            );
        }

        let client = Client::try_default().await?;
        Ok(Self { client })
    }
}

fn should_attempt_kubeclient_autodiscovery() -> bool {
    if env::var_os("KUBECONFIG").is_some() {
        return true;
    }

    env::var_os("KUBERNETES_SERVICE_HOST").is_some()
        && env::var_os("KUBERNETES_SERVICE_PORT").is_some()
}

impl SystemExtractor for K8sExtractor {
    async fn get_host_info(&self) -> anyhow::Result<HostNode> {
        anyhow::bail!("K8sExtractor does not support host info extraction")
    }

    async fn get_processes(&self) -> anyhow::Result<Vec<ProcessNode>> {
        anyhow::bail!("K8sExtractor does not support process info extraction")
    }

    async fn get_pods(&self) -> anyhow::Result<Vec<PodNode>> {
        let pods: Api<Pod> = Api::all(self.client.clone());
        let lp = ListParams::default();
        let list = pods.list(&lp).await?;

        Ok(list.items.into_iter().map(map_pod_to_node).collect())
    }

    async fn get_containers(&self) -> anyhow::Result<Vec<ContainerNode>> {
        Ok(vec![])
    }
}

impl TopologyExtractor for K8sExtractor {
    async fn list_active_resources(&self) -> anyhow::Result<Vec<ActiveResource>> {
        let pods: Api<Pod> = Api::all(self.client.clone());
        let services: Api<Service> = Api::all(self.client.clone());
        let ingresses: Api<Ingress> = Api::all(self.client.clone());
        let lp = ListParams::default();

        let pod_list = pods.list(&lp).await?;
        let service_list = services.list(&lp).await?;
        let ingress_list = ingresses.list(&lp).await?;

        Ok(pod_list
            .items
            .into_iter()
            .filter(is_active_pod)
            .filter(|pod| {
                pod.metadata
                    .namespace
                    .as_deref()
                    .map(should_include_k8s_namespace)
                    .unwrap_or(true)
            })
            .map(map_pod_to_node)
            .map(ActiveResource::Pod)
            .chain(
                service_list
                    .items
                    .into_iter()
                    .filter(|service| {
                        service
                            .metadata
                            .namespace
                            .as_deref()
                            .map(should_include_k8s_namespace)
                            .unwrap_or(true)
                    })
                    .map(map_service_to_node)
                    .map(ActiveResource::Service),
            )
            .chain(
                ingress_list
                    .items
                    .into_iter()
                    .filter(|ingress| {
                        ingress
                            .metadata
                            .namespace
                            .as_deref()
                            .map(should_include_k8s_namespace)
                            .unwrap_or(true)
                    })
                    .map(map_ingress_to_node)
                    .map(ActiveResource::Ingress),
            )
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::should_attempt_kubeclient_autodiscovery;

    #[test]
    fn kubeclient_autodiscovery_is_disabled_without_explicit_env() {
        assert!(!should_attempt_kubeclient_autodiscovery());
    }
}

pub fn is_active_pod(p: &Pod) -> bool {
    !matches!(
        p.status.as_ref().and_then(|status| status.phase.as_deref()),
        Some("Succeeded" | "Failed")
    )
}

pub fn map_pod_to_node(p: Pod) -> PodNode {
    let metadata = p.metadata;
    let spec = p.spec;
    let status = p.status;

    let mut containers = Vec::new();

    let pod_security_context = spec.as_ref().and_then(|s| s.security_context.as_ref());
    let pod_volumes = spec
        .as_ref()
        .and_then(|s| s.volumes.as_ref())
        .cloned()
        .unwrap_or_default();

    if let Some(s) = spec.as_ref() {
        for c in &s.containers {
            containers.push(map_pod_container(
                c,
                status.as_ref(),
                pod_security_context,
                &pod_volumes,
            ));
        }
    }

    PodNode {
        name: metadata.name.unwrap_or_else(|| "unknown".to_string()),
        namespace: metadata.namespace.unwrap_or_else(|| "default".to_string()),
        ip: status.as_ref().and_then(|s| s.pod_ip.clone()),
        labels: metadata.labels.unwrap_or_default(),
        containers,
        connections: Vec::new(),
    }
}

pub fn map_service_to_node(service: Service) -> ServiceNode {
    let metadata = service.metadata;
    let namespace = metadata.namespace.unwrap_or_else(|| "default".to_string());
    let name = metadata.name.unwrap_or_else(|| "unknown".to_string());
    let spec = service.spec;

    let mut ports = Vec::new();
    let mut selectors = BTreeMap::new();

    if let Some(spec) = spec {
        if let Some(selector) = spec.selector {
            selectors = selector;
        }

        if let Some(service_ports) = spec.ports {
            for port in service_ports {
                ports.push(ServicePortNode {
                    port: port.port,
                    protocol: port.protocol.unwrap_or_else(|| "TCP".to_string()),
                    name: port.name,
                    node_port: port.node_port,
                    target_port: port.target_port.map(|value| match value {
                        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(v) => {
                            v.to_string()
                        }
                        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::String(v) => v,
                    }),
                    app_protocol: port.app_protocol,
                });
            }
        }

        ServiceNode {
            name,
            namespace,
            service_type: spec.type_,
            cluster_ip: spec.cluster_ip,
            selectors,
            ports,
        }
    } else {
        ServiceNode {
            name,
            namespace,
            service_type: None,
            cluster_ip: None,
            selectors,
            ports,
        }
    }
}

pub fn map_ingress_to_node(ingress: Ingress) -> IngressNode {
    let metadata = ingress.metadata;
    let namespace = metadata.namespace.unwrap_or_else(|| "default".to_string());
    let name = metadata.name.unwrap_or_else(|| "unknown".to_string());
    let spec = ingress.spec;

    let mut rules = Vec::new();
    let mut default_backend = None;
    let mut ingress_class_name = None;

    if let Some(spec) = spec {
        ingress_class_name = spec.ingress_class_name;
        default_backend = spec
            .default_backend
            .as_ref()
            .and_then(|backend| ingress_backend_to_node(backend, &namespace));

        if let Some(rule_list) = spec.rules {
            for rule in rule_list {
                rules.push(map_ingress_rule(rule, &namespace));
            }
        }
    }

    IngressNode {
        name,
        namespace,
        ingress_class_name,
        default_backend,
        rules,
    }
}

fn map_ingress_rule(rule: IngressRule, namespace: &str) -> IngressRuleNode {
    let mut paths = Vec::new();

    if let Some(http) = rule.http {
        for path in http.paths {
            if let Some(backend) = ingress_backend_to_node(&path.backend, namespace) {
                paths.push(IngressPathNode {
                    path: path.path,
                    path_type: path.path_type,
                    backend,
                });
            }
        }
    }

    IngressRuleNode {
        host: rule.host,
        paths,
    }
}

fn ingress_backend_to_node(
    backend: &IngressBackend,
    namespace: &str,
) -> Option<IngressBackendNode> {
    if let Some(service) = &backend.service {
        let port_name = service.port.as_ref().and_then(|value| value.name.clone());
        let port_number = service.port.as_ref().and_then(|value| value.number);
        return Some(IngressBackendNode {
            service_name: service.name.clone(),
            namespace: namespace.to_string(),
            port_name,
            port_number,
        });
    }

    if let Some(resource) = &backend.resource {
        return Some(IngressBackendNode {
            service_name: resource.name.clone(),
            namespace: namespace.to_string(),
            port_name: None,
            port_number: None,
        });
    }

    None
}

fn map_pod_container(
    c: &Container,
    status: Option<&k8s_openapi::api::core::v1::PodStatus>,
    pod_security_context: Option<&k8s_openapi::api::core::v1::PodSecurityContext>,
    pod_volumes: &[Volume],
) -> ContainerNode {
    let mut env_map = BTreeMap::new();
    if let Some(envs) = &c.env {
        for e in envs {
            let value = redact_environment_entry(&e.name, e.value.as_deref());
            env_map.insert(e.name.clone(), value);
        }
    }

    let mut exposed_ports = Vec::new();
    if let Some(ports) = &c.ports {
        for port in ports {
            exposed_ports.push(PortBindingNode {
                number: port.container_port,
                protocol: port.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                host_ip: port.host_ip.clone(),
                host_port: port.host_port,
                source: Some("k8s_container".to_string()),
            });
        }
    }

    let (privileged, run_as_root) = container_privileges(c, pod_security_context);
    let sensitive_volumes = collect_sensitive_volumes(c, pod_volumes);
    let image_sha256 = status.and_then(|status| {
        status.container_statuses.as_ref().and_then(|statuses| {
            statuses
                .iter()
                .find(|container_status| container_status.name == c.name)
                .map(|container_status| container_status.image_id.clone())
        })
    });

    let mut node = ContainerNode {
        id: String::new(),
        name: c.name.clone(),
        image: c.image.clone().unwrap_or_else(|| "unknown".to_string()),
        image_sha256,
        state: "Unknown".to_string(),
        env: env_map,
        exposed_ports,
        privileged,
        run_as_root,
        sensitive_volumes,
    };

    if let Some(status) = status {
        if let Some(c_statuses) = &status.container_statuses {
            for cs in c_statuses {
                if cs.name == c.name {
                    node.id = cs.container_id.clone().unwrap_or_default();
                    node.image_sha256 = Some(cs.image_id.clone());
                    node.state = format!("{:?}", cs.state);
                }
            }
        }
    }

    node
}

fn container_privileges(
    container: &Container,
    pod_security_context: Option<&k8s_openapi::api::core::v1::PodSecurityContext>,
) -> (Option<bool>, Option<bool>) {
    let privileged = container
        .security_context
        .as_ref()
        .and_then(|ctx| ctx.privileged)
        .or_else(|| {
            pod_security_context
                .and_then(|ctx| ctx.run_as_non_root)
                .map(|value| !value)
        });

    let run_as_root = container
        .security_context
        .as_ref()
        .and_then(|ctx| ctx.run_as_user)
        .map(|value| value == 0)
        .or_else(|| {
            pod_security_context
                .and_then(|ctx| ctx.run_as_user)
                .map(|value| value == 0)
        })
        .or_else(|| {
            container
                .security_context
                .as_ref()
                .and_then(|ctx| ctx.run_as_non_root)
                .map(|value| !value)
        })
        .or_else(|| {
            pod_security_context
                .and_then(|ctx| ctx.run_as_non_root)
                .map(|value| !value)
        });

    (privileged, run_as_root)
}

fn collect_sensitive_volumes(container: &Container, pod_volumes: &[Volume]) -> Vec<String> {
    let mut sensitive = Vec::new();

    if let Some(volume_mounts) = &container.volume_mounts {
        for mount in volume_mounts {
            if looks_sensitive_volume(&mount.mount_path) {
                sensitive.push(mount.mount_path.clone());
            }

            if let Some(volume) = pod_volumes.iter().find(|volume| volume.name == mount.name) {
                if let Some(host_path) = &volume.host_path {
                    if looks_sensitive_volume(&host_path.path) {
                        sensitive.push(host_path.path.clone());
                    }
                }
                if volume.secret.is_some()
                    || volume.config_map.is_some()
                    || volume.projected.is_some()
                {
                    sensitive.push(format!("{}:{}", mount.name, mount.mount_path));
                }
                if volume.persistent_volume_claim.is_some() {
                    sensitive.push(format!("pvc:{}:{}", mount.name, mount.mount_path));
                }
            }
        }
    }

    sensitive
}

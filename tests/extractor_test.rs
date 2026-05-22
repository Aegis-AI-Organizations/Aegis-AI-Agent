use aegis_ai_agent::extractor::{
    docker, filter_host_processes, k8s, ActiveResource, ContainerNode, PodNode, ProcessNode,
    SysinfoExtractor, SystemExtractor, TopologyExtractor,
};
use k8s_openapi::api::core::v1::{Container, Pod, PodSpec, PodStatus};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use std::collections::{BTreeMap, HashMap};

fn assert_topology_extractor<T: TopologyExtractor>() {}

struct FakeTopologyExtractor;

impl TopologyExtractor for FakeTopologyExtractor {
    async fn list_active_resources(&self) -> anyhow::Result<Vec<ActiveResource>> {
        Ok(vec![
            ActiveResource::Container(ContainerNode {
                id: "container-1".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
            }),
            ActiveResource::Pod(PodNode {
                name: "api-pod".to_string(),
                namespace: "default".to_string(),
                ip: Some("10.0.0.10".to_string()),
                labels: BTreeMap::new(),
                containers: Vec::new(),
                connections: Vec::new(),
            }),
        ])
    }
}

#[tokio::test]
async fn test_sysinfo_extractor_real_data() {
    let extractor = SysinfoExtractor::new();

    // Test Host Info
    let host_info = extractor
        .get_host_info()
        .await
        .expect("Failed to get host info");
    assert!(
        !host_info.hostname.is_empty(),
        "Hostname should not be empty"
    );
    assert!(
        host_info.total_ram > 0,
        "Total RAM should be greater than 0"
    );

    // Test Processes
    let processes = extractor
        .get_processes()
        .await
        .expect("Failed to get processes");
    assert!(!processes.is_empty(), "Process list should not be empty");

    // Verify that the current process is present
    let current_pid = std::process::id();
    let found = processes.iter().any(|p| p.pid == current_pid);
    assert!(
        found,
        "Current PID {} not found in process list",
        current_pid
    );
}

#[test]
fn test_runtime_extractors_implement_topology_extractor() {
    assert_topology_extractor::<docker::DockerExtractor>();
    assert_topology_extractor::<k8s::K8sExtractor>();
}

#[tokio::test]
async fn test_topology_extractor_default_resource_filters() {
    let extractor = FakeTopologyExtractor;

    let containers = extractor.list_active_containers().await.unwrap();
    let pods = extractor.list_active_pods().await.unwrap();

    assert_eq!(containers.len(), 1);
    assert_eq!(containers[0].id, "container-1");
    assert_eq!(pods.len(), 1);
    assert_eq!(pods[0].name, "api-pod");
}

#[test]
fn test_normalize_container_name() {
    assert_eq!(docker::normalize_container_name("/redis"), "redis");
    assert_eq!(docker::normalize_container_name("mysql"), "mysql");
}

#[test]
fn test_docker_socket_candidates_prioritize_current_context() {
    let home = std::path::Path::new("/Users/tester");
    let candidates = docker::docker_socket_candidates(None, Some(home), Some("orbstack"));

    assert_eq!(
        candidates.first().map(String::as_str),
        Some("unix:///Users/tester/.orbstack/run/docker.sock")
    );
    assert!(candidates.contains(&"unix:///var/run/docker.sock".to_string()));
}

#[test]
fn test_docker_socket_candidates_honor_docker_host_override() {
    let candidates = docker::docker_socket_candidates(
        Some("unix:///tmp/custom-docker.sock"),
        Some(std::path::Path::new("/Users/tester")),
        Some("desktop-linux"),
    );

    assert_eq!(
        candidates.first().map(String::as_str),
        Some("unix:///tmp/custom-docker.sock")
    );
}

#[test]
fn test_filter_host_processes_keeps_runtime_processes_and_drops_noise() {
    let filtered = filter_host_processes(vec![
        ProcessNode {
            pid: 10,
            name: "aegis-ai-agent".to_string(),
            user: "Uid(501)".to_string(),
            args: None,
        },
        ProcessNode {
            pid: 11,
            name: "com.docker.backend".to_string(),
            user: "Uid(501)".to_string(),
            args: None,
        },
        ProcessNode {
            pid: 12,
            name: "Browser Helper (Renderer)".to_string(),
            user: "Uid(501)".to_string(),
            args: None,
        },
        ProcessNode {
            pid: 13,
            name: "WidgetConfigurationExtension".to_string(),
            user: "Uid(501)".to_string(),
            args: None,
        },
        ProcessNode {
            pid: 14,
            name: "node".to_string(),
            user: "Uid(501)".to_string(),
            args: None,
        },
        ProcessNode {
            pid: 15,
            name: "SystemUIServer".to_string(),
            user: "unknown".to_string(),
            args: None,
        },
    ]);

    assert!(filtered
        .iter()
        .any(|process| process.name == "aegis-ai-agent"));
    assert!(filtered
        .iter()
        .any(|process| process.name == "com.docker.backend"));
    assert!(filtered.iter().any(|process| process.name == "node"));
    assert!(!filtered
        .iter()
        .any(|process| process.name == "Browser Helper (Renderer)"));
    assert!(!filtered
        .iter()
        .any(|process| process.name == "WidgetConfigurationExtension"));
    assert!(!filtered
        .iter()
        .any(|process| process.name == "SystemUIServer"));
}

#[test]
fn test_is_sensitive_key() {
    assert!(docker::is_sensitive_key("DB_PASSWORD"));
    assert!(docker::is_sensitive_key("API_TOKEN"));
    assert!(docker::is_sensitive_key("APP_SECRET"));
    assert!(!docker::is_sensitive_key("APP_NAME"));
    assert!(!docker::is_sensitive_key("DB_HOST"));
}

#[test]
fn test_map_container_to_node_basic() {
    let summary = bollard::models::ContainerSummary {
        id: Some("1234567890abcdef".to_string()),
        names: Some(vec!["/test-container".to_string()]),
        image: Some("nginx:latest".to_string()),
        state: Some("running".to_string()),
        labels: Some(HashMap::from([("version".to_string(), "1.0".to_string())])),
        ..Default::default()
    };

    let node = docker::map_container_to_node(summary, None);
    assert_eq!(node.id, "1234567890abcdef");
    assert_eq!(node.name, "test-container");
    assert_eq!(node.image, "nginx:latest");
    assert_eq!(node.state, "running");
    assert_eq!(node.env.get("label:version").unwrap(), "1.0");
}

#[test]
fn test_enrich_node_with_inspect() {
    let mut node = ContainerNode {
        id: "123".to_string(),
        name: "test".to_string(),
        image: "img".to_string(),
        state: "stat".to_string(),
        env: BTreeMap::new(),
    };

    let inspect = bollard::models::ContainerInspectResponse {
        config: Some(bollard::models::ContainerConfig {
            env: Some(vec![
                "DB_USER=admin".to_string(),
                "DB_PASS=secret123".to_string(),
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };

    docker::enrich_node_with_inspect(&mut node, inspect);
    assert_eq!(node.env.get("DB_USER").unwrap(), "admin");
    assert_eq!(node.env.get("DB_PASS").unwrap(), "<redacted>");
}

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

    let node = k8s::map_pod_to_node(pod);
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

    let node = k8s::map_pod_to_node(pod);
    let env = &node.containers[0].env;
    assert_eq!(env.get("SECRET_KEY").unwrap(), "<redacted>");
}

#[test]
fn test_map_pod_to_node_enrichment() {
    let pod = Pod {
        metadata: ObjectMeta {
            name: Some("test-pod".to_string()),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "c1".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        status: Some(PodStatus {
            container_statuses: Some(vec![k8s_openapi::api::core::v1::ContainerStatus {
                name: "c1".to_string(),
                container_id: Some("docker://id123".to_string()),
                state: Some(k8s_openapi::api::core::v1::ContainerState {
                    running: Some(k8s_openapi::api::core::v1::ContainerStateRunning {
                        started_at: None,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }),
    };

    let node = k8s::map_pod_to_node(pod);
    assert_eq!(node.containers[0].id, "docker://id123");
    assert!(node.containers[0].state.contains("running"));
}

#[test]
fn test_is_active_pod_filters_completed_pods() {
    let running = Pod {
        status: Some(PodStatus {
            phase: Some("Running".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let pending = Pod {
        status: Some(PodStatus {
            phase: Some("Pending".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let succeeded = Pod {
        status: Some(PodStatus {
            phase: Some("Succeeded".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let failed = Pod {
        status: Some(PodStatus {
            phase: Some("Failed".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    assert!(k8s::is_active_pod(&running));
    assert!(k8s::is_active_pod(&pending));
    assert!(!k8s::is_active_pod(&succeeded));
    assert!(!k8s::is_active_pod(&failed));
}

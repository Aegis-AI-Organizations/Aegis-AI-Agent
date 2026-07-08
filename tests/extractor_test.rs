use aegis_ai_agent::extractor::{
    docker, filter_host_processes, image_version_from_reference, is_root_user, k8s,
    looks_sensitive_volume, normalize_image_hash, parse_port_key, redact_environment_entry,
    redact_environment_value, should_include_k8s_namespace, should_include_runtime_container,
    ActiveResource, ContainerNode, PodNode, ProcessNode, SysinfoExtractor, SystemExtractor,
    TopologyExtractor,
};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, HostPathVolumeSource, Pod, PodSecurityContext, PodSpec, PodStatus,
    SecretVolumeSource, SecurityContext, Service, ServicePort, ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, ServiceBackendPort,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
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
                image_version: None,
                image_hash: None,
                image_sha256: None,
                state: "running".to_string(),
                env: BTreeMap::new(),
                labels: BTreeMap::new(),
                networks: Vec::new(),
                exposed_ports: Vec::new(),
                privileged: None,
                run_as_root: None,
                sensitive_volumes: Vec::new(),
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
    let candidates = docker::docker_socket_candidates(None, Some(home), None, Some("orbstack"));

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
        None,
        Some("desktop-linux"),
    );

    assert_eq!(
        candidates.first().map(String::as_str),
        Some("unix:///tmp/custom-docker.sock")
    );
}

#[test]
fn test_docker_socket_candidates_include_rootless_runtime_socket() {
    let candidates = docker::docker_socket_candidates(
        None,
        None,
        Some(std::path::Path::new("/run/user/1000")),
        None,
    );

    assert!(candidates.contains(&"unix:///run/user/1000/docker.sock".to_string()));
    assert!(candidates.contains(&"unix:///var/run/docker.sock".to_string()));
}

#[test]
fn test_docker_socket_path_from_candidate_strips_unix_scheme() {
    assert_eq!(
        docker::docker_socket_path_from_candidate("unix:///var/run/docker.sock"),
        "/var/run/docker.sock"
    );
    assert_eq!(
        docker::docker_socket_path_from_candidate("/run/user/1000/docker.sock"),
        "/run/user/1000/docker.sock"
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
fn test_should_include_runtime_container_filters_pause_sandboxes() {
    assert!(!should_include_runtime_container(
        "k8s_POD_coredns",
        "rancher/mirrored-pause:3.6"
    ));
    assert!(should_include_runtime_container(
        "aegis-brain",
        "python:3.11-slim"
    ));
}

#[test]
fn test_should_include_k8s_namespace_filters_system_namespaces() {
    assert!(!should_include_k8s_namespace("kube-system"));
    assert!(!should_include_k8s_namespace("cert-manager"));
    assert!(should_include_k8s_namespace("default"));
    assert!(should_include_k8s_namespace("production"));
}

#[test]
fn test_extractor_helpers_parse_ports_users_and_sensitive_values() {
    assert_eq!(parse_port_key("8080/UDP"), Some((8080, "udp".to_string())));
    assert_eq!(parse_port_key("443"), Some((443, "tcp".to_string())));
    assert_eq!(parse_port_key("invalid/tcp"), None);
    assert!(looks_sensitive_volume("/var/run/docker.sock"));
    assert!(looks_sensitive_volume("/home/aegis/.aws"));
    assert!(!looks_sensitive_volume("/srv/public"));
    assert_eq!(is_root_user(Some(" root ")), Some(true));
    assert_eq!(is_root_user(Some("0:0")), Some(true));
    assert_eq!(is_root_user(Some("1000")), Some(false));
    assert_eq!(is_root_user(None), None);
    assert_eq!(redact_environment_value("plain-value"), "plain-value");
    assert_eq!(
        redact_environment_entry("DB_PASS", Some("secret123")),
        "aegis-mock-secret"
    );
    assert_eq!(
        redact_environment_entry("POSTGRES_PASSWORD", Some("secret456")),
        "aegis-mock-secret"
    );
    assert_eq!(
        redact_environment_entry("DB_HOST", Some("postgres")),
        "postgres"
    );
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
    assert_eq!(node.labels.get("version").unwrap(), "1.0");
    assert!(node.env.is_empty());
}

#[test]
fn test_map_container_to_node_maps_summary_ports_and_defaults() {
    let summary = bollard::models::ContainerSummary {
        id: Some("123".to_string()),
        ports: Some(vec![bollard::models::Port {
            private_port: 8080,
            public_port: Some(3000),
            ip: Some("127.0.0.1".to_string()),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let node = docker::map_container_to_node(summary, None);
    assert_eq!(node.name, "123");
    assert_eq!(node.image, "unknown");
    assert_eq!(node.state, "unknown");
    assert_eq!(node.exposed_ports.len(), 1);
    assert_eq!(node.exposed_ports[0].number, 8080);
    assert_eq!(node.exposed_ports[0].host_port, Some(3000));
}

#[test]
fn test_enrich_node_with_inspect() {
    let mut node = ContainerNode {
        id: "123".to_string(),
        name: "test".to_string(),
        image: "img".to_string(),
        image_version: None,
        image_hash: None,
        image_sha256: None,
        state: "stat".to_string(),
        env: BTreeMap::new(),
        labels: BTreeMap::new(),
        networks: Vec::new(),
        exposed_ports: Vec::new(),
        privileged: None,
        run_as_root: None,
        sensitive_volumes: Vec::new(),
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
    assert_eq!(node.env.get("DB_PASS").unwrap(), "aegis-mock-secret");
}

#[test]
fn test_enrich_node_with_inspect_maps_runtime_security_metadata() {
    let mut node = ContainerNode {
        id: "123".to_string(),
        name: "test".to_string(),
        image: "img".to_string(),
        ..Default::default()
    };
    let inspect = bollard::models::ContainerInspectResponse {
        config: Some(bollard::models::ContainerConfig {
            env: Some(vec!["APP_ENV=production".to_string()]),
            user: Some("0:0".to_string()),
            image: Some("api:latest".to_string()),
            ..Default::default()
        }),
        host_config: Some(bollard::models::HostConfig {
            privileged: Some(true),
            port_bindings: Some(HashMap::from([(
                "8080/tcp".to_string(),
                Some(vec![bollard::models::PortBinding {
                    host_ip: Some("127.0.0.1".to_string()),
                    host_port: Some("3000".to_string()),
                }]),
            )])),
            mounts: Some(vec![bollard::models::Mount {
                source: Some("/var/run/docker.sock".to_string()),
                target: Some("/var/run/docker.sock".to_string()),
                ..Default::default()
            }]),
            binds: Some(vec!["/home/aegis/.aws:/root/.aws:ro".to_string()]),
            ..Default::default()
        }),
        image: Some("sha256:abc".to_string()),
        ..Default::default()
    };

    docker::enrich_node_with_inspect(&mut node, inspect);
    assert_eq!(node.image, "api:latest");
    assert_eq!(node.image_hash.as_deref(), Some("sha256:abc"));
    assert_eq!(node.image_sha256.as_deref(), Some("sha256:abc"));
    assert_eq!(node.privileged, Some(true));
    assert_eq!(node.run_as_root, Some(true));
    assert_eq!(node.exposed_ports.len(), 1);
    assert_eq!(node.exposed_ports[0].host_port, Some(3000));
    assert_eq!(node.sensitive_volumes.len(), 2);
}

#[test]
fn test_image_metadata_helpers_parse_versions_and_hashes() {
    assert_eq!(
        image_version_from_reference("registry.local:5000/nginx:1.21.0-alpine").as_deref(),
        Some("registry.local:5000/nginx:1.21.0-alpine")
    );
    assert_eq!(image_version_from_reference("nginx:latest"), None);
    assert_eq!(image_version_from_reference("nginx@sha256:abc"), None);
    assert_eq!(
        normalize_image_hash(Some("docker-pullable://nginx@sha256:abc")).as_deref(),
        Some("sha256:abc")
    );
    assert_eq!(
        normalize_image_hash(Some("sha256:def")).as_deref(),
        Some("sha256:def")
    );
    assert_eq!(normalize_image_hash(Some("nginx:latest")), None);
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
    assert_eq!(env.get("SECRET_KEY").unwrap(), "aegis-mock-secret");
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
fn test_map_pod_to_node_maps_ports_privileges_and_sensitive_volumes() {
    let pod = Pod {
        spec: Some(PodSpec {
            security_context: Some(PodSecurityContext {
                run_as_user: Some(0),
                ..Default::default()
            }),
            containers: vec![Container {
                name: "api".to_string(),
                image: Some("api:latest".to_string()),
                security_context: Some(SecurityContext {
                    privileged: Some(true),
                    ..Default::default()
                }),
                ports: Some(vec![ContainerPort {
                    container_port: 8080,
                    host_port: Some(3000),
                    ..Default::default()
                }]),
                volume_mounts: Some(vec![
                    VolumeMount {
                        name: "docker".to_string(),
                        mount_path: "/var/run/docker.sock".to_string(),
                        ..Default::default()
                    },
                    VolumeMount {
                        name: "secret".to_string(),
                        mount_path: "/srv/config".to_string(),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }],
            volumes: Some(vec![
                Volume {
                    name: "docker".to_string(),
                    host_path: Some(HostPathVolumeSource {
                        path: "/var/run/docker.sock".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Volume {
                    name: "secret".to_string(),
                    secret: Some(SecretVolumeSource::default()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };

    let node = k8s::map_pod_to_node(pod);
    let container = &node.containers[0];
    assert_eq!(container.privileged, Some(true));
    assert_eq!(container.run_as_root, Some(true));
    assert_eq!(container.exposed_ports[0].host_port, Some(3000));
    assert!(container
        .sensitive_volumes
        .contains(&"/var/run/docker.sock".to_string()));
    assert!(container
        .sensitive_volumes
        .contains(&"secret:/srv/config".to_string()));
}

#[test]
fn test_map_service_and_ingress_to_nodes() {
    let service = Service {
        metadata: ObjectMeta {
            name: Some("api".to_string()),
            namespace: Some("production".to_string()),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            cluster_ip: Some("10.96.0.10".to_string()),
            selector: Some(BTreeMap::from([("app".to_string(), "api".to_string())])),
            ports: Some(vec![
                ServicePort {
                    port: 80,
                    protocol: Some("TCP".to_string()),
                    target_port: Some(IntOrString::String("http".to_string())),
                    node_port: Some(30080),
                    ..Default::default()
                },
                ServicePort {
                    port: 443,
                    target_port: Some(IntOrString::Int(8443)),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let service = k8s::map_service_to_node(service);
    assert_eq!(service.namespace, "production");
    assert_eq!(
        service.selectors.get("app").map(String::as_str),
        Some("api")
    );
    assert_eq!(service.ports[0].target_port.as_deref(), Some("http"));
    assert_eq!(service.ports[1].target_port.as_deref(), Some("8443"));

    let backend = IngressBackend {
        service: Some(IngressServiceBackend {
            name: "api".to_string(),
            port: Some(ServiceBackendPort {
                name: Some("http".to_string()),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    let ingress = Ingress {
        metadata: ObjectMeta {
            name: Some("api-ingress".to_string()),
            namespace: Some("production".to_string()),
            ..Default::default()
        },
        spec: Some(IngressSpec {
            ingress_class_name: Some("nginx".to_string()),
            default_backend: Some(backend.clone()),
            rules: Some(vec![IngressRule {
                host: Some("api.example.com".to_string()),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        backend,
                        path: Some("/v1".to_string()),
                        path_type: "Prefix".to_string(),
                    }],
                }),
            }]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let ingress = k8s::map_ingress_to_node(ingress);
    assert_eq!(ingress.ingress_class_name.as_deref(), Some("nginx"));
    assert_eq!(
        ingress.default_backend.unwrap().service_name,
        "api".to_string()
    );
    assert_eq!(ingress.rules[0].paths[0].path.as_deref(), Some("/v1"));
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

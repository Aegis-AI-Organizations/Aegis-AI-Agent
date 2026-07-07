use aegis_ai_agent::discovery::{
    build_network_topology, build_network_topology_from_resources, collect_topology, redact_payload,
};
use aegis_ai_agent::domain::{
    ActiveResource, ContainerNode, HostNode, IngressBackendNode, IngressNode, IngressPathNode,
    IngressRuleNode, NetworkTopologyPayload, PodNode, PortBindingNode, ProcessNode, ServiceNode,
    ServicePortNode,
};
use aegis_ai_agent::extractor::SysinfoExtractor;
use aegis_ai_agent::redaction::Redactor;
use std::collections::BTreeMap;

#[tokio::test]
async fn test_collect_topology() {
    let sys_extractor = SysinfoExtractor::new();
    let payload = collect_topology(&sys_extractor).await.unwrap();

    assert_eq!(payload.hosts.len(), 1);
    assert!(!payload.hosts[0].hostname.is_empty());
    assert!(!payload.hosts[0].processes.is_empty());
}

#[test]
fn test_redact_payload() {
    let redactor = Redactor::new();
    let mut payload = build_network_topology(
        HostNode {
            hostname: "my-secret-host".to_string(),
            os: "Linux".to_string(),
            kernel: "5.15".to_string(),
            uptime: 3600,
            total_ram: 16000,
        },
        vec![ProcessNode {
            pid: 1234,
            name: "test-process".to_string(),
            user: "root".to_string(),
            args: Some(vec![
                "--password".to_string(),
                "super-secret-1234567890123456789012345678901234567890".to_string(),
            ]),
        }],
        vec![ContainerNode {
            id: "c1".to_string(),
            name: "my-container".to_string(),
            image: "nginx".to_string(),
            state: "running".to_string(),
            env: {
                let mut env = BTreeMap::new();
                env.insert("AWS_KEY".to_string(), "AKIA1234567890ABCDEF".to_string());
                env
            },
            ..Default::default()
        }],
        Vec::new(),
    );

    redact_payload(&mut payload, &redactor);

    let host = &payload.hosts[0];
    assert_eq!(host.hostname, "my-secret-host"); // Hostname is not PII by default in our regex
    assert!(host.processes[0]
        .command_line
        .as_ref()
        .unwrap()
        .contains("<REDACTED_SECRET>"));
    assert_eq!(
        host.containers[0].env.get("AWS_KEY").unwrap(),
        "AKIA0000000000000000"
    );
}

#[test]
fn test_build_network_topology_merges_docker_and_k8s_without_duplicates() {
    let payload = build_network_topology(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        vec![ProcessNode {
            pid: 42,
            name: "agent".to_string(),
            user: "aegis".to_string(),
            args: Some(vec!["--scan".to_string()]),
        }],
        vec![
            ContainerNode {
                id: "abc123".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
                ..Default::default()
            },
            ContainerNode {
                id: "standalone".to_string(),
                name: "worker".to_string(),
                image: "worker:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
                ..Default::default()
            },
        ],
        vec![PodNode {
            name: "api-pod".to_string(),
            namespace: "default".to_string(),
            ip: Some("10.0.0.10".to_string()),
            labels: BTreeMap::new(),
            containers: vec![ContainerNode {
                id: "docker://abc123".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
                ..Default::default()
            }],
            connections: Vec::new(),
        }],
    );

    let host = &payload.hosts[0];
    assert_eq!(host.id, "host-1");
    assert_eq!(host.processes.len(), 1);
    assert_eq!(host.containers.len(), 2);
    assert!(host
        .containers
        .iter()
        .any(|container| container.id == "abc123"));
    assert!(host
        .containers
        .iter()
        .any(|container| container.id == "standalone"));
}

#[test]
fn test_build_network_topology_deduplicates_ports_and_routes() {
    let payload = build_network_topology_from_resources(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        vec![ProcessNode {
            pid: 42,
            name: "agent".to_string(),
            user: "aegis".to_string(),
            args: Some(vec!["--scan".to_string()]),
        }],
        vec![ActiveResource::Container(ContainerNode {
            id: "abc123".to_string(),
            name: "api".to_string(),
            image: "api:latest".to_string(),
            state: "running".to_string(),
            env: BTreeMap::new(),
            exposed_ports: vec![
                PortBindingNode {
                    number: 8080,
                    protocol: "TCP".to_string(),
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(3000),
                    source: Some("docker_summary".to_string()),
                },
                PortBindingNode {
                    number: 8080,
                    protocol: "tcp".to_string(),
                    host_ip: Some("::".to_string()),
                    host_port: Some(3000),
                    source: Some("docker_port_bindings".to_string()),
                },
                PortBindingNode {
                    number: 8080,
                    protocol: "TCP".to_string(),
                    host_ip: None,
                    host_port: None,
                    source: Some("docker_exposed_ports".to_string()),
                },
            ],
            ..Default::default()
        })],
    );

    let container = &payload.hosts[0].containers[0];
    assert_eq!(container.exposed_ports.len(), 2);
    assert_eq!(container.ports.len(), 1);
    assert_eq!(container.ports[0].protocol, "tcp");
    assert_eq!(container.ports[0].host_port, Some(3000));
    assert_eq!(payload.routes.len(), 1);
    assert_eq!(payload.routes[0].protocol.as_deref(), Some("tcp"));
}

#[test]
fn test_build_network_topology_exports_postgres_schema_target_from_env() {
    let mut payload = build_network_topology_from_resources(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        Vec::new(),
        vec![ActiveResource::Container(ContainerNode {
            id: "api-container".to_string(),
            name: "api".to_string(),
            image: "api:latest".to_string(),
            state: "running".to_string(),
            env: BTreeMap::from([(
                "DATABASE_URL".to_string(),
                "postgres://app_user:secret-password@postgres.default.svc:5432/app_db?sslmode=disable"
                    .to_string(),
            )]),
            ..Default::default()
        })],
    );

    assert_eq!(payload.database_schemas.len(), 1);
    let schema = &payload.database_schemas[0];
    assert_eq!(schema.engine, "postgresql");
    assert_eq!(schema.host.as_deref(), Some("postgres.default.svc"));
    assert_eq!(schema.port, Some(5432));
    assert_eq!(schema.database_name.as_deref(), Some("app_db"));
    assert_eq!(schema.username.as_deref(), Some("app_user"));
    assert_eq!(schema.source_container_id, "api-container");
    assert_eq!(schema.source_container_name, "api");
    assert!(schema.tables.is_empty());

    redact_payload(&mut payload, &Redactor::new());
    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("databaseSchemas"));
    assert!(!json.contains("secret-password"));
    serde_json::from_str::<NetworkTopologyPayload>(&json).unwrap();
}

#[test]
fn test_build_network_topology_merges_metadata_and_exports_k8s_routes() {
    let payload = build_network_topology_from_resources(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        Vec::new(),
        vec![
            ActiveResource::Container(ContainerNode {
                id: "docker://abc123".to_string(),
                name: "api".to_string(),
                image: "unknown".to_string(),
                exposed_ports: vec![PortBindingNode {
                    number: 8080,
                    protocol: " TCP ".to_string(),
                    host_ip: Some("127.0.0.1".to_string()),
                    host_port: Some(3000),
                    source: Some("docker_summary".to_string()),
                }],
                sensitive_volumes: vec!["/etc/config".to_string()],
                ..Default::default()
            }),
            ActiveResource::Container(ContainerNode {
                id: "abc123".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                image_sha256: Some("sha256:abc".to_string()),
                env: BTreeMap::from([("API_TOKEN".to_string(), "secret".to_string())]),
                exposed_ports: vec![PortBindingNode {
                    number: 8080,
                    protocol: "tcp".to_string(),
                    host_ip: Some("127.0.0.1".to_string()),
                    host_port: Some(3000),
                    source: Some("docker_port_bindings".to_string()),
                }],
                privileged: Some(true),
                run_as_root: Some(true),
                sensitive_volumes: vec!["/etc/config".to_string(), "/run/secrets".to_string()],
                ..Default::default()
            }),
            ActiveResource::Service(ServiceNode {
                name: "api".to_string(),
                namespace: "default".to_string(),
                cluster_ip: Some("10.96.0.10".to_string()),
                ports: vec![ServicePortNode {
                    port: 80,
                    protocol: "TCP".to_string(),
                    node_port: Some(30080),
                    target_port: Some("http".to_string()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ActiveResource::Ingress(IngressNode {
                name: "api-ingress".to_string(),
                namespace: "default".to_string(),
                default_backend: Some(IngressBackendNode {
                    service_name: "api".to_string(),
                    namespace: "default".to_string(),
                    port_number: Some(80),
                    ..Default::default()
                }),
                rules: vec![IngressRuleNode {
                    host: Some("api.example.com".to_string()),
                    paths: vec![IngressPathNode {
                        path: Some("/v1".to_string()),
                        path_type: "Prefix".to_string(),
                        backend: IngressBackendNode {
                            service_name: "api".to_string(),
                            namespace: "default".to_string(),
                            port_name: Some("http".to_string()),
                            ..Default::default()
                        },
                    }],
                }],
                ..Default::default()
            }),
        ],
    );

    let container = &payload.hosts[0].containers[0];
    assert_eq!(container.id, "abc123");
    assert_eq!(container.image, "api:latest");
    assert_eq!(container.image_sha256.as_deref(), Some("sha256:abc"));
    assert_eq!(container.privileged, Some(true));
    assert_eq!(container.run_as_root, Some(true));
    assert_eq!(container.sensitive_volumes.len(), 2);
    assert_eq!(container.exposed_ports.len(), 1);
    assert_eq!(
        container.exposed_ports[0].source.as_deref(),
        Some("docker_port_bindings")
    );
    assert_eq!(payload.routes.len(), 4);
    assert!(payload.routes.iter().any(|route| {
        route.kind == "k8s_service"
            && route.host.as_deref() == Some("10.96.0.10")
            && route.target_port.as_deref() == Some("http")
    }));
    assert!(payload.routes.iter().any(|route| {
        route.kind == "k8s_ingress"
            && route.host.as_deref() == Some("api.example.com")
            && route.path.as_deref() == Some("/v1")
            && route.target_port.as_deref() == Some("http")
    }));
}

#[test]
fn test_build_network_topology_from_resources_assembles_single_payload() {
    let payload = build_network_topology_from_resources(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        vec![ProcessNode {
            pid: 42,
            name: "agent".to_string(),
            user: "aegis".to_string(),
            args: Some(vec!["--scan".to_string()]),
        }],
        vec![
            ActiveResource::Container(ContainerNode {
                id: "abc123".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
                ..Default::default()
            }),
            ActiveResource::Pod(PodNode {
                name: "api-pod".to_string(),
                namespace: "default".to_string(),
                ip: Some("10.0.0.10".to_string()),
                labels: BTreeMap::new(),
                containers: vec![
                    ContainerNode {
                        id: "docker://abc123".to_string(),
                        name: "api".to_string(),
                        image: "api:latest".to_string(),
                        state: "running".to_string(),
                        env: BTreeMap::new(),
                        ..Default::default()
                    },
                    ContainerNode {
                        id: "pod-only".to_string(),
                        name: "sidecar".to_string(),
                        image: "sidecar:latest".to_string(),
                        state: "running".to_string(),
                        env: BTreeMap::new(),
                        ..Default::default()
                    },
                ],
                connections: Vec::new(),
            }),
        ],
    );

    assert_eq!(payload.hosts.len(), 1);
    assert_eq!(payload.hosts[0].containers.len(), 2);
    assert!(payload.hosts[0]
        .containers
        .iter()
        .any(|container| container.id == "abc123"));
    assert!(payload.hosts[0]
        .containers
        .iter()
        .any(|container| container.id == "pod-only"));
}

#[test]
fn test_network_topology_json_matches_protobuf_contract() {
    let payload = build_network_topology(
        HostNode {
            hostname: "host-1".to_string(),
            os: "Linux".to_string(),
            kernel: "6.1".to_string(),
            uptime: 42,
            total_ram: 1024,
        },
        vec![ProcessNode {
            pid: 42,
            name: "agent".to_string(),
            user: "aegis".to_string(),
            args: Some(vec!["--scan".to_string()]),
        }],
        vec![ContainerNode {
            id: "c1".to_string(),
            name: "api".to_string(),
            image: "api:latest".to_string(),
            state: "running".to_string(),
            env: BTreeMap::new(),
            ..Default::default()
        }],
        Vec::new(),
    );

    let json = serde_json::to_string(&payload).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let host = &value["hosts"][0];
    let process = &host["processes"][0];
    let container = &host["containers"][0];
    let root_keys: std::collections::BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let host_keys: std::collections::BTreeSet<_> = host
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let process_keys: std::collections::BTreeSet<_> = process
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let container_keys: std::collections::BTreeSet<_> = container
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();

    assert!(value.get("hosts").is_some());
    assert!(host.get("ipAddresses").is_some());
    assert!(process.get("commandLine").is_some());
    assert!(host.get("pods").is_none());
    assert!(container.get("env").is_none());
    assert!(container.get("state").is_none());
    assert_eq!(root_keys, ["hosts"].into_iter().collect());
    assert_eq!(
        host_keys,
        ["containers", "hostname", "id", "ipAddresses", "processes"]
            .into_iter()
            .collect()
    );
    assert_eq!(
        process_keys,
        ["commandLine", "name", "pid", "user"].into_iter().collect()
    );
    assert_eq!(
        container_keys,
        ["id", "image", "name", "ports", "processes"]
            .into_iter()
            .collect()
    );

    serde_json::from_str::<NetworkTopologyPayload>(&json).unwrap();
}

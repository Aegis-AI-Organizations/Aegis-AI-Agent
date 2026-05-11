use aegis_ai_agent::discovery::{build_network_topology, collect_topology, redact_payload};
use aegis_ai_agent::domain::{
    ContainerNode, HostNode, NetworkTopologyPayload, PodNode, ProcessNode,
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
            },
            ContainerNode {
                id: "standalone".to_string(),
                name: "worker".to_string(),
                image: "worker:latest".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
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
        }],
        Vec::new(),
    );

    let json = serde_json::to_string(&payload).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let host = &value["hosts"][0];
    let process = &host["processes"][0];
    let container = &host["containers"][0];

    assert!(value.get("hosts").is_some());
    assert!(host.get("ipAddresses").is_some());
    assert!(process.get("commandLine").is_some());
    assert!(host.get("pods").is_none());
    assert!(container.get("env").is_none());
    assert!(container.get("state").is_none());

    serde_json::from_str::<NetworkTopologyPayload>(&json).unwrap();
}

#[test]
fn test_topology_edge_cases() {
    use aegis_ai_agent::domain::{ProtoContainer, ProtoProcess};
    let redactor = Redactor::new();

    // 1. Test redact_payload with host.id != host.hostname
    let mut payload = NetworkTopologyPayload {
        hosts: vec![aegis_ai_agent::domain::ProtoHost {
            id: "AKIA1234567890ABCDEF".to_string(), // Secret ID
            hostname: "normal-host".to_string(),
            ip_addresses: vec![],
            containers: vec![ProtoContainer {
                id: "".to_string(), // Empty ID for fallback key
                name: "api".to_string(),
                image: "nginx".to_string(),
                processes: vec![ProtoProcess {
                    pid: 1,
                    name: "super-secret-process-AKIA1234567890ABCDEF".to_string(),
                    command_line: None,
                    user: None,
                }],
                ports: vec![],
            }],
            processes: vec![],
        }],
    };

    redact_payload(&mut payload, &redactor);

    assert_eq!(payload.hosts[0].id, "<REDACTED_AWS_KEY>");
    assert_eq!(
        payload.hosts[0].containers[0].processes[0].name,
        "super-secret-process-<REDACTED_AWS_KEY>"
    );

    // 2. Test build_network_topology with containers missing IDs (fallback key)
    let payload2 = build_network_topology(
        HostNode {
            hostname: "host".to_string(),
            os: "os".to_string(),
            kernel: "k".to_string(),
            uptime: 0,
            total_ram: 0,
        },
        vec![],
        vec![ContainerNode {
            id: "".to_string(),
            name: "nameless".to_string(),
            image: "image".to_string(),
            state: "running".to_string(),
            env: BTreeMap::new(),
        }],
        vec![],
    );
    assert_eq!(payload2.hosts[0].containers[0].name, "nameless");
}

#[test]
fn test_merge_container_edge_cases() {
    // Test merging when first container has "unknown" image or empty ID
    // We can trigger this by having a Docker container and a K8s pod with same ID
    let payload = build_network_topology(
        HostNode {
            hostname: "h".to_string(),
            os: "o".to_string(),
            kernel: "k".to_string(),
            uptime: 0,
            total_ram: 0,
        },
        vec![],
        vec![ContainerNode {
            id: "id123".to_string(),
            name: "api".to_string(),
            image: "unknown".to_string(), // Trigger image merge
            state: "running".to_string(),
            env: BTreeMap::new(),
        }],
        vec![PodNode {
            name: "pod".to_string(),
            namespace: "ns".to_string(),
            ip: None,
            labels: BTreeMap::new(),
            containers: vec![ContainerNode {
                id: "docker://id123".to_string(),
                name: "api".to_string(),
                image: "real-image".to_string(),
                state: "running".to_string(),
                env: BTreeMap::new(),
            }],
            connections: vec![],
        }],
    );

    let container = &payload.hosts[0].containers[0];
    assert_eq!(container.id, "id123");
    assert_eq!(container.image, "real-image");
}

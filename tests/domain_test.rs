use aegis_ai_agent::domain::*;

#[test]
fn test_host_node_serialization() {
    let node = HostNode {
        hostname: "test-host".to_string(),
        os: "linux".to_string(),
        kernel: "5.15.0".to_string(),
        uptime: 3600,
        total_ram: 16000000000,
    };
    let json = serde_json::to_string(&node).unwrap();
    assert!(json.contains("test-host"));
    assert!(json.contains("uptime"));

    let decoded: HostNode = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.hostname, "test-host");
}

#[test]
fn test_process_node_serialization() {
    let node = ProcessNode {
        pid: 1234,
        name: "test-proc".to_string(),
        user: "root".to_string(),
        args: Some(vec!["--help".to_string()]),
    };
    let json = serde_json::to_string(&node).unwrap();
    assert!(json.contains("test-proc"));
    assert!(json.contains("1234"));

    let decoded: ProcessNode = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.pid, 1234);
}

#[test]
fn test_network_topology_payload_serialization() {
    let payload = NetworkTopologyPayload {
        hosts: vec![ProtoHost {
            id: "host-1".to_string(),
            hostname: "host-1".to_string(),
            ip_addresses: vec!["127.0.0.1".to_string()],
            containers: vec![ProtoContainer {
                id: "container-1".to_string(),
                name: "api".to_string(),
                image: "api:latest".to_string(),
                processes: Vec::new(),
                ports: vec![ProtoPort {
                    number: 8080,
                    protocol: "tcp".to_string(),
                    state: Some("LISTEN".to_string()),
                }],
            }],
            processes: vec![ProtoProcess {
                pid: 1234,
                name: "agent".to_string(),
                command_line: Some("--scan".to_string()),
                user: Some("aegis".to_string()),
            }],
        }],
    };

    let json = serde_json::to_string(&payload).unwrap();
    assert!(json.contains("ipAddresses"));
    assert!(json.contains("commandLine"));
    assert!(json.contains("LISTEN"));

    let decoded: NetworkTopologyPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.hosts[0].containers[0].ports[0].number, 8080);
}

#[test]
fn test_active_resource_serialization() {
    let resource = ActiveResource::Container(ContainerNode {
        id: "container-1".to_string(),
        name: "api".to_string(),
        image: "api:latest".to_string(),
        state: "running".to_string(),
        env: Default::default(),
    });

    let json = serde_json::to_string(&resource).unwrap();
    assert!(json.contains("container"));

    let decoded: ActiveResource = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, resource);
}

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

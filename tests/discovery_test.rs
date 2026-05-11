use aegis_ai_agent::discovery::{collect_topology, redact_payload};
use aegis_ai_agent::domain::{ContainerNode, HostNode, ProcessNode, TopologyPayload};
use aegis_ai_agent::extractor::SysinfoExtractor;
use aegis_ai_agent::redaction::Redactor;
use std::collections::BTreeMap;

#[tokio::test]
async fn test_collect_topology() {
    let sys_extractor = SysinfoExtractor::new();
    let payload = collect_topology(&sys_extractor).await.unwrap();

    assert!(!payload.host.hostname.is_empty());
    assert!(!payload.processes.is_empty());
}

#[test]
fn test_redact_payload() {
    let redactor = Redactor::new();
    let mut payload = TopologyPayload {
        host: HostNode {
            hostname: "my-secret-host".to_string(),
            os: "Linux".to_string(),
            kernel: "5.15".to_string(),
            uptime: 3600,
            total_ram: 16000,
        },
        processes: vec![ProcessNode {
            pid: 1234,
            name: "test-process".to_string(),
            user: "root".to_string(),
            args: Some(vec![
                "--password".to_string(),
                "super-secret-1234567890123456789012345678901234567890".to_string(),
            ]),
        }],
        containers: Some(vec![ContainerNode {
            id: "c1".to_string(),
            name: "my-container".to_string(),
            image: "nginx".to_string(),
            state: "running".to_string(),
            env: {
                let mut env = BTreeMap::new();
                env.insert("AWS_KEY".to_string(), "AKIA1234567890ABCDEF".to_string());
                env
            },
        }]),
        pods: None,
    };

    redact_payload(&mut payload, &redactor);

    assert_eq!(payload.host.hostname, "my-secret-host"); // Hostname is not PII by default in our regex
    assert!(payload.processes[0].args.as_ref().unwrap()[1].contains("<REDACTED_SECRET>"));
    assert!(payload.containers.as_ref().unwrap()[0]
        .env
        .get("AWS_KEY")
        .unwrap()
        .contains("<REDACTED_AWS_KEY>"));
}

use aegis_ai_agent::agent::*;
use aegis_ai_agent::config::{self, AgentConfig};
use serial_test::serial;
use std::env;
use std::time::Duration;

#[test]
fn test_startup_message() {
    assert_eq!(startup_message(), "Aegis AI Agent initialized.");
}

#[tokio::test]
#[serial]
async fn test_load_or_register_agent_uses_local_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".agent_secret_test");
    let config = AgentConfig {
        agent_id: "agent-local".to_string(),
        agent_secret: "secret-local".to_string(),
    };

    unsafe {
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path.to_str().unwrap());
    }
    config::save_config(&config).unwrap();

    let loaded = load_or_register_agent_exposed().await.unwrap();
    assert_eq!(loaded.agent_id, "agent-local");
    assert_eq!(loaded.agent_secret, "secret-local");

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
    }
}

#[tokio::test]
#[serial]
async fn test_load_or_register_agent_requires_deployment_token_without_local_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".missing_agent_secret");

    unsafe {
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path.to_str().unwrap());
        env::remove_var("DEPLOYMENT_TOKEN");
    }

    let result = load_or_register_agent_exposed().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("DEPLOYMENT_TOKEN"));

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
    }
}

#[tokio::test]
#[serial]
async fn test_load_or_register_agent_registers_and_persists_config() {
    let mut server = mockito::Server::new_async().await;
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".agent_secret_test");

    let _m = server
        .mock("POST", "/api/agents/register")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"agent_id": "agent-new", "agent_secret": "secret-new"}"#)
        .create_async()
        .await;

    unsafe {
        env::remove_var("SKIP_AGENT_INIT");
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path.to_str().unwrap());
        env::set_var(
            "DEPLOYMENT_TOKEN",
            "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
        );
        env::set_var("GATEWAY_URL", server.url());
        env::set_var("AGENT_NAME", "agent-01");
        env::set_var("AGENT_ALLOW_HTTP", "true");
    }

    let config = load_or_register_agent_exposed().await.unwrap();
    assert_eq!(config.agent_id, "agent-new");
    assert_eq!(config.agent_secret, "secret-new");
    assert!(config::load_local_config().unwrap().is_some());

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
        env::remove_var("DEPLOYMENT_TOKEN");
        env::remove_var("GATEWAY_URL");
        env::remove_var("AGENT_NAME");
        env::remove_var("AGENT_ALLOW_HTTP");
    }
}

#[tokio::test]
#[serial]
async fn test_init_agent_loads_local_config() {
    let mut server = mockito::Server::new_async().await;
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".agent_secret_test");
    let config = AgentConfig {
        agent_id: "agent-local".to_string(),
        agent_secret: "secret-local".to_string(),
    };

    let _m = server
        .mock("POST", "/api/agents/agent-local/status")
        .with_status(200)
        .create_async()
        .await;

    unsafe {
        env::remove_var("SKIP_AGENT_INIT");
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path.to_str().unwrap());
        env::set_var("GATEWAY_URL", server.url());
        env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    config::save_config(&config).unwrap();

    init_agent().await.unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
        env::remove_var("GATEWAY_URL");
        env::remove_var("AGENT_ALLOW_HTTP");
    }
}

use crate::client::AegisClient;
use crate::config::{self, AgentConfig};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time;

pub fn startup_message() -> &'static str {
    "Aegis AI Agent initialized."
}

pub async fn init_agent() -> Result<()> {
    tracing::info!("{}", startup_message());

    if std::env::var("SKIP_AGENT_INIT")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        tracing::info!("Skipping agent initialization (SKIP_AGENT_INIT=1)");
        return Ok(());
    }

    let config = load_or_register_agent().await?;
    tracing::info!("Agent registered/loaded with ID: {}", config.agent_id);

    // Start heartbeat in background
    tokio::spawn(async move {
        if let Err(e) = start_heartbeat_loop(config).await {
            tracing::error!("Heartbeat error: {}", e);
        }
    });

    Ok(())
}

async fn load_or_register_agent() -> Result<AgentConfig> {
    if let Some(config) = config::load_local_config()? {
        return Ok(config);
    }

    // No local secret, perform registration
    let gateway_url = config::get_gateway_url();
    let deploy_token = config::get_deployment_token()?;
    let agent_name = config::get_agent_name();

    let client = AegisClient::new(gateway_url);
    let config = client.register(&deploy_token, agent_name).await?;

    // Save secret locally
    config::save_config(&config).context("Failed to persist agent configuration")?;

    Ok(config)
}

async fn start_heartbeat_loop(config: AgentConfig) -> Result<()> {
    let gateway_url = config::get_gateway_url();
    let client = AegisClient::new(gateway_url);
    let mut interval = time::interval(Duration::from_secs(30));

    loop {
        interval.tick().await;
        if let Err(e) = client.send_heartbeat(&config).await {
            tracing::error!("Heartbeat failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

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

        let loaded = load_or_register_agent().await.unwrap();
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

        let result = load_or_register_agent().await;
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
        }

        let config = load_or_register_agent().await.unwrap();
        assert_eq!(config.agent_id, "agent-new");
        assert_eq!(config.agent_secret, "secret-new");
        assert!(config::load_local_config().unwrap().is_some());

        unsafe {
            env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
            env::remove_var("DEPLOYMENT_TOKEN");
            env::remove_var("GATEWAY_URL");
            env::remove_var("AGENT_NAME");
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
        }
        config::save_config(&config).unwrap();

        init_agent().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        unsafe {
            env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
            env::remove_var("GATEWAY_URL");
        }
    }
}

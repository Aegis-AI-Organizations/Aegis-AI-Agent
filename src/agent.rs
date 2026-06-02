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

    if std::env::var("SKIP_AGENT_LOOPS")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        tracing::info!("Skipping agent background loops (SKIP_AGENT_LOOPS=1)");
        return Ok(());
    }

    // Start discovery and heartbeat in background
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::discovery::start_discovery_loop(config_clone).await {
            tracing::error!("Discovery loop error: {}", e);
        }
    });

    tokio::spawn(async move {
        if let Err(e) = start_heartbeat_loop(config).await {
            tracing::error!("Heartbeat error: {}", e);
        }
    });

    Ok(())
}

async fn load_or_register_agent() -> Result<AgentConfig> {
    if let Some(config) = config::load_local_config()? {
        if let Ok(validated) = validate_loaded_agent(config.clone()).await {
            return Ok(validated);
        }
    }

    // No local secret, perform registration
    register_agent().await
}

pub async fn load_or_register_agent_exposed() -> Result<AgentConfig> {
    load_or_register_agent().await
}

async fn validate_loaded_agent(config: AgentConfig) -> Result<AgentConfig> {
    let client = AegisClient::new(config::get_gateway_url());

    match client.send_heartbeat(&config).await {
        Ok(_) => Ok(config),
        Err(err) => {
            let err_text = err.to_string();
            if is_stale_agent_secret_error(&err_text) {
                tracing::warn!(
                    "Stored agent credentials are no longer valid, re-registering agent: {}",
                    err_text
                );
                register_agent().await
            } else {
                tracing::info!(
                    "Stored agent credentials could not be validated right now, keeping local config: {}",
                    err_text
                );
                Ok(config)
            }
        }
    }
}

async fn register_agent() -> Result<AgentConfig> {
    let gateway_url = config::get_gateway_url();
    let deploy_token = config::get_deployment_token()?;
    let agent_name = config::get_agent_name();

    let client = AegisClient::new(gateway_url);
    let config = client.register(&deploy_token, agent_name).await?;

    config::save_config(&config).context("Failed to persist agent configuration")?;

    Ok(config)
}

fn is_stale_agent_secret_error(err_text: &str) -> bool {
    let has_status = |status: &str| err_text.contains(&format!("status: {}", status));
    has_status("401") || has_status("403") || has_status("404")
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

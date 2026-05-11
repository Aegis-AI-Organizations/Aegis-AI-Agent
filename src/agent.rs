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

#[doc(hidden)]
pub async fn load_or_register_agent_exposed() -> Result<AgentConfig> {
    load_or_register_agent().await
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

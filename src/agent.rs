use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;
use tokio::time;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentConfig {
    pub agent_id: String,
    pub agent_secret: String,
}

#[derive(Serialize)]
struct RegisterRequest {
    name: String,
}

#[derive(Deserialize)]
struct RegisterResponse {
    agent_id: String,
    agent_secret: String,
}

#[derive(Serialize)]
struct StatusUpdate {
    status: String,
}

pub fn startup_message() -> &'static str {
    "Aegis AI Agent initialized."
}

pub async fn init_agent() -> Result<()> {
    println!("{}", startup_message());

    if std::env::var("SKIP_AGENT_INIT").map(|v| v == "1").unwrap_or(false) {
        println!("Skipping agent initialization (SKIP_AGENT_INIT=1)");
        return Ok(());
    }

    let config = load_or_register().await?;
    println!("Agent registered/loaded with ID: {}", config.agent_id);

    // Start heartbeat in background
    let config_clone = config.clone();
    tokio::spawn(async move {
        if let Err(e) = start_heartbeat(config_clone).await {
            eprintln!("Heartbeat error: {}", e);
        }
    });

    Ok(())
}

async fn load_or_register() -> Result<AgentConfig> {
    let secret_path = ".agent_secret";
    
    if Path::new(secret_path).exists() {
        let content = fs::read_to_string(secret_path)
            .context("Failed to read agent secret file")?;
        let config: AgentConfig = serde_json::from_str(&content)
            .context("Failed to parse agent secret file")?;
        return Ok(config);
    }

    // No local secret, perform registration
    let gateway_url = std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let deploy_token = std::env::var("DEPLOYMENT_TOKEN").context("DEPLOYMENT_TOKEN environment variable is required for registration")?;
    let agent_name = std::env::var("AGENT_NAME").unwrap_or_else(|_| "rust-agent-01".to_string());

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/agents/register", gateway_url))
        .header("Authorization", format!("Bearer {}", deploy_token))
        .json(&RegisterRequest { name: agent_name })
        .send()
        .await
        .context("Failed to send registration request")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let error_text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Registration failed with status {}: {}", status, error_text);
    }

    let reg_resp: RegisterResponse = resp.json().await.context("Failed to parse registration response")?;
    
    let config = AgentConfig {
        agent_id: reg_resp.agent_id,
        agent_secret: reg_resp.agent_secret,
    };

    // Save secret locally with 600 permissions
    let config_json = serde_json::to_string(&config)?;
    fs::write(secret_path, config_json).context("Failed to write agent secret file")?;
    
    let mut perms = fs::metadata(secret_path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(secret_path, perms).context("Failed to set permissions on agent secret file")?;

    Ok(config)
}

async fn start_heartbeat(config: AgentConfig) -> Result<()> {
    let gateway_url = std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = reqwest::Client::new();
    let mut interval = time::interval(Duration::from_secs(30));

    loop {
        interval.tick().await;
        
        let status = StatusUpdate {
            status: "RUNNING".to_string(),
        };

        let res = client
            .post(format!("{}/api/agents/{}/status", gateway_url, config.agent_id))
            .header("Authorization", format!("Bearer {}", config.agent_secret))
            .json(&status)
            .send()
            .await;

        match res {
            Ok(resp) if resp.status().is_success() => {
                // Heartbeat success
            }
            Ok(resp) => {
                eprintln!("Heartbeat failed with status: {}", resp.status());
            }
            Err(e) => {
                eprintln!("Heartbeat network error: {}", e);
            }
        }
    }
}

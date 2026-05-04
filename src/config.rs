use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const AGENT_SECRET_FILE: &str = ".agent_secret";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentConfig {
    pub agent_id: String,
    pub agent_secret: String,
}

pub fn load_local_config() -> Result<Option<AgentConfig>> {
    if Path::new(AGENT_SECRET_FILE).exists() {
        let content =
            fs::read_to_string(AGENT_SECRET_FILE).context("Failed to read agent secret file")?;
        let config: AgentConfig =
            serde_json::from_str(&content).context("Failed to parse agent secret file")?;
        return Ok(Some(config));
    }
    Ok(None)
}

pub fn save_config(config: &AgentConfig) -> Result<()> {
    let config_json = serde_json::to_string(config)?;
    fs::write(AGENT_SECRET_FILE, config_json).context("Failed to write agent secret file")?;

    let mut perms = fs::metadata(AGENT_SECRET_FILE)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(AGENT_SECRET_FILE, perms)
        .context("Failed to set permissions on agent secret file")?;

    Ok(())
}

pub fn get_gateway_url() -> String {
    std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

pub fn get_deployment_token() -> Result<String> {
    std::env::var("DEPLOYMENT_TOKEN").context("DEPLOYMENT_TOKEN environment variable is required")
}

pub fn get_agent_name() -> String {
    std::env::var("AGENT_NAME").unwrap_or_else(|_| "rust-agent-01".to_string())
}

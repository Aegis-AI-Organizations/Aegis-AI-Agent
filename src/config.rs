use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const DEPLOYMENT_TOKEN_PREFIX: &str = "ag_";
pub const DEPLOYMENT_TOKEN_BODY_MIN_LEN: usize = 43;

pub fn get_agent_secret_file() -> String {
    std::env::var("AGENT_SECRET_FILE_OVERRIDE").unwrap_or_else(|_| ".agent_secret".to_string())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentConfig {
    pub agent_id: String,
    pub agent_secret: String,
}

pub fn load_local_config() -> Result<Option<AgentConfig>> {
    let path = get_agent_secret_file();
    if Path::new(&path).exists() {
        let content = fs::read_to_string(&path).context("Failed to read agent secret file")?;
        let config: AgentConfig =
            serde_json::from_str(&content).context("Failed to parse agent secret file")?;
        return Ok(Some(config));
    }
    Ok(None)
}

pub fn save_config(config: &AgentConfig) -> Result<()> {
    let config_json = serde_json::to_string(config)?;
    let path = get_agent_secret_file();

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .context("Failed to create agent secret file with restricted permissions")?;

        use std::io::Write;
        file.write_all(config_json.as_bytes())
            .context("Failed to write to agent secret file")?;
    }

    #[cfg(not(unix))]
    {
        fs::write(&path, config_json).context("Failed to write agent secret file")?;
    }

    Ok(())
}

pub fn get_gateway_url() -> String {
    std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

pub fn get_deployment_token() -> Result<String> {
    let token = std::env::var("DEPLOYMENT_TOKEN")
        .context("DEPLOYMENT_TOKEN environment variable is required")?
        .trim()
        .to_string();

    if !is_valid_deployment_token(&token) {
        anyhow::bail!(
            "DEPLOYMENT_TOKEN must match the Aegis deployment token format: ag_<{}+ URL-safe chars>",
            DEPLOYMENT_TOKEN_BODY_MIN_LEN
        );
    }

    Ok(token)
}

pub fn get_agent_name() -> String {
    std::env::var("AGENT_NAME").unwrap_or_else(|_| "rust-agent-01".to_string())
}

pub fn is_valid_deployment_token(token: &str) -> bool {
    let Some(body) = token.strip_prefix(DEPLOYMENT_TOKEN_PREFIX) else {
        return false;
    };

    body.len() >= DEPLOYMENT_TOKEN_BODY_MIN_LEN
        && body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

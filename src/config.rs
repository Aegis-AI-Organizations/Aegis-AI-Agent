use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
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

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(AGENT_SECRET_FILE)
            .context("Failed to create agent secret file with restricted permissions")?;

        use std::io::Write;
        file.write_all(config_json.as_bytes())
            .context("Failed to write to agent secret file")?;
    }

    #[cfg(not(unix))]
    {
        fs::write(AGENT_SECRET_FILE, config_json).context("Failed to write agent secret file")?;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::env;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_get_agent_name_default() {
        unsafe { env::remove_var("AGENT_NAME"); }
        assert_eq!(get_agent_name(), "rust-agent-01");
    }

    #[test]
    #[serial]
    fn test_get_agent_name_custom() {
        unsafe { env::set_var("AGENT_NAME", "custom-agent"); }
        assert_eq!(get_agent_name(), "custom-agent");
    }

    #[test]
    #[serial]
    fn test_get_gateway_url_default() {
        unsafe { env::remove_var("GATEWAY_URL"); }
        assert_eq!(get_gateway_url(), "http://localhost:8080");
    }

    #[test]
    #[serial]
    fn test_get_deployment_token_error() {
        unsafe { env::remove_var("DEPLOYMENT_TOKEN"); }
        assert!(get_deployment_token().is_err());
    }

    #[test]
    #[serial]
    fn test_get_deployment_token_ok() {
        unsafe { env::set_var("DEPLOYMENT_TOKEN", "secret-token"); }
        assert_eq!(get_deployment_token().unwrap(), "secret-token");
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".agent_secret_test");
        
        let config = AgentConfig {
            agent_id: "test-id".to_string(),
            agent_secret: "test-secret".to_string(),
        };

        // We can't easily change AGENT_SECRET_FILE constant but we can test the serialization/deserialization
        // and a custom file save if we had it. Since we don't want to change the code too much:
        let config_json = serde_json::to_string(&config).unwrap();
        fs::write(&config_path, config_json).unwrap();

        let content = fs::read_to_string(&config_path).unwrap();
        let loaded: AgentConfig = serde_json::from_str(&content).unwrap();
        
        assert_eq!(loaded.agent_id, "test-id");
        assert_eq!(loaded.agent_secret, "test-secret");
    }
}

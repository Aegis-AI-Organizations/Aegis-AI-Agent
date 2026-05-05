use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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
    std::env::var("DEPLOYMENT_TOKEN").context("DEPLOYMENT_TOKEN environment variable is required")
}

pub fn get_agent_name() -> String {
    std::env::var("AGENT_NAME").unwrap_or_else(|_| "rust-agent-01".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    #[test]
    #[serial]
    fn test_get_agent_name_default() {
        unsafe {
            env::remove_var("AGENT_NAME");
        }
        assert_eq!(get_agent_name(), "rust-agent-01");
    }

    #[test]
    #[serial]
    fn test_get_agent_name_custom() {
        unsafe {
            env::set_var("AGENT_NAME", "custom-agent");
        }
        assert_eq!(get_agent_name(), "custom-agent");
    }

    #[test]
    #[serial]
    fn test_get_gateway_url_default() {
        unsafe {
            env::remove_var("GATEWAY_URL");
        }
        assert_eq!(get_gateway_url(), "http://localhost:8080");
    }

    #[test]
    #[serial]
    fn test_get_deployment_token_error() {
        unsafe {
            env::remove_var("DEPLOYMENT_TOKEN");
        }
        assert!(get_deployment_token().is_err());
    }

    #[test]
    #[serial]
    fn test_get_deployment_token_ok() {
        unsafe {
            env::set_var("DEPLOYMENT_TOKEN", "secret-token");
        }
        assert_eq!(get_deployment_token().unwrap(), "secret-token");
    }

    #[test]
    #[serial]
    fn test_save_and_load_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".agent_secret_test");
        let config_path_str = config_path.to_str().unwrap();

        unsafe {
            env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path_str);
        }

        let config = AgentConfig {
            agent_id: "test-id".to_string(),
            agent_secret: "test-secret".to_string(),
        };

        // Test save
        save_config(&config).expect("save_config failed");

        // Test load
        let loaded = load_local_config()
            .expect("load_local_config failed")
            .expect("Config not found");

        assert_eq!(loaded.agent_id, "test-id");
        assert_eq!(loaded.agent_secret, "test-secret");

        unsafe {
            env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
        }
    }
}

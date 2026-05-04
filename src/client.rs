use crate::config::AgentConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct RegisterRequest {
    name: String,
    token: String,
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

pub struct AegisClient {
    client: reqwest::Client,
    gateway_url: String,
}

impl AegisClient {
    pub fn new(gateway_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            gateway_url,
        }
    }

    pub async fn register(&self, deploy_token: &str, agent_name: String) -> Result<AgentConfig> {
        let resp = self
            .client
            .post(format!("{}/api/agents/register", self.gateway_url))
            .header("Authorization", format!("Bearer {}", deploy_token))
            .json(&RegisterRequest {
                name: agent_name,
                token: deploy_token.to_string(),
            })
            .send()
            .await
            .context("Failed to send registration request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Registration failed with status {}: {}", status, error_text);
        }

        let reg_resp: RegisterResponse = resp
            .json()
            .await
            .context("Failed to parse registration response")?;

        Ok(AgentConfig {
            agent_id: reg_resp.agent_id,
            agent_secret: reg_resp.agent_secret,
        })
    }

    pub async fn send_heartbeat(&self, config: &AgentConfig) -> Result<()> {
        let status = StatusUpdate {
            status: "RUNNING".to_string(),
        };

        let resp = self
            .client
            .post(format!(
                "{}/api/agents/{}/status",
                self.gateway_url, config.agent_id
            ))
            .header("Authorization", format!("Bearer {}", config.agent_secret))
            .json(&status)
            .send()
            .await
            .context("Failed to send heartbeat request")?;

        if !resp.status().is_success() {
            anyhow::bail!("Heartbeat failed with status: {}", resp.status());
        }

        Ok(())
    }
}

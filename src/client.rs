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

#[cfg(test)]
mod tests {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_register_success() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("POST", "/api/agents/register")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"agent_id": "123", "agent_secret": "abc"}"#)
            .create_async()
            .await;

        let client = AegisClient::new(url);
        let config = client.register("token", "agent-01".to_string()).await.unwrap();

        assert_eq!(config.agent_id, "123");
        assert_eq!(config.agent_secret, "abc");
    }

    #[tokio::test]
    async fn test_register_failure() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("POST", "/api/agents/register")
            .with_status(401)
            .with_body("Unauthorized")
            .create_async()
            .await;

        let client = AegisClient::new(url);
        let result = client.register("token", "agent-01".to_string()).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("401"));
    }

    #[tokio::test]
    async fn test_heartbeat_success() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _m = server
            .mock("POST", "/api/agents/123/status")
            .with_status(200)
            .create_async()
            .await;

        let client = AegisClient::new(url);
        let config = AgentConfig {
            agent_id: "123".to_string(),
            agent_secret: "abc".to_string(),
        };

        let result = client.send_heartbeat(&config).await;
        assert!(result.is_ok());
    }
}

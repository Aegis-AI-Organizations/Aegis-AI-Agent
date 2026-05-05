use crate::config::AgentConfig;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .min_tls_version(reqwest::tls::Version::V1_2)
            .https_only(!cfg!(test))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
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

    pub async fn upload_payload(&self, presigned_url: &str, data: Vec<u8>) -> Result<()> {
        let mut attempts = 0;
        let max_attempts = 5;
        let mut backoff = Duration::from_secs(2);

        loop {
            attempts += 1;
            let resp = self
                .client
                .put(presigned_url)
                .body(data.clone())
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => return Ok(()),
                Ok(r) if (r.status() == 429 || r.status().is_server_error()) && attempts < max_attempts => {
                    eprintln!(
                        "Upload failed with status {}. Retrying in {:?} (attempt {}/{})",
                        r.status(),
                        backoff,
                        attempts,
                        max_attempts
                    );
                }
                Err(e) if attempts < max_attempts => {
                    eprintln!(
                        "Upload network error: {}. Retrying in {:?} (attempt {}/{})",
                        e, backoff, attempts, max_attempts
                    );
                }
                Ok(r) => anyhow::bail!("Upload failed with status: {}", r.status()),
                Err(e) => anyhow::bail!("Upload failed after network error: {}", e),
            }

            tokio::time::sleep(backoff).await;
            backoff *= 2;
        }
    }
}

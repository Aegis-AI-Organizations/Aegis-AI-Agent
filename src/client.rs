use crate::config::{is_valid_deployment_token, AgentConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
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

#[derive(Deserialize)]
struct UploadLinkResponse {
    url: String,
    #[allow(dead_code)]
    method: String,
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
        let allow_http = cfg!(test) || env::var("AGENT_ALLOW_HTTP").unwrap_or_default() == "true";
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .https_only(!allow_http)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            client,
            gateway_url,
        }
    }

    pub async fn register(&self, deploy_token: &str, agent_name: String) -> Result<AgentConfig> {
        if !is_valid_deployment_token(deploy_token) {
            anyhow::bail!("Invalid deployment token format");
        }

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

    pub async fn get_upload_url(&self, config: &AgentConfig, filename: &str) -> Result<String> {
        let resp = self
            .client
            .get(format!(
                "{}/api/agents/{}/upload-url",
                self.gateway_url, config.agent_id
            ))
            .query(&[("filename", filename)])
            .header("Authorization", format!("Bearer {}", config.agent_secret))
            .send()
            .await
            .context("Failed to get upload URL")?;

        if !resp.status().is_success() {
            anyhow::bail!("Failed to get upload URL: {}", resp.status());
        }

        let link_resp: UploadLinkResponse = resp
            .json()
            .await
            .context("Failed to parse upload URL response")?;

        let mut final_url = link_resp.url;
        // WORKAROUND: If we are in local dev, replace docker internal hostname with localhost
        if env::var("AGENT_ALLOW_HTTP").unwrap_or_default() == "true" {
            final_url = final_url.replace("http://minio:9000", "http://localhost:9000");
        }

        Ok(final_url)
    }

    pub async fn upload_payload(&self, presigned_url: &str, data: Vec<u8>) -> Result<()> {
        let mut attempts = 0;
        let max_attempts = 5;
        let mut backoff = Duration::from_secs(2);

        loop {
            attempts += 1;
            let mut request = self.client.put(presigned_url).body(data.clone());

            // WORKAROUND: If we replaced minio with localhost, we must restore the Host header for S3 signature validation
            if presigned_url.contains("localhost:9000") {
                request = request.header("Host", "minio:9000");
            }

            let resp = request.send().await;

            match resp {
                Ok(r) if r.status().is_success() => return Ok(()),
                Ok(r)
                    if (r.status() == 429 || r.status().is_server_error())
                        && attempts < max_attempts =>
                {
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

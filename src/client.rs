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
    object_name: String,
}

#[derive(Serialize)]
struct StatusUpdate {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_key: Option<String>,
}

const HEARTBEAT_STATUS: &str = "RUNNING";

pub struct AegisClient {
    client: reqwest::Client,
    gateway_url: String,
    original_minio_host: std::sync::Mutex<Option<String>>,
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
            original_minio_host: std::sync::Mutex::new(None),
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
            status: HEARTBEAT_STATUS.to_string(),
            payload_key: None,
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

    pub async fn get_upload_url(
        &self,
        config: &AgentConfig,
        filename: &str,
    ) -> Result<(String, String)> {
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
            if let Some(start) = final_url.find("://") {
                let rest = &final_url[start + 3..];
                if let Some(end) = rest.find('/') {
                    let host_port = &rest[..end];
                    if host_port.contains("minio")
                        && host_port.contains(':')
                        && host_port != "localhost:9000"
                    {
                        if let Ok(mut guard) = self.original_minio_host.lock() {
                            *guard = Some(host_port.to_string());
                        }
                        final_url = final_url.replace(host_port, "localhost:9000");
                    }
                }
            }
        }

        Ok((final_url, link_resp.object_name))
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
                let host_header = if let Ok(guard) = self.original_minio_host.lock() {
                    guard.clone().unwrap_or_else(|| "minio:9000".to_string())
                } else {
                    "minio:9000".to_string()
                };
                request = request.header("Host", host_header);
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

    pub async fn update_status(
        &self,
        config: &AgentConfig,
        status: &str,
        payload_key: Option<&str>,
    ) -> Result<()> {
        let status_payload = StatusUpdate {
            status: status.to_string(),
            payload_key: payload_key.map(|s| s.to_string()),
        };

        let resp = self
            .client
            .post(format!(
                "{}/api/agents/{}/status",
                self.gateway_url, config.agent_id
            ))
            .header("Authorization", format!("Bearer {}", config.agent_secret))
            .json(&status_payload)
            .send()
            .await
            .context("Failed to send status update request")?;

        if !resp.status().is_success() {
            anyhow::bail!("Status update failed with status: {}", resp.status());
        }

        Ok(())
    }
}

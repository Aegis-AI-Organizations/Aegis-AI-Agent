use crate::client::AegisClient;
use crate::config::{self, AgentConfig};
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time;

/// Config shared between the heartbeat and discovery loops, so that when
/// either one re-registers after the Gateway rejects its credentials, the
/// other picks up the fresh agent_id/secret too instead of independently
/// re-registering its own, separate agent.
pub type SharedConfig = Arc<Mutex<AgentConfig>>;

pub fn startup_message() -> &'static str {
    "Aegis AI Agent initialized."
}

pub async fn init_agent() -> Result<()> {
    tracing::info!("{}", startup_message());

    if std::env::var("SKIP_AGENT_INIT")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        tracing::info!("Skipping agent initialization (SKIP_AGENT_INIT=1)");
        return Ok(());
    }

    let config = load_or_register_agent().await?;
    tracing::info!("Agent registered/loaded with ID: {}", config.agent_id);

    if std::env::var("SKIP_AGENT_LOOPS")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        tracing::info!("Skipping agent background loops (SKIP_AGENT_LOOPS=1)");
        return Ok(());
    }

    // Shared so a re-registration triggered by either loop (see
    // recover_from_stale_credentials) is visible to both, instead of each
    // loop independently registering its own agent on failure.
    let shared: SharedConfig = Arc::new(Mutex::new(config));

    let discovery_shared = shared.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::discovery::start_discovery_loop(discovery_shared).await {
            tracing::error!("Discovery loop error: {}", e);
        }
    });

    tokio::spawn(async move {
        if let Err(e) = start_heartbeat_loop(shared).await {
            tracing::error!("Heartbeat error: {}", e);
        }
    });

    Ok(())
}

async fn load_or_register_agent() -> Result<AgentConfig> {
    if let Some(config) = config::load_local_config()? {
        if let Ok(validated) = validate_loaded_agent(config.clone()).await {
            return Ok(validated);
        }
    }

    // No local secret, perform registration
    register_agent().await
}

pub async fn load_or_register_agent_exposed() -> Result<AgentConfig> {
    load_or_register_agent().await
}

async fn validate_loaded_agent(config: AgentConfig) -> Result<AgentConfig> {
    let client = AegisClient::new(config::get_gateway_url());

    match client.send_heartbeat(&config).await {
        Ok(_) => Ok(config),
        Err(err) => {
            let err_text = err.to_string();
            if is_stale_agent_secret_error(&err_text) {
                tracing::warn!(
                    "Stored agent credentials are no longer valid, re-registering agent: {}",
                    err_text
                );
                register_agent().await
            } else {
                tracing::info!(
                    "Stored agent credentials could not be validated right now, keeping local config: {}",
                    err_text
                );
                Ok(config)
            }
        }
    }
}

async fn register_agent() -> Result<AgentConfig> {
    let gateway_url = config::get_gateway_url();
    let deploy_token = config::get_deployment_token()?;
    let agent_name = config::get_agent_name();

    let client = AegisClient::new(gateway_url);
    let config = client.register(&deploy_token, agent_name).await?;

    config::save_config(&config).context("Failed to persist agent configuration")?;

    Ok(config)
}

fn is_stale_agent_secret_error(err_text: &str) -> bool {
    // reqwest's StatusCode Display always renders "<code> <reason phrase>"
    // (e.g. "403 Forbidden"), regardless of which call site's error message
    // wraps it ("Heartbeat failed with status: 403 Forbidden", "Failed to
    // get upload URL: 403 Forbidden", ...), so match on that instead of a
    // "status: " prefix that not every call site uses.
    ["401 Unauthorized", "403 Forbidden", "404 Not Found"]
        .iter()
        .any(|phrase| err_text.contains(phrase))
}

/// If `err` indicates the Gateway no longer accepts the credentials this
/// call was made with, re-registers the agent and updates `shared` in
/// place so every loop resyncs against the same, fresh identity. Returns
/// the config callers should immediately retry their failed request with,
/// or None if `err` wasn't a credentials problem (or re-registration
/// itself failed).
pub(crate) async fn recover_from_stale_credentials(
    shared: &SharedConfig,
    failed_secret: &str,
    err: &anyhow::Error,
) -> Option<AgentConfig> {
    if !is_stale_agent_secret_error(&err.to_string()) {
        return None;
    }

    let mut guard = shared.lock().await;
    // Another loop may have already re-registered while we were waiting
    // for the lock (heartbeat and discovery can both fail around the same
    // time) — reuse that instead of registering the agent a second time.
    if guard.agent_secret != failed_secret {
        return Some(guard.clone());
    }

    tracing::warn!(
        "Agent credentials rejected ({}), re-registering and resyncing...",
        err
    );
    match register_agent().await {
        Ok(fresh) => {
            *guard = fresh.clone();
            Some(fresh)
        }
        Err(register_err) => {
            tracing::error!(
                "Re-registration after rejected credentials failed: {}",
                register_err
            );
            None
        }
    }
}

async fn start_heartbeat_loop(shared: SharedConfig) -> Result<()> {
    let gateway_url = config::get_gateway_url();
    let client = AegisClient::new(gateway_url);
    let mut interval = time::interval(Duration::from_secs(30));

    loop {
        interval.tick().await;
        let config = shared.lock().await.clone();
        if let Err(e) = client.send_heartbeat(&config).await {
            tracing::error!("Heartbeat failed: {}", e);
            if let Some(fresh) =
                recover_from_stale_credentials(&shared, &config.agent_secret, &e).await
            {
                match client.send_heartbeat(&fresh).await {
                    Ok(_) => tracing::info!("Heartbeat resynced after re-registration."),
                    Err(e) => {
                        tracing::error!("Heartbeat retry after re-registration failed: {}", e)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    fn set_env(gateway_url: &str, deployment_token: &str) {
        unsafe {
            env::set_var("DEPLOYMENT_TOKEN", deployment_token);
            env::set_var("GATEWAY_URL", gateway_url);
            env::set_var("AGENT_NAME", "agent-01");
            env::set_var("AGENT_ALLOW_HTTP", "true");
        }
    }

    fn clear_env() {
        unsafe {
            env::remove_var("DEPLOYMENT_TOKEN");
            env::remove_var("GATEWAY_URL");
            env::remove_var("AGENT_NAME");
            env::remove_var("AGENT_ALLOW_HTTP");
            env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
        }
    }

    #[test]
    fn is_stale_agent_secret_error_matches_every_call_site_format() {
        assert!(is_stale_agent_secret_error(
            "Heartbeat failed with status: 403 Forbidden"
        ));
        assert!(is_stale_agent_secret_error(
            "Failed to get upload URL: 401 Unauthorized"
        ));
        assert!(is_stale_agent_secret_error(
            "Status update failed with status: 404 Not Found"
        ));
        assert!(!is_stale_agent_secret_error(
            "Heartbeat failed with status: 500 Internal Server Error"
        ));
        assert!(!is_stale_agent_secret_error(
            "network error: connection refused"
        ));
    }

    #[tokio::test]
    #[serial]
    async fn recover_from_stale_credentials_updates_shared_config_on_auth_error() {
        let mut server = mockito::Server::new_async().await;
        let temp_dir = tempfile::tempdir().unwrap();
        set_env(
            &server.url(),
            "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
        );
        unsafe {
            env::set_var(
                "AGENT_SECRET_FILE_OVERRIDE",
                temp_dir.path().join(".agent_secret_test").to_str().unwrap(),
            );
        }

        let _register = server
            .mock("POST", "/api/agents/register")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"agent_id": "agent-fresh", "agent_secret": "secret-fresh"}"#)
            .create_async()
            .await;

        let shared: SharedConfig = Arc::new(Mutex::new(AgentConfig {
            agent_id: "agent-stale".to_string(),
            agent_secret: "secret-stale".to_string(),
        }));
        let err = anyhow::anyhow!("Heartbeat failed with status: 403 Forbidden");

        let fresh = recover_from_stale_credentials(&shared, "secret-stale", &err)
            .await
            .expect("expected recovery to re-register");

        assert_eq!(fresh.agent_id, "agent-fresh");
        assert_eq!(shared.lock().await.agent_id, "agent-fresh");

        clear_env();
    }

    #[tokio::test]
    #[serial]
    async fn recover_from_stale_credentials_ignores_non_auth_errors() {
        let shared: SharedConfig = Arc::new(Mutex::new(AgentConfig {
            agent_id: "agent-x".to_string(),
            agent_secret: "secret-x".to_string(),
        }));
        let err = anyhow::anyhow!("Heartbeat failed with status: 500 Internal Server Error");

        let result = recover_from_stale_credentials(&shared, "secret-x", &err).await;

        assert!(result.is_none());
        assert_eq!(shared.lock().await.agent_secret, "secret-x");
    }

    #[tokio::test]
    #[serial]
    async fn recover_from_stale_credentials_reuses_config_already_refreshed_by_another_task() {
        let shared: SharedConfig = Arc::new(Mutex::new(AgentConfig {
            agent_id: "agent-fresh-already".to_string(),
            agent_secret: "secret-fresh-already".to_string(),
        }));
        let err = anyhow::anyhow!("Heartbeat failed with status: 403 Forbidden");

        // failed_secret no longer matches what's in `shared`, as if another
        // task already won the re-registration race.
        let result = recover_from_stale_credentials(&shared, "secret-stale", &err).await;

        let fresh = result.expect("expected the already-refreshed config to be reused");
        assert_eq!(fresh.agent_id, "agent-fresh-already");
    }
}

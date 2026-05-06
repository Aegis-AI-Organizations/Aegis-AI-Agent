pub mod agent;
pub mod client;
pub mod config;
pub mod domain;
pub mod error;
pub mod health;
pub mod server;

use std::net::SocketAddr;

pub fn startup_banner() -> &'static str {
    "Aegis AI Agent is starting..."
}

pub async fn prepare_run() -> anyhow::Result<SocketAddr> {
    tracing::info!("{}", startup_banner());

    // 1. Initialize Agent logic
    agent::init_agent().await?;

    // 2. Prepare address
    let bind_addr = std::env::var("HEALTH_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8081);
    let addr: SocketAddr = format!("{}:{}", bind_addr, port).parse()?;

    Ok(addr)
}

pub async fn run() -> anyhow::Result<()> {
    let addr = prepare_run().await?;
    server::start_server(addr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_startup_banner() {
        assert_eq!(startup_banner(), "Aegis AI Agent is starting...");
    }

    #[tokio::test]
    #[serial]
    async fn test_prepare_run_skip_init() {
        unsafe {
            std::env::set_var("SKIP_AGENT_INIT", "1");
        }
        let _ = prepare_run().await;

        unsafe {
            std::env::remove_var("SKIP_AGENT_INIT");
        }
    }
}

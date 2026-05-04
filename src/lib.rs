pub mod agent;
pub mod client;
pub mod config;
pub mod health;
pub mod server;

use std::net::SocketAddr;

pub fn startup_banner() -> &'static str {
    "Hello, world! Aegis AI Agent is starting..."
}

pub async fn prepare_run() -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let load_dotenv = cfg!(debug_assertions)
        || std::env::var("LOAD_DOTENV")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    if load_dotenv {
        let _ = dotenvy::dotenv();
    }
    println!("{}", startup_banner());

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

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let addr = prepare_run().await?;
    server::start_server(addr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_startup_banner() {
        assert_eq!(
            startup_banner(),
            "Hello, world! Aegis AI Agent is starting..."
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_prepare_run_env_logic() {
        // Test LOAD_DOTENV=1
        unsafe {
            std::env::set_var("LOAD_DOTENV", "1");
            std::env::set_var("SKIP_AGENT_INIT", "1");
        }
        let _ = prepare_run().await;

        // Test LOAD_DOTENV=true
        unsafe {
            std::env::set_var("LOAD_DOTENV", "true");
        }
        let _ = prepare_run().await;

        // Clean up
        unsafe {
            std::env::remove_var("LOAD_DOTENV");
            std::env::remove_var("SKIP_AGENT_INIT");
        }
    }
}

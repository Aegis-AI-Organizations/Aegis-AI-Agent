pub mod agent;
pub mod health;

use axum::{Router, routing::get};

pub fn create_app() -> Router {
    Router::new()
        .route("/health", get(health::health_handler))
        .route("/admin/system/health", get(health::health_handler))
}

pub fn startup_banner() -> &'static str {
    "Hello, world! Aegis AI Agent is starting..."
}

pub async fn prepare_run() -> Result<Router, Box<dyn std::error::Error>> {
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

    // 2. Build Axum router
    Ok(create_app())
}

pub async fn run_server(
    listener: tokio::net::TcpListener,
    app: Router,
) -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = prepare_run().await?;
    let bind_addr = std::env::var("HEALTH_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("HEALTH_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8081);
    let addr: std::net::SocketAddr = format!("{}:{}", bind_addr, port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    run_server(listener, app).await
}

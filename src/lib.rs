pub mod agent;
pub mod health;

use axum::{Router, routing::get};

pub fn create_app() -> Router {
    Router::new().route("/admin/system/health", get(health::health_handler))
}

pub fn startup_banner() -> &'static str {
    "Hello, world! Aegis AI Agent is starting..."
}

pub fn prepare_run() -> Router {
    dotenvy::dotenv().ok();
    println!("{}", startup_banner());

    // 1. Initialize Agent logic
    agent::init_agent();

    // 2. Build Axum router
    create_app()
}

pub async fn run_server(
    listener: tokio::net::TcpListener,
    app: Router,
) -> Result<(), Box<dyn std::error::Error>> {
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = prepare_run();
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8081));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    run_server(listener, app).await
}

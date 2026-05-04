use crate::health;
use axum::{Router, routing::get};
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub fn create_router() -> Router {
    Router::new()
        .route("/health", get(health::health_handler))
        .route("/admin/system/health", get(health::health_handler))
}

pub async fn start_server(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let app = create_router();
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    println!("Health server listening on {}", local_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

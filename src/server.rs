use crate::discovery;
use crate::health;
use axum::{
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn scan_handler() -> impl IntoResponse {
    if discovery::trigger_topology_scan() {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "success",
                "message": "Topology scan and upload triggered"
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "error",
                "message": "Discovery loop not active or channel full"
            })),
        )
    }
}

pub fn create_router() -> Router {
    Router::new()
        .route("/health", get(health::health_handler))
        .route("/admin/system/health", get(health::health_handler))
        .route("/admin/system/scan", post(scan_handler))
}

pub async fn start_server(addr: SocketAddr) -> anyhow::Result<()> {
    let app = create_router();
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!("Health server listening on {}", local_addr);
    axum::serve(listener, app).await?;
    Ok(())
}

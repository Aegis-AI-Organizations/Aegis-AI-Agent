use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use sqlx::PgPool;
use std::env;

#[derive(Serialize)]
pub struct HealthStatus {
    pub postgres: String,
    pub temporal: String,
}

pub async fn health_handler(
    axum::extract::State(pool): axum::extract::State<PgPool>,
) -> impl IntoResponse {
    let mut status = HealthStatus {
        postgres: "unknown".to_string(),
        temporal: "unknown".to_string(),
    };

    // 1. Check PostgreSQL
    match sqlx::query("SELECT 1").execute(&pool).await {
        Ok(_) => status.postgres = "OK".to_string(),
        Err(e) => status.postgres = format!("Error: {}", e),
    }

    // 2. Check Temporal
    let temporal_host = env::var("TEMPORAL_HOST").unwrap_or_else(|_| "localhost".to_string());
    let temporal_url = format!("http://{}:7233/healthz", temporal_host); // Placeholder port for health check

    // Note: Temporal gRPC is on 7233, but usually has an web/ui health endpoint on another port.
    // Assuming a standard sidecar or service check.
    match reqwest::get(&temporal_url).await {
        Ok(resp) if resp.status().is_success() => status.temporal = "OK".to_string(),
        Ok(resp) => status.temporal = format!("Status: {}", resp.status()),
        Err(e) => status.temporal = format!("Error: {}", e),
    }

    let overall_success = status.postgres == "OK" && status.temporal == "OK";

    let code = if overall_success {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (code, Json(status))
}

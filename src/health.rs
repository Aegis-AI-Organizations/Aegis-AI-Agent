use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use std::env;

#[derive(Serialize)]
pub struct HealthStatus {
    pub ingest: String,
}

pub async fn health_handler() -> impl IntoResponse {
    let mut status = HealthStatus {
        ingest: "unknown".to_string(),
    };

    // Check Ingest Worker
    let ingest_host = env::var("INGEST_HOST").unwrap_or_else(|_| "localhost".to_string());
    let ingest_port = env::var("INGEST_PORT").unwrap_or_else(|_| "7233".to_string());
    let ingest_url = format!("http://{}:{}/healthz", ingest_host, ingest_port);

    match reqwest::get(&ingest_url).await {
        Ok(resp) if resp.status().is_success() => status.ingest = "OK".to_string(),
        Ok(resp) => status.ingest = format!("Status: {}", resp.status()),
        Err(e) => status.ingest = format!("Error: {}", e),
    }

    let overall_success = status.ingest == "OK";

    let code = if overall_success {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (code, Json(status))
}

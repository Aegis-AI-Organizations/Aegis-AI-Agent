use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use std::env;
use std::sync::OnceLock;
use std::time::Duration;

static INGEST_HEALTH_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn ingest_health_client() -> &'static reqwest::Client {
    INGEST_HEALTH_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("failed to build ingest health client")
    })
}

#[derive(Serialize)]
pub struct HealthStatus {
    pub status: String,
    pub ingest: String,
    pub ingest_connection: String,
}

pub async fn health_handler() -> impl IntoResponse {
    let mut status = HealthStatus {
        status: "UP".to_string(),
        ingest: "unknown".to_string(),
        ingest_connection: "unknown".to_string(),
    };

    // Check Ingest Worker connection
    let ingest_url = env::var("INGEST_HEALTH_URL").unwrap_or_else(|_| {
        let ingest_host = env::var("INGEST_HOST").unwrap_or_else(|_| "localhost".to_string());
        let ingest_port = env::var("INGEST_PORT").unwrap_or_else(|_| "7233".to_string());
        format!("http://{}:{}/healthz", ingest_host, ingest_port)
    });

    match ingest_health_client().get(&ingest_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            status.ingest = "UP".to_string();
            status.ingest_connection = "OK".to_string();
        }
        Ok(resp) => {
            eprintln!(
                "Ingest health check returned non-success status: {}",
                resp.status()
            );
            status.ingest = "DOWN".to_string();
            status.ingest_connection = format!("HTTP {}", resp.status());
            status.status = "DOWN".to_string();
        }
        Err(e) => {
            eprintln!("Ingest health check failed: {}", e);
            status.ingest = "DOWN".to_string();
            status.ingest_connection = format!("ERROR: {}", e);
            status.status = "DOWN".to_string();
        }
    }

    // Agent is always UP if this handler is reached, but we return 503 if dependencies fail
    let code = if status.status == "UP" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (code, Json(status))
}

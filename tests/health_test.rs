use aegis_ai_agent::health::health_handler;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::env;

#[tokio::test]
async fn test_health_handler_ingest_ok() {
    let mut server = mockito::Server::new_async().await;
    let host_port = server.host_with_port();
    let host = host_port
        .split(':')
        .next()
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = host_port.split(':').nth(1).unwrap_or("7233").to_string();

    let _m = server
        .mock("GET", "/healthz")
        .with_status(200)
        .create_async()
        .await;

    unsafe {
        env::set_var("INGEST_HOST", &host);
        env::set_var("INGEST_PORT", &port);
    }

    let response = health_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_health_handler_ingest_fail() {
    let mut server = mockito::Server::new_async().await;
    let host_port = server.host_with_port();
    let host = host_port
        .split(':')
        .next()
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = host_port.split(':').nth(1).unwrap_or("7233").to_string();

    let _m = server
        .mock("GET", "/healthz")
        .with_status(500)
        .create_async()
        .await;

    unsafe {
        env::set_var("INGEST_HOST", &host);
        env::set_var("INGEST_PORT", &port);
    }

    let response = health_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_health_handler_ingest_timeout() {
    // Set a port where nothing is listening to trigger an error in reqwest
    unsafe {
        env::set_var("INGEST_HOST", "127.0.0.1");
        env::set_var("INGEST_PORT", "1"); // Hopefully nothing is on port 1
    }

    let response = health_handler().await.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

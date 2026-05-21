use aegis_ai_agent::server::create_router;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serial_test::serial;
use std::env;
use tower::ServiceExt; // for `oneshot` and `ready`

#[tokio::test]
#[serial]
async fn test_health_endpoint_integration() {
    let app = create_router();

    // Mock Ingest to avoid flake
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
        env::remove_var("INGEST_HEALTH_URL");
        env::set_var("INGEST_HOST", &host);
        env::set_var("INGEST_PORT", &port);
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/system/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_not_found_integration() {
    let app = create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/not-found")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn test_startup_banner() {
    assert_eq!(
        aegis_ai_agent::startup_banner(),
        "Aegis AI Agent is starting..."
    );
}

#[tokio::test]
#[serial]
async fn test_prepare_run() {
    unsafe {
        env::set_var("SKIP_AGENT_INIT", "1");
    }
    let _addr = aegis_ai_agent::prepare_run().await.unwrap();
}

#[tokio::test]
async fn test_run_server() {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();

    // Spawn server in background
    let handle = tokio::spawn(async move {
        aegis_ai_agent::server::start_server(addr).await.unwrap();
    });

    // Let it start, then abort
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    handle.abort();
}

#[tokio::test]
#[serial]
async fn test_run_failure() {
    // Set specific env vars for this test to avoid leakage
    unsafe {
        env::set_var("SKIP_AGENT_INIT", "1");
        env::set_var("HEALTH_BIND_ADDR", "127.0.0.1");
        env::set_var("HEALTH_PORT", "18081");
    }

    // 1. Bind to the port first to ensure run() fails to bind
    let _socket = std::net::TcpListener::bind("127.0.0.1:18081").unwrap();

    // 2. Call run() and expect it to fail with a bind error
    let result = aegis_ai_agent::run().await;
    match result {
        Err(e) => {
            let err_msg = e.to_string().to_lowercase();
            assert!(
                err_msg.contains("address already in use") || err_msg.contains("failed to bind"),
                "Expected bind failure, got: {}",
                e
            );
        }
        Ok(_) => panic!("Expected run() to fail due to port collision"),
    }
}

#[tokio::test]
#[serial]
async fn test_agent_init() {
    unsafe {
        env::set_var("SKIP_AGENT_INIT", "1");
        env::set_var("SKIP_AGENT_LOOPS", "1");
    }
    aegis_ai_agent::agent::init_agent().await.unwrap();
    unsafe {
        env::remove_var("SKIP_AGENT_LOOPS");
    }
}

#[tokio::test]
async fn test_binary_startup() {
    use std::process::{Command, Stdio};

    // Attempt to run the binary.
    // We assume it's already built by the current test run.
    let binary_path = env!("CARGO_BIN_EXE_aegis-ai-agent");
    let child = Command::new(binary_path)
        .env("SKIP_AGENT_INIT", "1")
        .env("SKIP_AGENT_LOOPS", "1")
        .stdout(Stdio::piped())
        .spawn();

    if let Ok(mut child) = child {
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
        assert!(child.try_wait().unwrap().is_none());
        child.kill().ok();
        child.wait().ok();
    }
}

#[tokio::test]
#[serial]
async fn test_upload_payload_retries() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let client = aegis_ai_agent::client::AegisClient::new(url.clone());

    // Mock failure then success
    let _m1 = server
        .mock("PUT", "/upload")
        .with_status(500)
        .expect(1)
        .create_async()
        .await;

    let _m2 = server
        .mock("PUT", "/upload")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;

    let upload_url = format!("{}/upload", url);
    let data = vec![1, 2, 3];

    // This should retry and eventually succeed
    // Note: It might take a few seconds due to backoff
    let result = client.upload_payload(&upload_url, data).await;
    assert!(result.is_ok());
}

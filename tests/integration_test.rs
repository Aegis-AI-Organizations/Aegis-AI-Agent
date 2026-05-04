use aegis_ai_agent::create_app;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use std::env;
use tower::ServiceExt; // for `oneshot` and `ready`

#[tokio::test]
async fn test_health_endpoint_integration() {
    let app = create_app();

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
    let app = create_app();

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
        "Hello, world! Aegis AI Agent is starting..."
    );
}

#[tokio::test]
async fn test_prepare_run() {
    unsafe {
        env::set_var("SKIP_AGENT_INIT", "1");
    }
    let _app = aegis_ai_agent::prepare_run().await.unwrap();
}

#[tokio::test]
async fn test_run_server() {
    let app = create_app();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

    // Spawn server in background
    let handle = tokio::spawn(async move {
        aegis_ai_agent::run_server(listener, app).await.unwrap();
    });

    // Let it start, then abort
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    handle.abort();
}

#[tokio::test]
async fn test_run_failure() {
    // Set specific env vars for this test to avoid leakage
    unsafe {
        env::set_var("HEALTH_BIND_ADDR", "127.0.0.1");
        env::set_var("HEALTH_PORT", "18081");
    }

    // 1. Bind to the port first to ensure run() fails to bind
    let _socket = std::net::TcpListener::bind("127.0.0.1:18081").unwrap();

    // 2. Call run() and expect it to fail
    let result = aegis_ai_agent::run().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_agent_init() {
    unsafe {
        env::set_var("SKIP_AGENT_INIT", "1");
    }
    aegis_ai_agent::agent::init_agent().await.unwrap();
}

#[tokio::test]
async fn test_binary_startup() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    // Attempt to run the binary.
    // We assume it's already built by the current test run.
    let child = Command::new("target/debug/aegis-ai-agent")
        .env("SKIP_AGENT_INIT", "1")
        .stdout(Stdio::piped())
        .spawn();

    if let Ok(mut child) = child {
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        // Wait for the banner to appear or timeout
        // We'll give it 1 second max
        let start = std::time::Instant::now();
        for line in reader.lines() {
            if let Ok(l) = line {
                if l.contains("Aegis AI Agent is starting") {
                    break;
                }
            }
            if start.elapsed().as_secs() > 1 {
                break;
            }
        }

        child.kill().ok();
    }
}

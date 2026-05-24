use aegis_ai_agent::client::AegisClient;
use aegis_ai_agent::config::AgentConfig;
use mockito::{self, Matcher};

#[tokio::test]
async fn test_register_success() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let _m = server
        .mock("POST", "/api/agents/register")
        .match_header(
            "authorization",
            "Bearer ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
        )
        .match_body(Matcher::JsonString(
            r#"{"name":"agent-01","token":"ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"}"#
                .to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"agent_id": "123", "agent_secret": "abc"}"#)
        .create_async()
        .await;

    let client = AegisClient::new(url);
    let config = client
        .register(
            "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
            "agent-01".to_string(),
        )
        .await
        .unwrap();

    assert_eq!(config.agent_id, "123");
    assert_eq!(config.agent_secret, "abc");
}

#[tokio::test]
async fn test_register_failure() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let _m = server
        .mock("POST", "/api/agents/register")
        .with_status(401)
        .with_body("Unauthorized")
        .create_async()
        .await;

    let client = AegisClient::new(url);
    let result = client
        .register(
            "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
            "agent-01".to_string(),
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("401"));
}

#[tokio::test]
async fn test_heartbeat_success() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let _m = server
        .mock("POST", "/api/agents/123/status")
        .match_header("authorization", "Bearer abc")
        .match_body(Matcher::JsonString(r#"{"status":"RUNNING"}"#.to_string()))
        .with_status(200)
        .create_async()
        .await;

    let client = AegisClient::new(url);
    let config = AgentConfig {
        agent_id: "123".to_string(),
        agent_secret: "abc".to_string(),
    };

    let result = client.send_heartbeat(&config).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_upload_url_success() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let _m = server
        .mock("GET", "/api/agents/123/upload-url?filename=test.txt")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"url": "http://minio/upload", "method": "PUT", "object_name": "agents/123/test.txt"}"#)
        .create_async()
        .await;

    let client = AegisClient::new(url);
    let config = AgentConfig {
        agent_id: "123".to_string(),
        agent_secret: "abc".to_string(),
    };

    let result = client.get_upload_url(&config, "test.txt").await;
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        (
            "http://minio/upload".to_string(),
            "agents/123/test.txt".to_string()
        )
    );
}

#[tokio::test]
async fn test_upload_payload_immediate_success() {
    unsafe {
        std::env::set_var("AGENT_ALLOW_HTTP", "true");
    }
    let mut server = mockito::Server::new_async().await;
    let url = server.url();
    let upload_path = "/upload";
    let full_url = format!("{}{}", url, upload_path);

    let _m = server
        .mock("PUT", upload_path)
        .with_status(200)
        .create_async()
        .await;

    let client = AegisClient::new(url);
    let result = client.upload_payload(&full_url, vec![1, 2, 3]).await;
    assert!(result.is_ok());
}

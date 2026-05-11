use aegis_ai_agent::config::*;
use serial_test::serial;
use std::env;
use std::fs;

#[test]
#[serial]
fn test_get_agent_name_default() {
    unsafe {
        env::remove_var("AGENT_NAME");
    }
    assert_eq!(get_agent_name(), "rust-agent-01");
}

#[test]
#[serial]
fn test_get_agent_name_custom() {
    unsafe {
        env::set_var("AGENT_NAME", "custom-agent");
    }
    assert_eq!(get_agent_name(), "custom-agent");
}

#[test]
#[serial]
fn test_get_gateway_url_default() {
    unsafe {
        env::remove_var("GATEWAY_URL");
    }
    assert_eq!(get_gateway_url(), "http://localhost:8080");
}

#[test]
#[serial]
fn test_get_deployment_token_error() {
    unsafe {
        env::remove_var("DEPLOYMENT_TOKEN");
    }
    assert!(get_deployment_token().is_err());
}

#[test]
#[serial]
fn test_get_deployment_token_ok() {
    unsafe {
        env::set_var(
            "DEPLOYMENT_TOKEN",
            "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg",
        );
    }
    assert_eq!(
        get_deployment_token().unwrap(),
        "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"
    );
}

#[test]
#[serial]
fn test_get_deployment_token_invalid_format() {
    unsafe {
        env::set_var("DEPLOYMENT_TOKEN", "secret-token");
    }
    assert!(get_deployment_token().is_err());
}

#[test]
fn test_is_valid_deployment_token() {
    assert!(is_valid_deployment_token(
        "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"
    ));
    assert!(is_valid_deployment_token(
        "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg_-"
    ));
    assert!(!is_valid_deployment_token(
        "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"
    ));
    assert!(!is_valid_deployment_token(
        "xx_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg"
    ));
    assert!(!is_valid_deployment_token(
        "ag_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef!"
    ));
}

#[test]
#[serial]
fn test_save_and_load_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".agent_secret_test");
    let config_path_str = config_path.to_str().unwrap();

    unsafe {
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path_str);
    }

    let config = AgentConfig {
        agent_id: "test-id".to_string(),
        agent_secret: "test-secret".to_string(),
    };

    // Test save
    save_config(&config).expect("save_config failed");

    // Test load
    let loaded = load_local_config()
        .expect("load_local_config failed")
        .expect("Config not found");

    assert_eq!(loaded.agent_id, "test-id");
    assert_eq!(loaded.agent_secret, "test-secret");

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
    }
}

#[test]
#[serial]
fn test_load_local_config_invalid_json() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".agent_secret_invalid");
    fs::write(&config_path, "{invalid-json").unwrap();

    unsafe {
        env::set_var("AGENT_SECRET_FILE_OVERRIDE", config_path.to_str().unwrap());
    }

    let result = load_local_config();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("parse"));

    unsafe {
        env::remove_var("AGENT_SECRET_FILE_OVERRIDE");
    }
}

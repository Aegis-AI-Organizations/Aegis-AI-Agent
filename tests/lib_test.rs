use aegis_ai_agent::{prepare_run, startup_banner};
use serial_test::serial;

#[test]
fn test_startup_banner() {
    assert_eq!(startup_banner(), "Aegis AI Agent is starting...");
}

#[tokio::test]
#[serial]
async fn test_prepare_run_skip_init() {
    unsafe {
        std::env::set_var("SKIP_AGENT_INIT", "1");
    }
    let _ = prepare_run().await;

    unsafe {
        std::env::remove_var("SKIP_AGENT_INIT");
    }
}

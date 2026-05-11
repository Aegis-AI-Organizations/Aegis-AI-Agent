use aegis_ai_agent::agent::startup_message;

#[test]
fn test_startup_message() {
    assert_eq!(startup_message(), "Aegis AI Agent initialized.");
}

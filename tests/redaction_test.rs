use aegis_ai_agent::redaction::Redactor;

#[test]
fn test_secret_scanner_redaction() {
    let redactor = Redactor::new();
    let input = "My AWS key is AKIA1234567890ABCDEF and my IP is 192.168.1.1. Also my password is super-secret-1234567890123456789012345678901234567890";
    let redacted = redactor.redact(input);

    assert!(redacted.contains("<REDACTED_AWS_KEY>"));
    assert!(redacted.contains("<REDACTED_IP>"));
    assert!(redacted.contains("<REDACTED_SECRET>"));
    assert!(!redacted.contains("AKIA1234567890ABCDEF"));
    assert!(!redacted.contains("192.168.1.1"));
}

#[test]
fn test_secret_scanner_default() {
    let _scanner = aegis_ai_agent::redaction::scanner::SecretScanner::default();
}

#[test]
fn test_redactor_no_nlp_fallback() {
    // If model files are missing, it should still work (just no NLP redaction)
    let redactor = Redactor::new();
    let input = "Hello John Doe, your key is AKIA1234567890ABCDEF";
    let redacted = redactor.redact(input);

    assert!(redacted.contains("<REDACTED_AWS_KEY>"));
    // John Doe might not be redacted if NLP is missing, but the function should not crash
    assert!(redacted.contains("John Doe") || redacted.contains("<REDACTED_PERSON>"));
}

#[test]
fn test_redact_ip_address() {
    let redactor = Redactor::new();
    let input = "Server at 10.0.0.1";
    let redacted = redactor.redact(input);
    assert!(redacted.contains("<REDACTED_IP>"));
}

#[test]
fn test_redactor_default() {
    let _redactor = Redactor::default();
}

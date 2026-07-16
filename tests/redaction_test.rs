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
fn test_redact_connection_url_password() {
    let redactor = Redactor::new();
    let redacted = aegis_ai_agent::extractor::redact_environment_value_with_redactor(
        "DATABASE_URL",
        "postgres://app_user:secret-password@postgres.default.svc:5432/app_db",
        &redactor,
    );

    assert_eq!(
        redacted,
        "postgres://app_user:aegis-mock-secret@postgres.default.svc:5432/app_db"
    );
    assert!(!redacted.contains("secret-password"));
}

#[test]
fn test_sql_redaction_preserves_dump_syntax() {
    let redactor = Redactor::new();
    let input = "INSERT INTO users (full_name, email, jwt, aws_key, note) VALUES ('John Doe', 'john.doe@example.com', 'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature', 'AKIA1234567890ABCDEF', 'kept');";

    let redacted = redactor.redact_sql_line(input);

    assert_eq!(redacted.matches('(').count(), input.matches('(').count());
    assert_eq!(redacted.matches(')').count(), input.matches(')').count());
    assert_eq!(redacted.matches('\'').count(), input.matches('\'').count());
    assert!(redacted.starts_with("INSERT INTO users"));
    assert!(redacted.ends_with(";"));
    assert!(redacted.contains("VALUES"));
    assert!(!redacted.contains("John Doe"));
    assert!(!redacted.contains("john.doe@example.com"));
    assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
    assert!(!redacted.contains("AKIA1234567890ABCDEF"));
    assert_eq!(redacted.matches("[REDACTED]").count(), 4);
}

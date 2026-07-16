use regex::Regex;

pub struct SecretScanner {
    patterns: Vec<(Regex, &'static str)>,
    sql_patterns: Vec<(Regex, &'static str)>,
}

impl SecretScanner {
    pub fn new() -> Self {
        let aws_key = Regex::new(r"AKIA[0-9A-Z]{16}").unwrap();
        let jwt = Regex::new(r"eyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").unwrap();
        let email = Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}").unwrap();
        let person_name = Regex::new(r"\b[A-Z][a-z]{2,}\s+[A-Z][a-z]{2,}\b").unwrap();

        let patterns = vec![
            // AWS Keys
            (aws_key.clone(), "<REDACTED_AWS_KEY>"),
            // JWTs
            (jwt.clone(), "<REDACTED_SECRET>"),
            // Generic high entropy string (e.g. JWT, passwords)
            // Simplified for now, real implementation would use Shannon entropy
            (
                Regex::new(r"[a-zA-Z0-9+/]{40,}={0,2}").unwrap(),
                "<REDACTED_SECRET>",
            ),
            // IPv4 (Not a secret but often requested to be redacted)
            (
                Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
                "<REDACTED_IP>",
            ),
        ];

        let sql_patterns = vec![
            (aws_key, "[REDACTED]"),
            (jwt, "[REDACTED]"),
            (email, "[REDACTED]"),
            (person_name, "[REDACTED]"),
        ];

        Self {
            patterns,
            sql_patterns,
        }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        for (re, replacement) in &self.patterns {
            output = re.replace_all(&output, *replacement).into_owned();
        }
        output
    }

    pub fn redact_sql_line(&self, input: &str) -> String {
        let mut output = input.to_string();
        for (re, replacement) in &self.sql_patterns {
            output = re.replace_all(&output, *replacement).into_owned();
        }
        output
    }
}

impl Default for SecretScanner {
    fn default() -> Self {
        Self::new()
    }
}

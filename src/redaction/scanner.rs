use regex::Regex;

pub struct SecretScanner {
    patterns: Vec<(Regex, &'static str)>,
}

impl SecretScanner {
    pub fn new() -> Self {
        let patterns = vec![
            // AWS Keys
            (
                Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
                "<REDACTED_AWS_KEY>",
            ),
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

        Self { patterns }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        for (re, replacement) in &self.patterns {
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

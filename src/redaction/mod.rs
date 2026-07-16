#[cfg(feature = "redaction")]
pub mod nlp;
pub mod scanner;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedactionType {
    PII,
    Secret,
}

pub struct Redactor {
    #[cfg(feature = "redaction")]
    nlp_engine: Option<nlp::NlpEngine>,
    secret_scanner: scanner::SecretScanner,
}

impl Redactor {
    pub fn new() -> Self {
        #[cfg(feature = "redaction")]
        let nlp_engine = nlp::NlpEngine::new().ok();

        Self {
            #[cfg(feature = "redaction")]
            nlp_engine,
            secret_scanner: scanner::SecretScanner::new(),
        }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();

        // 1. Redact secrets using entropy/patterns
        output = self.secret_scanner.redact(&output);

        // 2. Redact PII using NLP if available
        #[cfg(feature = "redaction")]
        if let Some(engine) = &self.nlp_engine {
            output = engine.redact(&output);
        }

        output
    }

    pub fn redact_sql_line(&self, input: &str) -> String {
        let mut output = self.secret_scanner.redact_sql_line(input);

        #[cfg(feature = "redaction")]
        if let Some(engine) = &self.nlp_engine {
            output = engine.redact(&output);
        }

        output
    }
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

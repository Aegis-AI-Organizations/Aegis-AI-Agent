pub fn startup_message() -> &'static str {
    "Aegis AI Agent initialized."
}

pub fn init_agent() {
    println!("{}", startup_message());
}

#[cfg(test)]
mod tests {
    use super::startup_message;

    #[test]
    fn startup_message_matches_expected_banner() {
        assert_eq!(startup_message(), "Aegis AI Agent initialized.");
    }
}

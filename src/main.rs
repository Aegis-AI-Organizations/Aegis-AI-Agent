mod agent;

fn startup_banner() -> &'static str {
    "Hello, world! Aegis AI Agent is starting..."
}

fn main() {
    println!("{}", startup_banner());
    agent::init_agent();
}

#[cfg(test)]
mod tests {
    use super::startup_banner;

    #[test]
    fn startup_banner_matches_expected_message() {
        assert_eq!(
            startup_banner(),
            "Hello, world! Aegis AI Agent is starting..."
        );
    }
}

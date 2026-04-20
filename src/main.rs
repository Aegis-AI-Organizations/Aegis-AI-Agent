mod agent;
mod health;

use axum::{Router, routing::get};
use dotenvy::dotenv;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::net::SocketAddr;

fn startup_banner() -> &'static str {
    "Hello, world! Aegis AI Agent is starting..."
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    println!("{}", startup_banner());

    // 1. Initialize Agent logic
    agent::init_agent();

    // 2. Initialize Database pool
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    // 3. Build Axum router
    let app = Router::new()
        .route("/admin/system/health", get(health::health_handler))
        .with_state(pool);

    // 4. Start HTTP server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8081));
    println!("📡 Health server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
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

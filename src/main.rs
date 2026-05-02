#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    aegis_ai_agent::run().await
}

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    parley::run().await
}

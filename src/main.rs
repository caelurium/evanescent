mod catalog;
mod controller;
mod expiry;
mod helm;
mod stack;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let catalog = catalog::Catalog::from_env()?;
    controller::run(kube::Client::try_default().await?, catalog).await;
    Ok(())
}

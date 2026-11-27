mod controller;
mod expiry;
mod helm;
mod stack;

#[tokio::main]
async fn main() -> Result<(), kube::Error> {
    tracing_subscriber::fmt::init();
    controller::run(kube::Client::try_default().await?).await;
    Ok(())
}

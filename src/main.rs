use clap::Parser;
use orchard::cli::{Cli, run};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run(Cli::parse().command).await
}

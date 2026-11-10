use clap::Parser;
use orchard::cli::{Cli, run};

#[tokio::main]
async fn main() {
    if let Err(error) = run(Cli::parse().command).await {
        eprintln!("orchard: {error}");
        std::process::exit(1);
    }
}

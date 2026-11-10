use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

use crate::db::{Store, StorePaths};
use crate::secrets::SecretKey;

#[derive(Parser)]
#[command(name = "orchard", version, about = "Self-hosted personal budgeting")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the HTTP server.
    Serve {
        /// Address to listen on; defaults to loopback only.
        #[arg(long, default_value = "127.0.0.1:3000")]
        bind: SocketAddr,
        /// Directory holding the SQLite store and original evidence.
        #[arg(long, default_value = "/data")]
        data_dir: PathBuf,
        /// Mounted file containing the 32-byte credential key as 64 hex characters.
        #[arg(long, env = "ORCHARD_SECRET_KEY_FILE")]
        key_file: PathBuf,
    },
}

pub async fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Serve {
            bind,
            data_dir,
            key_file,
        } => serve(bind, data_dir, key_file).await,
    }
}

async fn serve(
    bind: SocketAddr,
    data_dir: PathBuf,
    key_file: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let key = SecretKey::load(&key_file)?;
    let store = Store::open(StorePaths::new(data_dir), &key)?;

    let listener = TcpListener::bind(bind).await?;
    eprintln!("Orchard listening on http://{}", listener.local_addr()?);
    eprintln!("Store: {}", store.paths().root().display());

    axum::serve(listener, crate::web::router())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

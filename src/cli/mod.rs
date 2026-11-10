use std::error::Error;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::net::TcpListener;

use crate::auth;
use crate::db::{Store, StorePaths};
use crate::secrets::SecretKey;
use crate::web::{AppState, WebConfig, router};

#[derive(Parser)]
#[command(name = "orchard", version, about = "Self-hosted personal budgeting")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create the single owner, USD budget, and timezone (host access only).
    Init {
        /// Directory holding the SQLite store and original evidence.
        #[arg(long, default_value = "/data")]
        data_dir: PathBuf,
        /// Mounted file containing the 32-byte credential key as 64 hex characters.
        #[arg(long, env = "ORCHARD_SECRET_KEY_FILE")]
        key_file: PathBuf,
        /// IANA budgeting timezone, for example `America/Chicago`.
        #[arg(long)]
        timezone: String,
        /// Display name for the single budget.
        #[arg(long, default_value = "Orchard Budget")]
        budget_name: String,
        /// Read the password and its confirmation from stdin instead of a TTY.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Replace the owner password and revoke all sessions (host access only).
    Recover {
        /// Directory holding the SQLite store and original evidence.
        #[arg(long, default_value = "/data")]
        data_dir: PathBuf,
        /// Mounted file containing the 32-byte credential key as 64 hex characters.
        #[arg(long, env = "ORCHARD_SECRET_KEY_FILE")]
        key_file: PathBuf,
        /// Read the new password and its confirmation from stdin.
        #[arg(long)]
        password_stdin: bool,
    },
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
        /// Public HTTPS origin used by the reverse proxy, for example
        /// `https://budget.example.com`.
        #[arg(long)]
        public_url: Option<String>,
        /// Reverse-proxy address allowed to supply forwarded client headers.
        /// Repeat for multiple proxies.
        #[arg(long = "trusted-proxy", value_name = "IP")]
        trusted_proxies: Vec<IpAddr>,
        /// Omit the Secure cookie attribute. Use only for loopback HTTP
        /// verification, never for the public deployment.
        #[arg(long)]
        insecure_cookies: bool,
    },
}

pub async fn run(command: Command) -> Result<(), Box<dyn Error>> {
    match command {
        Command::Init {
            data_dir,
            key_file,
            timezone,
            budget_name,
            password_stdin,
        } => {
            initialize(&data_dir, &key_file, &timezone, &budget_name, password_stdin).await
        }
        Command::Recover {
            data_dir,
            key_file,
            password_stdin,
        } => recover(&data_dir, &key_file, password_stdin).await,
        Command::Serve {
            bind,
            data_dir,
            key_file,
            public_url,
            trusted_proxies,
            insecure_cookies,
        } => {
            serve(
                bind,
                data_dir,
                key_file,
                public_url,
                trusted_proxies,
                insecure_cookies,
            )
            .await
        }
    }
}

async fn initialize(
    data_dir: &Path,
    key_file: &Path,
    timezone: &str,
    budget_name: &str,
    password_stdin: bool,
) -> Result<(), Box<dyn Error>> {
    let password = read_new_password(password_stdin)?;
    let key = SecretKey::load(key_file)?;
    let store = Store::open(StorePaths::new(data_dir), &key)?;
    auth::initialize(store.db(), &password, timezone, budget_name).await?;
    println!(
        "Initialized one owner and one USD budget in timezone {}.",
        timezone
    );
    Ok(())
}

async fn recover(
    data_dir: &Path,
    key_file: &Path,
    password_stdin: bool,
) -> Result<(), Box<dyn Error>> {
    let password = read_new_password(password_stdin)?;
    let key = SecretKey::load(key_file)?;
    let store = Store::open(StorePaths::new(data_dir), &key)?;
    auth::recover(store.db(), &password).await?;
    println!("Owner password replaced; all existing sessions were revoked.");
    println!("Financial data, budget settings, and provider connections were preserved.");
    Ok(())
}

async fn serve(
    bind: SocketAddr,
    data_dir: PathBuf,
    key_file: PathBuf,
    public_url: Option<String>,
    trusted_proxies: Vec<IpAddr>,
    insecure_cookies: bool,
) -> Result<(), Box<dyn Error>> {
    if let Some(url) = &public_url
        && !url.starts_with("https://")
        && !url.starts_with("http://")
    {
        return Err("--public-url must start with https:// or http://".into());
    }
    if insecure_cookies {
        eprintln!(
            "warning: --insecure-cookies omits the Secure session-cookie attribute; use only for local HTTP verification"
        );
    }

    let key = SecretKey::load(&key_file)?;
    let store = Arc::new(Store::open(StorePaths::new(data_dir), &key)?);
    let state = AppState::new(
        Arc::clone(&store),
        WebConfig {
            public_url,
            trusted_proxies,
            secure_cookies: !insecure_cookies,
        },
    );

    let listener = TcpListener::bind(bind).await?;
    eprintln!("Orchard listening on http://{}", listener.local_addr()?);
    eprintln!("Store: {}", store.paths().root().display());

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

fn read_new_password(from_stdin: bool) -> Result<String, Box<dyn Error>> {
    if from_stdin {
        let mut first = String::new();
        let mut second = String::new();
        io::stdin().read_line(&mut first)?;
        io::stdin().read_line(&mut second)?;
        let first = first.trim_end_matches(['\r', '\n']).to_owned();
        let second = second.trim_end_matches(['\r', '\n']).to_owned();
        if first != second {
            return Err("the two passwords do not match".into());
        }
        return Ok(first);
    }

    let first = rpassword::prompt_password("Owner password: ")?;
    let second = rpassword::prompt_password("Repeat owner password: ")?;
    if first != second {
        return Err("the two passwords do not match".into());
    }
    Ok(first)
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

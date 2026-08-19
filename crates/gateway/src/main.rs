// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

//! Thin binary wrapper: read the environment, bind a port, serve.
//!
//! All the logic lives in the library so it can be reused without starting a
//! server. See the crate documentation for embedding.

use clap::Parser;
use std::sync::Arc;

use cram_vertex::{router, AppState};

mod banner;
mod cli;
mod config;
mod dashboard;
mod service;
mod update;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match cli.command.unwrap_or(cli::Commands::Serve {
        port: None,
        no_open: false,
        quiet: false,
    }) {
        cli::Commands::Dash => {
            let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
            let dash_url = format!("http://{addr}/_cram/");
            if webbrowser::open(&dash_url).is_err() {
                eprintln!("Failed to open browser. Navigate to: {dash_url}");
            }
            Ok(())
        }
        cli::Commands::Auth { provider } => auth(provider),
        cli::Commands::Service { action } => service::handle_service(action),
        cli::Commands::Serve {
            port,
            no_open,
            quiet,
        } => serve(port, no_open, quiet).await,
    }
}

fn auth(provider: cli::AuthProvider) -> anyhow::Result<()> {
    match provider {
        cli::AuthProvider::Vertex { key_file } => {
            if !key_file.exists() {
                eprintln!("cram: key file does not exist: {}", key_file.display());
                std::process::exit(1);
            }

            let content = std::fs::read_to_string(&key_file).map_err(|e| {
                anyhow::anyhow!("failed to read key file {}: {}", key_file.display(), e)
            })?;

            let json: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| anyhow::anyhow!("key file is not valid JSON: {}", e))?;

            if json.get("type").and_then(|v| v.as_str()) != Some("service_account")
                && json.get("client_email").is_none()
            {
                eprintln!(
                    "cram: {} does not appear to be a Google service account key file",
                    key_file.display()
                );
                std::process::exit(1);
            }

            let canonical = std::fs::canonicalize(&key_file)
                .unwrap_or_else(|_| key_file.clone())
                .display()
                .to_string();

            let home = config::cram_home();
            let creds_path = home.join("credentials.toml");
            let mut creds = config::load_credentials_file(&creds_path)?.unwrap_or_default();

            let mut vertex = creds.vertex.unwrap_or_default();
            vertex.key_file = Some(canonical);
            creds.vertex = Some(vertex);

            config::save_credentials_file(&creds_path, &creds)?;
            println!("Saved Vertex credentials to {}", creds_path.display());
            Ok(())
        }
    }
}

async fn serve(port: Option<u16>, no_open: bool, quiet: bool) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cram=info,cram_vertex=info".into()),
        )
        .init();

    let home = config::cram_home();
    let config_file = config::load_config_file(&home.join("config.toml")).unwrap_or_else(|e| {
        eprintln!("\ncram: {e}");
        std::process::exit(1);
    });

    let credentials_file = config::load_credentials_file(&home.join("credentials.toml"))
        .unwrap_or_else(|e| {
            eprintln!("\ncram: {e}");
            std::process::exit(1);
        });

    let update_disabled = config::is_update_check_disabled(config_file.as_ref());
    let update_notice = update::get_cached_update_notice(update_disabled);
    update::spawn_update_check(update_disabled);

    let resolved = config::resolve(port, config_file, credentials_file).unwrap_or_else(|e| {
        eprintln!("\ncram: {e}");
        std::process::exit(1);
    });

    if resolved.vertex.gateway_key().is_empty() {
        tracing::warn!("GATEWAY_API_KEY is empty — the gateway will not check authentication");
    }

    let observer = Arc::new(dashboard::DashboardObserver::new());
    let state = match AppState::discover(resolved.vertex.clone()).await {
        Ok(s) => s.with_observer(observer.clone()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("no available authentication method found") {
                eprintln!("\ncram: no Google Cloud credentials found\n");
                eprintln!("  Looked for:");
                eprintln!("    $GOOGLE_APPLICATION_CREDENTIALS   (not set)");
                eprintln!("    ~/.config/gcloud/application_default_credentials.json   (missing)");
                eprintln!("    GCE metadata server   (unavailable)\n");
                eprintln!("  To fix, either point at a service account key:");
                eprintln!("    export GOOGLE_APPLICATION_CREDENTIALS=/path/to/sa.json");
                eprintln!("    (fish: set -x GOOGLE_APPLICATION_CREDENTIALS /path/to/sa.json)\n");
                eprintln!("  or run:");
                eprintln!("    cram auth vertex --key-file /path/to/sa.json\n");
                eprintln!("  or log in with your own account:");
                eprintln!("    gcloud auth application-default login\n");
            } else {
                eprintln!("\n{}", msg);
            }
            std::process::exit(1);
        }
    };

    let bind_port = resolved.port;
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| format!("127.0.0.1:{bind_port}"));
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("\ncram: port {bind_port} is already in use.");
            eprintln!(
                "To fix, either stop the other instance or start cram on a different port:\n"
            );
            eprintln!("  cram serve --port {}\n", bind_port + 1);
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };

    if !quiet {
        banner::print_banner(&resolved.vertex, bind_port, update_notice);
    }

    if !no_open {
        let _ = webbrowser::open(&format!("http://127.0.0.1:{bind_port}/_cram/"));
    }

    let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    let app = router(state).merge(dashboard::router(observer, shutdown_tx.clone()));

    let serve = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal(shutdown_tx));

    tokio::select! {
        res = serve => {
            res?;
        }
        _ = async {
            let _ = shutdown_rx.recv().await;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        } => {
            tracing::warn!("shutdown deadline exceeded, forcing exit");
        }
    }

    Ok(())
}

async fn shutdown_signal(shutdown_tx: tokio::sync::broadcast::Sender<()>) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received, exiting");
    let _ = shutdown_tx.send(());
}

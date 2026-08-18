// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

//! Thin binary wrapper: read the environment, bind a port, serve.
//!
//! All the logic lives in the library so it can be reused without starting a
//! server. See the crate documentation for embedding.

use clap::Parser;
use std::sync::Arc;

use cram_vertex::{router, AppState, Config};

mod banner;
mod cli;
mod dashboard;

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
            return Ok(());
        }
        cli::Commands::Serve {
            port,
            no_open,
            quiet,
        } => serve(port, no_open, quiet).await,
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

    let cfg = Config::from_env().unwrap_or_else(|e| {
        let msg = e.to_string();
        if msg.contains("missing environment variable GCP_PROJECT_ID") {
            eprintln!("\ncram: missing GCP_PROJECT_ID");
            eprintln!(
                "Set it to the Google Cloud Project ID (not the display name or project number)."
            );
        } else {
            eprintln!("\ncram configuration error: {e}");
        }
        std::process::exit(1);
    });

    if cfg.gateway_key().is_empty() {
        tracing::warn!("GATEWAY_API_KEY is empty — the gateway will not check authentication");
    }

    let observer = Arc::new(dashboard::DashboardObserver::new());
    let state = match AppState::discover(cfg.clone()).await {
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
                eprintln!("  or log in with your own account:");
                eprintln!("    gcloud auth application-default login\n");
            } else {
                eprintln!("\n{}", msg);
            }
            std::process::exit(1);
        }
    };

    let port = port.unwrap_or(8787);
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| format!("127.0.0.1:{port}"));
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            eprintln!("\ncram: port {port} is already in use.");
            eprintln!(
                "To fix, either stop the other instance or start cram on a different port:\n"
            );
            eprintln!("  cram serve --port 8788\n");
            std::process::exit(1);
        }
        Err(e) => return Err(e.into()),
    };

    if !quiet {
        banner::print_banner(&cfg, port);
    }

    if !no_open {
        let _ = webbrowser::open(&format!("http://127.0.0.1:{port}/_cram/"));
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

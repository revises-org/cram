// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cram", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Serve the gateway
    Serve {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        no_open: bool,
        #[arg(long)]
        quiet: bool,
    },
    /// Open the dashboard in a browser
    Dash,
    /// Manage credentials
    Auth {
        #[command(subcommand)]
        provider: AuthProvider,
    },
    /// Manage systemd user service (Linux only)
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
pub enum AuthProvider {
    /// Authenticate Vertex AI with a service account key file
    Vertex {
        #[arg(long)]
        key_file: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum ServiceAction {
    /// Install systemd user service
    Install {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Uninstall systemd user service
    Uninstall,
    /// Check systemd user service status
    Status,
}

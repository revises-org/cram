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
}

#[derive(Subcommand)]
pub enum AuthProvider {
    /// Authenticate Vertex AI with a service account key file
    Vertex {
        #[arg(long)]
        key_file: PathBuf,
    },
}

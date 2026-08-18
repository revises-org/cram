use clap::{Parser, Subcommand};

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
}

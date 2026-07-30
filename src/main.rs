mod cli;
mod db;
mod lineage;
mod tui;

use clap::Parser;
use cli::{wants_cli, Cli};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    if wants_cli(&cli) {
        if let Err(msg) = cli::dispatch(cli).await {
            eprintln!("{}", msg);
            std::process::exit(1);
        }
        return Ok(());
    }
    tui::run().await
}

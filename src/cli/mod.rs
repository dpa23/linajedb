//! CLI entry points for scripts and AI agents.

mod arbol;

use clap::{Parser, Subcommand, ValueEnum};

pub use arbol::run_arbol;

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Tree,
    Json,
}

#[derive(Parser, Debug)]
#[command(
    name = "linajedb",
    about = "Multi-engine terminal DB client with row lineage tracing",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Trace row lineage with a readable dual tree (ascendencia / descendencia).
    /// Preferred command for AI agents and scripts.
    Arbol {
        /// Full connection URL (mysql://…, postgres://…, mongodb://…/db, bolt://…, or sqlite path)
        url: String,
        /// Table, collection, or Neo4j label
        tabla: String,
        /// Row filter: SQL condition, Mongo JSON, or Neo4j prop=value
        fila: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Tree)]
        format: OutputFormat,
    },
    /// Legacy flag-based lineage (same engine as `arbol`).
    Trace {
        #[arg(long)]
        url: String,
        #[arg(long)]
        table: String,
        #[arg(long)]
        r#where: String,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
}

pub async fn dispatch(cli: Cli) -> Result<(), String> {
    match cli.command {
        Some(Commands::Arbol {
            url,
            tabla,
            fila,
            format,
        }) => {
            run_arbol(&url, &tabla, &fila, matches!(format, OutputFormat::Json)).await
        }
        Some(Commands::Trace {
            url,
            table,
            r#where,
            format,
        }) => run_arbol(&url, &table, &r#where, matches!(format, OutputFormat::Json)).await,
        None => Ok(()), // fall through to TUI
    }
}

/// Returns true when a CLI subcommand was present (caller should not start TUI).
pub fn wants_cli(cli: &Cli) -> bool {
    cli.command.is_some()
}

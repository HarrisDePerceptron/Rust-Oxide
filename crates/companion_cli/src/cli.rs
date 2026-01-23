use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub const DEFAULT_DB: &str = "postgres";
pub const DEFAULT_PORT: u16 = 3000;

#[derive(Parser)]
#[command(author, version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Init(InitArgs),
    New(InitArgs),
}

#[derive(Parser, Clone)]
pub struct InitArgs {
    /// Project name (used for directory name and crate name derivation)
    pub name: Option<String>,
    /// Output directory (defaults to ./<name>)
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Database choice (only postgres supported for now)
    #[arg(long, default_value = DEFAULT_DB)]
    pub db: String,
    /// Database URL (overrides env/default)
    #[arg(long)]
    pub database_url: Option<String>,
    /// Server port (overrides env/default)
    #[arg(long)]
    pub port: Option<u16>,
    /// Enable auth (currently always enabled in template)
    #[arg(long, default_value_t = true)]
    pub auth: bool,
    /// Template repo URL (or set SAMPLE_SERVER_TEMPLATE_REPO)
    #[arg(long)]
    pub repo: Option<String>,
    /// Overwrite existing output directory
    #[arg(long)]
    pub force: bool,
    /// Disable interactive prompts
    #[arg(long)]
    pub non_interactive: bool,
}

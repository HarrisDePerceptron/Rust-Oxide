use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub const DEFAULT_DB: &str = "postgres";
pub const SQLITE_DB: &str = "sqlite";
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
    Api(ApiArgs),
}

#[derive(Parser, Clone)]
pub struct InitArgs {
    /// Project name (used for directory name and crate name derivation)
    pub name: Option<String>,
    /// Output directory (defaults to ./<name>)
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Database choice (supported: postgres, sqlite)
    #[arg(long, default_value = DEFAULT_DB)]
    pub db: String,
    /// Database URL (overrides env/default)
    #[arg(long)]
    pub database_url: Option<String>,
    /// Server port (overrides env/default)
    #[arg(long)]
    pub port: Option<u16>,
    /// Enable local auth provider scaffolding
    #[arg(long = "auth-local", default_value_t = true)]
    pub auth_local: bool,
    /// Disable local auth provider scaffolding
    #[arg(long = "no-auth-local", default_value_t = false)]
    pub no_auth_local: bool,
    /// Include todo example modules, routes, and tests
    #[arg(long = "todo-example", default_value_t = true)]
    pub todo_example: bool,
    /// Exclude todo example modules, routes, and tests
    #[arg(long = "no-todo-example", default_value_t = false)]
    pub no_todo_example: bool,
    /// Include docs page
    #[arg(long = "docs", default_value_t = true)]
    pub docs: bool,
    /// Exclude docs page
    #[arg(long = "no-docs", default_value_t = false)]
    pub no_docs: bool,
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

#[derive(Parser, Clone)]
pub struct AddApiArgs {
    /// Resource/entity name (singular)
    pub name: String,
    /// Override plural form (defaults to simple pluralization)
    #[arg(long)]
    pub plural: Option<String>,
    /// Override database table name (defaults to plural snake_case)
    #[arg(long)]
    pub table: Option<String>,
    /// Base path for CRUD routes (defaults to /<name> or /<plural>)
    #[arg(long)]
    pub base_path: Option<String>,
    /// Comma-separated field list (e.g. title:string,done:bool)
    #[arg(long)]
    pub fields: Option<String>,
    /// Disable auth middleware on the CRUD routes
    #[arg(long)]
    pub no_auth: bool,
    /// Print planned changes without writing files
    #[arg(long)]
    pub dry_run: bool,
    /// Overwrite existing files
    #[arg(long)]
    pub force: bool,
}

#[derive(Parser, Clone)]
pub struct RemoveApiArgs {
    /// Resource/entity name (singular)
    pub name: String,
    /// Print planned changes without writing files
    #[arg(long)]
    pub dry_run: bool,
    /// Remove registry entry even if some files/edits are missing
    #[arg(long)]
    pub prune: bool,
    /// Remove even if files were modified or missing
    #[arg(long)]
    pub force: bool,
}

#[derive(Parser, Clone)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub command: ApiCommands,
}

#[derive(Subcommand, Clone)]
pub enum ApiCommands {
    Add(AddApiArgs),
    Remove(RemoveApiArgs),
}

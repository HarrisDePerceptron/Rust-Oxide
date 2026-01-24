mod cli;
mod add_api;
mod api_remove;
mod init;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Init(args) | cli::Commands::New(args) => init::run(args),
        cli::Commands::AddApi(args) => add_api::run(args),
        cli::Commands::ApiRemove(args) => api_remove::run(args),
    }
}

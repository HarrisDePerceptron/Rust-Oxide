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
        cli::Commands::Api(api) => match api.command {
            cli::ApiCommands::Add(args) => add_api::run(args),
            cli::ApiCommands::Remove(args) => api_remove::run(args),
        },
    }
}

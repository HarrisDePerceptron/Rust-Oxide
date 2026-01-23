mod cli;
mod init;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Init(args) | cli::Commands::New(args) => init::run(args),
    }
}

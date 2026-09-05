use anyhow::Result;
use clap::Parser;
use ytkew::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    ytkew::run::run(Cli::parse()).await
}

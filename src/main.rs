use clap::Parser;
use reading_steiner::cli::{self, Cli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    cli::run(cli).await?;
    Ok(())
}

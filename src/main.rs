use clap::Parser;
use tracing_subscriber::EnvFilter;
use ttrpg_scheduling::{cli, config, db, serve};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = cli::Cli::parse();

    let config = config::Config::from_env()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
        )
        .init();
    let db = db::connect(&config.database_url).await?;

    match cli.command {
        Some(cli::Command::CreateAdmin {
            username,
            display_name,
        }) => cli::create_admin(&db, username, display_name).await,
        Some(cli::Command::Backup { destination }) => cli::backup(&db, &destination).await,
        None => serve(config, db).await,
    }
}

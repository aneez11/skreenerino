use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "skreenerino", about = "LAN screen projection")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Send your screen to a target user
    Send { target: String },
    /// Join a room as viewer/participant
    Join { room: String },
    /// List discovered peers/rooms (stub)
    Discover,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Send { target } => println!("send: {target} (not implemented)"),
        Commands::Join { room } => println!("join: {room} (not implemented)"),
        Commands::Discover => println!("discover: (not implemented)"),
    }
    Ok(())
}

mod assets;
mod build;
mod cli;
mod config;
mod daemon;
mod obsidian;
mod project;
mod registry;
mod renderer;
mod search;
mod server;
mod system_theme;
mod template;
mod themes;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "enginemd=info".into()),
        )
        .init();

    let cli = Cli::parse();

    if let Err(e) = config::ensure_dirs() {
        eprintln!("Error initializing config directories: {e}");
        std::process::exit(1);
    }

    match &cli.command {
        Some(Commands::New { name, js_support }) => {
            if let Err(e) = project::cmd_new(name, js_support.as_deref()) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Up { path, js_support }) => {
            if let Err(e) = project::cmd_up(path, js_support.as_deref()) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Down { name }) => {
            if let Err(e) = project::cmd_down(name) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Fetch { force }) => {
            if let Err(e) = registry::cmd_fetch(*force).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Build { out }) => {
            if let Err(e) = build::cmd_build(out, cli.path.as_deref()).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Daemon { action }) => {
            if let Err(e) = daemon::handle(action, &cli) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        None => {
            let settings = config::load_settings();
            let js_override = cli.js_support.as_ref().map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            });

            if let Err(e) = server::start_server(
                settings,
                cli.watch,
                cli.path.clone(),
                cli.port,
                cli.lang.clone(),
                js_override,
            )
            .await
            {
                eprintln!("Server error: {e}");
                std::process::exit(1);
            }
        }
    }
}

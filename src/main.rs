mod cli;
mod config;
mod project;
mod registry;
mod renderer;
mod server;
mod template;

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

    match cli.command {
        Some(Commands::New { name, css, js_support }) => {
            if let Err(e) = project::cmd_new(&name, css.as_deref(), js_support.as_deref()) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Up { path, css, js_support }) => {
            if let Err(e) = project::cmd_up(&path, css.as_deref(), js_support.as_deref()) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Down { name }) => {
            if let Err(e) = project::cmd_down(&name) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Some(Commands::Fetch) => {
            if let Err(e) = registry::cmd_fetch().await {
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
                cli.path,
                cli.port,
                cli.lang,
                js_override,
                cli.css_support,
            )
            .await
            {
                eprintln!("Server error: {e}");
                std::process::exit(1);
            }
        }
    }
}

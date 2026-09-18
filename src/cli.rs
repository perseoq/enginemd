use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "enginemd", version, about = "Markdown to HTML server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(long, help = "Enable hot-reload mode (default port: 9696)")]
    pub watch: bool,

    #[arg(long, global = true, help = "Serve a single directory directly (no listing)")]
    pub path: Option<String>,

    #[arg(long, global = true, help = "Override server port")]
    pub port: Option<u16>,

    #[arg(long, global = true, help = "Override language")]
    pub lang: Option<String>,

    #[arg(long, global = true, help = "Comma-separated JS libs: mathjax,mermaid,chartjs,...")]
    pub js_support: Option<String>,

    #[arg(long, global = true, help = "CSS theme or path to custom CSS")]
    pub css_support: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Create a new site project")]
    New {
        name: String,
        #[arg(long, help = "CSS theme for this site")]
        css: Option<String>,
        #[arg(long, help = "Comma-separated JS libs for this site")]
        js_support: Option<String>,
    },
    #[command(about = "Register an existing directory as a site")]
    Up {
        path: String,
        #[arg(long, help = "CSS theme for this site")]
        css: Option<String>,
        #[arg(long, help = "Comma-separated JS libs for this site")]
        js_support: Option<String>,
    },
    #[command(about = "Remove/unregister a site")]
    Down {
        name: String,
    },
    #[command(about = "Download JS/CSS dependencies")]
    Fetch {
        #[arg(long, help = "Re-download assets even if already cached")]
        force: bool,
    },
    #[command(about = "Manage the background daemon")]
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
pub enum DaemonAction {
    #[command(about = "Start the server in the background (survives reboot)")]
    Start,
    #[command(about = "Stop the daemon and disable autostart")]
    Stop,
    #[command(about = "Restart the daemon")]
    Restart,
    #[command(about = "Show daemon status")]
    Status,
}

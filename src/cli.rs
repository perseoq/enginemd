use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "enginemd", version, about = "Markdown to HTML server")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[arg(long, help = "Enable hot-reload mode (default port: 9696)")]
    pub watch: bool,

    #[arg(long, help = "Serve a single directory directly (no listing)")]
    pub path: Option<String>,

    #[arg(long, help = "Override server port")]
    pub port: Option<u16>,

    #[arg(long, help = "Override language")]
    pub lang: Option<String>,

    #[arg(long, help = "Comma-separated JS libs: mathjax,mermaid,chartjs,...")]
    pub js_support: Option<String>,

    #[arg(long, help = "CSS theme or path to custom CSS")]
    pub css_support: Option<String>,

    #[arg(long, help = "Enable Obsidian wikilinks/embeds")]
    pub obsidian: bool,
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
    #[command(about = "Download JS/CSS dependencies from CDN")]
    Fetch,
}

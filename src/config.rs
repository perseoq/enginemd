use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_watch_port")]
    pub watch_port: u16,
    #[serde(default = "default_lang")]
    pub lang: String,
    #[serde(default = "default_listing_per_page")]
    pub listing_per_page: usize,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub styles: HashMap<String, String>,
    #[serde(default)]
    pub directories: Vec<DirectoryEntry>,
    #[serde(default)]
    pub obsidian: bool,
    #[serde(default = "default_true")]
    pub auto_fetch: bool,
    #[serde(default = "default_true")]
    pub sri: bool,
    #[serde(default)]
    pub assets_dir: Option<String>,
    #[serde(default)]
    pub cdn_base: Option<String>,
    #[serde(default)]
    pub cdn_fallbacks: Vec<String>,
    #[serde(default = "default_theme_source")]
    pub theme_source: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    #[serde(default = "default_active")]
    pub active: bool,
    pub js_support: Option<Vec<String>>,
    pub lang: Option<String>,
    #[serde(default)]
    pub obsidian: Option<bool>,
}

fn default_port() -> u16 {
    10300
}
fn default_watch_port() -> u16 {
    9696
}
fn default_lang() -> String {
    "en".to_string()
}
fn default_listing_per_page() -> usize {
    20
}
fn default_active() -> bool {
    true
}
fn default_true() -> bool {
    true
}
fn default_theme_source() -> String {
    "auto".to_string()
}

pub fn enginemd_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".enginemd")
}

pub fn settings_path() -> PathBuf {
    enginemd_dir().join("settings.json")
}

pub fn sites_dir() -> PathBuf {
    enginemd_dir().join("sites")
}

pub fn pid_path() -> PathBuf {
    enginemd_dir().join("enginemd.pid")
}

pub fn log_path() -> PathBuf {
    enginemd_dir().join("enginemd.log")
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    if !path.exists() {
        let settings = default_settings();
        let _ = save_settings(&settings);
        return settings;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Warning: failed to parse settings.json: {e}");
                eprintln!("Using defaults and will overwrite on next save.");
                let s = default_settings();
                let _ = save_settings(&s);
                s
            }
        },
        Err(e) => {
            eprintln!("Warning: could not read settings.json: {e}");
            default_settings()
        }
    }
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create config dir: {e}"))?;
    let content =
        serde_json::to_string_pretty(settings).map_err(|e| format!("serialization error: {e}"))?;
    std::fs::write(&path, &content).map_err(|e| format!("cannot write settings: {e}"))?;
    Ok(())
}

pub fn default_settings() -> Settings {
    let mut dependencies = HashMap::new();
    dependencies.insert(
        "mathjax".to_string(),
        "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js".to_string(),
    );
    dependencies.insert(
        "mermaid".to_string(),
        "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js".to_string(),
    );
    dependencies.insert(
        "chartjs".to_string(),
        "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js".to_string(),
    );
    dependencies.insert(
        "highlight".to_string(),
        "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/highlight.min.js".to_string(),
    );
    dependencies.insert(
        "katex".to_string(),
        "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js".to_string(),
    );
    dependencies.insert(
        "fontawesome".to_string(),
        "https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@6/css/all.min.css".to_string(),
    );
    dependencies.insert(
        "anchor".to_string(),
        "https://cdn.jsdelivr.net/npm/anchor-js@5/anchor.min.js".to_string(),
    );

    let mut styles = HashMap::new();
    let theme_names = [
        "auto",
        "github",
        "github-dark",
        "gitlab",
        "gitlab-dark",
        "stackoverflow",
        "stackoverflow-dark",
        "readthedocs",
        "readthedocs-dark",
        "medium",
        "hackernews",
        "material",
        "material-dark",
        "tailwind",
        "bootstrap",
        "bulma",
        "shadcn",
        "shadcn-dark",
        "windows",
        "macos",
        "clean",
        "white",
        "typewriter",
        "paper",
        "slate",
        "monochrome",
        "air",
        "book",
        "retro",
        "nord",
        "nord-dark",
        "solarized-light",
        "solarized-dark",
        "dracula",
        "monokai",
        "gruvbox-light",
        "gruvbox-dark",
        "catppuccin-latte",
        "catppuccin-mocha",
        "tokyo-night",
        "ayu-light",
        "ayu-dark",
        "rose-pine",
        "everforest-light",
        "everforest-dark",
    ];
    for name in &theme_names {
        styles.insert(name.to_string(), format!("{name}.css"));
    }

    Settings {
        port: 10300,
        watch_port: 9696,
        lang: "en".to_string(),
        listing_per_page: 20,
        dependencies,
        styles,
        directories: Vec::new(),
        obsidian: false,
        auto_fetch: true,
        sri: true,
        assets_dir: None,
        cdn_base: None,
        cdn_fallbacks: Vec::new(),
        theme_source: "auto".to_string(),
    }
}

/// Base directory for downloaded assets (js/, css/, assets.json).
pub fn assets_base(settings: &Settings) -> PathBuf {
    match &settings.assets_dir {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => enginemd_dir(),
    }
}

pub fn ensure_dirs() -> Result<(), String> {
    let root = enginemd_dir();
    for sub in &["", "js", "css", "sites"] {
        let p = if sub.is_empty() {
            root.clone()
        } else {
            root.join(sub)
        };
        std::fs::create_dir_all(&p).map_err(|e| format!("cannot create {}: {e}", p.display()))?;
    }
    Ok(())
}

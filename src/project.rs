use crate::config::{self, DirectoryEntry};
use std::path::PathBuf;

pub fn cmd_new(name: &str, js_support: Option<&str>) -> Result<(), String> {
    let sites_dir = config::sites_dir();
    let site_path = sites_dir.join(name);

    if site_path.exists() {
        return Err(format!(
            "Site '{}' already exists at {}",
            name,
            site_path.display()
        ));
    }

    std::fs::create_dir_all(&site_path)
        .map_err(|e| format!("cannot create site directory: {e}"))?;
    std::fs::create_dir_all(site_path.join("assets"))
        .map_err(|e| format!("cannot create assets directory: {e}"))?;

    let index_md = format!(
        "---\ntitle: {name}\ndescription: Welcome to {name}\n---\n\n# {name}\n\nStart editing this file.\n\n## Features\n\n- Write in **Markdown**\n- Math: $E = mc^2$\n- Diagrams with Mermaid\n- Charts with Chart.js\n"
    );
    std::fs::write(site_path.join("index.md"), &index_md)
        .map_err(|e| format!("cannot write index.md: {e}"))?;

    let js_vec: Option<Vec<String>> = js_support.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    });

    let mut settings = config::load_settings();
    settings.directories.push(DirectoryEntry {
        name: name.to_string(),
        path: site_path.to_string_lossy().to_string(),
        active: true,
        js_support: js_vec,
        lang: None,
        obsidian: None,
    });
    config::save_settings(&settings)?;

    println!("Created site '{}' at {}", name, site_path.display());
    Ok(())
}

pub fn cmd_up(path: &str, js_support: Option<&str>) -> Result<(), String> {
    let dir_path = PathBuf::from(path);
    if !dir_path.is_dir() {
        return Err(format!("Path '{}' is not a valid directory", path));
    }

    let canonical =
        std::fs::canonicalize(&dir_path).map_err(|e| format!("cannot resolve path: {e}"))?;
    let name = dir_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| "cannot determine directory name".to_string())?;

    let mut settings = config::load_settings();

    if settings.directories.iter().any(|d| d.name == name) {
        return Err(format!("Site '{}' is already registered", name));
    }

    let js_vec: Option<Vec<String>> = js_support.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    });

    settings.directories.push(DirectoryEntry {
        name: name.clone(),
        path: canonical.to_string_lossy().to_string(),
        active: true,
        js_support: js_vec,
        lang: None,
        obsidian: None,
    });
    config::save_settings(&settings)?;

    println!("Registered '{}' ({})", name, canonical.display());
    Ok(())
}

pub fn cmd_down(name: &str) -> Result<(), String> {
    let mut settings = config::load_settings();
    let len_before = settings.directories.len();
    settings.directories.retain(|d| d.name != name);
    if settings.directories.len() == len_before {
        return Err(format!("Site '{}' not found", name));
    }
    config::save_settings(&settings)?;
    println!("Removed site '{}'", name);
    Ok(())
}

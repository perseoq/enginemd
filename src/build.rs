use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::assets::{self, AssetManager};
use crate::config;
use crate::obsidian::VaultIndex;
use crate::renderer::{self, Frontmatter, RenderContext};
use crate::template::TemplateEngine;

type SiteEntry = (String, PathBuf, Option<Vec<String>>);

pub async fn cmd_build(out: &str, path: Option<&str>) -> Result<(), String> {
    let settings = config::load_settings();
    let out_root = PathBuf::from(out);
    std::fs::create_dir_all(&out_root).map_err(|e| format!("cannot create output dir: {e}"))?;

    let assets_base = config::assets_base(&settings);
    let manager = AssetManager::new(
        assets_base.clone(),
        settings.cdn_base.clone(),
        settings.cdn_fallbacks.clone(),
    );
    manager.prefetch_all().await;

    let asset_out = out_root.join("__enginemd");
    copy_dir(&assets_base.join("js"), &asset_out.join("js"))?;
    copy_dir(&assets_base.join("css"), &asset_out.join("css"))?;
    write_css_bundle(&asset_out.join("css"))?;

    let templates = TemplateEngine::new(false);

    let mut sites: Vec<SiteEntry> = Vec::new();
    if let Some(p) = path {
        let pb = PathBuf::from(p);
        let name = pb
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "site".to_string());
        sites.push((name, pb, None));
    } else {
        for d in &settings.directories {
            if d.active {
                sites.push((d.name.clone(), PathBuf::from(&d.path), d.js_support.clone()));
            }
        }
    }

    for (name, root, js_support) in &sites {
        let index = VaultIndex::build(root);
        let obsidian = settings.obsidian
            || root.join(".obsidian").is_dir()
            || crate::server::site_uses_obsidian_syntax(root);

        build_site(
            &templates,
            &manager,
            &settings,
            &out_root,
            name,
            root,
            js_support.as_deref(),
            obsidian,
            &index,
        )
        .await?;
    }

    println!("Build complete: {}", out_root.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn build_site(
    templates: &TemplateEngine,
    manager: &AssetManager,
    settings: &config::Settings,
    out_root: &Path,
    name: &str,
    root: &Path,
    site_js: Option<&[String]>,
    obsidian: bool,
    index: &VaultIndex,
) -> Result<(), String> {
    let css_name = "auto";
    let css_file = "auto.css".to_string();

    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;

    for rel in files {
        let src = root.join(&rel);
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        if rel_str.to_ascii_lowercase().ends_with(".md") {
            let content = std::fs::read_to_string(&src)
                .map_err(|e| format!("cannot read {}: {e}", src.display()))?;
            let mut fm = Frontmatter::default();

            let sub = rel.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let depth = sub.components().count();
            let url_prefix = "../".repeat(depth);
            let asset_prefix = format!("{}__enginemd/", "../".repeat(depth + 1));

            let render_ctx = if obsidian {
                Some(RenderContext {
                    site_root: root,
                    url_prefix: &url_prefix,
                    current_rel: &rel_str,
                    index,
                    obsidian: true,
                })
            } else {
                None
            };

            let body_html = renderer::render_markdown(&content, &mut fm, render_ctx.as_ref());
            let title = fm
                .title
                .as_deref()
                .map(|s| s.to_string())
                .or_else(|| renderer::extract_first_heading(&content));
            let title = title.as_deref();

            let mut needed: HashSet<&'static str> = assets::detect_assets(&content);
            if let Some(list) = site_js {
                for key in list {
                    if let Some(spec) = assets::find(key) {
                        needed.insert(spec.key);
                    }
                }
            }

            let (head, body_scripts, css_assets, inline) =
                manager.asset_list(&needed, css_name, &asset_prefix, settings.sri);

            let body_class = fm.cssclasses.join(" ");
            let html = templates.render_page(
                title,
                fm.description.as_deref(),
                &body_html,
                name,
                fm.lang.as_deref().unwrap_or(&settings.lang),
                &css_file,
                None,
                &body_class,
                &fm.tags,
                &asset_prefix,
                &css_assets,
                &head,
                &inline,
                &body_scripts,
                false,
            );
            let html = rewrite_md_links(&html);

            let out_file = output_html_path(out_root, name, &rel, &rel_str);
            if let Some(parent) = out_file.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            std::fs::write(&out_file, html)
                .map_err(|e| format!("cannot write {}: {e}", out_file.display()))?;
        } else {
            let dest = out_root.join(name).join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            std::fs::copy(&src, &dest)
                .map_err(|e| format!("cannot copy {}: {e}", src.display()))?;
        }
    }

    Ok(())
}

fn output_html_path(out_root: &Path, name: &str, rel: &Path, rel_str: &str) -> PathBuf {
    let file_name = rel
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let lower = file_name.to_ascii_lowercase();
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));

    if lower == "index.md" || lower == "init.md" {
        out_root.join(name).join(parent).join("index.html")
    } else {
        let stem = rel_str.trim_end_matches(".md").trim_end_matches(".MD");
        out_root.join(name).join(format!("{stem}.html"))
    }
}

fn rewrite_md_links(html: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"href="([^"]+)\.md(#[^"]*)?""#).unwrap());
    re.replace_all(html, |caps: &regex::Captures| {
        let frag = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        format!(r#"href="{}.html{frag}""#, &caps[1])
    })
    .into_owned()
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let path = entry.path();
        if file_type.is_dir() {
            if name == "node_modules" || name == "target" {
                continue;
            }
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let target = dest.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|e| format!("cannot copy {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

fn write_css_bundle(css_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(css_dir)
        .map_err(|e| format!("cannot create {}: {e}", css_dir.display()))?;
    std::fs::write(css_dir.join("base.css"), include_str!("../css/base.css"))
        .map_err(|e| format!("cannot write base.css: {e}"))?;

    let auto = crate::themes::render_auto_theme_css();
    std::fs::write(css_dir.join("auto.css"), auto)
        .map_err(|e| format!("cannot write auto.css: {e}"))?;

    Ok(())
}

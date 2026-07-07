use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use axum::{
    Router,
    extract::{Path as AxumPath, Query, State, WebSocketUpgrade, ws},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::config::{self, DirectoryEntry, Settings};
use crate::renderer::{self, render_markdown, Frontmatter};
use crate::template::{ListingEntry, TemplateEngine};

#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub settings: Settings,
    pub templates: Arc<TemplateEngine>,
    pub watch_mode: bool,
    pub reload_tx: broadcast::Sender<String>,
    pub override_path: Option<String>,
    pub override_lang: Option<String>,
    pub override_js: Option<Vec<String>>,
    pub override_css: Option<String>,
}

pub async fn start_server(
    settings: Settings,
    watch_mode: bool,
    override_path: Option<String>,
    override_port: Option<u16>,
    override_lang: Option<String>,
    override_js: Option<Vec<String>>,
    override_css: Option<String>,
) -> Result<(), String> {
    let (reload_tx, _) = broadcast::channel::<String>(100);
    let templates = Arc::new(TemplateEngine::new(watch_mode));

    let state = AppState {
        settings: settings.clone(),
        templates,
        watch_mode,
        reload_tx: reload_tx.clone(),
        override_path,
        override_lang,
        override_js,
        override_css,
    };

    let app = Router::new()
        .route("/", get(listing_or_single_handler))
        .route("/{*path}", get(catch_all_handler))
        .route("/__enginemd/js/{*file}", get(js_handler))
        .route("/__enginemd/css/{*file}", get(css_handler))
        .route("/__enginemd/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let bind_port = override_port.unwrap_or(if watch_mode {
        settings.watch_port
    } else {
        settings.port
    });

    let addr = format!("0.0.0.0:{bind_port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("cannot bind to {addr}: {e}"))?;

    info!("EngineMD listening on http://{}", addr);

    if watch_mode {
        let dirs: Vec<String> = if let Some(ref p) = state.override_path {
            vec![p.clone()]
        } else {
            settings.directories.iter()
                .filter(|d| d.active)
                .map(|d| d.path.clone())
                .collect()
        };
        let tx = reload_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = start_file_watcher(dirs, tx) {
                eprintln!("File watcher error: {e}");
            }
        });
    }

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))?;

    Ok(())
}

fn start_file_watcher(
    paths: Vec<String>,
    reload_tx: broadcast::Sender<String>,
) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer(
        std::time::Duration::from_millis(300),
        tx,
    )
    .map_err(|e| format!("debouncer: {e}"))?;

    for p in &paths {
        debouncer
            .watcher()
            .watch(Path::new(p), RecursiveMode::Recursive)
            .map_err(|e| format!("cannot watch {p}: {e}"))?;
    }

    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for _event in &events {
                        let _ = reload_tx.send("reload".to_string());
                        break;
                    }
                }
                Err(e) => {
                    warn!("Debouncer error: {e}");
                }
            }
        }
    });

    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state.reload_tx))
}

async fn handle_ws(socket: ws::WebSocket, reload_tx: broadcast::Sender<String>) {
    let (mut sender, _receiver) = socket.split();
    let mut rx = reload_tx.subscribe();

    while let Ok(msg) = rx.recv().await {
        if sender
            .send(ws::Message::Text(msg.into()))
            .await
            .is_err()
        {
            break;
        }
    }
}

async fn listing_or_single_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    if let Some(ref single_path) = state.override_path {
        return serve_single_site(
            &state,
            single_path,
            "",
            params.get("page").and_then(|p| p.parse::<usize>().ok()),
        )
        .await;
    }
    listing_handler(&state, params.get("page")).await
}

async fn listing_handler(
    state: &AppState,
    page_param: Option<&String>,
) -> Response {
    let per_page = state.settings.listing_per_page.max(1);
    let page = page_param
        .and_then(|p| p.parse::<usize>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(1);

    let active: Vec<&DirectoryEntry> = state.settings.directories.iter()
        .filter(|d| d.active)
        .collect();

    let total = active.len();
    let total_pages = total.max(1).div_ceil(per_page);
    let current_page = page.min(total_pages.max(1));

    let start = (current_page - 1) * per_page;
    let slice: Vec<&DirectoryEntry> = active.iter()
        .skip(start)
        .take(per_page)
        .copied()
        .collect();

    let mut entries = Vec::new();
    for dir in &slice {
        let site_path = Path::new(&dir.path);
        let (description, last_modified) = get_site_meta(site_path);
        entries.push(ListingEntry {
            name: dir.name.clone(),
            path: dir.path.clone(),
            description,
            last_modified,
            active: dir.active,
        });
    }

    let css_file = state.settings.styles
        .get(&state.settings.listing_css)
        .cloned()
        .unwrap_or_else(|| format!("{}.css", state.settings.listing_css));

    let html = state.templates.render_listing(
        &state.settings.lang,
        &css_file,
        &entries,
        current_page,
        total_pages.max(1),
        total,
    );

    Html(html).into_response()
}

fn get_site_meta(site_path: &Path) -> (String, String) {
    let index_md = site_path.join("index.md");
    let desc_text = if index_md.exists() {
        if let Ok(content) = std::fs::read_to_string(&index_md) {
            let mut fm = Frontmatter::default();
            crate::renderer::extract_frontmatter(&content, &mut fm);
            fm.description.unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let modified = std::fs::metadata(site_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M").to_string()
        })
        .unwrap_or_default();

    (desc_text, modified)
}

async fn serve_single_site(
    state: &AppState,
    site_path: &str,
    sub_path: &str,
    _page_param: Option<usize>,
) -> Response {
    let site_name = Path::new(site_path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Site".to_string());
    let config = PageConfig {
        css: state.override_css.as_deref().unwrap_or("github"),
        js_support: state.override_js.as_ref(),
        lang: state.override_lang.as_deref().unwrap_or(&state.settings.lang),
        site_title: &site_name,
    };
    serve_internal(state, site_path, sub_path, &config).await
}

async fn catch_all_handler(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if let Some(ref single_path) = state.override_path {
        return serve_single_site(&state, single_path, &path, None).await;
    }

    let (site_name, remainder) = path.split_once('/').unwrap_or((&path, ""));

    let dir_entry = state.settings.directories.iter()
        .find(|d| d.name == site_name && d.active)
        .cloned();

    match dir_entry {
        Some(entry) => serve_path_with_config(&state, &entry, remainder).await,
        None => {
            let css_file = state.settings.styles
                .get(&state.settings.listing_css)
                .cloned()
                .unwrap_or_else(|| format!("{}.css", state.settings.listing_css));
            let html = state.templates.render_error(
                404,
                &format!("Site '{}' not found or inactive.", site_name),
                &state.settings.lang,
                &css_file,
            );
            (StatusCode::NOT_FOUND, Html(html)).into_response()
        }
    }
}

async fn serve_path_with_config(
    state: &AppState,
    entry: &DirectoryEntry,
    path: &str,
) -> Response {
    let site_path = &entry.path;
    let css = entry.css.as_deref().unwrap_or("github");
    let js_support = entry.js_support.as_ref();
    let lang = entry.lang.as_deref().unwrap_or(&state.settings.lang);

    let config = PageConfig {
        css,
        js_support,
        lang,
        site_title: &entry.name,
    };

    serve_internal(state, site_path, path, &config).await
}

struct PageConfig<'a> {
    css: &'a str,
    js_support: Option<&'a Vec<String>>,
    lang: &'a str,
    site_title: &'a str,
}

async fn serve_internal(
    state: &AppState,
    site_path: &str,
    path: &str,
    config: &PageConfig<'_>,
) -> Response {
    let base = Path::new(site_path);

    if path.is_empty() {
        for name in &["index.md", "init.md"] {
            let file = base.join(name);
            if file.exists() {
                return render_file(state, &file, config, None).await;
            }
        }
        let css_file = state.settings.styles.get(config.css).cloned()
            .unwrap_or_else(|| format!("{}.css", config.css));
        let html = state.templates.render_error(
            404,
            "No index.md or init.md found in this site.",
            config.lang,
            &css_file,
        );
        return (StatusCode::NOT_FOUND, Html(html)).into_response();
    }

    let md_path = base.join(format!("{}.md", path));
    if md_path.exists() {
        return render_file(state, &md_path, config, Some(path)).await;
    }

    let dir = base.join(path);
    if dir.is_dir() {
        for name in &["index.md", "init.md"] {
            let file = dir.join(name);
            if file.exists() {
                return render_file(state, &file, config, Some(path)).await;
            }
        }
    }

    let static_file = base.join(path);
    if static_file.exists() && static_file.is_file() {
        return serve_static_file(&static_file).await;
    }

    let css_file = state.settings.styles.get(config.css).cloned()
        .unwrap_or_else(|| format!("{}.css", config.css));
    let html = state.templates.render_error(
        404,
        &format!("Page '{}' not found.", path),
        config.lang,
        &css_file,
    );
    (StatusCode::NOT_FOUND, Html(html)).into_response()
}

async fn render_file(
    state: &AppState,
    file_path: &Path,
    config: &PageConfig<'_>,
    base_path: Option<&str>,
) -> Response {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => {
            let css_file = state.settings.styles.get(config.css).cloned()
                .unwrap_or_else(|| format!("{}.css", config.css));
            let html = state.templates.render_error(
                500,
                &format!("Cannot read file: {e}"),
                config.lang,
                &css_file,
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, Html(html)).into_response();
        }
    };

    let mut fm = Frontmatter::default();
    let html_body = render_markdown(&content, &mut fm);

    let title = fm.title.as_deref().map(|s| s.to_string())
        .or_else(|| renderer::extract_first_heading(&content));
    let title = title.as_deref();
    let description = fm.description.as_deref();
    let page_lang = fm.lang.as_deref().unwrap_or(config.lang);

    let base_url = base_path.map(|p| {
        if p.ends_with('/') { p.to_string() } else { format!("{p}/") }
    });

    let js_assets = resolve_js(&state.settings.dependencies, config.js_support);

    let css_file = state.settings.styles
        .get(config.css)
        .cloned()
        .unwrap_or_else(|| format!("{}.css", config.css));

    let html = state.templates.render_page(
        title,
        description,
        &html_body,
        config.site_title,
        page_lang,
        &css_file,
        base_url.as_deref(),
        &js_assets.extra_css,
        &js_assets.head_scripts,
        &js_assets.head_inline,
        &js_assets.body_scripts,
        state.watch_mode,
    );

    Html(html).into_response()
}

struct JsAssets {
    head_scripts: Vec<String>,   // <script src="...">
    head_inline: Vec<String>,    // raw <script>...</script> HTML
    body_scripts: Vec<String>,   // <script src="..."> before </body>
    extra_css: Vec<String>,      // <link rel="stylesheet" href="...">
}

fn resolve_js(
    dep_map: &std::collections::HashMap<String, String>,
    js_support: Option<&Vec<String>>,
) -> JsAssets {
    let mut assets = JsAssets {
        head_scripts: Vec::new(),
        head_inline: Vec::new(),
        body_scripts: Vec::new(),
        extra_css: Vec::new(),
    };

    let js_keys: Vec<&String> = js_support
        .map(|list| list.iter().filter(|k| dep_map.contains_key(*k)).collect())
        .unwrap_or_default();

    for key in &js_keys {
        let url = cdn_or_local(key, &dep_map);

        match key.as_str() {
            "mermaid" => {
                assets.head_inline.push(
                    "<script>document.addEventListener('DOMContentLoaded',function(){typeof mermaid!=='undefined'&&mermaid.initialize({startOnLoad:true});});</script>".to_string()
                );
                assets.body_scripts.push(url);
            }
            "highlight" => {
                assets.head_inline.push(
                    "<script>document.addEventListener('DOMContentLoaded',function(){typeof hljs!=='undefined'&&hljs.highlightAll();});</script>".to_string()
                );
                assets.head_scripts.push(url);
            }
            "mathjax" => {
                assets.head_inline.push(
                    "<script>window.MathJax={tex:{inlineMath:[['$','$'],['\\\\(','\\\\)']],displayMath:[['$$','$$'],['\\\\[','\\\\]']]}};</script>".to_string()
                );
                assets.head_scripts.push(url);
            }
            "katex" => {
                assets.extra_css.push("https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.css".to_string());
                assets.body_scripts.push(url);
            }
            "anchor" => {
                assets.head_inline.push(
                    "<script>document.addEventListener('DOMContentLoaded',function(){typeof anchors!=='undefined'&&anchors.add('.markdown-body h2,.markdown-body h3,.markdown-body h4');});</script>".to_string()
                );
                assets.head_scripts.push(url);
            }
            "chartjs" => {
                assets.body_scripts.push(url);
            }
            "fontawesome" => {
                assets.extra_css.push(url);
            }
            _ => {
                if url.ends_with(".css") {
                    assets.extra_css.push(url);
                } else {
                    assets.head_scripts.push(url);
                }
            }
        }
    }

    assets
}

fn cdn_or_local(key: &str, dep_map: &std::collections::HashMap<String, String>) -> String {
    static KNOWN_CDN: &[(&str, &str)] = &[
        ("mathjax", "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js"),
        ("mermaid", "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js"),
        ("chartjs", "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js"),
        ("highlight", "https://cdn.jsdelivr.net/npm/highlight.js@11/lib/index.js"),
        ("katex", "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js"),
        ("katex_css", "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.css"),
        ("fontawesome", "https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@6/css/all.min.css"),
        ("anchor", "https://cdn.jsdelivr.net/npm/anchor-js@5/anchor.min.js"),
    ];

    // First check if it's a known library — use CDN
    if let Some((_, cdn)) = KNOWN_CDN.iter().find(|(k, _)| *k == key) {
        return cdn.to_string();
    }

    // Otherwise use the value from settings
    if let Some(value) = dep_map.get(key) {
        if value.starts_with("http://") || value.starts_with("https://") {
            return value.clone();
        }
        return format!("/__enginemd/js/{}", value);
    }

    format!("/__enginemd/js/{}.js", key)
}

async fn serve_static_file(path: &Path) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    match tokio::fs::read(path).await {
        Ok(data) => {
            let headers = {
                let mut h = HeaderMap::new();
                h.insert(
                    "Content-Type",
                    mime.to_string().parse().unwrap(),
                );
                h
            };
            (headers, data).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}

async fn js_handler(
    State(_state): State<AppState>,
    AxumPath(file): AxumPath<String>,
) -> Response {
    let js_dir = config::js_dir();
    let file_path = js_dir.join(&file);
    serve_local_file(&file_path).await
}

async fn css_handler(
    State(_state): State<AppState>,
    AxumPath(file): AxumPath<String>,
) -> Response {
    let css_dir = config::css_dir();
    let file_path = css_dir.join(&file);

    if file_path.exists() {
        return serve_local_file(&file_path).await;
    }

    let bundled = crate::template::bundled_css(&file);
    if !bundled.is_empty() {
        let mime = if file.ends_with(".css") {
            "text/css; charset=utf-8"
        } else {
            "application/octet-stream"
        };
        let headers = {
            let mut h = HeaderMap::new();
            h.insert("Content-Type", mime.parse().unwrap());
            h
        };
        return (headers, bundled.to_owned()).into_response();
    }

    (StatusCode::NOT_FOUND, "CSS not found").into_response()
}

async fn serve_local_file(file_path: &Path) -> Response {
    let mime = mime_guess::from_path(file_path).first_or_octet_stream();
    match tokio::fs::read(file_path).await {
        Ok(data) => {
            let headers = {
                let mut h = HeaderMap::new();
                h.insert(
                    "Content-Type",
                    mime.to_string().parse().unwrap(),
                );
                h
            };
            (headers, data).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}

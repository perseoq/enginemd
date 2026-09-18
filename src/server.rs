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
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::config::{self, DirectoryEntry, Settings};
use crate::obsidian::VaultIndex;
use crate::renderer::{self, render_markdown, Frontmatter, RenderContext};
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
    pub vaults: Arc<std::sync::RwLock<HashMap<String, Arc<VaultIndex>>>>,
    pub obsidian_detect: Arc<std::sync::RwLock<HashMap<String, bool>>>,
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
        vaults: Arc::new(std::sync::RwLock::new(HashMap::new())),
        obsidian_detect: Arc::new(std::sync::RwLock::new(HashMap::new())),
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
        let vaults = state.vaults.clone();
        let detect = state.obsidian_detect.clone();
        tokio::spawn(async move {
            if let Err(e) = start_file_watcher(dirs, tx, vaults, detect) {
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
    vaults: Arc<std::sync::RwLock<HashMap<String, Arc<VaultIndex>>>>,
    detect: Arc<std::sync::RwLock<HashMap<String, bool>>>,
) -> Result<(), String> {
    let (event_tx, event_rx) = std::sync::mpsc::channel::<()>();

    let mut watcher = recommended_watcher(move |res: notify::Result<Event>| {
        let event = match res {
            Ok(event) => event,
            Err(e) => {
                warn!("Watch error: {e}");
                return;
            }
        };

        // Ignore read/open events: the server itself opens files on every
        // request, which would otherwise trigger an endless reload loop.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }

        // Ignore bookkeeping directories such as .git/ and .obsidian/.
        if event.paths.iter().all(|p| is_ignored_path(p)) {
            return;
        }

        let _ = event_tx.send(());
    })
    .map_err(|e| format!("watcher: {e}"))?;

    for p in &paths {
        watcher
            .watch(Path::new(p), RecursiveMode::Recursive)
            .map_err(|e| format!("cannot watch {p}: {e}"))?;
    }

    std::thread::spawn(move || {
        // Keep the watcher (and its watches) alive for the lifetime of this thread.
        let _watcher = watcher;

        while event_rx.recv().is_ok() {
            // Trailing debounce: wait for a quiet period before reloading.
            loop {
                match event_rx.recv_timeout(std::time::Duration::from_millis(300)) {
                    Ok(()) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }

            if let Ok(mut v) = vaults.write() {
                v.clear();
            }
            if let Ok(mut d) = detect.write() {
                d.clear();
            }
            let _ = reload_tx.send("reload".to_string());
        }
    });

    Ok(())
}

fn is_ignored_path(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(name) if name == ".git" || name == ".obsidian"
        )
    })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state.reload_tx))
}

async fn handle_ws(socket: ws::WebSocket, reload_tx: broadcast::Sender<String>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = reload_tx.subscribe();

    loop {
        tokio::select! {
            msg = rx.recv() => match msg {
                Ok(msg) => {
                    if sender
                        .send(ws::Message::Text(msg.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            incoming = receiver.next() => match incoming {
                Some(Ok(ws::Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
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

    let mut page_start = current_page.saturating_sub(2).max(1);
    let page_end = (page_start + 4).min(total_pages);
    page_start = page_end.saturating_sub(4).max(1);

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
        page_start,
        page_end,
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
        url_prefix: "/".to_string(),
        obsidian: state.obsidian_enabled(None, site_path),
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
        url_prefix: format!("/{}/", entry.name),
        obsidian: state.obsidian_enabled(entry.obsidian, site_path),
    };

    serve_internal(state, site_path, path, &config).await
}

struct PageConfig<'a> {
    css: &'a str,
    js_support: Option<&'a Vec<String>>,
    lang: &'a str,
    site_title: &'a str,
    url_prefix: String,
    obsidian: bool,
}

impl AppState {
    fn obsidian_enabled(&self, explicit: Option<bool>, site_path: &str) -> bool {
        if let Some(v) = explicit {
            return v;
        }
        if self.settings.obsidian {
            return true;
        }
        if Path::new(site_path).join(".obsidian").is_dir() {
            return true;
        }
        if let Some(v) = self.obsidian_detect.read().unwrap().get(site_path) {
            return *v;
        }
        let detected = site_uses_obsidian_syntax(Path::new(site_path));
        self.obsidian_detect
            .write()
            .unwrap()
            .insert(site_path.to_string(), detected);
        detected
    }
}

fn site_uses_obsidian_syntax(root: &Path) -> bool {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;

    let mut bytes = 0u64;
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
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
                stack.push(path);
            } else if file_type.is_file() && name.to_ascii_lowercase().ends_with(".md") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    bytes += content.len() as u64;
                    if content.contains("![[") || content.contains("[[") {
                        return true;
                    }
                    if bytes >= MAX_BYTES {
                        return false;
                    }
                }
            }
        }
    }

    false
}

fn get_vault(state: &AppState, site_path: &str) -> Arc<VaultIndex> {
    if let Some(v) = state.vaults.read().unwrap().get(site_path) {
        return v.clone();
    }
    let index = Arc::new(VaultIndex::build(Path::new(site_path)));
    state
        .vaults
        .write()
        .unwrap()
        .insert(site_path.to_string(), index.clone());
    index
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
                return render_file(state, site_path, &file, config, None).await;
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

    let render_path = strip_md_ext(path);
    let is_markdown_request = render_path.len() != path.len();

    if let Some(md_path) = safe_join(base, &format!("{render_path}.md")) {
        if md_path.exists() {
            let base_url = page_base_url(render_path, false);
            return render_file(state, site_path, &md_path, config, base_url.as_deref()).await;
        }
    }

    if let Some(dir) = safe_join(base, render_path) {
        if dir.is_dir() {
            for name in &["index.md", "init.md"] {
                let file = dir.join(name);
                if file.exists() {
                    let base_url = page_base_url(render_path, true);
                    return render_file(state, site_path, &file, config, base_url.as_deref()).await;
                }
            }
        }

        if !is_markdown_request && dir.is_file() {
            return serve_static_file(&dir).await;
        }
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

fn page_base_url(path: &str, is_dir: bool) -> Option<String> {
    if is_dir {
        return Some(format!("{}/", path.trim_end_matches('/')));
    }
    // File pages already resolve relative links against their own directory,
    // so no <base> is needed (and a relative one would be wrong).
    None
}

fn strip_md_ext(path: &str) -> &str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".markdown") {
        &path[..path.len() - ".markdown".len()]
    } else if lower.ends_with(".md") {
        &path[..path.len() - ".md".len()]
    } else {
        path
    }
}

fn safe_join(base: &Path, rel: &str) -> Option<std::path::PathBuf> {
    use std::path::Component;

    let mut clean = std::path::PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(c) => clean.push(c),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    let joined = base.join(&clean);
    if let (Ok(base_canon), Ok(joined_canon)) = (base.canonicalize(), joined.canonicalize()) {
        if !joined_canon.starts_with(&base_canon) {
            return None;
        }
    }
    Some(joined)
}

async fn render_file(
    state: &AppState,
    site_path: &str,
    file_path: &Path,
    config: &PageConfig<'_>,
    base_url: Option<&str>,
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

    let current_rel = file_path
        .strip_prefix(site_path)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    let vault = if config.obsidian {
        Some(get_vault(state, site_path))
    } else {
        None
    };

    let render_ctx = vault.as_ref().map(|index| RenderContext {
        site_root: Path::new(site_path),
        url_prefix: &config.url_prefix,
        current_rel: &current_rel,
        index: index.as_ref(),
        obsidian: true,
    });

    let mut fm = Frontmatter::default();
    let html_body = render_markdown(&content, &mut fm, render_ctx.as_ref());

    let title = fm.title.as_deref().map(|s| s.to_string())
        .or_else(|| renderer::extract_first_heading(&content));
    let title = title.as_deref();
    let description = fm.description.as_deref();
    let page_lang = fm.lang.as_deref().unwrap_or(config.lang);

    let js_assets = resolve_js(&state.settings.dependencies, config.js_support, config.css);

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
        base_url,
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
    css_theme: &str,
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
        let url = cdn_or_local(key, dep_map);

        match key.as_str() {
            "mermaid" => {
                assets.head_inline.push(
                    "<script>document.addEventListener('DOMContentLoaded',function(){typeof mermaid!=='undefined'&&mermaid.initialize({startOnLoad:true});});</script>".to_string()
                );
                assets.body_scripts.push(url);
            }
            "highlight" => {
                let hl_theme = if css_theme.contains("dark") { "github-dark" } else { "github" };
                assets.extra_css.push(format!(
                    "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/styles/{hl_theme}.min.css"
                ));
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
                assets.head_scripts.push(url);
                assets.head_scripts.push("https://cdn.jsdelivr.net/npm/katex@0.16/dist/contrib/auto-render.min.js".to_string());
                assets.head_inline.push(
                    "<script>document.addEventListener('DOMContentLoaded',function(){typeof renderMathInElement!=='undefined'&&renderMathInElement(document.body,{delimiters:[{left:'$$',right:'$$',display:true},{left:'\\\\[',right:'\\\\]',display:true},{left:'$',right:'$',display:false},{left:'\\\\(',right:'\\\\)',display:false}]});});</script>".to_string()
                );
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
        ("highlight", "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/highlight.min.js"),
        ("katex", "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js"),
        ("katex_css", "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.css"),
        ("fontawesome", "https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@6/css/all.min.css"),
        ("anchor", "https://cdn.jsdelivr.net/npm/anchor-js@5/anchor.min.js"),
    ];

    let value = dep_map.get(key);

    // Prefer a locally fetched copy when one exists.
    if let Some(v) = value {
        let local = config::dep_local_name(key, v);
        if config::js_dir().join(&local).exists() {
            return format!("/__enginemd/js/{local}");
        }
    }

    // Known library -> canonical CDN URL.
    if let Some((_, cdn)) = KNOWN_CDN.iter().find(|(k, _)| *k == key) {
        return cdn.to_string();
    }

    // Otherwise use the value from settings.
    match value {
        Some(v) if v.starts_with("http://") || v.starts_with("https://") => v.clone(),
        Some(v) => format!("/__enginemd/js/{}", config::dep_local_name(key, v)),
        None => format!("/__enginemd/js/{key}.js"),
    }
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
    match safe_join(&js_dir, &file) {
        Some(file_path) => serve_local_file(&file_path).await,
        None => (StatusCode::BAD_REQUEST, "Invalid path").into_response(),
    }
}

async fn css_handler(
    State(_state): State<AppState>,
    AxumPath(file): AxumPath<String>,
) -> Response {
    // 1. Serve base.css from bundled assets
    if file == "base.css" {
        let bundled = include_str!("../css/base.css");
        return serve_css(bundled);
    }

    // 2. Try file from ~/.enginemd/css/ (user custom themes)
    let css_dir = config::css_dir();
    if let Some(file_path) = safe_join(&css_dir, &file) {
        if file_path.exists() {
            return serve_local_file(&file_path).await;
        }
    }

    // 3. Try generated theme from themes.rs
    let theme_name = file.trim_end_matches(".css");
    if let Some(theme) = crate::themes::find_theme(theme_name) {
        let css = crate::themes::render_theme_css(theme);
        return serve_css(&css);
    }

    // 4. Try bundled CSS (legacy)
    let bundled = crate::template::bundled_css(&file);
    if !bundled.is_empty() {
        return serve_css(bundled);
    }

    (StatusCode::NOT_FOUND, "CSS not found").into_response()
}

fn serve_css(css: &str) -> Response {
    let headers = {
        let mut h = HeaderMap::new();
        h.insert("Content-Type", "text/css; charset=utf-8".parse().unwrap());
        h
    };
    (headers, css.to_owned()).into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_paths() {
        assert!(is_ignored_path(Path::new("/site/.git/index")));
        assert!(is_ignored_path(Path::new("/site/.obsidian/workspace.json")));
        assert!(is_ignored_path(Path::new("a/.git/b/c.md")));
        assert!(!is_ignored_path(Path::new("/site/docs/guide.md")));
        assert!(!is_ignored_path(Path::new("/site/.hidden.md")));
    }

    #[test]
    fn safe_join_rejects_traversal() {
        let base = Path::new("/tmp");
        assert!(safe_join(base, "../etc/passwd").is_none());
        assert!(safe_join(base, "/etc/passwd").is_none());
        assert_eq!(safe_join(base, "docs/a.md"), Some(Path::new("/tmp/docs/a.md").to_path_buf()));
    }

    #[test]
    fn strips_markdown_extension() {
        assert_eq!(strip_md_ext("docs/guia.md"), "docs/guia");
        assert_eq!(strip_md_ext("docs/guia.MD"), "docs/guia");
        assert_eq!(strip_md_ext("docs/guia.markdown"), "docs/guia");
        assert_eq!(strip_md_ext("docs/guia"), "docs/guia");
        assert_eq!(strip_md_ext("docs/a.md.txt"), "docs/a.md.txt");
    }

    #[test]
    fn base_url_only_for_directories() {
        assert_eq!(page_base_url("docs", true), Some("docs/".to_string()));
        assert_eq!(page_base_url("docs/guia", false), None);
    }

    #[test]
    fn detects_obsidian_syntax_by_content() {
        let base = std::env::temp_dir().join(format!("enginemd-obsidian-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        std::fs::write(base.join("plain.md"), "# Plain\n\nSin sintaxis especial.\n").unwrap();
        assert!(!site_uses_obsidian_syntax(&base));

        std::fs::write(
            base.join("note.md"),
            "Texto con una imagen\n\n![[Pasted image 1.png]]\n",
        )
        .unwrap();
        assert!(site_uses_obsidian_syntax(&base));

        let _ = std::fs::remove_dir_all(&base);
    }
}

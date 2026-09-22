use axum::{
    extract::{ws, Path as AxumPath, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use notify::{recommended_watcher, Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::assets::{self, AssetManager, CssAsset, FileKind, Position, ScriptAsset};
use crate::config::{self, DirectoryEntry, Settings};
use crate::obsidian::VaultIndex;
use crate::renderer::{self, render_markdown, Frontmatter, RenderContext};
use crate::search::{self, ContentIndex};
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
    pub vaults: Arc<std::sync::RwLock<HashMap<String, Arc<VaultIndex>>>>,
    pub obsidian_detect: Arc<std::sync::RwLock<HashMap<String, bool>>>,
    pub search_indexes: Arc<std::sync::RwLock<HashMap<String, Arc<ContentIndex>>>>,
    pub assets: Arc<AssetManager>,
    pub assets_base: std::path::PathBuf,
    pub theme_cache: Arc<crate::system_theme::ThemeCache>,
}

pub async fn start_server(
    settings: Settings,
    watch_mode: bool,
    override_path: Option<String>,
    override_port: Option<u16>,
    override_lang: Option<String>,
    override_js: Option<Vec<String>>,
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
        vaults: Arc::new(std::sync::RwLock::new(HashMap::new())),
        obsidian_detect: Arc::new(std::sync::RwLock::new(HashMap::new())),
        search_indexes: Arc::new(std::sync::RwLock::new(HashMap::new())),
        assets: Arc::new(AssetManager::new(
            config::assets_base(&settings),
            settings.cdn_base.clone(),
            settings.cdn_fallbacks.clone(),
        )),
        assets_base: config::assets_base(&settings),
        theme_cache: Arc::new(crate::system_theme::ThemeCache::new(
            std::time::Duration::from_secs(2),
        )),
    };

    if settings.auto_fetch {
        let assets = state.assets.clone();
        tokio::spawn(async move {
            assets.prefetch_all().await;
        });
    }

    let app = build_router(state.clone());

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
            settings
                .directories
                .iter()
                .filter(|d| d.active)
                .map(|d| d.path.clone())
                .collect()
        };
        let tx = reload_tx.clone();
        let vaults = state.vaults.clone();
        let detect = state.obsidian_detect.clone();
        let search = state.search_indexes.clone();
        tokio::spawn(async move {
            if let Err(e) = start_file_watcher(dirs, tx, vaults, detect, search) {
                eprintln!("File watcher error: {e}");
            }
        });
    }

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))?;

    Ok(())
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(listing_or_single_handler))
        .route("/{*path}", get(catch_all_handler))
        .route("/__enginemd/js/{*file}", get(js_handler))
        .route("/__enginemd/css/{*file}", get(css_handler))
        .route("/__enginemd/health", get(health_handler))
        .route("/__enginemd/theme", get(theme_handler))
        .route("/__enginemd/search", get(search_handler))
        .route("/__enginemd/ws", get(ws_handler))
        .layer(CompressionLayer::new())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health_handler(State(state): State<AppState>) -> Response {
    let body = format!("{{\"status\":\"ok\",\"watch\":{}}}", state.watch_mode);
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    (headers, body).into_response()
}

async fn theme_handler(State(state): State<AppState>) -> Response {
    let theme = state
        .theme_cache
        .get()
        .map(|dark| if dark { "dark" } else { "light" });
    let body = match theme {
        Some(t) => format!("{{\"theme\":\"{t}\"}}"),
        None => "{\"theme\":null}".to_string(),
    };
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("Cache-Control", "no-store".parse().unwrap());
    (headers, body).into_response()
}

#[derive(serde::Serialize)]
struct SearchResponse {
    query: String,
    sites: Vec<SearchSite>,
}

#[derive(serde::Serialize)]
struct SearchSite {
    name: String,
    url: String,
    matches: Vec<SearchMatch>,
}

#[derive(serde::Serialize)]
struct SearchMatch {
    title: String,
    page: String,
    url: String,
    snippet: String,
}

const SEARCH_MIN_CHARS: usize = 2;
const SEARCH_PER_SITE: usize = 10;
const SEARCH_TOTAL: usize = 50;

async fn search_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let query = params.get("q").map(|s| s.trim()).unwrap_or("").to_string();

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert("Cache-Control", "no-store".parse().unwrap());

    if query.chars().count() < SEARCH_MIN_CHARS {
        return (
            headers,
            serde_json::json!({ "query": "", "sites": [] }).to_string(),
        )
            .into_response();
    }

    // (name, path, url_prefix, description)
    let mut sites: Vec<(String, String, String, String)> = Vec::new();
    if let Some(ref single) = state.override_path {
        let name = Path::new(single)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Site".to_string());
        let (description, _heading, _modified) = get_site_meta(Path::new(single));
        sites.push((name, single.clone(), "/".to_string(), description));
    } else {
        for dir in state.settings.directories.iter().filter(|d| d.active) {
            let (description, _heading, _modified) = get_site_meta(Path::new(&dir.path));
            sites.push((
                dir.name.clone(),
                dir.path.clone(),
                format!("/{}/", dir.name),
                description,
            ));
        }
    }

    let needle = query.to_lowercase();
    let mut out: Vec<SearchSite> = Vec::new();
    let mut total = 0usize;

    for (name, path, prefix, description) in &sites {
        if total >= SEARCH_TOTAL {
            break;
        }
        let name_hit =
            name.to_lowercase().contains(&needle) || description.to_lowercase().contains(&needle);

        let index = get_search_index(&state, path);
        let limit = SEARCH_PER_SITE.min(SEARCH_TOTAL - total);
        let mut matches: Vec<SearchMatch> = Vec::new();
        if limit > 0 && !index.is_empty() {
            for hit in search::search(&index, &query, limit) {
                matches.push(SearchMatch {
                    url: crate::obsidian::encode_path(&format!("{prefix}{}", hit.page)),
                    title: hit.title,
                    page: hit.page,
                    snippet: hit.snippet,
                });
            }
        }
        total += matches.len();

        if name_hit || !matches.is_empty() {
            out.push(SearchSite {
                name: name.clone(),
                url: prefix.clone(),
                matches,
            });
        }
    }

    let payload = SearchResponse { query, sites: out };
    let body = serde_json::to_string(&payload).unwrap_or_else(|_| "{\"sites\":[]}".to_string());
    (headers, body).into_response()
}

fn get_search_index(state: &AppState, site_path: &str) -> Arc<ContentIndex> {
    if let Some(index) = state.search_indexes.read().unwrap().get(site_path) {
        return index.clone();
    }
    let index = Arc::new(ContentIndex::build(Path::new(site_path)));
    state
        .search_indexes
        .write()
        .unwrap()
        .insert(site_path.to_string(), index.clone());
    index
}

fn start_file_watcher(
    paths: Vec<String>,
    reload_tx: broadcast::Sender<String>,
    vaults: Arc<std::sync::RwLock<HashMap<String, Arc<VaultIndex>>>>,
    detect: Arc<std::sync::RwLock<HashMap<String, bool>>>,
    search: Arc<std::sync::RwLock<HashMap<String, Arc<ContentIndex>>>>,
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
            if let Ok(mut s) = search.write() {
                s.clear();
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

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
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

async fn listing_handler(state: &AppState, page_param: Option<&String>) -> Response {
    let per_page = state.settings.listing_per_page.max(1);
    let page = page_param
        .and_then(|p| p.parse::<usize>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(1);

    let active: Vec<&DirectoryEntry> = state
        .settings
        .directories
        .iter()
        .filter(|d| d.active)
        .collect();

    let total = active.len();
    let total_pages = total.max(1).div_ceil(per_page);
    let current_page = page.min(total_pages.max(1));

    let mut page_start = current_page.saturating_sub(2).max(1);
    let page_end = (page_start + 4).min(total_pages);
    page_start = page_end.saturating_sub(4).max(1);

    let start = (current_page - 1) * per_page;
    let slice: Vec<&DirectoryEntry> = active.iter().skip(start).take(per_page).copied().collect();

    let mut entries = Vec::new();
    for dir in &slice {
        let site_path = Path::new(&dir.path);
        let (description, heading, last_modified) = get_site_meta(site_path);
        entries.push(ListingEntry {
            name: dir.name.clone(),
            path: dir.path.clone(),
            description,
            heading,
            last_modified,
            active: dir.active,
        });
    }

    let css_file = "auto.css".to_string();

    let html = state.templates.render_listing(
        &state.settings.lang,
        &css_file,
        &entries,
        current_page,
        total_pages.max(1),
        total,
        page_start,
        page_end,
        &state.settings.app_title,
        &state.settings.listing_subtitle,
        &state.settings.footer_text,
        &theme_ctx(state),
    );

    Html(html).into_response()
}

fn get_site_meta(site_path: &Path) -> (String, String, String) {
    let mut description = String::new();
    let mut heading = String::new();

    for name in &["index.md", "init.md"] {
        let index_md = site_path.join(name);
        if !index_md.exists() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&index_md) {
            let mut fm = Frontmatter::default();
            crate::renderer::extract_frontmatter(&content, &mut fm);
            description = fm.description.unwrap_or_default();
            heading = crate::renderer::extract_first_heading(&content).unwrap_or_default();
        }
        break;
    }

    let modified = std::fs::metadata(site_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%Y-%m-%d %H:%M").to_string()
        })
        .unwrap_or_default();

    (description, heading, modified)
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
        css: "auto",
        js_support: state.override_js.as_ref(),
        lang: state
            .override_lang
            .as_deref()
            .unwrap_or(&state.settings.lang),
        site_title: &site_name,
        url_prefix: "/".to_string(),
        asset_prefix: "/__enginemd/".to_string(),
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

    let dir_entry = state
        .settings
        .directories
        .iter()
        .find(|d| d.name == site_name && d.active)
        .cloned();

    match dir_entry {
        Some(entry) => serve_path_with_config(&state, &entry, remainder).await,
        None => {
            let html = state.templates.render_error(
                404,
                &format!("Site '{}' not found or inactive.", site_name),
                &state.settings.lang,
                "auto.css",
                &state.settings.app_title,
                &theme_ctx(&state),
            );
            (StatusCode::NOT_FOUND, Html(html)).into_response()
        }
    }
}

async fn serve_path_with_config(state: &AppState, entry: &DirectoryEntry, path: &str) -> Response {
    let site_path = &entry.path;
    let js_support = state.override_js.as_ref().or(entry.js_support.as_ref());
    let lang = entry.lang.as_deref().unwrap_or(&state.settings.lang);

    let config = PageConfig {
        css: "auto",
        js_support,
        lang,
        site_title: &entry.name,
        url_prefix: format!("/{}/", entry.name),
        asset_prefix: "/__enginemd/".to_string(),
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
    asset_prefix: String,
    obsidian: bool,
}

fn theme_ctx(state: &AppState) -> crate::template::ThemeContext {
    let source = state.settings.theme_source.to_lowercase();
    let os = state
        .theme_cache
        .get()
        .map(|dark| if dark { "dark" } else { "light" }.to_string());
    let attr = if source == "system" { os.clone() } else { None };
    crate::template::ThemeContext {
        source,
        default: os,
        attr,
    }
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

pub(crate) fn site_uses_obsidian_syntax(root: &Path) -> bool {
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
        let css_file = state
            .settings
            .styles
            .get(config.css)
            .cloned()
            .unwrap_or_else(|| format!("{}.css", config.css));
        let html = state.templates.render_error(
            404,
            "No index.md or init.md found in this site.",
            config.lang,
            &css_file,
            &state.settings.app_title,
            &theme_ctx(state),
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

    let css_file = state
        .settings
        .styles
        .get(config.css)
        .cloned()
        .unwrap_or_else(|| format!("{}.css", config.css));
    let html = state.templates.render_error(
        404,
        &format!("Page '{}' not found.", path),
        config.lang,
        &css_file,
        &state.settings.app_title,
        &theme_ctx(state),
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
    let content = match tokio::fs::read_to_string(file_path).await {
        Ok(c) => c,
        Err(e) => {
            let css_file = state
                .settings
                .styles
                .get(config.css)
                .cloned()
                .unwrap_or_else(|| format!("{}.css", config.css));
            let html = state.templates.render_error(
                500,
                &format!("Cannot read file: {e}"),
                config.lang,
                &css_file,
                &state.settings.app_title,
                &theme_ctx(state),
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

    let title = fm
        .title
        .as_deref()
        .map(|s| s.to_string())
        .or_else(|| renderer::extract_first_heading(&content));
    let title = title.as_deref();
    let description = fm.description.as_deref();
    let page_lang = fm.lang.as_deref().unwrap_or(config.lang);
    let body_class = fm.cssclasses.join(" ");

    let js_assets = resolve_assets(state, config, &content).await;

    let css_file = state
        .settings
        .styles
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
        &body_class,
        &fm.tags,
        &config.asset_prefix,
        &js_assets.extra_css,
        &js_assets.head_scripts,
        &js_assets.head_inline,
        &js_assets.body_scripts,
        state.watch_mode,
        &state.settings.home_label,
        &theme_ctx(state),
    );

    Html(html).into_response()
}

#[derive(Default)]
struct JsAssets {
    head_scripts: Vec<ScriptAsset>,
    head_inline: Vec<String>,
    body_scripts: Vec<ScriptAsset>,
    extra_css: Vec<CssAsset>,
}

async fn resolve_assets(state: &AppState, config: &PageConfig<'_>, body: &str) -> JsAssets {
    let mut out = JsAssets::default();
    let sri = state.settings.sri;

    let allow: std::collections::HashSet<&str> = config
        .js_support
        .map(|list| list.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    // Detection activates libraries even if they are not explicitly listed.
    let mut needed: std::collections::HashSet<&'static str> =
        assets::detect_assets(body).into_iter().collect();
    for key in &allow {
        match assets::find(key) {
            Some(spec) => {
                needed.insert(spec.key);
            }
            None => warn!("unknown js-support key: {key}"),
        }
    }

    // mathjax and katex are mutually exclusive; explicit selection wins.
    if needed.contains("mathjax") && needed.contains("katex") {
        if allow.contains("katex") && !allow.contains("mathjax") {
            needed.remove("mathjax");
        } else {
            needed.remove("katex");
        }
    }

    for key in needed {
        let spec = match assets::find(key) {
            Some(s) => s,
            None => continue,
        };

        for file in spec.files {
            // Highlight CSS: pick per theme, or both with media in auto mode.
            let mut media: Option<String> = None;
            if spec.key == "highlight" && file.kind == FileKind::Css {
                if config.css == "auto" {
                    media = Some(
                        match file.local {
                            "highlight-github.css" => "(prefers-color-scheme: light)",
                            "highlight-github-dark.css" => "(prefers-color-scheme: dark)",
                            _ => continue,
                        }
                        .to_string(),
                    );
                } else {
                    let wanted = if config.css.contains("dark") {
                        "highlight-github-dark.css"
                    } else {
                        "highlight-github.css"
                    };
                    if file.local != wanted {
                        continue;
                    }
                }
            }

            let hash = match state.assets.ensure(file.url, file.local, file.kind).await {
                Ok(h) => h,
                Err(e) => {
                    warn!("asset '{}' ({}) unavailable: {e}", spec.key, file.local);
                    continue;
                }
            };
            let version = assets::version_token(&hash);
            let integrity = if sri { Some(hash) } else { None };

            match file.kind {
                FileKind::Js => {
                    let script = ScriptAsset {
                        src: format!("{}js/{}?v={}", config.asset_prefix, file.local, version),
                        integrity,
                    };
                    match spec.position {
                        Position::Head => out.head_scripts.push(script),
                        Position::Body => out.body_scripts.push(script),
                    }
                }
                FileKind::Css => out.extra_css.push(CssAsset {
                    href: format!("{}css/{}?v={}", config.asset_prefix, file.local, version),
                    integrity,
                    media,
                }),
            }
        }

        for init in spec.init {
            out.head_inline.push(init.to_string());
        }
    }

    out
}

async fn serve_static_file(path: &Path) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let modified = tokio::fs::metadata(path)
        .await
        .ok()
        .and_then(|m| m.modified().ok());
    match tokio::fs::read(path).await {
        Ok(data) => {
            let mut headers = HeaderMap::new();
            headers.insert("Content-Type", mime.to_string().parse().unwrap());
            headers.insert("Cache-Control", "no-cache".parse().unwrap());
            headers.insert("Content-Length", data.len().to_string().parse().unwrap());
            if let Some(t) = modified {
                let dt: chrono::DateTime<chrono::Utc> = t.into();
                if let Ok(v) = dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string().parse() {
                    headers.insert("Last-Modified", v);
                }
            }
            (headers, data).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "File not found").into_response(),
    }
}

async fn js_handler(
    State(state): State<AppState>,
    AxumPath(file): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let js_dir = state.assets_base.join("js");
    let versioned = params.contains_key("v");
    match safe_join(&js_dir, &file) {
        Some(file_path) => serve_local_file(&file_path, versioned).await,
        None => (StatusCode::BAD_REQUEST, "Invalid path").into_response(),
    }
}

async fn css_handler(
    State(state): State<AppState>,
    AxumPath(file): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let versioned = params.contains_key("v");

    // 1. Serve base.css from bundled assets
    if file == "base.css" {
        let bundled = include_str!("../css/base.css");
        return serve_css(bundled, false);
    }

    // 2. Automatic theme: follows the OS color scheme.
    if file == "auto.css" {
        let css = crate::themes::render_auto_theme_css();
        return serve_css(&css, false);
    }

    // 3. Try file from the assets dir (downloaded or user custom themes)
    let css_dir = state.assets_base.join("css");
    if let Some(file_path) = safe_join(&css_dir, &file) {
        if file_path.exists() {
            return serve_local_file(&file_path, versioned).await;
        }
    }

    // 4. Try generated theme from themes.rs
    let theme_name = file.trim_end_matches(".css");
    if let Some(theme) = crate::themes::find_theme(theme_name) {
        let css = crate::themes::render_theme_css(theme);
        return serve_css(&css, false);
    }

    // 5. Try bundled CSS (legacy)
    let bundled = crate::template::bundled_css(&file);
    if !bundled.is_empty() {
        return serve_css(bundled, false);
    }

    (StatusCode::NOT_FOUND, "CSS not found").into_response()
}

fn cache_header(versioned: bool) -> &'static str {
    if versioned {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn serve_css(css: &str, versioned: bool) -> Response {
    let headers = {
        let mut h = HeaderMap::new();
        h.insert("Content-Type", "text/css; charset=utf-8".parse().unwrap());
        h.insert("Cache-Control", cache_header(versioned).parse().unwrap());
        h
    };
    (headers, css.to_owned()).into_response()
}

async fn serve_local_file(file_path: &Path, versioned: bool) -> Response {
    let mime = mime_guess::from_path(file_path).first_or_octet_stream();
    match tokio::fs::read(file_path).await {
        Ok(data) => {
            let headers = {
                let mut h = HeaderMap::new();
                h.insert("Content-Type", mime.to_string().parse().unwrap());
                h.insert("Cache-Control", cache_header(versioned).parse().unwrap());
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
        assert_eq!(
            safe_join(base, "docs/a.md"),
            Some(Path::new("/tmp/docs/a.md").to_path_buf())
        );
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

    fn test_state() -> AppState {
        let mut settings = config::default_settings();
        let base = std::env::temp_dir().join(format!("enginemd-http-{}", std::process::id()));
        settings.assets_dir = Some(base.to_string_lossy().to_string());
        AppState {
            settings,
            templates: Arc::new(TemplateEngine::new(false)),
            watch_mode: false,
            reload_tx: broadcast::channel(10).0,
            override_path: None,
            override_lang: None,
            override_js: None,
            vaults: Arc::new(std::sync::RwLock::new(HashMap::new())),
            obsidian_detect: Arc::new(std::sync::RwLock::new(HashMap::new())),
            search_indexes: Arc::new(std::sync::RwLock::new(HashMap::new())),
            assets: Arc::new(AssetManager::new(base.clone(), None, Vec::new())),
            assets_base: base,
            theme_cache: Arc::new(crate::system_theme::ThemeCache::new(
                std::time::Duration::from_secs(2),
            )),
        }
    }

    async fn get(app: Router, uri: &str) -> (StatusCode, HeaderMap, String) {
        use axum::body::Body;
        use tower::ServiceExt;
        let request = axum::http::Request::builder()
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, String::from_utf8_lossy(&bytes).to_string())
    }

    #[tokio::test]
    async fn health_endpoint_ok() {
        let (status, _headers, body) = get(build_router(test_state()), "/__enginemd/health").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"status\":\"ok\""));
    }

    #[tokio::test]
    async fn base_css_has_cache_control() {
        let (status, headers, body) =
            get(build_router(test_state()), "/__enginemd/css/base.css").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("cache-control").is_some());
        assert!(body.contains("--body-bg"));
    }

    #[tokio::test]
    async fn auto_css_follows_system_scheme() {
        let (status, _headers, body) =
            get(build_router(test_state()), "/__enginemd/css/auto.css").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("prefers-color-scheme: dark"));
        assert!(body.contains(":root[data-theme=\"dark\"]"));
        assert!(body.contains("#282a36"));
        assert!(body.contains("#ffffff"));
    }

    #[tokio::test]
    async fn theme_endpoint_ok() {
        let (status, headers, body) = get(build_router(test_state()), "/__enginemd/theme").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers.get("cache-control").is_some());
        assert!(body.contains("\"theme\""));
    }

    #[tokio::test]
    async fn page_includes_theme_toggle() {
        let site = std::env::temp_dir().join(format!("enginemd-theme-{}", std::process::id()));
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.md"), "# Hi\n").unwrap();
        let mut state = test_state();
        state.override_path = Some(site.to_string_lossy().to_string());
        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("id=\"theme-toggle\""));
        assert!(body.contains("data-theme-source="));
        assert!(body.contains("color-scheme"));
    }

    #[tokio::test]
    async fn listing_uses_configured_titles() {
        let mut state = test_state();
        state.settings.app_title = "Mi App".to_string();
        state.settings.listing_subtitle = "Sitios".to_string();
        state.settings.footer_text = "Pie personalizado".to_string();
        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Mi App"));
        assert!(body.contains("Sitios"));
        assert!(body.contains("Pie personalizado"));
    }

    #[tokio::test]
    async fn listing_shows_heading_without_folder_name() {
        let root = std::env::temp_dir().join(format!("enginemd-heading-{}", std::process::id()));
        let site = root.join("proyecto-demo");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(
            site.join("index.md"),
            "---\ndescription: Una descripcion\n---\n\n# Titulo Bonito\n\nTexto.\n",
        )
        .unwrap();

        let mut state = test_state();
        state.settings.directories.push(DirectoryEntry {
            name: "proyecto-demo".to_string(),
            path: site.to_string_lossy().to_string(),
            active: true,
            js_support: None,
            lang: None,
            obsidian: None,
        });

        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains(r#"<h1 class="site-name">Titulo Bonito</h1>"#),
            "expected markdown heading as title: {body}"
        );
        assert!(
            !body.contains("site-folder"),
            "folder name must not be shown"
        );
        assert!(
            !body.contains(&site.to_string_lossy().to_string()),
            "full path must not appear in listing"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn listing_without_heading_uses_folder_as_title() {
        let root = std::env::temp_dir().join(format!("enginemd-nohead-{}", std::process::id()));
        let site = root.join("sin-titulo");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.md"), "Solo texto, sin encabezado.\n").unwrap();

        let mut state = test_state();
        state.settings.directories.push(DirectoryEntry {
            name: "sin-titulo".to_string(),
            path: site.to_string_lossy().to_string(),
            active: true,
            js_support: None,
            lang: None,
            obsidian: None,
        });

        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains(r#"<h1 class="site-name">sin-titulo</h1>"#),
            "folder name must be the h1 when there is no heading"
        );
        assert!(
            !body.contains("site-folder"),
            "no folder line without heading"
        );
        assert!(!body.contains(&site.to_string_lossy().to_string()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_endpoint_finds_page_content() {
        let root = std::env::temp_dir().join(format!("enginemd-search-{}", std::process::id()));
        let site = root.join("manual-demo");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.md"), "# Portada\n\nBienvenido.\n").unwrap();
        std::fs::write(
            site.join("guia.md"),
            "# Guia\n\nContiene una palabraunica especial.\n",
        )
        .unwrap();

        let mut state = test_state();
        state.settings.directories.push(DirectoryEntry {
            name: "manual-demo".to_string(),
            path: site.to_string_lossy().to_string(),
            active: true,
            js_support: None,
            lang: None,
            obsidian: None,
        });

        let (status, headers, body) =
            get(build_router(state), "/__enginemd/search?q=palabraunica").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("application/json"))
            .unwrap_or(false));
        assert!(
            body.contains("\"manual-demo\""),
            "site name missing: {body}"
        );
        assert!(body.contains("\"page\":\"guia\""), "page missing: {body}");
        assert!(body.contains("palabraunica"), "snippet missing: {body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn search_endpoint_ignores_short_query() {
        let (status, _headers, body) =
            get(build_router(test_state()), "/__enginemd/search?q=a").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"sites\":[]"), "unexpected body: {body}");
    }

    #[tokio::test]
    async fn page_uses_configured_home_label() {
        let site = std::env::temp_dir().join(format!("enginemd-home-{}", std::process::id()));
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.md"), "# Hi\n").unwrap();
        let mut state = test_state();
        state.settings.home_label = "Inicio".to_string();
        state.override_path = Some(site.to_string_lossy().to_string());
        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Inicio"));
    }

    #[tokio::test]
    async fn unknown_js_support_key_does_not_panic() {
        let mut state = test_state();
        state.override_js = Some(vec!["noexiste".to_string()]);
        let site = std::env::temp_dir().join(format!("enginemd-http-site-{}", std::process::id()));
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.md"), "# Hi\n").unwrap();
        state.override_path = Some(site.to_string_lossy().to_string());
        let (status, _headers, body) = get(build_router(state), "/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<h1"));
    }
}

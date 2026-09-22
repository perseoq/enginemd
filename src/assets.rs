use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use base64::Engine as _;
use regex::Regex;
use sha2::{Digest, Sha384};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Js,
    Css,
}

impl FileKind {
    fn dir_name(self) -> &'static str {
        match self {
            FileKind::Js => "js",
            FileKind::Css => "css",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AssetFile {
    pub url: &'static str,
    pub local: &'static str,
    pub kind: FileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Head,
    Body,
}

#[derive(Debug, Clone, Copy)]
pub struct AssetSpec {
    pub key: &'static str,
    pub files: &'static [AssetFile],
    pub init: &'static [&'static str],
    pub position: Position,
}

const JS: FileKind = FileKind::Js;
const CSS: FileKind = FileKind::Css;

/// Catalog of Markdown-oriented JS/CSS libraries. Everything listed here is
/// downloaded once and served locally; the CDN is never referenced in pages.
pub static CATALOG: &[AssetSpec] = &[
    AssetSpec {
        key: "mathjax",
        files: &[AssetFile {
            url: "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js",
            local: "mathjax.js",
            kind: JS,
        }],
        init: &["<script>window.MathJax={tex:{inlineMath:[['$','$'],['\\\\(','\\\\)']],displayMath:[['$$','$$'],['\\\\[','\\\\]']]}};</script>"],
        position: Position::Head,
    },
    AssetSpec {
        key: "katex",
        files: &[
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js",
                local: "katex.js",
                kind: JS,
            },
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.css",
                local: "katex.css",
                kind: CSS,
            },
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/katex@0.16/dist/contrib/auto-render.min.js",
                local: "katex-auto-render.js",
                kind: JS,
            },
        ],
        init: &["<script>document.addEventListener('DOMContentLoaded',function(){typeof renderMathInElement!=='undefined'&&renderMathInElement(document.body,{delimiters:[{left:'$$',right:'$$',display:true},{left:'\\\\[',right:'\\\\]',display:true},{left:'$',right:'$',display:false},{left:'\\\\(',right:'\\\\)',display:false}]});});</script>"],
        position: Position::Head,
    },
    AssetSpec {
        key: "mermaid",
        files: &[AssetFile {
            url: "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js",
            local: "mermaid.js",
            kind: JS,
        }],
        init: &["<script>document.addEventListener('DOMContentLoaded',function(){typeof mermaid!=='undefined'&&mermaid.initialize({startOnLoad:true});});</script>"],
        position: Position::Body,
    },
    AssetSpec {
        key: "chartjs",
        files: &[AssetFile {
            url: "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js",
            local: "chartjs.js",
            kind: JS,
        }],
        init: &[],
        position: Position::Body,
    },
    AssetSpec {
        key: "highlight",
        files: &[
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/highlight.min.js",
                local: "highlight.js",
                kind: JS,
            },
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/styles/github.min.css",
                local: "highlight-github.css",
                kind: CSS,
            },
            AssetFile {
                url: "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/styles/github-dark.min.css",
                local: "highlight-github-dark.css",
                kind: CSS,
            },
        ],
        init: &["<script>document.addEventListener('DOMContentLoaded',function(){typeof hljs!=='undefined'&&hljs.highlightAll();});</script>"],
        position: Position::Head,
    },
    AssetSpec {
        key: "anchor",
        files: &[AssetFile {
            url: "https://cdn.jsdelivr.net/npm/anchor-js@5/anchor.min.js",
            local: "anchor.js",
            kind: JS,
        }],
        init: &["<script>document.addEventListener('DOMContentLoaded',function(){typeof anchors!=='undefined'&&anchors.add('.markdown-body h2,.markdown-body h3,.markdown-body h4');});</script>"],
        position: Position::Head,
    },
    AssetSpec {
        key: "fontawesome",
        files: &[AssetFile {
            url: "https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@6/css/all.min.css",
            local: "fontawesome.css",
            kind: CSS,
        }],
        init: &[],
        position: Position::Head,
    },
];

pub fn find(key: &str) -> Option<&'static AssetSpec> {
    CATALOG.iter().find(|s| s.key == key)
}

/// Detect which catalog libraries a Markdown body needs.
pub fn detect_assets(body: &str) -> HashSet<&'static str> {
    let mut out = HashSet::new();
    let lower = body.to_ascii_lowercase();

    if lower.contains("```mermaid") || lower.contains("~~~mermaid") {
        out.insert("mermaid");
    }
    if lower.contains("```chart") || lower.contains("~~~chart") {
        out.insert("chartjs");
    }
    if has_math(body) {
        out.insert("mathjax");
        out.insert("katex");
    }

    out
}

fn has_math(body: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\$\$[^$]+\$\$|\$[^$\s][^$\n]*[^\s$]\$|\$[^$\s]\$|\\\(|\\\[").unwrap()
    });
    re.is_match(body)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScriptAsset {
    pub src: String,
    pub integrity: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CssAsset {
    pub href: String,
    pub integrity: Option<String>,
    pub media: Option<String>,
}

pub fn version_token(hash: &str) -> String {
    hash.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .skip("sha384".len())
        .take(16)
        .collect()
}

/// Short content hash used to bust caches for static, in-binary content (CSS).
pub fn content_version(content: &str) -> String {
    let mut hasher = Sha384::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let hash = format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    );
    version_token(&hash)
}

/// Manages the local cache of downloaded assets and their SRI hashes.
pub struct AssetManager {
    js_dir: PathBuf,
    css_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: RwLock<HashMap<String, String>>,
    inflight: tokio::sync::Mutex<HashSet<String>>,
    primary_base: Option<String>,
    fallbacks: Vec<String>,
}

impl AssetManager {
    pub fn new(base: PathBuf, primary_base: Option<String>, fallbacks: Vec<String>) -> Self {
        let js_dir = base.join("js");
        let css_dir = base.join("css");
        let manifest_path = base.join("assets.json");
        let manifest = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        let _ = std::fs::create_dir_all(&js_dir);
        let _ = std::fs::create_dir_all(&css_dir);

        Self {
            js_dir,
            css_dir,
            manifest_path,
            manifest: RwLock::new(manifest),
            inflight: tokio::sync::Mutex::new(HashSet::new()),
            primary_base: primary_base.filter(|b| !b.trim().is_empty()),
            fallbacks: fallbacks
                .into_iter()
                .filter(|b| !b.trim().is_empty())
                .collect(),
        }
    }

    fn candidates(&self, url: &str) -> Vec<String> {
        let mut list = Vec::new();
        match &self.primary_base {
            Some(base) => list.push(with_base(url, base)),
            None => list.push(url.to_string()),
        }
        for base in &self.fallbacks {
            list.push(with_base(url, base));
        }
        list
    }

    fn dir_for(&self, kind: FileKind) -> &Path {
        match kind {
            FileKind::Js => &self.js_dir,
            FileKind::Css => &self.css_dir,
        }
    }

    pub fn exists(&self, kind: FileKind, local: &str) -> bool {
        self.dir_for(kind).join(local).exists()
    }

    pub fn integrity(&self, kind: FileKind, local: &str) -> Option<String> {
        self.manifest
            .read()
            .unwrap()
            .get(&manifest_key(kind, local))
            .cloned()
    }

    /// Ensure the file exists locally, downloading it if needed, and return its
    /// SRI hash (`sha384-...`).
    pub async fn ensure(&self, url: &str, local: &str, kind: FileKind) -> Result<String, String> {
        let path = self.dir_for(kind).join(local);

        if path.exists() {
            if let Some(hash) = self.integrity(kind, local) {
                return Ok(hash);
            }
            let hash = hash_file(&path)?;
            self.store_manifest(kind, local, &hash);
            return Ok(hash);
        }

        let lock_key = manifest_key(kind, local);
        {
            let mut inflight = self.inflight.lock().await;
            if !inflight.insert(lock_key.clone()) {
                drop(inflight);
                return self.wait_for(kind, local, &path).await;
            }
        }

        let result = download(&self.candidates(url), &path).await;
        self.inflight.lock().await.remove(&lock_key);

        match result {
            Ok(()) => {
                let hash = hash_file(&path)?;
                self.store_manifest(kind, local, &hash);
                Ok(hash)
            }
            Err(e) => Err(e),
        }
    }

    async fn wait_for(&self, kind: FileKind, local: &str, path: &Path) -> Result<String, String> {
        for _ in 0..100 {
            if path.exists() {
                if let Some(hash) = self.integrity(kind, local) {
                    return Ok(hash);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!("timed out waiting for {}", path.display()))
    }

    fn store_manifest(&self, kind: FileKind, local: &str, hash: &str) {
        let mut manifest = self.manifest.write().unwrap();
        manifest.insert(manifest_key(kind, local), hash.to_string());
        if let Ok(json) = serde_json::to_string_pretty(&*manifest) {
            let _ = std::fs::write(&self.manifest_path, json);
        }
    }

    /// Download every catalog file that is missing (used by `auto_fetch`).
    pub async fn prefetch_all(&self) {
        for spec in CATALOG {
            for file in spec.files {
                if self.exists(file.kind, file.local) {
                    continue;
                }
                if let Err(e) = self.ensure(file.url, file.local, file.kind).await {
                    eprintln!("[assets] {} ({}): {e}", spec.key, file.local);
                }
            }
        }
    }

    /// Build the asset references for a set of libraries from the local cache.
    /// Assumes the files are already downloaded.
    #[allow(clippy::type_complexity)]
    pub fn asset_list(
        &self,
        needed: &HashSet<&'static str>,
        css_theme: &str,
        prefix: &str,
        sri: bool,
    ) -> (
        Vec<ScriptAsset>,
        Vec<ScriptAsset>,
        Vec<CssAsset>,
        Vec<String>,
    ) {
        let mut head = Vec::new();
        let mut body = Vec::new();
        let mut css = Vec::new();
        let mut inline = Vec::new();

        for key in needed {
            let spec = match find(key) {
                Some(s) => s,
                None => continue,
            };
            for file in spec.files {
                let mut media: Option<String> = None;
                if spec.key == "highlight" && file.kind == FileKind::Css {
                    if css_theme == "auto" {
                        media = Some(
                            match file.local {
                                "highlight-github.css" => "(prefers-color-scheme: light)",
                                "highlight-github-dark.css" => "(prefers-color-scheme: dark)",
                                _ => continue,
                            }
                            .to_string(),
                        );
                    } else {
                        let wanted = if css_theme.contains("dark") {
                            "highlight-github-dark.css"
                        } else {
                            "highlight-github.css"
                        };
                        if file.local != wanted {
                            continue;
                        }
                    }
                }
                let integrity = self.integrity(file.kind, file.local);
                if sri && integrity.is_none() {
                    continue;
                }
                let version = integrity.as_deref().map(version_token).unwrap_or_default();
                let integrity_attr = if sri { integrity } else { None };

                match file.kind {
                    FileKind::Js => {
                        let script = ScriptAsset {
                            src: format!("{prefix}js/{}?v={}", file.local, version),
                            integrity: integrity_attr,
                        };
                        match spec.position {
                            Position::Head => head.push(script),
                            Position::Body => body.push(script),
                        }
                    }
                    FileKind::Css => css.push(CssAsset {
                        href: format!("{prefix}css/{}?v={}", file.local, version),
                        integrity: integrity_attr,
                        media,
                    }),
                }
            }
            for init in spec.init {
                inline.push(init.to_string());
            }
        }

        (head, body, css, inline)
    }
}

fn manifest_key(kind: FileKind, local: &str) -> String {
    format!("{}/{}", kind.dir_name(), local)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read error: {e}"))?;
    let mut hasher = Sha384::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    Ok(format!(
        "sha384-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    ))
}

fn with_base(url: &str, base: &str) -> String {
    const PREFIX: &str = "https://cdn.jsdelivr.net/npm/";
    match url.strip_prefix(PREFIX) {
        Some(rest) => format!("{}/{}", base.trim_end_matches('/'), rest),
        None => url.to_string(),
    }
}

async fn download(urls: &[String], path: &Path) -> Result<(), String> {
    let mut last_err = "no download source".to_string();
    for url in urls {
        match download_one(url, path).await {
            Ok(()) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

async fn download_one(url: &str, path: &Path) -> Result<(), String> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("http client")
    });

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP error for {url}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {} for {url}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("read error: {e}"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create dirs: {e}"))?;
    }

    let tmp = path.with_extension("tmp");
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| format!("write error: {e}"))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| format!("rename error: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mermaid_and_chart() {
        let set = detect_assets("# T\n\n```mermaid\ngraph TD;\n```\n");
        assert!(set.contains("mermaid"));
        assert!(!set.contains("chartjs"));

        let set = detect_assets("```chart\n{\"type\":\"bar\"}\n```\n");
        assert!(set.contains("chartjs"));
    }

    #[test]
    fn detects_math() {
        assert!(detect_assets("Formula $E=mc^2$ aqui").contains("mathjax"));
        assert!(detect_assets("$$\\int x dx$$").contains("katex"));
        assert!(!detect_assets("precio: 5$ y 10$").contains("mathjax"));
    }

    #[test]
    fn catalog_has_unique_keys() {
        let mut keys = HashSet::new();
        for spec in CATALOG {
            assert!(keys.insert(spec.key), "duplicate key {}", spec.key);
        }
    }

    #[test]
    fn rewrites_cdn_base() {
        assert_eq!(
            with_base(
                "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js",
                "https://unpkg.com"
            ),
            "https://unpkg.com/mathjax@3/es5/tex-mml-chtml.js"
        );
        assert_eq!(
            with_base(
                "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js",
                "http://mirror.local/npm/"
            ),
            "http://mirror.local/npm/mermaid@11/dist/mermaid.min.js"
        );
    }
}

#[cfg(test)]
mod plain_tests {
    use super::detect_assets;

    #[test]
    fn plain_currency_is_not_math() {
        let body = "# Plain\n\nSin diagramas ni formulas ni graficos.\n\nPrecio: 5$ y 10$.\n";
        let set = detect_assets(body);
        assert!(set.is_empty(), "unexpected detection: {set:?}");
    }
}

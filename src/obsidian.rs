use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use regex::Regex;

const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'\\')
    .add(b'^')
    .add(b'[')
    .add(b']');

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultKind {
    Note,
    Asset,
}

#[derive(Debug, Clone)]
pub struct VaultFile {
    pub rel: String,
    pub kind: VaultKind,
}

#[derive(Debug, Default)]
pub struct VaultIndex {
    by_name: HashMap<String, Vec<VaultFile>>,
    by_rel: HashMap<String, VaultFile>,
}

impl VaultIndex {
    pub fn build(root: &Path) -> Self {
        let mut index = VaultIndex::default();
        let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

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
                } else if file_type.is_file() {
                    if let Some(vf) = make_vault_file(root, &path) {
                        index.insert(vf);
                    }
                }
            }
        }

        index
    }

    fn insert(&mut self, vf: VaultFile) {
        let rel_lower = vf.rel.to_lowercase();
        let base = rel_lower
            .rsplit('/')
            .next()
            .unwrap_or(&rel_lower)
            .to_string();
        let key = match vf.kind {
            VaultKind::Note => base.trim_end_matches(".md").to_string(),
            VaultKind::Asset => base,
        };
        self.by_rel.insert(rel_lower, vf.clone());
        self.by_name.entry(key).or_default().push(vf);
    }

    pub fn resolve(&self, target: &str) -> Option<VaultFile> {
        let target = target.trim().trim_start_matches("./");
        if target.is_empty() {
            return None;
        }
        let lower = target.to_lowercase();

        if let Some(f) = self.by_rel.get(&lower) {
            return Some(f.clone());
        }
        let with_md = format!("{lower}.md");
        if let Some(f) = self.by_rel.get(&with_md) {
            return Some(f.clone());
        }

        let base = lower.rsplit('/').next().unwrap_or(&lower);
        let key = base.trim_end_matches(".md").to_string();
        self.by_name
            .get(&key)
            .and_then(|c| pick_shortest(c).cloned())
    }
}

fn make_vault_file(root: &Path, path: &Path) -> Option<VaultFile> {
    let rel = path.strip_prefix(root).ok()?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let kind = if rel.to_lowercase().ends_with(".md") {
        VaultKind::Note
    } else {
        VaultKind::Asset
    };
    Some(VaultFile { rel, kind })
}

fn pick_shortest(candidates: &[VaultFile]) -> Option<&VaultFile> {
    candidates.iter().min_by(|a, b| {
        let da = a.rel.matches('/').count();
        let db = b.rel.matches('/').count();
        da.cmp(&db).then_with(|| a.rel.cmp(&b.rel))
    })
}

pub fn url_for(file: &VaultFile, prefix: &str, fragment: Option<&str>) -> String {
    let rel = match file.kind {
        VaultKind::Note => file.rel.trim_end_matches(".md").to_string(),
        VaultKind::Asset => file.rel.clone(),
    };
    let mut url = String::new();
    url.push_str(prefix);
    if !url.ends_with('/') {
        url.push('/');
    }
    url.push_str(&encode_path(&rel));
    if let Some(f) = fragment {
        url.push('#');
        url.push_str(f);
    }
    url
}

fn url_for_unresolved(target: &str, prefix: &str, fragment: Option<&str>) -> String {
    let rel = target.trim().trim_start_matches("./");
    let rel = rel.strip_suffix(".md").unwrap_or(rel);
    let mut url = String::new();
    url.push_str(prefix);
    if !url.ends_with('/') {
        url.push('/');
    }
    url.push_str(&encode_path(rel));
    if let Some(f) = fragment {
        url.push('#');
        url.push_str(f);
    }
    url
}

fn encode_path(rel: &str) -> String {
    rel.split('/')
        .map(|seg| utf8_percent_encode(seg, PATH_ENCODE_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn is_image(rel: &str) -> bool {
    let ext = rel.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "avif" | "ico"
    )
}

pub fn parse_size(label: &str) -> (Option<u32>, Option<u32>) {
    let label = label.trim();
    if label.is_empty() {
        return (None, None);
    }
    if let Some((w, h)) = label.split_once(['x', 'X']) {
        (w.trim().parse().ok(), h.trim().parse().ok())
    } else {
        (label.parse().ok(), None)
    }
}

pub struct WikiTarget {
    pub path: String,
    pub fragment: Option<String>,
    pub is_block: bool,
}

pub fn parse_target(raw: &str) -> WikiTarget {
    let (path, frag) = match raw.split_once('#') {
        Some((p, f)) => (p.trim().to_string(), Some(f.trim().to_string())),
        None => (raw.trim().to_string(), None),
    };
    let (fragment, is_block) = match frag {
        Some(f) if f.starts_with('^') && f.len() > 1 => (Some(f[1..].to_string()), true),
        Some(f) if !f.is_empty() => (Some(f), false),
        _ => (None, false),
    };
    WikiTarget {
        path,
        fragment,
        is_block,
    }
}

pub fn slugify_heading(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c == ' ' {
            out.push('-');
        } else if c == '-' || c == '_' || c.is_alphanumeric() {
            out.push(c);
        }
    }
    out
}

pub struct EmbedContext<'a> {
    pub root: &'a Path,
    pub prefix: &'a str,
    pub index: &'a VaultIndex,
    pub max_depth: usize,
}

pub fn resolve_obsidian(
    html: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    wikilink_re()
        .replace_all(html, |caps: &regex::Captures| {
            let is_embed = &caps[1] == "!";
            let raw_href = decode_href(&caps[2]);
            let label = caps[3].to_string();
            let target = parse_target(&raw_href);

            if is_embed {
                render_embed(&target, &label, ctx, depth, visited)
            } else {
                render_link(&target, &label, ctx)
            }
        })
        .into_owned()
}

pub fn resolve_embeds(
    html: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    let code_re = code_re();
    let mut result = String::with_capacity(html.len());
    let mut last = 0;

    for m in code_re.find_iter(html) {
        result.push_str(&replace_embeds(&html[last..m.start()], ctx, depth, visited));
        result.push_str(m.as_str());
        last = m.end();
    }
    result.push_str(&replace_embeds(&html[last..], ctx, depth, visited));
    result
}

fn replace_embeds(
    segment: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    let standalone = standalone_embed_re()
        .replace_all(segment, |caps: &regex::Captures| {
            render_embed_inner(&caps[1], ctx, depth, visited)
        })
        .into_owned();

    embed_re()
        .replace_all(&standalone, |caps: &regex::Captures| {
            render_embed_inner(&caps[1], ctx, depth, visited)
        })
        .into_owned()
}

fn render_embed_inner(
    inner: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    let (target_str, label) = match inner.split_once('|') {
        Some((t, l)) => (t.trim(), l.trim()),
        None => (inner.trim(), ""),
    };
    let target = parse_target(&decode_href(target_str));
    render_embed(&target, label, ctx, depth, visited)
}

fn render_link(target: &WikiTarget, label: &str, ctx: &EmbedContext<'_>) -> String {
    let display = if label.trim().is_empty() {
        html_escape(&target.path)
    } else {
        label.to_string()
    };
    let fragment = fragment_id(target);

    if target.path.is_empty() {
        let href = fragment
            .as_deref()
            .map(|f| format!("#{f}"))
            .unwrap_or_else(|| "#".to_string());
        return format!(r#"<a class="wikilink" href="{href}">{display}</a>"#);
    }

    match ctx.index.resolve(&target.path) {
        Some(file) => {
            let url = url_for(&file, ctx.prefix, fragment.as_deref());
            format!(r#"<a class="wikilink" href="{url}">{display}</a>"#)
        }
        None => {
            let url = url_for_unresolved(&target.path, ctx.prefix, fragment.as_deref());
            format!(r#"<a class="wikilink wikilink-new" href="{url}">{display}</a>"#)
        }
    }
}

fn render_embed(
    target: &WikiTarget,
    label: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    if target.path.is_empty() {
        return format!(
            r#"<span class="embed embed-missing">{}</span>"#,
            html_escape(&target.path)
        );
    }

    let file = match ctx.index.resolve(&target.path) {
        Some(f) => f,
        None => {
            return format!(
                r#"<a class="wikilink wikilink-new" href="{}">{}</a>"#,
                url_for_unresolved(&target.path, ctx.prefix, None),
                html_escape(&target.path)
            );
        }
    };

    match file.kind {
        VaultKind::Asset if is_image(&file.rel) => {
            let url = url_for(&file, ctx.prefix, None);
            let (w, h) = parse_size(label);
            let mut attrs = String::new();
            if let Some(w) = w {
                attrs.push_str(&format!(r#" width="{w}""#));
            }
            if let Some(h) = h {
                attrs.push_str(&format!(r#" height="{h}""#));
            }
            format!(
                r#"<span class="embed embed-image"><img src="{url}" alt="{}"{attrs} loading="lazy"></span>"#,
                html_escape(&file.rel)
            )
        }
        VaultKind::Asset => {
            let url = url_for(&file, ctx.prefix, None);
            format!(
                r#"<a class="wikilink" href="{url}">{}</a>"#,
                html_escape(&file.rel)
            )
        }
        VaultKind::Note => {
            if depth >= ctx.max_depth {
                return format!(
                    r#"<span class="embed embed-missing">embed depth limit: {}</span>"#,
                    html_escape(&file.rel)
                );
            }
            if visited.contains(&file.rel) {
                return format!(
                    r#"<a class="wikilink" href="{}">{}</a>"#,
                    url_for(&file, ctx.prefix, None),
                    html_escape(&file.rel)
                );
            }

            let path = ctx.root.join(&file.rel);
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => {
                    return format!(
                        r#"<span class="embed embed-missing">cannot read {}</span>"#,
                        html_escape(&file.rel)
                    );
                }
            };

            let body = crate::renderer::body_without_frontmatter(&content);
            let section = match target.fragment.as_deref() {
                Some(frag) if target.is_block => extract_block(body, frag),
                Some(frag) => extract_section(body, frag),
                None => body.to_string(),
            };

            let inner_ctx = EmbedContext {
                root: ctx.root,
                prefix: ctx.prefix,
                index: ctx.index,
                max_depth: ctx.max_depth,
            };

            visited.insert(file.rel.clone());
            let inner = crate::renderer::render_body(&section, &inner_ctx, depth + 1, visited);
            visited.remove(&file.rel);

            format!(r#"<div class="embed embed-note">{inner}</div>"#)
        }
    }
}

fn fragment_id(target: &WikiTarget) -> Option<String> {
    target.fragment.as_deref().map(|f| {
        if target.is_block {
            f.to_string()
        } else {
            slugify_heading(f)
        }
    })
}

pub fn apply_block_ids(html: &str) -> String {
    static RE_P: OnceLock<Regex> = OnceLock::new();
    static RE_LI: OnceLock<Regex> = OnceLock::new();

    let re_p = RE_P.get_or_init(|| Regex::new(r#"\s*\^([A-Za-z0-9-]+)\s*</p>"#).unwrap());
    let re_li = RE_LI.get_or_init(|| Regex::new(r#"\s*\^([A-Za-z0-9-]+)\s*</li>"#).unwrap());

    let html = re_p.replace_all(html, r#" <a id="$1" class="block-ref"></a></p>"#);
    re_li
        .replace_all(&html, r#" <a id="$1" class="block-ref"></a></li>"#)
        .to_string()
}

fn wikilink_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(!?)<a href="([^"]*)" data-wikilink="true">([\s\S]*?)</a>"#).unwrap()
    })
}

fn code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"<pre\b[\s\S]*?</pre>|<code\b[\s\S]*?</code>"#).unwrap())
}

fn embed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"!\[\[([^\]\n]+)\]\]"#).unwrap())
}

fn standalone_embed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"<p>\s*!\[\[([^\]\n]+)\]\]\s*</p>"#).unwrap())
}

fn decode_href(raw: &str) -> String {
    let unescaped = raw
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'");
    percent_decode_str(&unescaped)
        .decode_utf8_lossy()
        .to_string()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn parse_atx_heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    if !t.starts_with('#') {
        return None;
    }
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((hashes, rest.trim().trim_end_matches('#').trim()))
}

fn extract_section(body: &str, heading: &str) -> String {
    let want = heading.trim().to_lowercase();
    let lines: Vec<&str> = body.lines().collect();

    let mut start = None;
    let mut level = 0;
    for (i, line) in lines.iter().enumerate() {
        if let Some((lvl, text)) = parse_atx_heading(line) {
            if text.trim().to_lowercase() == want {
                start = Some(i + 1);
                level = lvl;
                break;
            }
        }
    }

    let start = match start {
        Some(s) => s,
        None => return body.to_string(),
    };

    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if let Some((lvl, _)) = parse_atx_heading(line) {
            if lvl <= level {
                end = i;
                break;
            }
        }
    }

    lines[start..end].join("\n")
}

fn extract_block(body: &str, id: &str) -> String {
    let marker = format!("^{id}");
    for line in body.lines() {
        if line.trim_end().ends_with(&marker) {
            return line
                .trim_end()
                .trim_end_matches(&marker)
                .trim_end()
                .to_string();
        }
    }
    body.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_comrak() {
        assert_eq!(
            slugify_heading("Título de la sección"),
            "título-de-la-sección"
        );
        assert_eq!(slugify_heading("Hello, World!"), "hello-world");
        assert_eq!(slugify_heading("Foo_Bar"), "foo_bar");
    }

    #[test]
    fn parse_targets() {
        let t = parse_target("Nota#Encabezado");
        assert_eq!(t.path, "Nota");
        assert_eq!(t.fragment.as_deref(), Some("Encabezado"));
        assert!(!t.is_block);

        let b = parse_target("Nota#^bloque-1");
        assert_eq!(b.fragment.as_deref(), Some("bloque-1"));
        assert!(b.is_block);

        let s = parse_target("#Solo");
        assert!(s.path.is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("640x480"), (Some(640), Some(480)));
        assert_eq!(parse_size("100"), (Some(100), None));
        assert_eq!(parse_size("not-a-size"), (None, None));
    }

    #[test]
    fn block_ids() {
        let html = "<p>Hola mundo ^abc123</p>";
        assert!(apply_block_ids(html).contains(r#"id="abc123""#));
    }

    #[test]
    fn section_extraction() {
        let body = "# Uno\n\nalpha\n\n## Dos\n\nbeta\n\n# Tres\n\ngamma\n";
        let sec = extract_section(body, "Dos");
        assert!(sec.contains("beta"));
        assert!(!sec.contains("gamma"));
    }
}

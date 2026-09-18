use comrak::{markdown_to_html, ComrakOptions};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::SyntaxSet;

use crate::obsidian::{self, EmbedContext, VaultIndex};

#[derive(Debug, Default, Deserialize, Clone)]
pub struct Frontmatter {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub aliases: Vec<String>,
    #[serde(default, deserialize_with = "string_or_seq")]
    pub cssclasses: Vec<String>,
    #[serde(default)]
    pub draft: bool,
}

/// Accepts either a YAML sequence or a single string (Obsidian allows both).
fn string_or_seq<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let value = Option::<OneOrMany>::deserialize(deserializer)?;
    Ok(match value {
        Some(OneOrMany::One(s)) => s
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|x| x.trim_start_matches('#').to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        Some(OneOrMany::Many(list)) => list,
        None => Vec::new(),
    })
}

fn merge_frontmatter(dst: &mut Frontmatter, src: Frontmatter) {
    if src.title.is_some() {
        dst.title = src.title;
    }
    if src.description.is_some() {
        dst.description = src.description;
    }
    if src.lang.is_some() {
        dst.lang = src.lang;
    }
    if !src.tags.is_empty() {
        dst.tags = src.tags;
    }
    if !src.aliases.is_empty() {
        dst.aliases = src.aliases;
    }
    if !src.cssclasses.is_empty() {
        dst.cssclasses = src.cssclasses;
    }
    if src.draft {
        dst.draft = true;
    }
}

pub fn extract_frontmatter(input: &str, fm: &mut Frontmatter) {
    let (_body, parsed) = parse_frontmatter(input);
    if let Some(f) = parsed {
        merge_frontmatter(fm, f);
    }
}

pub struct RenderContext<'a> {
    pub site_root: &'a Path,
    pub url_prefix: &'a str,
    pub current_rel: &'a str,
    pub index: &'a VaultIndex,
    pub obsidian: bool,
}

pub fn render_markdown(
    input: &str,
    frontmatter: &mut Frontmatter,
    ctx: Option<&RenderContext<'_>>,
) -> String {
    let (body, fm) = parse_frontmatter(input);
    if let Some(f) = fm {
        merge_frontmatter(frontmatter, f);
    }

    let html = match ctx {
        Some(c) if c.obsidian => {
            let embed_ctx = EmbedContext {
                root: c.site_root,
                prefix: c.url_prefix,
                index: c.index,
                max_depth: 5,
            };
            let mut visited = HashSet::new();
            if !c.current_rel.is_empty() {
                visited.insert(c.current_rel.to_string());
            }
            render_body(body, &embed_ctx, 0, &mut visited)
        }
        _ => apply_callouts(&markdown_to_html(body, &comrak_options(false))),
    };

    highlight_code_blocks(&html)
}

pub fn render_body(
    markdown: &str,
    ctx: &EmbedContext<'_>,
    depth: usize,
    visited: &mut HashSet<String>,
) -> String {
    let html = markdown_to_html(markdown, &comrak_options(true));
    let html = apply_callouts(&html);
    let html = obsidian::apply_block_ids(&html);
    let html = obsidian::resolve_embeds(&html, ctx, depth, visited);
    obsidian::resolve_obsidian(&html, ctx, depth, visited)
}

fn apply_callouts(html: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"<blockquote>\s*<p>\[!([A-Za-z]+)\][+-]?\s*([^\n<]*)"#).unwrap()
    });
    re.replace_all(html, |caps: &regex::Captures| {
        format!(
            r#"<blockquote class="callout callout-{}"><p class="callout-title">{}</p><p class="callout-body">"#,
            caps[1].to_lowercase(),
            caps[2].trim_end()
        )
    })
    .into_owned()
}

pub fn body_without_frontmatter(input: &str) -> &str {
    parse_frontmatter(input).0
}

fn comrak_options(obsidian: bool) -> ComrakOptions<'static> {
    let ext = comrak::ExtensionOptions {
        strikethrough: true,
        table: true,
        autolink: true,
        tasklist: true,
        header_ids: Some(String::new()),
        footnotes: true,
        wikilinks_title_after_pipe: obsidian,
        ..Default::default()
    };

    let parse = comrak::ParseOptions {
        smart: true,
        default_info_string: Some(String::new()),
        ..Default::default()
    };

    let render = comrak::RenderOptions {
        github_pre_lang: true,
        full_info_string: true,
        ..Default::default()
    };

    ComrakOptions {
        extension: ext,
        parse,
        render,
    }
}

pub fn extract_first_heading(input: &str) -> Option<String> {
    let body = {
        let trimmed = input.trim();
        if let Some(rest) = trimmed.strip_prefix("---") {
            let end = rest.find("\n---").map(|i| i + 3);
            match end {
                Some(pos) => trimmed[pos + 4..].trim(),
                None => trimmed,
            }
        } else {
            trimmed
        }
    };

    for line in body.lines() {
        if let Some(rest) = line.trim().strip_prefix("# ") {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn parse_frontmatter(input: &str) -> (&str, Option<Frontmatter>) {
    let input = input.trim();
    if !input.starts_with("---") {
        return (input, None);
    }

    let end = input[3..].find("\n---").map(|i| i + 3);
    match end {
        Some(pos) => {
            let yaml_str = &input[3..=pos];
            let body = input[pos + 4..].trim();
            match serde_yaml::from_str(yaml_str) {
                Ok(fm) => (body, Some(fm)),
                Err(_) => (input, None),
            }
        }
        None => (input, None),
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SS: std::sync::OnceLock<SyntaxSet> = std::sync::OnceLock::new();
    SS.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static TS: std::sync::OnceLock<ThemeSet> = std::sync::OnceLock::new();
    TS.get_or_init(ThemeSet::load_defaults)
}

fn code_block_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"<pre(?: lang="([^"]*)")?><code(?: class="language-([^"]*)")?>([\s\S]*?)</code></pre>"#,
        )
        .unwrap()
    })
}

fn highlight_code_blocks(html: &str) -> String {
    let ss = syntax_set();
    let ts = theme_set();
    let theme = &ts.themes["base16-ocean.light"];
    let re = code_block_re();

    let mut result = String::with_capacity(html.len() + 4096);
    let mut last_end = 0;
    let mut chart_idx = 0;

    for cap in re.captures_iter(html) {
        let m = cap.get(0).unwrap();
        result.push_str(&html[last_end..m.start()]);

        let lang = cap
            .get(1)
            .and_then(|m| {
                let s = m.as_str();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .or_else(|| {
                cap.get(2).and_then(|m| {
                    let s = m.as_str();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                })
            })
            .unwrap_or("");
        let code = cap.get(3).unwrap().as_str();
        let decoded = decode_html_entities(code);

        if lang == "chart" || lang == "chartjs" {
            if let Some(chart_html) = render_chart_block(&decoded, &mut chart_idx) {
                result.push_str(&chart_html);
            } else {
                result.push_str(m.as_str());
            }
        } else {
            match highlight(&decoded, lang, ss, theme) {
                Ok(highlighted) => {
                    result.push_str("<pre><code class=\"language-");
                    result.push_str(lang);
                    result.push_str("\">");
                    result.push_str(&highlighted);
                    result.push_str("</code></pre>");
                }
                Err(_) => {
                    result.push_str(m.as_str());
                }
            }
        }

        last_end = m.end();
    }

    result.push_str(&html[last_end..]);
    result
}

fn render_chart_block(json_str: &str, idx: &mut i32) -> Option<String> {
    let data: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let _chart_type = data.get("type").and_then(|v| v.as_str()).unwrap_or("bar");
    let _labels = data.get("data").and_then(|d| d.get("labels"));
    let _datasets = data.get("data").and_then(|d| d.get("datasets"));
    let _options = data.get("options");

    let id = format!("enginemd-chart-{}", idx);
    *idx += 1;

    let mut html = String::new();
    html.push_str(&format!(
        r#"<div class="chart-container" style="max-width:100%;margin:1rem 0"><canvas id="{}"></canvas></div>"#,
        id
    ));

    html.push_str(r#"<script>document.addEventListener('DOMContentLoaded',function(){"#);
    html.push_str(&format!(
        r#"var ctx=document.getElementById('{}');if(!ctx)return;new Chart(ctx,{});"#,
        id,
        json_str.replace("</", "<\\/"),
    ));
    html.push_str(r#"});</script>"#);

    Some(html)
}

fn highlight(
    code: &str,
    lang: &str,
    ss: &SyntaxSet,
    _theme: &syntect::highlighting::Theme,
) -> Result<String, ()> {
    let syntax = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut gen = ClassedHTMLGenerator::new_with_class_style(syntax, ss, ClassStyle::Spaced);

    // syntect requires each line to include its trailing newline; normalize so
    // the last line does too, and pass chunks from split_inclusive verbatim.
    let normalized;
    let code = if code.ends_with('\n') {
        code
    } else {
        normalized = format!("{code}\n");
        normalized.as_str()
    };

    for line in code.split_inclusive('\n') {
        gen.parse_html_for_line_which_includes_newline(line)
            .map_err(|_| ())?;
    }

    let hl = gen.finalize();
    Ok(hl)
}

fn decode_html_entities(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            let mut entity = String::new();
            for ch in chars.by_ref() {
                if ch == ';' {
                    break;
                }
                entity.push(ch);
            }
            let decoded = match entity.as_str() {
                "lt" => "<",
                "gt" => ">",
                "amp" => "&",
                "quot" => "\"",
                "#39" => "'",
                _ => {
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
                    continue;
                }
            };
            result.push_str(decoded);
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_blocks_preserve_newlines_and_indent() {
        let md = "```rust\nfn main() {\n    let x = 1;\n}\n```\n";
        let mut fm = Frontmatter::default();
        let html = render_markdown(md, &mut fm, None);

        let pre = html
            .split_once("<pre>")
            .and_then(|(_, rest)| rest.split_once("</pre>"))
            .map(|(inner, _)| inner)
            .expect("expected a <pre> block");

        let text = strip_tags(pre);
        assert!(
            text.contains('\n'),
            "code block lost its newlines: {text:?}"
        );
        assert!(
            text.contains("    let x = 1;"),
            "indentation lost: {text:?}"
        );
    }

    fn strip_tags(input: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        for c in input.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod fm_tests {
    use super::*;

    #[test]
    fn frontmatter_tags_and_classes() {
        let input = "---\ntitle: T\ntags: [a, b]\ncssclasses: c\n---\n\n# H\n";
        let mut fm = Frontmatter::default();
        let _ = render_markdown(input, &mut fm, None);
        assert_eq!(fm.title.as_deref(), Some("T"));
        assert_eq!(fm.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(fm.cssclasses, vec!["c".to_string()]);
    }

    #[test]
    fn frontmatter_string_lists() {
        let input = "---\ntags: rust, docs\ncssclasses: wide\n---\n\n# H\n";
        let mut fm = Frontmatter::default();
        let _ = render_markdown(input, &mut fm, None);
        assert_eq!(fm.tags, vec!["rust".to_string(), "docs".to_string()]);
        assert_eq!(fm.cssclasses, vec!["wide".to_string()]);
    }
}

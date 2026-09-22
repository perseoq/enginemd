use std::path::{Path, PathBuf};

use regex::RegexBuilder;

use crate::renderer;

/// Upper bound for files included in the content index (larger files are skipped).
const MAX_FILE_BYTES: u64 = 512 * 1024;

pub struct ContentEntry {
    pub rel: String,
    pub title: String,
    pub text: String,
}

#[derive(Default)]
pub struct ContentIndex {
    entries: Vec<ContentEntry>,
}

impl ContentIndex {
    /// Walk a site directory and index the text of every Markdown file.
    pub fn build(root: &Path) -> Self {
        let mut index = ContentIndex::default();
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
                    if let Some(entry) = make_entry(root, &path) {
                        index.entries.push(entry);
                    }
                }
            }
        }

        index
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn make_entry(root: &Path, path: &Path) -> Option<ContentEntry> {
    let name = path.file_name()?.to_string_lossy().to_string();
    if !is_markdown(&name) {
        return None;
    }
    if std::fs::metadata(path)
        .map(|m| m.len() > MAX_FILE_BYTES)
        .unwrap_or(false)
    {
        return None;
    }

    let content = std::fs::read_to_string(path).ok()?;
    let rel = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");

    let mut fm = renderer::Frontmatter::default();
    renderer::extract_frontmatter(&content, &mut fm);
    let title = fm
        .title
        .or_else(|| renderer::extract_first_heading(&content))
        .unwrap_or_else(|| page_path(&rel));

    let text = renderer::body_without_frontmatter(&content).to_string();
    Some(ContentEntry { rel, title, text })
}

pub struct Hit {
    pub page: String,
    pub title: String,
    pub snippet: String,
}

/// Case-insensitive substring search over the indexed Markdown text.
pub fn search(index: &ContentIndex, query: &str, limit: usize) -> Vec<Hit> {
    let re = match RegexBuilder::new(&regex::escape(query))
        .case_insensitive(true)
        .build()
    {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };

    let mut hits = Vec::new();
    for entry in &index.entries {
        if hits.len() >= limit {
            break;
        }
        if let Some(m) = re.find(&entry.text) {
            hits.push(Hit {
                page: page_path(&entry.rel),
                title: entry.title.clone(),
                snippet: snippet(&entry.text, m.start(), m.end()),
            });
        }
    }
    hits
}

/// Relative path without its Markdown extension, used for URLs.
fn page_path(rel: &str) -> String {
    let lower = rel.to_ascii_lowercase();
    if lower.ends_with(".markdown") {
        rel[..rel.len() - ".markdown".len()].to_string()
    } else if lower.ends_with(".md") {
        rel[..rel.len() - ".md".len()].to_string()
    } else {
        rel.to_string()
    }
}

fn snippet(text: &str, start: usize, end: usize) -> String {
    const PAD: usize = 80;
    let mut from = floor_char_boundary(text, start.saturating_sub(PAD));
    let mut to = ceil_char_boundary(text, (end + PAD).min(text.len()));

    // Snap to word boundaries so the excerpt does not begin or end mid-word.
    if from > 0 {
        while from > 0 {
            let prev = match text[..from].chars().next_back() {
                Some(c) => c,
                None => break,
            };
            if prev.is_whitespace() {
                break;
            }
            from -= prev.len_utf8();
        }
    }
    if to < text.len() {
        while to < text.len() {
            let next = match text[to..].chars().next() {
                Some(c) => c,
                None => break,
            };
            if next.is_whitespace() {
                break;
            }
            to += next.len_utf8();
        }
    }

    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.push_str(
        &text[from..to]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    );
    if to < text.len() {
        out.push('…');
    }
    out
}

fn floor_char_boundary(text: &str, mut i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(text: &str, mut i: usize) -> usize {
    if i >= text.len() {
        return text.len();
    }
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("enginemd-searchunit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_content_case_insensitive() {
        let root = temp_root("hit");
        std::fs::write(root.join("a.md"), "# Uno\n\nHola MUNDO cruel.\n").unwrap();

        let index = ContentIndex::build(&root);
        let hits = search(&index, "mundo", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, "a");
        assert!(hits[0].snippet.contains("MUNDO"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ignores_hidden_and_non_markdown() {
        let root = temp_root("ign");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git").join("x.md"), "secreto\n").unwrap();
        std::fs::write(root.join("data.txt"), "secreto\n").unwrap();
        std::fs::write(root.join("ok.md"), "secreto\n").unwrap();

        let index = ContentIndex::build(&root);
        let hits = search(&index, "secreto", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page, "ok");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_match_returns_empty() {
        let root = temp_root("none");
        std::fs::write(root.join("a.md"), "nada relevante\n").unwrap();

        let index = ContentIndex::build(&root);
        assert!(search(&index, "zzzz", 10).is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn snippet_snaps_to_word_boundaries() {
        let root = temp_root("snippet");
        let filler = "documentacion ".repeat(8);
        std::fs::write(root.join("a.md"), format!("{filler}zarzamora final\n")).unwrap();

        let index = ContentIndex::build(&root);
        let hits = search(&index, "zarzamora", 10);
        assert!(hits[0].snippet.contains("zarzamora"));
        assert!(
            hits[0].snippet.starts_with("…documentacion"),
            "excerpt cut mid-word: {}",
            hits[0].snippet
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn strips_extension_for_page_url() {
        assert_eq!(page_path("docs/guia.md"), "docs/guia");
        assert_eq!(page_path("docs/guia.MD"), "docs/guia");
        assert_eq!(page_path("docs/guia.markdown"), "docs/guia");
        assert_eq!(page_path("docs/guia"), "docs/guia");
    }
}

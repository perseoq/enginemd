use std::collections::HashMap;
use tera::{Context, Tera};

pub struct TemplateEngine {
    tera: Tera,
}

impl TemplateEngine {
    pub fn new(_watch_mode: bool) -> Self {
        let mut tera = Tera::default();

        tera.add_raw_template("page", include_str!("../templates/page.html"))
            .expect("page template");
        tera.add_raw_template("listing", include_str!("../templates/listing.html"))
            .expect("listing template");
        tera.add_raw_template("error", include_str!("../templates/error.html"))
            .expect("error template");

        tera.register_filter("range", range_filter);

        Self { tera }
    }

    pub fn render_listing(
        &self,
        lang: &str,
        listing_css: &str,
        directories: &[ListingEntry],
        current_page: usize,
        total_pages: usize,
        total: usize,
        page_start: usize,
        page_end: usize,
    ) -> String {
        let mut ctx = Context::new();
        ctx.insert("lang", lang);
        ctx.insert("listing_css", listing_css);
        ctx.insert("directories", directories);
        ctx.insert("current_page", &current_page);
        ctx.insert("total_pages", &total_pages);
        ctx.insert("total", &total);
        ctx.insert("page_start", &page_start);
        ctx.insert("page_end", &page_end);

        self.tera.render("listing", &ctx).unwrap_or_else(|e| {
            format!("<h1>Template error</h1><p>{e}</p>")
        })
    }

    pub fn render_page(
        &self,
        title: Option<&str>,
        description: Option<&str>,
        content: &str,
        site_title: &str,
        lang: &str,
        css_theme: &str,
        base_url: Option<&str>,
        extra_css: &[String],
        head_scripts: &[String],
        head_inline: &[String],
        body_scripts: &[String],
        watch_mode: bool,
    ) -> String {
        let mut ctx = Context::new();
        ctx.insert("title", &title);
        ctx.insert("description", &description);
        ctx.insert("content", content);
        ctx.insert("site_title", site_title);
        ctx.insert("lang", lang);
        ctx.insert("css_theme", css_theme);
        ctx.insert("base_url", &base_url);
        ctx.insert("extra_css", extra_css);
        ctx.insert("head_scripts", head_scripts);
        ctx.insert("head_inline", head_inline);
        ctx.insert("body_scripts", body_scripts);
        ctx.insert("watch_mode", &watch_mode);

        self.tera.render("page", &ctx).unwrap_or_else(|e| {
            format!("<h1>Template error</h1><p>{e}</p>")
        })
    }

    pub fn render_error(
        &self,
        status: u16,
        message: &str,
        lang: &str,
        css_theme: &str,
    ) -> String {
        let mut ctx = Context::new();
        ctx.insert("status", &status);
        ctx.insert("message", message);
        ctx.insert("lang", lang);
        ctx.insert("css_theme", css_theme);

        self.tera.render("error", &ctx).unwrap_or_else(|e| {
            format!("<h1>{status}</h1><p>{message}</p><p>Template error: {e}</p>")
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ListingEntry {
    pub name: String,
    pub path: String,
    pub description: String,
    pub last_modified: String,
    pub active: bool,
}

fn range_filter(
    _value: &tera::Value,
    args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let start = args.get("start").and_then(|v| v.as_i64()).unwrap_or(1);
    let end = args.get("end").and_then(|v| v.as_i64()).unwrap_or(0);
    let values: Vec<i64> = (start..end).collect();
    Ok(tera::Value::Array(values.into_iter().map(|v| tera::Value::Number(v.into())).collect()))
}

pub fn bundled_css(name: &str) -> &'static str {
    match name {
        "github.css" => include_str!("../css/github.css"),
        "dark.css" => include_str!("../css/dark.css"),
        "simple.css" => include_str!("../css/simple.css"),
        _ => "",
    }
}

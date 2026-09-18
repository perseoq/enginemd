use std::path::Path;

pub async fn cmd_fetch() -> Result<(), String> {
    let settings = crate::config::load_settings();
    let js_dir = crate::config::js_dir();
    let css_dir = crate::config::css_dir();

    std::fs::create_dir_all(&js_dir)
        .map_err(|e| format!("cannot create js dir: {e}"))?;
    std::fs::create_dir_all(&css_dir)
        .map_err(|e| format!("cannot create css dir: {e}"))?;

    for (name, value) in &settings.dependencies {
        let url = resolve_url(value);
        if url.is_empty() {
            continue;
        }

        let local_name = crate::config::dep_local_name(name, value);
        let target = js_dir.join(&local_name);

        if target.exists() {
            println!("  [skip] {name} (already cached)");
            continue;
        }

        println!("  [fetch] {name}...");
        match download_to(&url, &target).await {
            Ok(_) => println!("  [done]  {name}"),
            Err(e) => eprintln!("  [warn]  {name}: {e}"),
        }
    }

    for (name, file) in &settings.styles {
        let target = css_dir.join(file);
        if target.exists() {
            println!("  [skip] style '{name}' (already cached)");
            continue;
        }

        let bundled = crate::template::bundled_css(file);
        if !bundled.is_empty() {
            std::fs::create_dir_all(target.parent().unwrap())
                .map_err(|e| format!("cannot create css subdir: {e}"))?;
            std::fs::write(&target, bundled)
                .map_err(|e| format!("cannot write bundled css: {e}"))?;
            println!("  [done]  {name} (from bundle)");
            continue;
        }

        // Generated at request time from themes.rs; nothing to download.
        if crate::themes::find_theme(name).is_some() {
            println!("  [skip] style '{name}' (generated at runtime)");
            continue;
        }

        let url = resolve_url(file);
        if url.is_empty() {
            continue;
        }
        println!("  [fetch] style '{name}'...");
        match download_to(&url, &target).await {
            Ok(_) => println!("  [done]  {name}"),
            Err(e) => eprintln!("  [warn]  {name}: {e}"),
        }
    }

    println!("Fetch complete.");
    Ok(())
}

fn resolve_url(value: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("https://cdn.jsdelivr.net/npm/{}", value)
    }
}

async fn download_to(url: &str, target: &Path) -> Result<(), String> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| format!("HTTP error for {url}: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {} for {url}", response.status()));
    }

    let bytes = response.bytes()
        .await
        .map_err(|e| format!("read error: {e}"))?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create dirs: {e}"))?;
    }
    std::fs::write(target, &bytes)
        .map_err(|e| format!("write error: {e}"))?;

    Ok(())
}

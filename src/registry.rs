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
        let (url, local_rel) = resolve_fetch_path(value);
        let target = js_dir.join(&local_rel);

        if target.exists() {
            println!("  [skip] {name} (already cached)");
            continue;
        }

        if url.is_empty() {
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
        println!("  [fetch] style '{name}'...");

        let bundled = crate::template::bundled_css(file);
        if !bundled.is_empty() {
            std::fs::create_dir_all(target.parent().unwrap())
                .map_err(|e| format!("cannot create css subdir: {e}"))?;
            std::fs::write(&target, bundled)
                .map_err(|e| format!("cannot write bundled css: {e}"))?;
            println!("  [done]  {name} (from bundle)");
            continue;
        }

        let (url, _) = resolve_fetch_path(file);
        if url.is_empty() {
            continue;
        }
        match download_to(&url, &target).await {
            Ok(_) => println!("  [done]  {name}"),
            Err(e) => eprintln!("  [warn]  {name}: {e}"),
        }
    }

    println!("Fetch complete.");
    Ok(())
}

fn resolve_fetch_path(value: &str) -> (String, String) {
    if value.starts_with("http://") || value.starts_with("https://") {
        let local = value
            .split("://")
            .nth(1)
            .and_then(|s| s.split('/').skip(1).collect::<Vec<_>>().join("/").into())
            .unwrap_or_else(|| value.to_string());
        (value.to_string(), local)
    } else {
        let url = format!("https://cdn.jsdelivr.net/npm/{}", value);
        (url, value.to_string())
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

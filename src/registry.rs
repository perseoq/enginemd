use crate::assets::{AssetManager, CATALOG};

pub async fn cmd_fetch(force: bool) -> Result<(), String> {
    let manager = AssetManager::new(crate::config::enginemd_dir());
    let settings = crate::config::load_settings();
    let css_dir = crate::config::css_dir();

    std::fs::create_dir_all(&css_dir).map_err(|e| format!("cannot create css dir: {e}"))?;

    for spec in CATALOG {
        for file in spec.files {
            if !force && manager.exists(file.kind, file.local) {
                println!("  [skip] {} ({})", spec.key, file.local);
                continue;
            }
            println!("  [fetch] {} ({})...", spec.key, file.local);
            match manager.ensure(file.url, file.local, file.kind).await {
                Ok(_) => println!("  [done]  {}", file.local),
                Err(e) => eprintln!("  [warn]  {}: {e}", file.local),
            }
        }
    }

    for (name, file) in &settings.styles {
        let target = css_dir.join(file);
        if target.exists() && !force {
            println!("  [skip] style '{name}' (already cached)");
            continue;
        }

        let bundled = crate::template::bundled_css(file);
        if !bundled.is_empty() {
            std::fs::write(&target, bundled)
                .map_err(|e| format!("cannot write bundled css: {e}"))?;
            println!("  [done]  style '{name}' (from bundle)");
            continue;
        }

        // Generated at request time from themes.rs; nothing to download.
        if crate::themes::find_theme(name).is_some() {
            println!("  [skip] style '{name}' (generated at runtime)");
            continue;
        }

        println!("  [skip] style '{name}' (no source)");
    }

    println!("Fetch complete.");
    Ok(())
}

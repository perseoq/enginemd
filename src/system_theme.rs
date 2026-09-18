use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Detect the desktop color scheme of the machine running the server.
/// Returns `Some(true)` for dark, `Some(false)` for light, `None` if unknown
/// (e.g. Windows/macOS/headless, or no session).
pub fn detect() -> Option<bool> {
    from_gsettings()
        .or_else(from_portal)
        .or_else(from_gtk_settings_ini)
        .or_else(from_env)
}

pub fn parse_color_scheme(output: &str) -> Option<bool> {
    let value = output.trim().trim_matches('\'').to_lowercase();
    match value.as_str() {
        "prefer-dark" | "dark" => Some(true),
        "prefer-light" | "default" | "light" => Some(false),
        _ => None,
    }
}

fn theme_name_dark(output: &str) -> Option<bool> {
    let value = output.trim().trim_matches('\'').to_lowercase();
    if value.contains("dark") {
        Some(true)
    } else {
        None
    }
}

fn from_gsettings() -> Option<bool> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        if let Some(v) = parse_color_scheme(&stdout) {
            return Some(v);
        }
    }

    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
        .ok()?;
    if out.status.success() {
        return theme_name_dark(&String::from_utf8_lossy(&out.stdout));
    }
    None
}

fn from_portal() -> Option<bool> {
    let out = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.Read",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if stdout.contains("uint32 1") {
        Some(true)
    } else if stdout.contains("uint32 2") {
        Some(false)
    } else {
        None
    }
}

fn from_gtk_settings_ini() -> Option<bool> {
    let home = std::env::var("HOME").ok().map(PathBuf::from)?;
    for rel in [
        ".config/gtk-3.0/settings.ini",
        ".config/gtk-4.0/settings.ini",
    ] {
        let path = home.join(rel);
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                if let Some(rest) = line
                    .trim()
                    .strip_prefix("gtk-application-prefer-dark-theme")
                {
                    let value = rest.trim_start_matches(['=', ' ', '\t']).trim();
                    if value == "1" || value.eq_ignore_ascii_case("true") {
                        return Some(true);
                    }
                    if value == "0" || value.eq_ignore_ascii_case("false") {
                        return Some(false);
                    }
                }
            }
        }
    }
    None
}

fn from_env() -> Option<bool> {
    if let Ok(theme) = std::env::var("GTK_THEME") {
        if theme.to_lowercase().contains("dark") {
            return Some(true);
        }
    }
    None
}

/// Caches the detected theme for a short TTL so OS changes are picked up
/// without spawning `gsettings` on every request.
pub struct ThemeCache {
    inner: Mutex<Option<(Instant, Option<bool>)>>,
    ttl: Duration,
}

impl ThemeCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(None),
            ttl,
        }
    }

    pub fn get(&self) -> Option<bool> {
        let mut guard = self.inner.lock().unwrap();
        if let Some((at, value)) = *guard {
            if at.elapsed() < self.ttl {
                return value;
            }
        }
        let value = detect();
        *guard = Some((Instant::now(), value));
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_color_scheme() {
        assert_eq!(parse_color_scheme("'prefer-dark'\n"), Some(true));
        assert_eq!(parse_color_scheme("'default'\n"), Some(false));
        assert_eq!(parse_color_scheme("'prefer-light'\n"), Some(false));
        assert_eq!(parse_color_scheme("weird"), None);
    }

    #[test]
    fn parses_gtk_theme() {
        assert_eq!(theme_name_dark("'Adwaita-dark'\n"), Some(true));
        assert_eq!(theme_name_dark("'Adwaita'\n"), None);
    }
}

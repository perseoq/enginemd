use crate::cli::{Cli, DaemonAction};

const AUTOSTART_MARKER: &str = "# enginemd-daemon";

#[cfg(unix)]
pub fn handle(action: &DaemonAction, cli: &Cli) -> Result<(), String> {
    imp::handle(action, cli)
}

#[cfg(not(unix))]
pub fn handle(_action: &DaemonAction, _cli: &Cli) -> Result<(), String> {
    Err("daemon mode is only supported on Unix systems".to_string())
}

fn managed_block(exe: &str) -> String {
    format!("\n{AUTOSTART_MARKER}\n@reboot {exe} daemon start >/dev/null 2>&1\n")
}

fn strip_managed(text: &str) -> String {
    let mut out = String::new();
    let mut skip_next = false;

    for line in text.lines() {
        if skip_next {
            skip_next = false;
            if line.contains("enginemd") && line.contains("daemon start") {
                continue;
            }
        }
        if line.trim() == AUTOSTART_MARKER {
            skip_next = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    out
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    pub fn handle(action: &DaemonAction, cli: &Cli) -> Result<(), String> {
        match action {
            DaemonAction::Start => start(cli),
            DaemonAction::Stop => stop(),
            DaemonAction::Restart => {
                let _ = stop();
                start(cli)
            }
            DaemonAction::Status => status(),
            DaemonAction::Logs => logs(),
        }
    }

    fn logs() -> Result<(), String> {
        let path = crate::config::log_path();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        println!("# {}", path.display());
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(80);
        for line in &lines[start..] {
            println!("{line}");
        }
        Ok(())
    }

    fn start(cli: &Cli) -> Result<(), String> {
        let settings = crate::config::load_settings();
        let port = cli.port.unwrap_or(settings.port);

        if let Some(pid) = read_pid() {
            if process_alive(pid) && is_enginemd(pid) {
                println!("EngineMD daemon already running (pid {pid}).");
                ensure_autostart();
                return Ok(());
            }
            let _ = std::fs::remove_file(crate::config::pid_path());
        }

        let exe = std::env::current_exe().map_err(|e| format!("cannot resolve executable: {e}"))?;

        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(crate::config::log_path())
            .map_err(|e| format!("cannot open log file: {e}"))?;

        let mut cmd = Command::new(&exe);
        cmd.args(forward_args(cli));
        cmd.stdin(Stdio::null());
        cmd.stdout(log.try_clone().map_err(|e| format!("log error: {e}"))?);
        cmd.stderr(log);

        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = cmd
            .spawn()
            .map_err(|e| format!("cannot start daemon: {e}"))?;
        let pid = child.id();

        std::fs::write(crate::config::pid_path(), pid.to_string())
            .map_err(|e| format!("cannot write pid file: {e}"))?;

        if wait_ready(pid, port, Duration::from_secs(5)) {
            println!("EngineMD daemon started (pid {pid}) on port {port}.");
        } else {
            eprintln!(
                "Warning: daemon did not become ready; check {}",
                crate::config::log_path().display()
            );
        }

        ensure_autostart();
        Ok(())
    }

    fn stop() -> Result<(), String> {
        let removed = match remove_autostart() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Warning: could not disable autostart: {e}");
                false
            }
        };

        match read_pid() {
            Some(pid) if process_alive(pid) && is_enginemd(pid) => {
                terminate(pid)?;
                let _ = std::fs::remove_file(crate::config::pid_path());
                println!("EngineMD daemon stopped (pid {pid}).");
            }
            Some(_) => {
                let _ = std::fs::remove_file(crate::config::pid_path());
                println!("EngineMD daemon was not running (removed stale pid file).");
            }
            None => println!("EngineMD daemon is not running."),
        }

        if removed {
            println!("Autostart disabled.");
        }
        Ok(())
    }

    fn status() -> Result<(), String> {
        match read_pid() {
            Some(pid) if process_alive(pid) && is_enginemd(pid) => {
                println!("EngineMD daemon: running (pid {pid})");
            }
            Some(pid) => println!("EngineMD daemon: stopped (stale pid {pid})"),
            None => println!("EngineMD daemon: stopped"),
        }

        match autostart_installed() {
            Ok(true) => println!("Autostart: enabled (@reboot)"),
            Ok(false) => println!("Autostart: disabled"),
            Err(e) => println!("Autostart: unknown ({e})"),
        }
        Ok(())
    }

    fn forward_args(cli: &Cli) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(p) = cli.port {
            args.push("--port".to_string());
            args.push(p.to_string());
        }
        if let Some(p) = &cli.path {
            args.push("--path".to_string());
            args.push(p.clone());
        }
        if let Some(l) = &cli.lang {
            args.push("--lang".to_string());
            args.push(l.clone());
        }
        if let Some(j) = &cli.js_support {
            args.push("--js-support".to_string());
            args.push(j.clone());
        }
        args
    }

    fn wait_ready(pid: u32, port: u16, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !process_alive(pid) {
                return false;
            }
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn read_pid() -> Option<u32> {
        std::fs::read_to_string(crate::config::pid_path())
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    fn process_alive(pid: u32) -> bool {
        unsafe {
            if libc::kill(pid as i32, 0) == 0 {
                return true;
            }
            std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }

    fn is_enginemd(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            match std::fs::read(format!("/proc/{pid}/cmdline")) {
                Ok(cmdline) => String::from_utf8_lossy(&cmdline).contains("enginemd"),
                Err(_) => false,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
            true
        }
    }

    fn terminate(pid: u32) -> Result<(), String> {
        unsafe {
            if libc::kill(pid as i32, libc::SIGTERM) == -1 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("cannot terminate pid {pid}: {err}"));
                }
            }
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !process_alive(pid) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
        Ok(())
    }

    fn ensure_autostart() {
        match install_autostart() {
            Ok(true) => println!("Autostart enabled (@reboot)."),
            Ok(false) => {}
            Err(e) => eprintln!("Warning: could not enable autostart: {e}"),
        }
    }

    fn install_autostart() -> Result<bool, String> {
        let exe = std::env::current_exe().map_err(|e| format!("cannot resolve executable: {e}"))?;
        let exe = exe.to_string_lossy().to_string();

        let current = read_crontab()?;
        let already = current.lines().any(|l| l.trim() == AUTOSTART_MARKER);
        let cleaned = strip_managed(&current);

        let block = managed_block(&exe);
        let new = if cleaned.trim().is_empty() {
            block.trim_start().to_string()
        } else {
            format!("{}\n{}", cleaned.trim_end(), block)
        };

        if !already || strip_managed(&current) != cleaned {
            write_crontab(&new)?;
        }

        Ok(!already)
    }

    fn remove_autostart() -> Result<bool, String> {
        let current = read_crontab()?;
        if !current.lines().any(|l| l.trim() == AUTOSTART_MARKER) {
            return Ok(false);
        }
        let cleaned = strip_managed(&current);
        write_crontab(&cleaned)?;
        Ok(true)
    }

    fn autostart_installed() -> Result<bool, String> {
        let current = read_crontab()?;
        Ok(current.lines().any(|l| l.trim() == AUTOSTART_MARKER))
    }

    fn read_crontab() -> Result<String, String> {
        match Command::new("crontab").arg("-l").output() {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                if err.contains("no crontab") {
                    Ok(String::new())
                } else {
                    Err(format!("crontab -l failed: {}", err.trim()))
                }
            }
            Err(e) => Err(format!("cannot run crontab: {e}")),
        }
    }

    fn write_crontab(text: &str) -> Result<(), String> {
        let mut child = Command::new("crontab")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run crontab: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("cannot write crontab: {e}"))?;
        }

        let out = child
            .wait_with_output()
            .map_err(|e| format!("crontab error: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "crontab failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crontab_block_roundtrip() {
        let existing = "0 0 * * * /usr/bin/foo\n";
        let block = managed_block("/usr/bin/enginemd");
        let combined = format!("{}\n{}", existing.trim_end(), block);

        assert!(combined.contains(AUTOSTART_MARKER));
        assert!(combined.contains("/usr/bin/enginemd daemon start"));

        let cleaned = strip_managed(&combined);
        assert!(!cleaned.contains(AUTOSTART_MARKER));
        assert!(cleaned.contains("/usr/bin/foo"));
    }

    #[test]
    fn strip_managed_keeps_unrelated_lines() {
        let text = "# enginemd-daemon\n@reboot /x/enginemd daemon start\n0 0 * * * /usr/bin/bar\n";
        let cleaned = strip_managed(text);
        assert_eq!(cleaned, "0 0 * * * /usr/bin/bar\n");
    }
}

// Copyright 2026 Huy Nguyen Nhu
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::ServiceAction;
use crate::config;

pub fn handle_service(action: ServiceAction) -> anyhow::Result<()> {
    match action {
        ServiceAction::Install { port } => install(port),
        ServiceAction::Uninstall => uninstall(),
        ServiceAction::Status => status(),
    }
}

fn check_system_support() -> anyhow::Result<()> {
    if !cfg!(target_os = "linux") {
        if cfg!(target_os = "macos") {
            eprintln!("cram service is currently supported on Linux (systemd) only.\n");
            eprintln!("On macOS, you can set up a launchd user agent manually:");
            eprintln!("  1. Create ~/Library/LaunchAgents/ink.cram.plist");
            eprintln!("  2. Load it with: launchctl load ~/Library/LaunchAgents/ink.cram.plist\n");
            eprintln!("Or run `cram serve` in a terminal multiplexer (like tmux).");
        } else if cfg!(target_os = "windows") {
            eprintln!("cram service is currently supported on Linux (systemd) only.\n");
            eprintln!(
                "On Windows, you can set up cram as a startup task using Task Scheduler or NSSM."
            );
            eprintln!("Or run `cram serve` in a background terminal session.");
        } else {
            eprintln!("cram service is currently supported on Linux (systemd) only.");
            eprintln!(
                "Consider running `cram serve` in a background session or terminal multiplexer."
            );
        }
        std::process::exit(1);
    }

    let status = Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .output();

    match status {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let state = stdout.trim();
            if out.status.success()
                || matches!(state, "running" | "degraded" | "initializing" | "starting")
            {
                Ok(())
            } else {
                eprintln!("cram: systemctl not found or not a systemd system.");
                eprintln!("  Consider running `cram serve` in a terminal multiplexer (such as tmux or screen) instead.");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("cram: systemctl not found or not a systemd system.");
            eprintln!("  Consider running `cram serve` in a terminal multiplexer (such as tmux or screen) instead.");
            std::process::exit(1);
        }
    }
}

fn user_service_path() -> anyhow::Result<PathBuf> {
    let config_dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            PathBuf::from(xdg)
        } else {
            get_home_config_dir()?
        }
    } else {
        get_home_config_dir()?
    };
    Ok(config_dir.join("systemd/user/cram.service"))
}

fn get_home_config_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("could not determine home directory ($HOME is not set)"))?;
    Ok(PathBuf::from(home).join(".config"))
}

fn format_user_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Ok(strip) = path.strip_prefix(&home) {
            return format!("~/{}", strip.display());
        }
    }
    path.display().to_string()
}

pub fn generate_unit_file(exe_path: &Path, port: Option<u16>) -> String {
    let exec_start = match port {
        Some(p) => format!(
            "{} serve --port {} --quiet --no-open",
            exe_path.display(),
            p
        ),
        None => format!("{} serve --quiet --no-open", exe_path.display()),
    };

    format!(
        r#"[Unit]
Description=cram — local AI gateway
Documentation=https://cram.ink
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec_start}
Restart=always
RestartSec=2
StartLimitBurst=5
StartLimitIntervalSec=60

NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6
MemoryMax=256M

[Install]
WantedBy=default.target
"#
    )
}

fn install(port: Option<u16>) -> anyhow::Result<()> {
    check_system_support()?;

    let service_path = user_service_path()?;
    if service_path.exists() {
        eprintln!(
            "cram: unit file already exists at {}",
            format_user_path(&service_path)
        );
        eprintln!("  Run `cram service uninstall` first to remove it before installing again.");
        std::process::exit(1);
    }

    let home = config::cram_home();
    let config_exists = home.join("config.toml").exists();
    let creds_exists = home.join("credentials.toml").exists();
    if !config_exists || !creds_exists {
        eprintln!("Warning: systemd user services do not inherit shell environment variables.");
        eprintln!(
            "  Configuration or credentials file missing in {}.",
            format_user_path(&home)
        );
        eprintln!("  Run `cram auth vertex` or set up config before running the service.\n");
    }

    let current_exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("failed to get current executable path: {e}"))?;
    let canonical_exe = current_exe.canonicalize().unwrap_or(current_exe);

    let unit_content = generate_unit_file(&canonical_exe, port);

    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&service_path, unit_content)?;

    let reload_res = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    if let Err(e) = reload_res {
        eprintln!("Failed to execute `systemctl --user daemon-reload`: {e}");
        std::process::exit(1);
    }

    let enable_res = Command::new("systemctl")
        .args(["--user", "enable", "--now", "cram"])
        .status();

    if let Err(e) = enable_res {
        eprintln!("Failed to execute `systemctl --user enable --now cram`: {e}");
        std::process::exit(1);
    }

    let display_path = format_user_path(&service_path);
    println!("  installed  {display_path}");
    println!("  status     systemctl --user status cram");
    println!("  logs       journalctl --user -u cram -f");
    println!("  remove     cram service uninstall");
    println!();
    println!("  Note: the service stops when you log out and starts again when you log back");
    println!("  in. To keep it running while logged out: loginctl enable-linger $USER");

    Ok(())
}

fn uninstall() -> anyhow::Result<()> {
    check_system_support()?;

    let service_path = user_service_path()?;

    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "cram"])
        .status();

    if service_path.exists() {
        std::fs::remove_file(&service_path)?;
    }

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    println!("  removed    {}", format_user_path(&service_path));
    Ok(())
}

fn status() -> anyhow::Result<()> {
    check_system_support()?;

    let status = Command::new("systemctl")
        .args(["--user", "status", "cram"])
        .status()?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_unit_file_without_port() {
        let exe = Path::new("/home/user/.local/bin/cram");
        let content = generate_unit_file(exe, None);
        assert!(content.contains("ExecStart=/home/user/.local/bin/cram serve --quiet --no-open"));
        assert!(content.contains("[Unit]"));
        assert!(content.contains("Description=cram — local AI gateway"));
        assert!(content.contains("Documentation=https://cram.ink"));
        assert!(content.contains("After=network-online.target"));
        assert!(content.contains("Wants=network-online.target"));
        assert!(content.contains("[Service]"));
        assert!(content.contains("Type=simple"));
        assert!(content.contains("Restart=always"));
        assert!(content.contains("RestartSec=2"));
        assert!(content.contains("StartLimitBurst=5"));
        assert!(content.contains("StartLimitIntervalSec=60"));
        assert!(content.contains("NoNewPrivileges=true"));
        assert!(content.contains("PrivateTmp=true"));
        assert!(content.contains("ProtectSystem=strict"));
        assert!(content.contains("ProtectKernelTunables=true"));
        assert!(content.contains("ProtectControlGroups=true"));
        assert!(content.contains("RestrictAddressFamilies=AF_INET AF_INET6"));
        assert!(content.contains("MemoryMax=256M"));
        assert!(content.contains("[Install]"));
        assert!(content.contains("WantedBy=default.target"));
    }

    #[test]
    fn test_generate_unit_file_with_port() {
        let exe = Path::new("/home/user/.cargo/bin/cram");
        let content = generate_unit_file(exe, Some(9000));
        assert!(content
            .contains("ExecStart=/home/user/.cargo/bin/cram serve --port 9000 --quiet --no-open"));
    }

    #[test]
    fn test_format_user_path() {
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            let path = PathBuf::from(&home).join(".config/systemd/user/cram.service");
            assert_eq!(
                format_user_path(&path),
                "~/.config/systemd/user/cram.service"
            );
        }
    }
}

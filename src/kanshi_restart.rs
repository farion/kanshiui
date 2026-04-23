use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::kanshi_config::kanshi_config_path;

use anyhow::{anyhow, Context, Result};

pub fn restart_kanshi() -> Result<()> {
    if restart_with_systemd("kanshi.service").is_ok() {
        return Ok(());
    }

    let _ = ensure_kanshi_user_service();

    if restart_with_systemd("kanshi").is_ok() {
        return Ok(());
    }
    if restart_with_systemd("kanshi.service").is_ok() {
        return Ok(());
    }
    restart_fallback()
}

fn restart_with_systemd(unit: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "restart", unit])
        .status()
        .with_context(|| format!("failed to run systemctl for unit {unit}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("systemctl restart failed for unit {unit}"))
    }
}

fn restart_fallback() -> Result<()> {
    let _ = Command::new("pkill")
        .args(["-TERM", "-x", "kanshi"])
        .status();
    // If we can, pass the UI-managed config path to kanshi so it uses the
    // dedicated kanshiui file.
    let mut cmd = Command::new("kanshi");
    if let Ok(cfg) = kanshi_config_path() {
        cmd.arg("--config").arg(cfg);
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to spawn kanshi fallback process")?;
    let _pid = child.id();
    Ok(())
}

pub fn ensure_kanshi_user_service() -> Result<()> {
    let home = dirs::home_dir().context("unable to determine home directory")?;
    let user_systemd_dir = home.join(".config").join("systemd").join("user");
    fs::create_dir_all(&user_systemd_dir).with_context(|| {
        format!(
            "failed to create user systemd directory {}",
            user_systemd_dir.display()
        )
    })?;

    let service_path: PathBuf = user_systemd_dir.join("kanshi.service");

    // Desired unit content pointing to the kanshiui file.
    let cfg_path = kanshi_config_path().unwrap_or_else(|_| home.join(".config").join("kanshiui"));
    // Use ExecStart with an unquoted absolute path argument; systemd will
    // parse this correctly. We avoid single quotes which can appear verbatim
    // in the unit file and not match expectations.
    let desired_unit = format!(
        "[Unit]\nDescription=Kanshi output profile daemon (managed by KanshiUI)\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart=kanshi --config {}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        cfg_path.display()
    );

    // Back up existing unit and overwrite unconditionally so the service
    // will use the kanshiui configuration file.
    if service_path.exists() {
        let ts = format!(
            "{}.bak.{}",
            service_path.display(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        );
        let _ = fs::copy(&service_path, ts);
    }
    fs::write(&service_path, desired_unit).with_context(|| {
        format!(
            "failed to write systemd service file {}",
            service_path.display()
        )
    })?;

    // Reload and restart the user service so it will pick up our unit.
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "enable", "--now", "kanshi.service"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "restart", "kanshi.service"])
        .status();
    Ok(())
}

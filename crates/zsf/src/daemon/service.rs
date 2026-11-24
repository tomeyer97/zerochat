//! Starting and stopping the daemon: in its own session, or through the systemd user unit
//! `zsf@<feature>.service` when it is installed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::store::Feature;

/// Starts `zsf daemon <feature>` in its own session (or through the systemd user unit).
pub fn spawn(feature: &Feature, state_root: &Path, use_systemd: bool) -> Result<String> {
    spawn_with(feature, state_root, use_systemd, &std::env::current_exe()?)
}

/// [`spawn`] with the `zsf` executable given (for another program, such as zerochat, that is
/// not `zsf` itself).
pub fn spawn_with(
    feature: &Feature,
    state_root: &Path,
    use_systemd: bool,
    exe: &Path,
) -> Result<String> {
    if let Some(pid) = feature.daemon_pid() {
        return Ok(format!("daemon already running (pid {pid})"));
    }
    if use_systemd && unit_path().is_file() {
        let unit = unit_name(feature);
        systemctl("enable", &unit)?;
        return Ok(format!(
            "daemon started by systemd ({unit}, enabled: it starts again after a reboot)"
        ));
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(feature.daemon_log())?;
    let mut command = Command::new(exe);
    command
        .arg("--state-dir")
        .arg(state_root)
        .arg("daemon")
        .arg(feature.name())
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    crate::sys::detach(&mut command);
    let child = command.spawn().context("cannot start the daemon")?;
    let pid = child.id();
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(format!(
        "daemon started (pid {pid}); it stops with the shell's machine, \
         see `zsf install-service`"
    ))
}

/// Stops the daemon (SIGTERM, or the systemd unit). Runs it started keep going.
pub fn stop(feature: &Feature) -> Result<String> {
    if unit_path().is_file() {
        // May fail without harm: this feature's daemon may run without the unit (started with
        // systemd off), or the user's systemd may not be reachable. The pid file below stops
        // the daemon either way.
        let _ = systemctl("disable", &unit_name(feature));
    }
    match feature.daemon_pid() {
        Some(pid) => {
            // SAFETY: plain kill of a pid we read from our own pid file.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            Ok(format!(
                "daemon (pid {pid}) stopping; running Zeroshot runs continue"
            ))
        }
        None => Ok("daemon not running".to_owned()),
    }
}

/// The feature's instance of the unit: `zsf@<feature>.service`.
fn unit_name(feature: &Feature) -> String {
    format!("zsf@{}.service", feature.name())
}

/// `systemctl --user <action> --now <unit>`, failing with systemctl's message.
fn systemctl(action: &str, unit: &str) -> Result<()> {
    let out = Command::new("systemctl")
        .args(["--user", action, "--now", unit])
        .output()
        .context("cannot run systemctl")?;
    if !out.status.success() {
        let message = String::from_utf8_lossy(&out.stderr);
        bail!("systemctl --user {action} --now {unit}: {}", message.trim());
    }
    Ok(())
}

/// `$XDG_CONFIG_HOME/systemd/user/zsf@.service` (default `~/.config/...`).
pub fn unit_path() -> PathBuf {
    let config = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::store::home().join(".config"));
    config.join("systemd/user/zsf@.service")
}

/// The unit file: `zsf daemon %i` for the feature named by the instance.
pub fn unit_text(exe: &Path, state_root: &Path) -> String {
    format!(
        "[Unit]\n\
         Description=zsf feature driver for %i\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={} --state-dir {} daemon %i\n\
         Restart=on-failure\n\
         RestartSec=30\n\
         # The Zeroshot runs the daemon starts must outlive it: stop only the daemon itself.\n\
         KillMode=process\n\
         TimeoutStopSec=15min\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe.display(),
        state_root.display()
    )
}

/// Writes the unit (does not enable anything).
pub fn install_service(state_root: &Path) -> Result<PathBuf> {
    let path = unit_path();
    let exe = std::env::current_exe()?;
    crate::store::write_atomic(&path, unit_text(&exe, state_root).as_bytes())?;
    Ok(path)
}

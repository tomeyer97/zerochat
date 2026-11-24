//! Small OS facts and actions: process liveness, boot time, free disk, detached spawning,
//! stopping processes, copying a directory.
//!
//! Linux reads `/proc`; macOS and Windows ask the OS through `sysinfo` ([`portable`]), which
//! gives the same answers (the Linux tests check both agree). Signals and `setsid` are Unix only;
//! Windows kills and starts a detached process group instead.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::Result;

pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    pid_exists(pid)
}

#[cfg(unix)]
fn pid_exists(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_exists(pid: u32) -> bool {
    portable::pid_exists(pid)
}

/// Unix seconds of the last boot (Linux `/proc/stat` `btime`; elsewhere the OS's boot time).
#[cfg(target_os = "linux")]
pub fn boot_time_secs() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("btime "))
        .and_then(|v| v.trim().parse().ok())
}

#[cfg(not(target_os = "linux"))]
pub fn boot_time_secs() -> Option<u64> {
    portable::boot_time_secs()
}

/// Free space in GB on the file system holding `path` (or its nearest existing ancestor).
pub fn free_gb(path: &Path) -> f64 {
    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return f64::INFINITY,
        }
    }
    free_gb_of(probe)
}

#[cfg(not(unix))]
fn free_gb_of(probe: &Path) -> f64 {
    portable::free_gb(probe)
}

#[cfg(unix)]
fn free_gb_of(probe: &Path) -> f64 {
    let Ok(c_path) = std::ffi::CString::new(probe.as_os_str().as_encoded_bytes()) else {
        return f64::INFINITY;
    };
    // SAFETY: statvfs fills the zeroed struct for a valid C string.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 {
        return f64::INFINITY;
    }
    (stat.f_bavail as f64) * (stat.f_frsize as f64) / 1e9
}

/// Runs the child in its own session, so stopping zsf (or its terminal) does not stop it.
#[cfg(unix)]
pub fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

/// Runs the child without a console, in its own process group (no Ctrl-C from zsf's console).
#[cfg(windows)]
pub fn detach(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
pub fn detach(_command: &mut Command) {}

/// How long stopped processes get to exit on SIGTERM before they get SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(5);
/// How often [`stop_processes_mentioning`] looks whether they are gone.
const STOP_POLL: Duration = Duration::from_millis(100);

/// Stops (SIGTERM, then SIGKILL after [`STOP_GRACE`]) every process of this user whose command
/// line contains `needle`, e.g. the agents of a run whose controller died (they live on,
/// reparented). Returns how many there were.
pub fn stop_processes_mentioning(needle: &str) -> usize {
    let found = pids_mentioning(needle);
    for pid in &found {
        terminate(*pid);
    }
    let all_gone = !found.is_empty()
        && poll_until(STOP_GRACE, STOP_POLL, || pids_mentioning(needle).is_empty());
    if !all_gone {
        for pid in pids_mentioning(needle) {
            kill(pid);
        }
    }
    found.len()
}

/// Asks process `pid` to stop (SIGTERM; Windows: ends it, there is no SIGTERM). For
/// `daemon/service.rs`, whose `libc::kill` does not build on Windows yet.
#[allow(dead_code)]
pub fn stop_pid(pid: u32) {
    if pid == 0 || pid > i32::MAX as u32 {
        return;
    }
    #[cfg(unix)]
    terminate(pid as i32);
    #[cfg(not(unix))]
    kill(pid as i32);
}

#[cfg(unix)]
fn terminate(pid: i32) {
    // SAFETY: plain signal to a process found by `pids_mentioning`.
    unsafe { libc::kill(pid, libc::SIGTERM) };
}

#[cfg(unix)]
fn kill(pid: i32) {
    // SAFETY: as above.
    unsafe { libc::kill(pid, libc::SIGKILL) };
}

/// Windows has no SIGTERM: the grace period passes and [`kill`] ends them.
#[cfg(not(unix))]
fn terminate(_pid: i32) {}

#[cfg(not(unix))]
fn kill(pid: i32) {
    portable::kill(pid as u32);
}

/// The processes (other than this one) whose command line contains `needle`.
#[cfg(not(target_os = "linux"))]
fn pids_mentioning(needle: &str) -> Vec<i32> {
    portable::pids_mentioning(needle)
}

/// The processes (other than this one) whose command line contains `needle`.
#[cfg(target_os = "linux")]
fn pids_mentioning(needle: &str) -> Vec<i32> {
    let me = std::process::id();
    let mentions = |pid: &i32| {
        std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|raw| {
            String::from_utf8_lossy(&raw)
                .replace('\0', " ")
                .contains(needle)
        })
    };
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| pid as u32 != me)
        .filter(mentions)
        .collect()
}

/// Calls `done` every `interval` until it says true (returns true) or `timeout` passed (false).
pub(crate) fn poll_until(
    timeout: Duration,
    interval: Duration,
    mut done: impl FnMut() -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(interval);
    }
    false
}

/// Copies the directory `from` into `to` (created when missing), recursively, skipping `.git`.
pub fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let kind = entry.file_type()?;
        let target = to.join(&name);
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The `/proc` answers above, from `sysinfo` (macOS, Windows). Built on every platform so the
/// Linux tests can check that both agree.
#[allow(dead_code)] // each platform uses part of it
mod portable {
    use std::path::Path;

    use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    fn processes(which: ProcessesToUpdate<'_>) -> System {
        let mut system = System::new();
        system.refresh_processes_specifics(
            which,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        system
    }

    pub fn pid_exists(pid: u32) -> bool {
        let pid = Pid::from_u32(pid);
        processes(ProcessesToUpdate::Some(&[pid]))
            .process(pid)
            .is_some()
    }

    pub fn boot_time_secs() -> Option<u64> {
        Some(System::boot_time()).filter(|t| *t > 0)
    }

    /// Free space on the disk whose mount point is the longest prefix of `path`.
    pub fn free_gb(path: &Path) -> f64 {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
        let disks = Disks::new_with_refreshed_list();
        disks
            .list()
            .iter()
            .filter(|d| path.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map_or(f64::INFINITY, |d| d.available_space() as f64 / 1e9)
    }

    pub fn pids_mentioning(needle: &str) -> Vec<i32> {
        let me = std::process::id();
        let system = processes(ProcessesToUpdate::All);
        let mut pids: Vec<i32> = system
            .processes()
            .iter()
            .filter(|(pid, process)| pid.as_u32() != me && process.thread_kind().is_none())
            .filter(|(_, process)| {
                let line: String = process
                    .cmd()
                    .iter()
                    .map(|a| format!("{} ", a.to_string_lossy()))
                    .collect();
                line.contains(needle)
            })
            .map(|(pid, _)| pid.as_u32() as i32)
            .collect();
        pids.sort_unstable();
        pids
    }

    pub fn kill(pid: u32) {
        let pid = Pid::from_u32(pid);
        if let Some(process) = processes(ProcessesToUpdate::Some(&[pid])).process(pid) {
            process.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn stops_processes_by_command_line() {
        let tag = format!("/tmp/zsf-test-{}/runs/r1/", std::process::id());
        let mut child = std::process::Command::new("bash")
            .arg("-c")
            .arg("sleep 60; : \"$0\"")
            .arg(&tag)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(super::stop_processes_mentioning(&tag), 1);
        assert!(!child.wait().unwrap().success());
        assert_eq!(super::stop_processes_mentioning(&tag), 0);
    }

    // F30: macOS and Windows use `portable`; on Linux it must give the `/proc` answers.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_portable_reader_agrees_with_proc() {
        let tag = format!("/tmp/zsf-portable-{}/runs/r1/", std::process::id());
        let mut child = std::process::Command::new("bash")
            .arg("-c")
            .arg("sleep 60; : \"$0\"")
            .arg(&tag)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let from_proc = super::pids_mentioning(&tag);
        assert_eq!(from_proc, [child.id() as i32]);
        assert_eq!(super::portable::pids_mentioning(&tag), from_proc);
        assert!(super::portable::pid_exists(child.id()));
        super::portable::kill(child.id());
        assert!(!child.wait().unwrap().success());
        assert!(!super::portable::pid_exists(child.id()));
        assert!(super::portable::pids_mentioning(&tag).is_empty());

        assert_eq!(super::portable::boot_time_secs(), super::boot_time_secs());
        let dir = tempfile::tempdir().unwrap();
        let (statvfs, disks) = (
            super::free_gb(dir.path()),
            super::portable::free_gb(dir.path()),
        );
        assert!(
            statvfs.is_finite() && (statvfs - disks).abs() < 1.0,
            "{statvfs} vs {disks}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_pid_stops_a_process() {
        use std::os::unix::process::ExitStatusExt;
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        super::stop_pid(child.id());
        assert_eq!(child.wait().unwrap().signal(), Some(libc::SIGTERM));
        super::stop_pid(0);
    }
}

//! What the operating system says about a run's processes: whether its controller still runs,
//! which agent CLIs still work in its workspace, and this machine's hostname. Linux reads
//! `/proc`; macOS and Windows ask the OS through `sysinfo` ([`portable`]), which gives the same
//! answers (the Linux tests check both agree).

#[cfg(target_os = "linux")]
use std::path::Path;

use super::{AgentProcess, Liveness};
use crate::harness_name::Harness;

/// Provider CLIs whose working directory is `workspace` (Linux `/proc`; empty elsewhere).
#[cfg(target_os = "linux")]
pub fn agent_processes(workspace: &str) -> Vec<AgentProcess> {
    let target = Path::new(workspace);
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cwd) = std::fs::read_link(entry.path().join("cwd")) else {
            continue;
        };
        if cwd != target {
            continue;
        }
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let args = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if let Some(name) = provider_name(&argv0_basename(&cmdline), &args) {
            out.push(AgentProcess {
                pid,
                name: name.to_owned(),
            });
        }
    }
    out.sort_by_key(|p| p.pid);
    out
}

#[cfg(not(target_os = "linux"))]
pub fn agent_processes(workspace: &str) -> Vec<AgentProcess> {
    portable::agent_processes(workspace)
}

/// The program name of a NUL-separated command line, without its directory
/// (`/usr/bin/claude\0-p\0…` → `claude`).
#[cfg(target_os = "linux")]
fn argv0_basename(cmdline: &[u8]) -> String {
    let argv0 = cmdline
        .split(|b| *b == 0)
        .next()
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .unwrap_or_default();
    argv0.rsplit('/').next().unwrap_or("").to_owned()
}

/// The agent CLI a process runs: the CLI's own binary (`claude`), or Node running a script
/// whose path names it (`node …/copilot/index.js`, with `args` space-separated).
fn provider_name(base: &str, args: &str) -> Option<&'static str> {
    let runs_script = |name: &str| base == "node" && args.contains(&format!("/{name}"));
    Harness::ALL
        .map(Harness::name)
        .into_iter()
        .find(|name| base == *name || runs_script(name))
}

/// Whether `pid` is this run's controller. A pid alone proves nothing: after a restart, or once
/// a transient controller of `zeroshot status` exited, the number belongs to any process. The
/// controller's command line is `zeroshot __zeroshot-run-controller --bootstrap
/// <state>/runs/<run id>/controller.bootstrap.json`, so both must appear in it.
#[cfg(target_os = "linux")]
pub(super) fn controller_liveness(pid: u32, run_id: &str) -> Liveness {
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return Liveness::Dead;
    };
    let cmdline = String::from_utf8_lossy(&cmdline);
    if !is_controller_of(&cmdline, run_id) {
        return Liveness::Dead;
    }
    // Field 3 of /proc/<pid>/stat (after the parenthesised name) is the state.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    match stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.trim_start().chars().next())
    {
        Some('Z' | 'X') => Liveness::Dead,
        Some('T' | 't') => Liveness::Suspended,
        _ => Liveness::Alive,
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn controller_liveness(pid: u32, run_id: &str) -> Liveness {
    portable::controller_liveness(pid, run_id)
}

/// `cmdline` is NUL-separated argv.
pub fn is_controller_of(cmdline: &str, run_id: &str) -> bool {
    let mut args = cmdline.split('\0');
    args.any(|a| a == "__zeroshot-run-controller")
        && cmdline.split('\0').any(|a| a.contains(run_id))
}

/// Whether any process has this pid (a benchmark batch's runner).
#[cfg(target_os = "linux")]
pub(crate) fn process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_alive(pid: u32) -> bool {
    portable::process_alive(pid)
}

/// This machine's hostname, the same value `socket.gethostname()` reads on Linux (the uname
/// nodename) — used to match pb's own `batches.host`/`runs.limits_json.runner_host` columns
/// without depending on a hostname crate.
#[cfg(target_os = "linux")]
pub(crate) fn hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn hostname() -> Option<String> {
    sysinfo::System::host_name()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
}

/// The same answers as the `/proc` readers above, from `sysinfo` (macOS, Windows). Built on every
/// platform so the Linux tests can check that both agree.
#[allow(dead_code)] // each platform uses part of it
mod portable {
    use std::path::Path;

    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

    use super::super::{AgentProcess, Liveness};
    use super::{is_controller_of, provider_name};

    /// A process table with the command line (and, when asked, the working directory) of `which`.
    fn processes(which: ProcessesToUpdate<'_>, cwd: bool) -> System {
        let mut kind = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always);
        if cwd {
            kind = kind.with_cwd(UpdateKind::Always);
        }
        let mut system = System::new();
        system.refresh_processes_specifics(which, true, kind);
        system
    }

    /// The program name without directory or `.exe` (`C:\\…\\claude.exe` → `claude`).
    pub(super) fn program_name(argv0: &str) -> String {
        let base = argv0.rsplit(['/', '\\']).next().unwrap_or("");
        base.strip_suffix(".exe").unwrap_or(base).to_owned()
    }

    pub fn agent_processes(workspace: &str) -> Vec<AgentProcess> {
        let target = Path::new(workspace);
        let system = processes(ProcessesToUpdate::All, true);
        let mut out: Vec<AgentProcess> = system
            .processes()
            .iter()
            .filter(|(_, process)| process.cwd() == Some(target) && process.thread_kind().is_none())
            .filter_map(|(pid, process)| {
                let argv: Vec<String> = process
                    .cmd()
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                // `/proc` joins argv with NUL and ends with one; this keeps `args` the same shape,
                // with Windows separators turned into `/` for the script-path test.
                let args = argv
                    .iter()
                    .map(|a| format!("{a} "))
                    .collect::<String>()
                    .replace('\\', "/");
                let name = provider_name(&program_name(argv.first()?), &args)?;
                Some(AgentProcess {
                    pid: pid.as_u32(),
                    name: name.to_owned(),
                })
            })
            .collect();
        out.sort_by_key(|p| p.pid);
        out
    }

    pub fn controller_liveness(pid: u32, run_id: &str) -> Liveness {
        let pid = Pid::from_u32(pid);
        let system = processes(ProcessesToUpdate::Some(&[pid]), false);
        let Some(process) = system.process(pid) else {
            return Liveness::Dead;
        };
        let cmdline: String = process
            .cmd()
            .iter()
            .map(|a| format!("{}\0", a.to_string_lossy()))
            .collect();
        if !is_controller_of(&cmdline, run_id) {
            return Liveness::Dead;
        }
        match process.status() {
            ProcessStatus::Zombie | ProcessStatus::Dead => Liveness::Dead,
            ProcessStatus::Stop | ProcessStatus::Tracing => Liveness::Suspended,
            _ => Liveness::Alive,
        }
    }

    pub fn process_alive(pid: u32) -> bool {
        let pid = Pid::from_u32(pid);
        processes(ProcessesToUpdate::Some(&[pid]), false)
            .process(pid)
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_is_recognised_by_its_command_line() {
        let id = "01a1a6e3-2125-7332-af81-ec638b2d39de";
        let argv = format!(
            "/usr/bin/zeroshot\0__zeroshot-run-controller\0--bootstrap\0/home/a/.local/state/zeroshot/runs/{id}/controller.bootstrap.json\0"
        );
        assert!(is_controller_of(&argv, id));
        assert!(
            !is_controller_of(&argv, "01a1a6e3-0000-7332-af81-ec638b2d39de"),
            "another run's controller"
        );
        assert!(
            !is_controller_of(&format!("sleep\07200\0{id}"), id),
            "a reused pid"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_reused_pid_is_not_a_live_controller() {
        // This test process is alive but is not a controller.
        assert_eq!(
            controller_liveness(std::process::id(), "01a1a6e3-2125-7332-af81-ec638b2d39de"),
            Liveness::Dead
        );
        assert_eq!(controller_liveness(u32::MAX - 1, "x"), Liveness::Dead);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn agent_clis_are_named_by_their_binary_or_their_node_script() {
        assert_eq!(argv0_basename(b"/usr/bin/claude\0-p\0hi\0"), "claude");
        assert_eq!(
            provider_name("claude", "/usr/bin/claude -p hi "),
            Some("claude")
        );
        assert_eq!(
            provider_name(
                "node",
                "node /usr/lib/node_modules/@github/copilot/index.js "
            ),
            Some("copilot")
        );
        assert_eq!(provider_name("node", "node /srv/app.js "), None);
        assert_eq!(provider_name("bash", "bash -c claude "), None);
    }

    /// Spawns `program` (a copy of `sleep` under that name) with `args`, in `cwd`.
    #[cfg(target_os = "linux")]
    fn sleeper(dir: &Path, program: &str, cwd: &Path, args: &[&str]) -> std::process::Child {
        let bin = dir.join(program);
        if !bin.exists() {
            std::fs::copy("/bin/sleep", &bin).unwrap();
        }
        let child = std::process::Command::new(&bin)
            .args(args)
            .current_dir(cwd)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        child
    }

    // F30: macOS and Windows use `portable`; on Linux it must give the `/proc` answers.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_portable_reader_agrees_with_proc_about_a_controller() {
        let id = "01a1a6e3-2125-7332-af81-ec638b2d39de";
        let bootstrap = format!("/state/runs/{id}/controller.bootstrap.json");
        // `sh -c 'sleep 30; :' __zeroshot-run-controller <bootstrap>`: the controller's command line.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30; :", "__zeroshot-run-controller", &bootstrap])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(150));
        let pid = child.id();
        assert_eq!(controller_liveness(pid, id), Liveness::Alive);
        assert_eq!(portable::controller_liveness(pid, id), Liveness::Alive);
        assert_eq!(
            portable::controller_liveness(pid, "another-run"),
            Liveness::Dead
        );
        unsafe { libc::kill(pid as i32, libc::SIGSTOP) };
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(controller_liveness(pid, id), Liveness::Suspended);
        assert_eq!(portable::controller_liveness(pid, id), Liveness::Suspended);
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!portable::process_alive(pid));
        assert_eq!(portable::controller_liveness(pid, id), Liveness::Dead);
        assert!(portable::process_alive(std::process::id()));
        assert_eq!(
            portable::controller_liveness(std::process::id(), id),
            Liveness::Dead,
            "a reused pid"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_portable_reader_agrees_with_proc_about_agent_clis() {
        let dir = tempfile::tempdir().unwrap();
        // /proc/<pid>/cwd is the resolved path: TMPDIR may be behind a symlink.
        let workspace = dir.path().canonicalize().unwrap().join("ws");
        std::fs::create_dir(&workspace).unwrap();
        let mut claude = sleeper(dir.path(), "claude", &workspace, &["30"]);
        let mut other = sleeper(dir.path(), "claude", dir.path(), &["30"]);
        let mut unrelated = sleeper(dir.path(), "sleepy", &workspace, &["30"]);
        let ws = workspace.to_str().unwrap();
        let pairs = |found: Vec<AgentProcess>| {
            found
                .into_iter()
                .map(|p| (p.pid, p.name))
                .collect::<Vec<_>>()
        };
        let from_proc = pairs(agent_processes(ws));
        assert_eq!(from_proc, [(claude.id(), "claude".to_owned())]);
        assert_eq!(pairs(portable::agent_processes(ws)), from_proc);
        for child in [&mut claude, &mut other, &mut unrelated] {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        assert!(portable::agent_processes(ws).is_empty());
    }

    #[test]
    fn windows_program_names_lose_their_directory_and_exe() {
        assert_eq!(
            portable::program_name(r"C:\Users\a\AppData\Roaming\npm\claude.exe"),
            "claude"
        );
        assert_eq!(portable::program_name("/usr/local/bin/codex"), "codex");
    }
}

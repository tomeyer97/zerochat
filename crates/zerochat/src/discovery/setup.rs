//! Can this machine work with zerochat? One check, three outputs: the startup banner,
//! `zerochat doctor` and `GET /api/setup` (the web's first-run card and Zeroshot strip).
//!
//! The facts are gathered elsewhere (`server::api::setup::check`: the zeroshot program's
//! `--version` and `steer --help`, zsf's `--version`, config.toml); this module turns them into
//! the answer, with every problem carrying the command that fixes it.

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use crate::paths::LEDGER_FILE;

/// The oldest Zeroshot zerochat drives: stock 16.1 has force-stop, resume and run-wide steer.
pub const ZEROSHOT_MINIMUM: (u64, u64, u64) = (16, 1, 0);
/// The command that installs (or updates) Zeroshot.
pub const ZEROSHOT_INSTALL: &str = "npm i -g @the-open-engine-company/zeroshot@latest";
/// How to get zsf when it is not found (the banner and `zerochat doctor` print it after the arrow).
pub const ZSF_INSTALL: &str = "included in the zerochat npm package; from source: `make`, or `cargo install --path crates/zsf`";
/// What the banner and `zerochat doctor` say about pb when it is not configured.
pub const PB_OPTIONAL: &str = "optional; pb is not public yet";
/// `Setup::app`: how a second zerochat recognises a running one on a busy port.
pub const APP_NAME: &str = "zerochat";

/// The whole check.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Setup {
    /// Always `zerochat`.
    pub app: String,
    /// zerochat's own version.
    pub version: String,
    /// No problem: Zeroshot is there and new enough, and the state directory can be read.
    pub ok: bool,
    pub zeroshot: ZeroshotCheck,
    pub runs: RunsCheck,
    pub zsf: ZsfCheck,
    pub pb: PbCheck,
    /// What is wrong, each with its fix; empty when `ok`.
    pub problems: Vec<SetupProblem>,
}

/// The zeroshot program zerochat runs for stop, resume and steer.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ZeroshotCheck {
    /// The program exists (on PATH, or at the path given with `--zeroshot`).
    pub found: bool,
    /// Its resolved path when found, else what was looked for.
    pub program: String,
    /// `x.y.z` as it reported it, `None` when it is missing or printed no version.
    pub version: Option<String>,
    /// `16.1.0`.
    pub minimum: String,
    /// Found and at least `minimum` (or, without a version, it steers like 16.1 does).
    pub meets_minimum: bool,
    /// Stop and resume a run (`zeroshot force-stop`, `zeroshot resume`).
    pub stop_resume: bool,
    /// Steer a whole run (`zeroshot steer --mode --text-file`).
    pub steer: bool,
    /// Steer one agent (`zeroshot steer --node`, newer than 16.1).
    pub steer_node: bool,
}

/// The state directory zerochat watches for local runs.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunsCheck {
    pub state_dir: String,
    /// It exists (it does not before the first `zeroshot run`, which is fine).
    pub exists: bool,
    /// It can be listed (a missing directory counts as readable: nothing to read yet).
    pub readable: bool,
    /// Runs in it (`runs/<id>/runs.sqlite3`).
    pub count: usize,
}

/// zsf (Zeroshot features), the slice scheduler. Optional.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ZsfCheck {
    pub found: bool,
    pub program: Option<String>,
    pub version: Option<String>,
    /// How many features zsf's state directory has.
    pub features: usize,
}

/// pb (ProgramBench runner), the benchmark loop. Optional.
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PbCheck {
    /// `pb_root` is set in config.toml.
    pub configured: bool,
    pub root: Option<String>,
}

/// One thing that keeps zerochat from working fully, and how to fix it.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetupProblem {
    /// Which check: `zeroshot` or `runs`.
    #[ts(type = "\"zeroshot\" | \"runs\"")]
    pub check: String,
    pub message: String,
    /// A command or step that fixes it.
    pub fix: Option<String>,
}

/// What was found out about the zeroshot program.
pub struct ZeroshotFacts {
    pub program: String,
    pub found: bool,
    /// Its `--version` output.
    pub version_output: Option<String>,
    /// `steer_capabilities` of its `steer --help`.
    pub capabilities: Vec<String>,
}

/// The first `x.y[.z]` in `text` (`zeroshot 16.1.0`, `v16.1.0`, `16.1.0`).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split(|c: char| c.is_whitespace() || c == '@' || c == '/')
        .find_map(|word| {
            let word = word.trim_start_matches('v');
            let core = word.split(['-', '+']).next()?;
            let mut parts = core.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = match parts.next() {
                Some(p) => p.parse().ok()?,
                None => 0,
            };
            Some((major, minor, patch))
        })
}

fn version_text((a, b, c): (u64, u64, u64)) -> String {
    format!("{a}.{b}.{c}")
}

pub fn zeroshot_check(facts: ZeroshotFacts) -> (ZeroshotCheck, Option<SetupProblem>) {
    let version = facts.version_output.as_deref().and_then(parse_version);
    let steer = facts.found && facts.capabilities.iter().any(|c| c == "steer");
    let steer_node = steer && facts.capabilities.iter().any(|c| c == "steer_node");
    // `zeroshot steer` exists from 16.1 on, so it proves the minimum whatever the version says: a build from
    // source (e.g. an upstream branch) prints the crate version, `0.1.0`.
    let meets_minimum = facts.found && (steer || version.is_some_and(|v| v >= ZEROSHOT_MINIMUM));
    let minimum = version_text(ZEROSHOT_MINIMUM);
    let problem = |message: String| {
        Some(SetupProblem {
            check: "zeroshot".to_owned(),
            message,
            fix: Some(ZEROSHOT_INSTALL.to_owned()),
        })
    };
    let problem = if !facts.found {
        let place = if facts.program.contains(std::path::MAIN_SEPARATOR) {
            format!("at {}", facts.program)
        } else {
            "on PATH".to_owned()
        };
        problem(format!("Zeroshot not found {place}"))
    } else if meets_minimum {
        None
    } else {
        match version {
            Some(v) => problem(format!(
                "Zeroshot {} is too old (needs 16.1+)",
                version_text(v)
            )),
            None => problem("Zeroshot printed no version (needs 16.1+)".to_owned()),
        }
    };
    let check = ZeroshotCheck {
        found: facts.found,
        program: facts.program,
        version: version.map(version_text),
        minimum,
        meets_minimum,
        stop_resume: meets_minimum,
        steer: meets_minimum && steer,
        steer_node: meets_minimum && steer_node,
    };
    (check, problem)
}

/// Counts the runs in `state_dir` (`runs/<id>/runs.sqlite3`).
pub fn runs_check(state_dir: &Path) -> (RunsCheck, Option<SetupProblem>) {
    let shown = state_dir.display().to_string();
    let unreadable = |error: std::io::Error| {
        let check = RunsCheck {
            state_dir: shown.clone(),
            exists: true,
            readable: false,
            count: 0,
        };
        let problem = SetupProblem {
            check: "runs".to_owned(),
            message: format!("Cannot read {shown}: {error}"),
            fix: Some(
                "Make it readable, or point ZEROSHOT_STATE_DIR at Zeroshot's state directory"
                    .to_owned(),
            ),
        };
        (check, Some(problem))
    };
    if let Err(error) = std::fs::read_dir(state_dir) {
        return match error.kind() {
            std::io::ErrorKind::NotFound => (
                RunsCheck {
                    state_dir: shown,
                    exists: false,
                    readable: true,
                    count: 0,
                },
                None,
            ),
            _ => unreadable(error),
        };
    }
    let count = match std::fs::read_dir(state_dir.join("runs")) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().join(LEDGER_FILE).is_file())
            .count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return unreadable(error),
    };
    (
        RunsCheck {
            state_dir: shown,
            exists: true,
            readable: true,
            count,
        },
        None,
    )
}

impl Setup {
    pub fn new(
        zeroshot: (ZeroshotCheck, Option<SetupProblem>),
        runs: (RunsCheck, Option<SetupProblem>),
        zsf: ZsfCheck,
        pb: PbCheck,
    ) -> Setup {
        let problems: Vec<SetupProblem> = [zeroshot.1, runs.1].into_iter().flatten().collect();
        Setup {
            app: APP_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ok: problems.is_empty(),
            zeroshot: zeroshot.0,
            runs: runs.0,
            zsf,
            pb,
            problems,
        }
    }

    /// The check as aligned terminal lines (banner and `zerochat doctor`); `home` is shown as `~`.
    pub fn lines(&self, home: &Path) -> Vec<String> {
        let tilde = |path: &str| shorten_home(path, home);
        let problem = |check: &str| self.problems.iter().find(|p| p.check == check);
        let mut lines = Vec::new();
        let z = &self.zeroshot;
        let zeroshot = match problem("zeroshot") {
            Some(p) => format!("✗ {}: {}", p.message, p.fix.as_deref().unwrap_or_default()),
            None => {
                let steering = if z.steer_node {
                    "✓ steering (whole run and per agent)"
                } else if z.steer {
                    "✓ steering (whole run; per-agent needs a newer Zeroshot)"
                } else {
                    "✗ steering (this Zeroshot has no `steer --mode`)"
                };
                format!(
                    "{} at {}   ✓ stop/resume {steering}",
                    z.version.as_deref().unwrap_or("?"),
                    tilde(&z.program)
                )
            }
        };
        lines.push(row("Zeroshot", &zeroshot));
        let runs = match problem("runs") {
            Some(p) => format!("✗ {}: {}", p.message, p.fix.as_deref().unwrap_or_default()),
            None if !self.runs.exists => format!("{}   no runs yet", tilde(&self.runs.state_dir)),
            None => format!(
                "{}   {}",
                tilde(&self.runs.state_dir),
                plural(self.runs.count, "run")
            ),
        };
        lines.push(row("Runs", &runs));
        let zsf = match (&self.zsf.program, &self.zsf.version) {
            (Some(_), Some(v)) => format!(
                "zsf (Zeroshot features) {v}   {}",
                plural(self.zsf.features, "feature")
            ),
            (Some(path), None) => format!(
                "zsf (Zeroshot features) at {}   {}",
                tilde(path),
                plural(self.zsf.features, "feature")
            ),
            _ => format!("zsf (Zeroshot features) not found → {ZSF_INSTALL}"),
        };
        lines.push(row("Features", &zsf));
        let pb = match &self.pb.root {
            Some(root) => format!("pb (ProgramBench runner) at {}", tilde(root)),
            None => format!("pb (ProgramBench runner) not configured → {PB_OPTIONAL}"),
        };
        lines.push(row("Benchmarks", &pb));
        lines
    }
}

fn row(label: &str, text: &str) -> String {
    format!("  {label:<11}{text}")
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// `path` with a leading `home` shown as `~`.
pub fn shorten_home(path: &str, home: &Path) -> String {
    let home = home.display().to_string();
    match path.strip_prefix(home.as_str()) {
        Some(rest) if home.len() > 1 && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn facts(found: bool, version: Option<&str>, caps: &[&str]) -> ZeroshotFacts {
        ZeroshotFacts {
            program: if found {
                "/usr/bin/zeroshot".into()
            } else {
                "zeroshot".into()
            },
            found,
            version_output: version.map(str::to_owned),
            capabilities: caps.iter().map(|c| c.to_string()).collect(),
        }
    }

    fn setup(z: ZeroshotFacts) -> Setup {
        let runs = RunsCheck {
            state_dir: "/home/u/.local/state/zeroshot".into(),
            exists: true,
            readable: true,
            count: 12,
        };
        Setup::new(
            zeroshot_check(z),
            (runs, None),
            ZsfCheck {
                found: false,
                program: None,
                version: None,
                features: 0,
            },
            PbCheck {
                configured: false,
                root: None,
            },
        )
    }

    #[test]
    fn parses_the_version_zeroshot_prints() {
        assert_eq!(parse_version("16.1.0"), Some((16, 1, 0)));
        assert_eq!(parse_version("zeroshot 16.1.2\n"), Some((16, 1, 2)));
        assert_eq!(
            parse_version("@the-open-engine-company/zeroshot v17.0.0-rc.1"),
            Some((17, 0, 0))
        );
        assert_eq!(parse_version("11.0"), Some((11, 0, 0)));
        assert_eq!(parse_version("unknown"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn zeroshot_missing_is_a_problem_with_the_install_command() {
        let s = setup(facts(false, None, &[]));
        assert!(!s.ok);
        assert!(
            !s.zeroshot.found
                && !s.zeroshot.stop_resume
                && !s.zeroshot.steer
                && !s.zeroshot.steer_node
        );
        assert_eq!(
            s.problems,
            [SetupProblem {
                check: "zeroshot".into(),
                message: "Zeroshot not found on PATH".into(),
                fix: Some(ZEROSHOT_INSTALL.into())
            }]
        );
        let lines = s.lines(Path::new("/home/u"));
        assert_eq!(
            lines[0],
            format!("  Zeroshot   ✗ Zeroshot not found on PATH: {ZEROSHOT_INSTALL}")
        );
        assert_eq!(lines[1], "  Runs       ~/.local/state/zeroshot   12 runs");
    }

    #[test]
    fn a_source_build_with_steer_meets_the_minimum() {
        // Built from a Zeroshot branch: the crate version, but `zeroshot steer --help` works.
        let s = setup(facts(
            true,
            Some("zeroshot 0.1.0"),
            &["steer", "steer_node"],
        ));
        assert!(s.ok && s.problems.is_empty());
        assert!(
            s.zeroshot.meets_minimum
                && s.zeroshot.stop_resume
                && s.zeroshot.steer
                && s.zeroshot.steer_node
        );
    }

    #[test]
    fn zeroshot_11_is_too_old() {
        let s = setup(facts(true, Some("11.0.0"), &[]));
        assert!(!s.ok);
        assert_eq!(s.zeroshot.version.as_deref(), Some("11.0.0"));
        assert!(!s.zeroshot.meets_minimum && !s.zeroshot.stop_resume);
        assert_eq!(
            s.problems[0].message,
            "Zeroshot 11.0.0 is too old (needs 16.1+)"
        );
        assert_eq!(
            s.lines(Path::new("/home/u"))[0],
            format!("  Zeroshot   ✗ Zeroshot 11.0.0 is too old (needs 16.1+): {ZEROSHOT_INSTALL}")
        );
    }

    #[test]
    fn zeroshot_16_1_steers_whole_runs() {
        let s = setup(facts(true, Some("16.1.0"), &["steer"]));
        assert!(s.ok, "{:?}", s.problems);
        assert!(
            s.zeroshot.meets_minimum
                && s.zeroshot.stop_resume
                && s.zeroshot.steer
                && !s.zeroshot.steer_node
        );
        assert_eq!(
            s.lines(Path::new("/home/u"))[0],
            "  Zeroshot   16.1.0 at /usr/bin/zeroshot   ✓ stop/resume ✓ steering (whole run; per-agent needs a newer Zeroshot)"
        );
    }

    #[test]
    fn zeroshot_with_steer_node_steers_each_agent() {
        let s = setup(facts(true, Some("16.1.0"), &["steer", "steer_node"]));
        assert!(s.ok);
        assert!(s.zeroshot.steer_node);
        assert_eq!(
            s.lines(Path::new("/home/u"))[0],
            "  Zeroshot   16.1.0 at /usr/bin/zeroshot   ✓ stop/resume ✓ steering (whole run and per agent)"
        );
    }

    #[test]
    fn a_zeroshot_without_a_version_is_judged_by_what_it_can_do() {
        assert!(setup(facts(true, Some(""), &["steer"])).ok);
        let s = setup(facts(true, None, &[]));
        assert!(!s.ok);
        assert_eq!(
            s.problems[0].message,
            "Zeroshot printed no version (needs 16.1+)"
        );
    }

    #[test]
    fn counts_runs_and_accepts_a_missing_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        let (check, problem) = runs_check(&missing);
        assert!(!check.exists && check.readable && check.count == 0 && problem.is_none());
        for (id, ledger) in [("a", true), ("b", true), ("c", false)] {
            let run = dir.path().join("runs").join(id);
            std::fs::create_dir_all(&run).unwrap();
            if ledger {
                std::fs::write(run.join(LEDGER_FILE), "").unwrap();
            }
        }
        let (check, problem) = runs_check(dir.path());
        assert!(check.exists && check.readable && problem.is_none());
        assert_eq!(check.count, 2);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_state_dir_is_a_problem() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        std::fs::create_dir(&state).unwrap();
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = std::fs::read_dir(&state).is_ok(); // root reads everything
        let (check, problem) = runs_check(&state);
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
        if readable_anyway {
            return;
        }
        assert!(check.exists && !check.readable);
        assert_eq!(problem.unwrap().check, "runs");
    }

    #[test]
    fn shows_home_as_tilde() {
        let home = PathBuf::from("/home/u");
        assert_eq!(
            shorten_home("/home/u/.local/state/zeroshot", &home),
            "~/.local/state/zeroshot"
        );
        assert_eq!(shorten_home("/home/user2/x", &home), "/home/user2/x");
        assert_eq!(shorten_home("/x", Path::new("/")), "/x");
    }

    #[test]
    fn optional_tools_are_named_and_never_problems() {
        let mut s = setup(facts(true, Some("16.1.0"), &["steer"]));
        s.zsf = ZsfCheck {
            found: true,
            program: Some("/home/u/.cargo/bin/zsf".into()),
            version: Some("0.1.0".into()),
            features: 2,
        };
        s.pb = PbCheck {
            configured: true,
            root: Some("/home/u/pb".into()),
        };
        let lines = s.lines(Path::new("/home/u"));
        assert_eq!(
            lines[2],
            "  Features   zsf (Zeroshot features) 0.1.0   2 features"
        );
        assert_eq!(lines[3], "  Benchmarks pb (ProgramBench runner) at ~/pb");
        let plain = setup(facts(true, Some("16.1.0"), &["steer"])).lines(Path::new("/home/u"));
        assert_eq!(
            plain[2],
            "  Features   zsf (Zeroshot features) not found → included in the zerochat npm package; from source: `make`, or `cargo install --path crates/zsf`"
        );
        assert_eq!(
            plain[3],
            "  Benchmarks pb (ProgramBench runner) not configured → optional; pb is not public yet"
        );
    }
}

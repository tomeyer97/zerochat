//! The fixture world every golden test reads: one temporary directory, built once per test
//! process from `testdata/` (never from the machine's own state):
//!
//! - `state/`      the local Zeroshot state root: every synthetic ledger (`testdata/ledgers/
//!   synthetic/*.json`, loaded into `runs.sqlite3`) whose `where` is `local`, plus the recorded
//!   fork and Zeroshot 16.1 runs (`testdata/recorded/*/state`)
//! - `observe/`    a configured `[[source]]` (observe-only) with the `observe` synthetic runs
//! - `home/`       HOME for session logs (Claude Code, Copilot)
//! - `pb/`         a copy of `testdata/pb`; `zsf/` the zsf features (built here, with real git
//!   repositories so commits and diffs are real); `work/` run workspaces
//! - `zerochat-config/config.toml`, `zeroshot-config/` (no targets.json: local only)
//!
//! Workspaces recorded as `/work/<…>` are moved under `<root>/work/<…>` (and the session-log
//! project directories renamed to match), so a run's workspace can exist where needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};

/// The fixed "now" of every golden (synthetic ledgers start at 1_795_000_000_000).
pub const NOW_MS: u64 = 1_795_000_000_000 + 3_600_000;

pub const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

pub fn testdata() -> PathBuf {
    Path::new(MANIFEST).join("testdata")
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: String,
    pub run_id: String,
    pub target: String,
    pub tags: Vec<String>,
}

pub struct World {
    pub root: PathBuf,
    pub state: PathBuf,
    pub observe: PathBuf,
    pub home: PathBuf,
    pub pb: PathBuf,
    pub zsf: PathBuf,
    pub zerochat_config: PathBuf,
    pub zeroshot_config: PathBuf,
    pub synthetic: Vec<Scenario>,
    /// (family, scenario, run id) of testdata/recorded/<family>/manifest.json.
    pub recorded: Vec<Scenario>,
    pub succeeded_commits: (String, String),
}

static WORLD: OnceLock<World> = OnceLock::new();

pub fn world() -> &'static World {
    WORLD.get_or_init(build)
}

/// `[(root, "<root>")]`: what golden output replaces.
pub fn normalizers() -> Vec<(String, String)> {
    let w = world();
    let root = w.root.display().to_string();
    vec![
        (root.clone(), "<root>".to_owned()),
        (slug(&root), "<root-slug>".to_owned()),
    ]
}

pub fn normalize(mut value: Value) -> Value {
    super::golden::replace_strings(&mut value, &normalizers());
    round_floats(&mut value);
    value
}

/// Floats rounded to 9 decimals: some sums run in HashMap order, so their last bit varies from
/// run to run (tests/KNOWN-BUGS.md); anything that matters shows far above 1e-9.
pub fn round_floats(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().unwrap();
            let r = (f * 1e9).round() / 1e9;
            *value = json!(r);
        }
        Value::Array(items) => items.iter_mut().for_each(round_floats),
        Value::Object(map) => map.values_mut().for_each(round_floats),
        _ => {}
    }
}

pub fn slug(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn copy_tree(from: &Path, to: &Path) {
    if !from.exists() {
        return;
    }
    for entry in walk(from) {
        let rel = entry.strip_prefix(from).unwrap();
        let dst = to.join(rel);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &dst).unwrap();
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

pub fn write_ledger(run_dir: &Path, run_id: &str, stored: &Value, events: &[(u64, Value)]) {
    std::fs::create_dir_all(run_dir).unwrap();
    let path = run_dir.join("runs.sqlite3");
    let _ = std::fs::remove_file(&path);
    let mut c = rusqlite::Connection::open(&path).unwrap();
    c.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=OFF;")
        .unwrap();
    let c = c.transaction().unwrap();
    c.execute_batch(
        "CREATE TABLE v2_runs (run_id TEXT PRIMARY KEY NOT NULL, submission_key TEXT UNIQUE NOT NULL, submission_digest TEXT NOT NULL, cursor INTEGER NOT NULL, stored_json TEXT NOT NULL) STRICT;
         CREATE TABLE v2_run_events (run_id TEXT NOT NULL, sequence INTEGER NOT NULL, event_json TEXT NOT NULL, PRIMARY KEY (run_id, sequence), FOREIGN KEY (run_id) REFERENCES v2_runs(run_id) ON DELETE CASCADE) STRICT;",
    )
    .unwrap();
    let cursor = events.last().map(|e| e.0).unwrap_or(0) as i64;
    c.execute(
        "INSERT INTO v2_runs VALUES (?1, ?2, 'digest', ?3, ?4)",
        rusqlite::params![run_id, format!("key-{run_id}"), cursor, stored.to_string()],
    )
    .unwrap();
    for (seq, body) in events {
        c.execute(
            "INSERT INTO v2_run_events VALUES (?1, ?2, ?3)",
            rusqlite::params![run_id, *seq as i64, body.to_string()],
        )
        .unwrap();
    }
    c.commit().unwrap();
}

/// Fixed identity and dates, so commit ids are the same on every machine.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .env("GIT_AUTHOR_DATE", "2026-11-18T08:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-11-18T08:00:00Z")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
}

fn commit_all(dir: &Path, message: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

fn build() -> World {
    let root = std::env::temp_dir().join(format!("zerochat-goldens-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let state = root.join("state");
    let observe = root.join("observe");
    let home = root.join("home");
    let pb = root.join("pb");
    let zsf = root.join("zsf");
    let zerochat_config = root.join("zerochat-config");
    let zeroshot_config = root.join("zeroshot-config");
    for d in [&state, &observe, &home, &zerochat_config, &zeroshot_config] {
        std::fs::create_dir_all(d).unwrap();
    }
    let work = |ws: &str| -> String {
        if let Some(rest) = ws.strip_prefix("/work/") {
            root.join("work").join(rest).display().to_string()
        } else {
            ws.to_owned()
        }
    };

    // The textkit repository the `succeeded` run worked in: base and the run's commit.
    let repo = root.join("work/succeeded/repo");
    init_repo(&repo);
    std::fs::create_dir_all(repo.join("textkit")).unwrap();
    std::fs::write(
        repo.join("textkit/core.py"),
        "def title(text):\n    return text.title()\n",
    )
    .unwrap();
    std::fs::write(repo.join("README.md"), "# textkit\n").unwrap();
    let base = commit_all(&repo, "textkit");
    std::fs::write(repo.join("textkit/core.py"), "import re\n\n\ndef title(text):\n    return text.title()\n\n\ndef slugify(text):\n    return re.sub(r\"[^a-z0-9]+\", \"-\", text.lower()).strip(\"-\")\n").unwrap();
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(
        repo.join("tests/test_core.py"),
        "from textkit.core import slugify\n\nassert slugify(\"A b!\") == \"a-b\"\n",
    )
    .unwrap();
    let done = commit_all(&repo, "Add slugify");
    // Uncommitted work after the run: shows in "the workspace as it is now".
    std::fs::write(repo.join("NOTES.md"), "later edit\n").unwrap();

    // Synthetic ledgers.
    let mut synthetic = Vec::new();
    let mut paths = walk(&testdata().join("ledgers/synthetic"));
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    for path in paths {
        let mut s = read_json(&path);
        let name = s["name"].as_str().unwrap().to_owned();
        if name == "succeeded" {
            let text = s.to_string();
            let zero = "0".repeat(40);
            let one = format!("{}1", "0".repeat(39));
            let mut text = text.replace(&zero, &base).replace(&one, &base);
            for n in [2u64, 3, 101, 102, 103] {
                text = text.replace(&format!("{n:040x}"), &done);
            }
            s = serde_json::from_str(&text).unwrap();
        }
        let run_id = s["runId"].as_str().unwrap().to_owned();
        let where_ = s["where"].as_str().unwrap().to_owned();
        let state_root = if where_ == "observe" {
            &observe
        } else {
            &state
        };
        let run_dir = state_root.join("runs").join(&run_id);
        let events: Vec<(u64, Value)> = s["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["seq"].as_u64().unwrap(), e["body"].clone()))
            .collect();
        write_ledger(&run_dir, &run_id, &s["stored"], &events);
        if let Some(recovery) = s["recovery"].as_object() {
            let mut recovery = Value::Object(recovery.clone());
            if let Some(ws) = recovery["workspace"].as_str() {
                recovery["workspace"] = json!(work(ws));
            }
            std::fs::write(
                run_dir.join("workspace-recovery.json"),
                serde_json::to_string_pretty(&recovery).unwrap(),
            )
            .unwrap();
        }
        if let Some(pid) = s["controllerPid"].as_u64() {
            std::fs::write(run_dir.join("controller.ready.json"), json!({"kind": "zeroshot.portable-controller-ready/v1", "runId": run_id, "pid": pid}).to_string()).unwrap();
        }
        if s["checkpoints"] == true {
            std::fs::create_dir_all(run_dir.join("checkpoints")).unwrap();
            std::fs::write(
                run_dir.join("checkpoints/index.json"),
                "{\"checkpoints\":[]}\n",
            )
            .unwrap();
        }
        let target = if where_ == "observe" {
            "observe"
        } else {
            "local"
        }
        .to_owned();
        let tags = s["tags"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        synthetic.push(Scenario {
            name,
            run_id,
            target,
            tags,
        });
    }
    synthetic.sort_by(|a, b| a.name.cmp(&b.name));

    // Recorded runs (real Zeroshot output), into the local state root.
    let mut recorded = Vec::new();
    for family in ["fork", "zeroshot16"] {
        let dir = testdata().join("recorded").join(family);
        if !dir.join("state").is_dir() {
            continue;
        }
        copy_tree(&dir.join("state"), &state);
        copy_tree(&dir.join("home"), &home);
    }
    let manifest = testdata().join("recorded/manifest.json");
    if manifest.is_file() {
        for entry in read_json(&manifest).as_array().into_iter().flatten() {
            let family = entry["family"].as_str().unwrap_or("");
            let run_id = entry["runId"].as_str().unwrap_or("").to_owned();
            if !state
                .join("runs")
                .join(&run_id)
                .join("runs.sqlite3")
                .is_file()
            {
                continue;
            }
            recorded.push(Scenario {
                name: format!("{family}/{}", entry["scenario"].as_str().unwrap_or("")),
                run_id,
                target: "local".into(),
                tags: vec![family.to_owned()],
            });
        }
    }
    // Every recorded workspace under <root>/work.
    for run in std::fs::read_dir(state.join("runs"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = run.path().join("workspace-recovery.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let mut v: Value = serde_json::from_str(&text).unwrap();
            if let Some(ws) = v["workspace"].as_str().filter(|w| w.starts_with("/work/")) {
                v["workspace"] = json!(work(ws));
                std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
            }
        }
    }

    // Session logs: synthetic ones, then project directories renamed for the moved workspaces.
    // Fixtures keep `dot-claude`/`dot-copilot` (dot directories are gitignored); HOME has the dots.
    copy_tree(&testdata().join("ledgers/home"), &home);
    for name in ["claude", "copilot"] {
        let from = home.join(format!("dot-{name}"));
        copy_tree(&from, &home.join(format!(".{name}")));
        let _ = std::fs::remove_dir_all(&from);
    }
    let projects = home.join(".claude/projects");
    for dir in std::fs::read_dir(&projects).into_iter().flatten().flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix("-work-") {
            std::fs::rename(
                dir.path(),
                projects.join(format!("{}-work-{rest}", slug(&root.display().to_string()))),
            )
            .unwrap();
        }
    }

    // pb.
    copy_tree(&testdata().join("pb"), &pb);

    // zsf features.
    super::zsf_fixture::build(&root, &zsf, &state);

    std::fs::write(
        zerochat_config.join("config.toml"),
        format!(
            "pb_root = \"{}\"\nzsf_state_dir = \"{}\"\nzsf_bin = \"{}\"\npb_command = \"{}\"\n\n[[source]]\nname = \"observe\"\nstate_dir = \"{}\"\nactions = \"observe\"\n",
            pb.display(),
            zsf.display(),
            root.join("no-such-zsf").display(),
            root.join("no-such-pb").display(),
            observe.display()
        ),
    )
    .unwrap();

    World {
        root,
        state,
        observe,
        home,
        pb,
        zsf,
        zerochat_config,
        zeroshot_config,
        synthetic,
        recorded,
        succeeded_commits: (base, done),
    }
}

impl World {
    pub fn scenario(&self, name: &str) -> &Scenario {
        self.synthetic
            .iter()
            .chain(&self.recorded)
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no scenario {name}"))
    }
}

//! zerochat's own configuration: run sources beyond this machine's default local state
//! directory, and other tools' data (`pb_root`).
//!
//! The user documentation of every key, with defaults and an example, is
//! [docs/configuration.md](https://github.com/tomeyer97/zerochat/blob/main/docs/configuration.md).
//! In short, `~/.config/zerochat/config.toml` (or `$XDG_CONFIG_HOME/zerochat/config.toml`):
//! ```toml
//! pb_root = "/home/you/zeroshot-benchmarking"
//! zsf_state_dir = "/home/you/.local/state/zsf"   # default: zsf's own ($ZSF_STATE_DIR, XDG)
//! zsf_bin = "/home/you/.cargo/bin/zsf"            # default: beside zerochat, else on PATH
//! pb_command = "uv run --project /home/you/zeroshot-benchmarking pb"   # optional; see below
//!
//! [[source]]
//! name = "ci"
//! state_dir = "/mnt/ci/zeroshot-state"
//! actions = "observe"
//! ```
//! A plain Zeroshot user has no such file: `load` then returns an empty config and no issue.
//!
//! `pb_command` is how zerochat invokes `pb` for the actions that need pb's own code (starting
//! and stopping a batch, `pb sweep-report`): split on whitespace, then pb's own subcommand and
//! flags are appended, and the process is always started with its current directory set to
//! `pb_root` (so pb's own `pb.toml` search, which walks up from the current directory, finds the
//! right project even when `pb_command` names a different checkout's `uv` environment — useful
//! for pointing a newer pb at an older pb_root's database without touching it). Defaults to
//! `uv run --project <pb_root> pb` when unset.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::discovery::{self, Target, TargetKind};
use crate::pb_db;

/// What `config.toml` adds: configured sources (already resolved to `Target`s of kind
/// `Observe`) and, separately, `pb_root` (used for the run-context provider, independent of
/// whether a `pb` source target exists).
#[derive(Default)]
pub struct Config {
    pub sources: Vec<Target>,
    pub pb_root: Option<PathBuf>,
    /// zsf's state root when set here; otherwise zsf's own resolution applies.
    pub zsf_state_dir: Option<PathBuf>,
    pub zsf_bin: Option<PathBuf>,
    /// Explicit `pb_command` from `config.toml`, or `None` to use the `uv run --project
    /// <pb_root> pb` default (computed by `pb_command()`, which needs `pb_root`).
    pub pb_command_override: Option<String>,
    pub issue: Option<String>,
}

impl Config {
    /// The command line to invoke `pb` with: the explicit `pb_command`, else `uv run --project
    /// <pb_root> pb`. `None` when no `pb_root` is configured at all (nothing to invoke against).
    pub fn pb_command(&self) -> Option<String> {
        self.pb_command_override.clone().or_else(|| {
            self.pb_root
                .as_ref()
                .map(|root| format!("uv run --project {} pb", root.display()))
        })
    }
}

#[derive(Deserialize, Default)]
struct RawConfig {
    pb_root: Option<String>,
    zsf_state_dir: Option<String>,
    zsf_bin: Option<String>,
    pb_command: Option<String>,
    #[serde(default, rename = "source")]
    sources: Vec<RawSource>,
}

#[derive(Deserialize)]
struct RawSource {
    name: String,
    state_dir: Option<String>,
    glob: Option<String>,
    #[serde(default)]
    actions: Actions,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Actions {
    #[default]
    All,
    Observe,
}

/// Reads and resolves `config.toml`. Never fails the caller: a missing or invalid file (or a
/// bad glob, or a name collision) is reported in `issue`, not as an error — the rest of the
/// config, if any was usable, is still returned.
pub fn load(config_root: &Path) -> Config {
    let path = config_root.join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Config::default(),
        Err(error) => {
            return Config {
                issue: Some(format!("cannot read {}: {error}", path.display())),
                ..Default::default()
            };
        }
    };
    let raw: RawConfig = match toml::from_str(&text) {
        Ok(raw) => raw,
        Err(error) => {
            return Config {
                issue: Some(format!("{} is not valid: {error}", path.display())),
                ..Default::default()
            };
        }
    };
    let mut sources = SourceList::new();
    for source in raw.sources {
        match build_roots(&source) {
            Ok(roots) => sources.add(&source.name, roots, source.actions == Actions::Observe),
            Err(message) => sources
                .issues
                .push(format!("source `{}`: {message}", source.name)),
        }
    }
    let pb_root = raw.pb_root.map(PathBuf::from);
    if let Some(root) = &pb_root {
        sources.add_pb(root);
    }
    let SourceList {
        sources, issues, ..
    } = sources;
    let issue = if issues.is_empty() {
        None
    } else {
        Some(issues.join("; "))
    };
    let path = |v: String| PathBuf::from(zsf::store::expand_home(&v));
    Config {
        sources,
        pb_root,
        pb_command_override: raw.pb_command,
        zsf_state_dir: raw.zsf_state_dir.map(path),
        zsf_bin: raw.zsf_bin.map(path),
        issue,
    }
}

/// The configured sources as they are resolved, with the ids already taken (`local` from the
/// start) and the problems found on the way.
struct SourceList {
    sources: Vec<Target>,
    ids: HashSet<String>,
    issues: Vec<String>,
}

impl SourceList {
    fn new() -> Self {
        Self {
            sources: Vec::new(),
            ids: HashSet::from([discovery::LOCAL_TARGET.to_owned()]),
            issues: Vec::new(),
        }
    }

    /// Adds a source named `name`, unless the name is empty, reserved (`local`, `target:…`,
    /// which Zeroshot's own targets use) or already taken.
    fn add(&mut self, name: &str, roots: Vec<PathBuf>, observe_only: bool) {
        if name.is_empty() || name == discovery::LOCAL_TARGET || name.starts_with("target:") {
            self.issues.push(format!("source `{name}`: invalid name"));
            return;
        }
        if !self.ids.insert(name.to_owned()) {
            self.issues
                .push(format!("source `{name}`: duplicate name, ignored"));
            return;
        }
        self.sources.push(Target {
            id: name.to_owned(),
            name: name.to_owned(),
            kind: TargetKind::Observe,
            origin: None,
            cli_name: None,
            roots,
            actions_allowed: !observe_only,
        });
    }

    /// `pb_root` is an implicit, observe-only source: every pb run's state directory. An
    /// explicit `[[source]]` of the same name wins.
    fn add_pb(&mut self, pb_root: &Path) {
        if self.ids.contains(pb_db::SOURCE_ID) {
            self.issues.push(format!("`pb_root` is set but a source named `{}` is already configured; keeping the explicit one", pb_db::SOURCE_ID));
            return;
        }
        let pattern = format!(
            "{}/{}",
            pb_root.display().to_string().trim_end_matches('/'),
            pb_db::RUN_STATE_DIRS
        );
        match expand_glob(&pattern) {
            Ok(roots) => self.add(pb_db::SOURCE_ID, roots, true),
            Err(message) => self
                .issues
                .push(format!("pb_root `{}`: {message}", pb_root.display())),
        }
    }
}

fn build_roots(source: &RawSource) -> Result<Vec<PathBuf>, String> {
    match (&source.state_dir, &source.glob) {
        (Some(_), Some(_)) => Err("set either `state_dir` or `glob`, not both".to_owned()),
        (None, None) => Err("needs `state_dir` or `glob`".to_owned()),
        (Some(dir), None) => Ok(vec![PathBuf::from(dir)]),
        (None, Some(pattern)) => expand_glob(pattern),
    }
}

fn expand_glob(pattern: &str) -> Result<Vec<PathBuf>, String> {
    let mut roots: Vec<PathBuf> = glob::glob(pattern)
        .map_err(|error| format!("bad glob: {error}"))?
        .filter_map(Result::ok)
        .filter(|p| p.is_dir())
        .collect();
    roots.sort();
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `config.toml` as a user would (TOML ignores the indentation of the raw strings).
    fn write_config(dir: &Path, text: &str) {
        std::fs::write(dir.join("config.toml"), text).unwrap();
    }

    /// docs/configuration.md is the user documentation: it names every key `RawConfig` and
    /// `RawSource` read.
    #[test]
    fn the_user_docs_name_every_key() {
        let doc = include_str!("../../../docs/configuration.md");
        for key in [
            "pb_root",
            "zsf_state_dir",
            "zsf_bin",
            "pb_command",
            "[[source]]",
            "name",
            "state_dir",
            "glob",
            "actions",
        ] {
            assert!(
                doc.contains(&format!("| `{key}` |")),
                "docs/configuration.md: {key}"
            );
        }
    }

    #[test]
    fn missing_file_is_not_an_issue() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(dir.path());
        assert!(cfg.issue.is_none());
        assert!(cfg.sources.is_empty());
        assert!(cfg.pb_root.is_none());
    }

    #[test]
    fn state_dir_source_is_observe_by_default_all() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
            [[source]]
            name = "ci"
            state_dir = "/mnt/ci"
            "#,
        );
        let cfg = load(dir.path());
        assert!(cfg.issue.is_none());
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].id, "ci");
        assert_eq!(cfg.sources[0].kind, TargetKind::Observe);
        assert_eq!(cfg.sources[0].roots, vec![PathBuf::from("/mnt/ci")]);
        assert!(cfg.sources[0].actions_allowed);
    }

    #[test]
    fn explicit_actions_observe_refuses() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
            [[source]]
            name = "ci"
            state_dir = "/mnt/ci"
            actions = "observe"
            "#,
        );
        let cfg = load(dir.path());
        assert!(!cfg.sources[0].actions_allowed);
    }

    #[test]
    fn glob_expands_to_matching_directories_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("runs/b/agent/zeroshot-state")).unwrap();
        std::fs::create_dir_all(dir.path().join("runs/a/agent/zeroshot-state")).unwrap();
        write_config(
            dir.path(),
            &format!(r#"pb_root = "{}""#, dir.path().display()),
        );
        let cfg = load(dir.path());
        assert!(cfg.issue.is_none(), "{:?}", cfg.issue);
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].id, "pb");
        assert!(!cfg.sources[0].actions_allowed);
        assert_eq!(cfg.sources[0].roots.len(), 2);
        assert!(cfg.sources[0].roots[0] < cfg.sources[0].roots[1]);
        assert_eq!(cfg.pb_root, Some(dir.path().to_path_buf()));
    }

    #[test]
    fn pb_source_name_collision_keeps_the_explicit_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mine")).unwrap();
        let config = format!(
            r#"
            pb_root = "/does/not/exist"

            [[source]]
            name = "pb"
            state_dir = "{}"
            "#,
            dir.path().join("mine").display()
        );
        write_config(dir.path(), &config);
        let cfg = load(dir.path());
        assert_eq!(cfg.sources.len(), 1);
        assert_eq!(cfg.sources[0].roots, vec![dir.path().join("mine")]);
        assert!(cfg.issue.is_some());
    }

    #[test]
    fn invalid_name_and_duplicate_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"
            [[source]]
            name = "local"
            state_dir = "/x"
            "#,
        );
        let cfg = load(dir.path());
        assert!(cfg.sources.is_empty());
        assert!(cfg.issue.unwrap().contains("invalid name"));
    }

    #[test]
    fn pb_command_defaults_to_uv_run_against_pb_root() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!(r#"pb_root = "{}""#, dir.path().display()),
        );
        let cfg = load(dir.path());
        assert_eq!(
            cfg.pb_command(),
            Some(format!("uv run --project {} pb", dir.path().display()))
        );
    }

    #[test]
    fn explicit_pb_command_overrides_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let config = format!(
            r#"
            pb_root = "{}"
            pb_command = "uv run --project /elsewhere pb"
            "#,
            dir.path().display()
        );
        write_config(dir.path(), &config);
        let cfg = load(dir.path());
        assert_eq!(
            cfg.pb_command(),
            Some("uv run --project /elsewhere pb".to_owned())
        );
    }

    #[test]
    fn no_pb_root_means_no_pb_command() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(dir.path());
        assert_eq!(cfg.pb_command(), None);
    }
}

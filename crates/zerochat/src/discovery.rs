//! Where Zeroshot keeps local runs and which targets the user configured, and whether this
//! machine can work with them (`setup`: the Zeroshot check of the banner, `zerochat doctor` and
//! `GET /api/setup`).

mod setup;

pub use setup::*;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

pub const LOCAL_TARGET: &str = "local";

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub id: String,
    pub name: String,
    pub kind: TargetKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub origin: Option<String>,
    /// The `--target` value for the zeroshot CLI; `None` for the local controller.
    #[serde(skip)]
    pub cli_name: Option<String>,
    /// `Observe` only: one or more state directories (a configured `state_dir`, or every match
    /// of a `glob`), each contributing its own `runs/<uuid>` the way the local root does.
    #[serde(skip)]
    pub roots: Vec<PathBuf>,
    /// `Observe` only: the configured source's `actions` policy (`"all"` vs `"observe"`). Even
    /// when `true`, zerochat does not yet drive these sources (no controller wiring for a
    /// configured directory); it only changes the refusal message.
    #[serde(skip)]
    pub actions_allowed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Local,
    Direct,
    Hosted,
    /// A source from `~/.config/zerochat/config.toml` (`[[source]]` or `pb_root`): one or more
    /// local-shaped state directories this machine can read directly, same as `Local`, but not
    /// the implicit `$ZEROSHOT_STATE_DIR` and never actioned (see `Target::actions_allowed`).
    Observe,
}

/// Zeroshot's target registry, `<zeroshot config>/targets.json`: only the parts zerochat reads.
/// ```json
/// {"version": 5, "targets": {"lab": {"origin": "http://127.0.0.1:8080", "access": {"mode": "direct"}}}}
/// ```
#[derive(Deserialize)]
struct Registry {
    /// By target name; a `BTreeMap`, so in name order.
    #[serde(default)]
    targets: BTreeMap<String, Value>,
}

/// One target of the registry. Read leniently, field by field: a field zerochat cannot make
/// sense of counts as absent (no origin; an access mode other than `"direct"` is hosted) and
/// leaves the other field as it is.
#[derive(Default)]
struct RegistryEntry {
    origin: Option<String>,
    access: Access,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Access {
    /// `"direct"`: zerochat reads the controller's API itself. Anything else (`"hosted"`, the
    /// Platform) zerochat cannot read yet.
    mode: String,
}

impl RegistryEntry {
    fn parse(entry: &Value) -> Self {
        Self {
            origin: entry
                .get("origin")
                .and_then(|origin| String::deserialize(origin).ok()),
            access: entry
                .get("access")
                .and_then(|access| Access::deserialize(access).ok())
                .unwrap_or_default(),
        }
    }

    fn kind(&self) -> TargetKind {
        if self.access.mode == "direct" {
            TargetKind::Direct
        } else {
            TargetKind::Hosted
        }
    }
}

/// This machine's controller: the `local` target, driven by the zeroshot program without `--target`.
pub fn local_target() -> Target {
    Target {
        id: LOCAL_TARGET.to_owned(),
        name: "This machine".to_owned(),
        kind: TargetKind::Local,
        origin: None,
        cli_name: None,
        roots: Vec::new(),
        actions_allowed: true,
    }
}

/// The local controller first, then every configured target sorted by name. Direct targets
/// that share an origin are listed once (the first name wins) so runs do not appear twice.
pub fn targets(config_root: &Path) -> (Vec<Target>, Option<String>) {
    let mut targets = vec![local_target()];
    let path = config_root.join("targets.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (targets, None),
        Err(error) => {
            return (
                targets,
                Some(format!("cannot read {}: {error}", path.display())),
            );
        }
    };
    let registry: Registry = match serde_json::from_str(&text) {
        Ok(registry) => registry,
        Err(error) => {
            return (
                targets,
                Some(format!("{} is not valid: {error}", path.display())),
            );
        }
    };
    let mut direct_origins = HashSet::new();
    for (name, entry) in registry.targets {
        let entry = RegistryEntry::parse(&entry);
        let kind = entry.kind();
        let origin = entry.origin.map(|o| o.trim_end_matches('/').to_owned());
        if let (TargetKind::Direct, Some(origin)) = (kind, &origin)
            && !direct_origins.insert(origin.clone())
        {
            continue;
        }
        targets.push(Target {
            id: format!("target:{name}"),
            name: name.clone(),
            kind,
            origin,
            cli_name: Some(name),
            roots: Vec::new(),
            actions_allowed: true,
        });
    }
    (targets, None)
}

/// The types of this module the web reads (`web/src/generated/discovery.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, discovery: TargetKind, Target, ZeroshotCheck, RunsCheck, ZsfCheck, PbCheck, SetupProblem, Setup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_local_first_and_dedupes_direct_origins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("targets.json"),
            r#"{"version":5,"targets":{
              "lab":{"origin":"http://127.0.0.1:8080","access":{"mode":"direct"}},
              "local":{"origin":"http://127.0.0.1:8080/","access":{"mode":"direct"}},
              "cloud":{"origin":"https://api.cloud.zeroshot.sh","access":{"mode":"hosted","deviceToken":"x"}}}}"#,
        )
        .unwrap();
        let (targets, issue) = targets(dir.path());
        assert!(issue.is_none());
        let ids: Vec<_> = targets.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["local", "target:cloud", "target:lab"]);
        assert_eq!(targets[1].kind, TargetKind::Hosted);
    }

    #[test]
    fn an_entry_zerochat_cannot_read_is_listed_as_hosted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("targets.json"),
            r#"{"targets":{
              "odd":{"access":"direct"},
              "bare":{},
              "odd-access":{"origin":"http://a","access":"direct"},
              "odd-origin":{"origin":5,"access":{"mode":"direct"}}}}"#,
        )
        .unwrap();
        let (targets, issue) = targets(dir.path());
        assert!(issue.is_none());
        let kinds: Vec<_> = targets
            .iter()
            .map(|t| (t.id.as_str(), t.kind, t.origin.as_deref()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("local", TargetKind::Local, None),
                ("target:bare", TargetKind::Hosted, None),
                ("target:odd", TargetKind::Hosted, None),
                ("target:odd-access", TargetKind::Hosted, Some("http://a")),
                ("target:odd-origin", TargetKind::Direct, None),
            ]
        );
    }

    #[test]
    fn missing_registry_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let (targets, issue) = targets(dir.path());
        assert_eq!(targets.len(), 1);
        assert!(issue.is_none());
    }
}

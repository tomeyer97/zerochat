//! What a pb harness *is*, in words a person can check: the graph's agent nodes in order, each
//! with the model and effort its runtime gives it, the loops and parallel groups they sit in,
//! and which harness under `pb_root/harness` (the default, a variant, or neither) it matches.
//!
//! Used in three places:
//! - the batch page describes the harness every run used, read from the copy pb keeps of it
//!   (`runs/<run_id>/harness/graph.json` and `runtime.json`, written before the agent starts);
//! - the Start form's custom-harness picker lists the graph and runtime files under
//!   `pb_root/harness`;
//! - that picker's preview checks a chosen pair before Start.
//!
//! Every path here is relative to `pb_root` and must stay inside it (no `..`, no symlink out),
//! and no error names an absolute server path.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::validate::is_path_segment;

/// Graph/runtime files are small (pb's own are ~25 kB); anything much larger is not one.
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;

/// One agent node of a graph, in the order the graph lists it (depth first).
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessNode {
    pub name: String,
    /// `step` (writes code) or `verifier` (reviews it), as the graph names it.
    pub kind: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The innermost loop this node runs in, and its round limit.
    pub loop_name: Option<String>,
    pub loop_max: Option<i64>,
    /// The parallel group (`par`) this node runs in, if any.
    pub parallel: Option<String>,
}

/// One harness, described.
#[derive(Serialize, Clone, Debug, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessInfo {
    /// `default` (pb.toml's own graph+runtime), `variant` (a `harness/variants/<name>/`),
    /// `custom` (neither).
    pub source: String,
    /// The variant's directory name, for `source == "variant"`.
    pub variant: Option<String>,
    /// Set when the graph matches the default or a variant but the runtime does not match its
    /// `runtime.json`: the other runtime file in that directory it does match, if any (e.g.
    /// `runtime-sonnet-build.json`), else `None` with `runtime_matches == false`.
    pub runtime_file: Option<String>,
    pub runtime_matches: bool,
    /// The runtime's own top-level `harness` (e.g. `claude`).
    pub agent_harness: Option<String>,
    /// `pb run --model`: replaces every agent node's model (already applied to `nodes`).
    pub model_override: Option<String>,
    pub nodes: Vec<HarnessNode>,
    /// How many of the batch's runs used exactly this harness (batch page only).
    pub runs: usize,
}

// ---------- a graph's agent nodes ----------

/// Keys of a Zeroshot graph node that hold types, bindings or conditions, never child nodes.
/// Skipping them keeps a field that happens to be called `name` from reading as a node.
const NON_NODE_KEYS: &[&str] = &[
    "input",
    "output",
    "state",
    "initialInput",
    "when",
    "inputBindings",
    "writeBindings",
    "type",
    "signals",
    "bindings",
];

/// The agent nodes of a graph (`step`/`verifier` nodes with a `worker`), depth first, with the
/// loop and parallel group each one sits in.
pub fn graph_nodes(graph: &Value, runtime: Option<&Value>) -> Vec<HarnessNode> {
    let mut walker = Walker {
        runtime,
        loops: Vec::new(),
        out: Vec::new(),
    };
    if let Some(root) = graph.get("root") {
        walker.walk(root, None);
    }
    walker.out
}

/// The depth-first walk behind [`graph_nodes`].
struct Walker<'a> {
    runtime: Option<&'a Value>,
    /// The loops around the current node, innermost last, with their round limits.
    loops: Vec<(String, Option<i64>)>,
    out: Vec<HarnessNode>,
}

impl Walker<'_> {
    /// `par` is the parallel group around `node`, if any.
    fn walk<'v>(&mut self, node: &'v Value, par: Option<&'v str>) {
        match node {
            Value::Array(items) => {
                for item in items {
                    self.walk(item, par);
                }
            }
            Value::Object(map) => self.walk_object(map, par),
            _ => {}
        }
    }

    fn walk_object<'v>(&mut self, map: &'v Map<String, Value>, par: Option<&'v str>) {
        let kind = map.get("kind").and_then(Value::as_str).unwrap_or("");
        let name = map.get("name").and_then(Value::as_str);
        if let Some(name) = name
            && map.contains_key("worker")
        {
            let node = self.agent_node(name, kind, par);
            self.out.push(node);
            return;
        }
        let loop_name = if kind == "loop" { name } else { None };
        if let Some(loop_name) = loop_name {
            self.loops.push((
                loop_name.to_owned(),
                map.get("maxIterations").and_then(Value::as_i64),
            ));
        }
        let par_here = if kind == "par" { name.or(par) } else { par };
        for (key, value) in map {
            if !NON_NODE_KEYS.contains(&key.as_str()) {
                self.walk(value, par_here);
            }
        }
        if loop_name.is_some() {
            self.loops.pop();
        }
    }

    fn agent_node(&self, name: &str, kind: &str, par: Option<&str>) -> HarnessNode {
        let settings = self
            .runtime
            .and_then(|r| r.get("nodes"))
            .and_then(|n| n.get(name));
        let setting = |key: &str| {
            settings
                .and_then(|s| s.get(key))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let (loop_name, loop_max) = self
            .loops
            .last()
            .cloned()
            .map_or((None, None), |(n, m)| (Some(n), m));
        HarnessNode {
            name: name.to_owned(),
            kind: kind.to_owned(),
            model: setting("model"),
            effort: setting("effort"),
            loop_name,
            loop_max,
            parallel: par.map(str::to_owned),
        }
    }
}

/// `pb run --model`: pb's `effective_runtime` sets every agent node's model to it.
fn apply_override(runtime: &Value, model_override: Option<&str>) -> Value {
    let mut rt = runtime.clone();
    if let Some(m) = model_override
        && let Some(nodes) = rt.get_mut("nodes").and_then(Value::as_object_mut)
    {
        for node in nodes.values_mut() {
            if node.get("kind").and_then(Value::as_str) == Some("agent")
                && let Some(obj) = node.as_object_mut()
            {
                obj.insert("model".to_owned(), Value::String(m.to_owned()));
            }
        }
    }
    rt
}

// ---------- reading graph and runtime files ----------

enum JsonFileError {
    /// Larger than [`MAX_JSON_BYTES`], or its size cannot be read.
    TooLarge,
    /// Unreadable, or not JSON: the reason.
    Invalid(String),
}

fn read_json_file(path: &Path) -> Result<Value, JsonFileError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() <= MAX_JSON_BYTES => {}
        _ => return Err(JsonFileError::TooLarge),
    }
    let text =
        std::fs::read_to_string(path).map_err(|e| JsonFileError::Invalid(e.kind().to_string()))?;
    serde_json::from_str(&text).map_err(|e| JsonFileError::Invalid(e.to_string()))
}

/// A small JSON file, or `None` for anything else (missing, a directory, too large, not JSON).
fn read_json(path: &Path) -> Option<Value> {
    if !path.is_file() {
        return None;
    }
    read_json_file(path).ok()
}

// ---------- the harnesses pb_root knows ----------

/// pb.toml's `[harness] graph` / `runtime` (relative to `pb_root`), defaulting like pb does.
pub fn default_paths(pb_root: &Path) -> (String, String) {
    let doc: Option<toml::Value> = std::fs::read_to_string(pb_root.join("pb.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    let harness = doc.as_ref().and_then(|d| d.get("harness"));
    let path = |key: &str, default: &str| {
        harness
            .and_then(|h| h.get(key))
            .and_then(toml::Value::as_str)
            .unwrap_or(default)
            .to_owned()
    };
    (
        path("graph", "harness/graph.json"),
        path("runtime", "harness/runtime.json"),
    )
}

/// A `harness/variants/<name>/` directory: pb's place for a named alternative harness. pb runs
/// it as its `graph.json` with its `runtime.json`; other `runtime*.json` files next to them are
/// alternative runtimes for the same graph.
pub struct Variant {
    dir: PathBuf,
}

impl Variant {
    /// The directory name (`None` if it is not UTF-8).
    pub fn name(&self) -> Option<&str> {
        self.dir.file_name().and_then(|n| n.to_str())
    }

    pub fn graph(&self) -> PathBuf {
        self.dir.join("graph.json")
    }

    pub fn runtime(&self) -> PathBuf {
        self.dir.join("runtime.json")
    }

    /// Has both files `pb run --graph --runtime` needs.
    pub fn is_complete(&self) -> bool {
        self.graph().is_file() && self.runtime().is_file()
    }

    /// The names of the `runtime*.json` entries in the directory, `runtime.json` first.
    fn runtime_files(&self) -> Vec<String> {
        let entries = std::fs::read_dir(&self.dir).into_iter().flatten().flatten();
        let mut names: Vec<String> = entries
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| n.starts_with("runtime") && n.ends_with(".json"))
            .collect();
        names.sort_by_key(|n| (n != "runtime.json", n.clone()));
        names
    }
}

/// The variant directory `name` (which may not exist). `name` must already be a plain name.
pub fn variant(pb_root: &Path, name: &str) -> Variant {
    Variant {
        dir: variants_dir(pb_root).join(name),
    }
}

/// Every directory under `harness/variants`, complete or not, sorted.
pub fn variants(pb_root: &Path) -> Vec<Variant> {
    let entries = std::fs::read_dir(variants_dir(pb_root))
        .into_iter()
        .flatten()
        .flatten();
    let mut dirs: Vec<PathBuf> = entries.map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs.into_iter().map(|dir| Variant { dir }).collect()
}

fn variants_dir(pb_root: &Path) -> PathBuf {
    pb_root.join("harness").join("variants")
}

/// Where a known harness comes from ([`HarnessInfo::source`]).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Source {
    Default,
    Variant,
    Custom,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Default => "default",
            Source::Variant => "variant",
            Source::Custom => "custom",
        }
    }
}

/// A harness a run's copy can match: the default, or a variant with each of its runtime files
/// (`runtime.json` first).
struct Candidate {
    source: Source,
    variant: Option<String>,
    graph: Value,
    runtimes: Vec<(String, Value)>,
}

/// The default harness (when its graph reads), then every variant with a readable `graph.json`.
fn candidates(pb_root: &Path) -> Vec<Candidate> {
    let mut out = Vec::new();
    let (graph_path, runtime_path) = default_paths(pb_root);
    if let Some(graph) = read_json(&pb_root.join(&graph_path)) {
        let runtimes = read_json(&pb_root.join(&runtime_path))
            .map(|v| vec![(file_name(&runtime_path), v)])
            .unwrap_or_default();
        out.push(Candidate {
            source: Source::Default,
            variant: None,
            graph,
            runtimes,
        });
    }
    for variant in variants(pb_root) {
        let Some(graph) = read_json(&variant.graph()) else {
            continue;
        };
        let runtimes = variant
            .runtime_files()
            .into_iter()
            .filter_map(|n| read_json(&variant.dir.join(&n)).map(|v| (n, v)))
            .collect();
        out.push(Candidate {
            source: Source::Variant,
            variant: variant.name().map(str::to_owned),
            graph,
            runtimes,
        });
    }
    out
}

fn file_name(p: &str) -> String {
    Path::new(p)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(p)
        .to_owned()
}

/// Describes one graph+runtime pair: which known harness it is, and its nodes.
fn describe(
    graph: &Value,
    runtime: &Value,
    model_override: Option<&str>,
    known: &[Candidate],
) -> HarnessInfo {
    let effective = apply_override(runtime, model_override);
    let mut info = HarnessInfo {
        source: Source::Custom.as_str().to_owned(),
        variant: None,
        runtime_file: None,
        runtime_matches: false,
        agent_harness: runtime
            .get("harness")
            .and_then(Value::as_str)
            .map(str::to_owned),
        model_override: model_override.map(str::to_owned),
        nodes: graph_nodes(graph, Some(&effective)),
        runs: 0,
    };
    if let Some(c) = known.iter().find(|c| &c.graph == graph) {
        info.source = c.source.as_str().to_owned();
        info.variant = c.variant.clone();
        // pb copies the *effective* runtime (with `--model` applied) into the run; compare like
        // with like.
        let matched = c
            .runtimes
            .iter()
            .find(|(_, rt)| apply_override(rt, model_override) == effective || rt == runtime);
        if let Some((name, _)) = matched {
            info.runtime_matches = true;
            if name != "runtime.json" && c.source == Source::Variant {
                info.runtime_file = Some(name.clone());
            }
        }
    }
    info
}

// ---------- the batch page ----------

/// Runs that used the same graph, runtime and `--model`.
struct HarnessGroup {
    graph: Value,
    runtime: Value,
    model_override: Option<String>,
    runs: usize,
}

/// Every distinct harness the given runs used, most-used first, read from pb's own copy at
/// `pb_root/runs/<run_id>/harness/`. `runs` pairs each run id with its `runs.model_override`.
/// Runs whose copy is missing (deleted, or a pb from before it kept one) are skipped; an empty
/// result means none had one.
pub fn batch_harnesses(pb_root: &Path, runs: &[(String, Option<String>)]) -> Vec<HarnessInfo> {
    let mut groups: Vec<HarnessGroup> = Vec::new();
    for (run_id, model_override) in runs {
        if !is_path_segment(run_id) {
            continue;
        }
        let dir = pb_root.join("runs").join(run_id).join("harness");
        let (Some(graph), Some(runtime)) = (
            read_json(&dir.join("graph.json")),
            read_json(&dir.join("runtime.json")),
        ) else {
            continue;
        };
        let same = |g: &&mut HarnessGroup| {
            g.graph == graph && g.runtime == runtime && &g.model_override == model_override
        };
        match groups.iter_mut().find(same) {
            Some(group) => group.runs += 1,
            None => groups.push(HarnessGroup {
                graph,
                runtime,
                model_override: model_override.clone(),
                runs: 1,
            }),
        }
    }
    let known = candidates(pb_root);
    let mut out: Vec<HarnessInfo> = groups
        .into_iter()
        .map(|group| {
            // The copy already has the override applied (pb writes its effective runtime), so
            // describing it again with the override is a no-op on the models — kept for the
            // label and for matching against the unmodified variant runtime.
            let mut info = describe(
                &group.graph,
                &group.runtime,
                group.model_override.as_deref(),
                &known,
            );
            info.runs = group.runs;
            info
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.runs));
    out
}

/// The model a run's own runtime copy (`runs/<run>/harness/runtime.json`, pb's effective
/// runtime, overrides applied) gives one node — for a replay with no recorded node execution
/// yet.
pub fn runtime_model(pb_root: &Path, run_id: &str, node: &str) -> Option<String> {
    if !is_path_segment(run_id) {
        return None;
    }
    let runtime = read_json(
        &pb_root
            .join("runs")
            .join(run_id)
            .join("harness")
            .join("runtime.json"),
    )?;
    runtime
        .get("nodes")?
        .get(node)?
        .get("model")?
        .as_str()
        .map(str::to_owned)
}

// ---------- the start form: listing and previewing harness files ----------

#[derive(Serialize, Default, Debug, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessFiles {
    /// Graph files under `pb_root/harness`, relative to `pb_root` (`harness/variants/x/graph.json`).
    pub graphs: Vec<String>,
    pub runtimes: Vec<String>,
}

/// How deep [`list_harness_files`] looks below `pb_root/harness`.
const MAX_HARNESS_DEPTH: usize = 3;

/// Python's bytecode cache: never a harness file, often next to one.
const PYCACHE: &str = "__pycache__";

enum HarnessFileKind {
    Graph,
    Runtime,
}

/// What a JSON file is, by its content: a graph has a `root` object, a runtime a `nodes`
/// object. (A variant's runtimes are found by file name instead, see [`Variant`]: the picker
/// offers any file that reads as one, the batch page matches pb's own layout.)
fn classify(value: &Value) -> Option<HarnessFileKind> {
    if value.get("root").is_some_and(Value::is_object) {
        Some(HarnessFileKind::Graph)
    } else if value.get("nodes").is_some_and(Value::is_object) {
        Some(HarnessFileKind::Runtime)
    } else {
        None
    }
}

/// `path` relative to `root`, with `/` between components on every platform.
fn rel_slash_path(path: &Path, root: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Every `*.json` under `pb_root/harness` (`MAX_HARNESS_DEPTH` levels deep at most, symlinks
/// not followed, hidden entries and `__pycache__` skipped) that reads as a graph or a runtime,
/// relative to `pb_root`, sorted.
pub fn list_harness_files(pb_root: &Path) -> HarnessFiles {
    let mut files = HarnessFiles::default();
    let mut stack = vec![(pb_root.join("harness"), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == PYCACHE {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() && depth < MAX_HARNESS_DEPTH {
                stack.push((path, depth + 1));
                continue;
            }
            if !file_type.is_file() || !name.ends_with(".json") {
                continue;
            }
            let Some(kind) = read_json(&path).as_ref().and_then(classify) else {
                continue;
            };
            let Some(rel) = rel_slash_path(&path, pb_root) else {
                continue;
            };
            match kind {
                HarnessFileKind::Graph => files.graphs.push(rel),
                HarnessFileKind::Runtime => files.runtimes.push(rel),
            }
        }
    }
    files.graphs.sort();
    files.runtimes.sort();
    files
}

/// Resolves a path the form sent (relative to `pb_root`, or absolute but inside it) to a file
/// inside `pb_root`. The error names only what the person typed, never the server's own
/// directory.
pub fn resolve_in_root(pb_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("No file chosen.".to_owned());
    }
    let p = Path::new(raw);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        if p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!(
                "{raw}: use a path inside the pb project, without `..`."
            ));
        }
        pb_root.join(p)
    };
    let root = pb_root
        .canonicalize()
        .map_err(|_| "The pb project directory cannot be read.".to_owned())?;
    let canon = full
        .canonicalize()
        .map_err(|_| format!("File not found: {raw}."))?;
    if !canon.starts_with(&root) {
        return Err(format!("{raw} is outside the pb project."));
    }
    if !canon.is_file() {
        return Err(format!("{raw} is not a file."));
    }
    Ok(canon)
}

#[derive(Serialize, Debug, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessPreview {
    pub info: Option<HarnessInfo>,
    /// Problems that would make `pb run` fail: shown next to the field, and Start stays blocked.
    pub errors: Vec<String>,
    /// Worth a look, but pb would still start (e.g. an agent node the runtime has no entry for).
    pub warnings: Vec<String>,
}

/// The file the form named as its `what` (`graph` or `runtime`), read and parsed; the error is
/// the message shown next to that field.
fn load_checked(pb_root: &Path, raw: &str, what: &str) -> Result<Value, String> {
    let path = resolve_in_root(pb_root, raw).map_err(|e| format!("{what}: {e}"))?;
    read_json_file(&path).map_err(|error| match error {
        JsonFileError::TooLarge => format!("{what}: {raw} is too large to be a {what}."),
        JsonFileError::Invalid(reason) => format!("{what}: {raw} is not valid JSON ({reason})."),
    })
}

/// What `pb run --graph <graph> --runtime <runtime> [--model <model>]` would run, checked
/// before Start: both files exist inside the project and parse, the graph has a root with at
/// least one agent node, the runtime has a `nodes` table.
pub fn preview(
    pb_root: &Path,
    graph: &str,
    runtime: &str,
    model_override: Option<&str>,
) -> HarnessPreview {
    let graph_value = load_checked(pb_root, graph, "graph");
    let runtime_value = load_checked(pb_root, runtime, "runtime");
    let mut errors: Vec<String> = [&graph_value, &runtime_value]
        .into_iter()
        .filter_map(|r| r.as_ref().err().cloned())
        .collect();
    let (graph_value, runtime_value) = (graph_value.ok(), runtime_value.ok());
    if let Some(g) = &graph_value
        && !g.get("root").is_some_and(Value::is_object)
    {
        errors.push(format!(
            "graph: {} has no `root`; is it a runtime file?",
            graph.trim()
        ));
    }
    if let Some(r) = &runtime_value
        && !r.get("nodes").is_some_and(Value::is_object)
    {
        errors.push(format!(
            "runtime: {} has no `nodes`; is it a graph file?",
            runtime.trim()
        ));
    }
    let mut warnings = Vec::new();
    let info = match (&graph_value, &runtime_value) {
        (Some(g), Some(r)) if errors.is_empty() => {
            let info = describe(
                g,
                r,
                model_override.filter(|m| !m.trim().is_empty()),
                &candidates(pb_root),
            );
            if info.nodes.is_empty() {
                errors.push("graph: it has no agent nodes.".to_owned());
            }
            warnings = cross_check(&info, r);
            Some(info)
        }
        _ => None,
    };
    HarnessPreview {
        info,
        errors,
        warnings,
    }
}

/// Where the graph and the runtime disagree: agent nodes the runtime gives no model, and runtime
/// entries for nodes the graph does not have. pb still starts with either.
fn cross_check(info: &HarnessInfo, runtime: &Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let missing: Vec<&str> = info
        .nodes
        .iter()
        .filter(|n| n.model.is_none())
        .map(|n| n.name.as_str())
        .collect();
    if !missing.is_empty() {
        warnings.push(format!(
            "The runtime gives no model to: {}.",
            missing.join(", ")
        ));
    }
    let graph_nodes: HashSet<&str> = info.nodes.iter().map(|n| n.name.as_str()).collect();
    let runtime_nodes = runtime["nodes"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, _)| k);
    let unused: Vec<&str> = runtime_nodes
        .map(String::as_str)
        .filter(|k| !graph_nodes.contains(k))
        .collect();
    if !unused.is_empty() {
        warnings.push(format!(
            "The runtime has entries for nodes this graph does not have: {}.",
            unused.join(", ")
        ));
    }
    warnings
}

/// The types of this module the web reads (`web/src/generated/harness.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, harness: HarnessNode, HarnessInfo, HarnessFiles, HarnessPreview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph() -> Value {
        json!({"profile": "p", "root": {"kind": "seq", "name": "run", "children": [
            {"kind": "step", "name": "worker", "worker": "w", "input": {"kind": "record", "fields": {"name": {"type": {"kind": "string"}}}}},
            {"kind": "choice", "name": "c", "branches": [{"when": {}, "node": {"kind": "fail", "name": "f"}}], "otherwise": {
                "kind": "loop", "name": "review_loop", "maxIterations": 3, "body": {"kind": "seq", "name": "round", "children": [
                    {"kind": "par", "name": "reviews", "branches": [
                        {"kind": "verifier", "name": "acceptance", "worker": "v"},
                        {"kind": "verifier", "name": "code", "worker": "v"}
                    ]},
                    {"kind": "step", "name": "repair", "worker": "w"}
                ]}
            }}
        ]}})
    }

    fn runtime(model: &str) -> Value {
        json!({"harness": "claude", "nodes": {
            "worker": {"kind": "agent", "model": model, "effort": "high"},
            "acceptance": {"kind": "agent", "model": "claude-haiku-4-5", "effort": "medium"},
            "code": {"kind": "agent", "model": "claude-haiku-4-5"},
            "repair": {"kind": "agent", "model": model, "effort": "high"}
        }})
    }

    #[test]
    fn graph_nodes_are_listed_in_order_with_their_loop_parallel_group_and_model() {
        let nodes = graph_nodes(&graph(), Some(&runtime("claude-sonnet-5")));
        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["worker", "acceptance", "code", "repair"]);
        assert_eq!(nodes[0].model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(nodes[0].effort.as_deref(), Some("high"));
        assert_eq!(nodes[0].loop_name, None);
        assert_eq!(nodes[1].kind, "verifier");
        assert_eq!(nodes[1].loop_name.as_deref(), Some("review_loop"));
        assert_eq!(nodes[1].loop_max, Some(3));
        assert_eq!(nodes[1].parallel.as_deref(), Some("reviews"));
        assert_eq!(nodes[3].parallel, None);
        assert_eq!(nodes[3].loop_name.as_deref(), Some("review_loop"));
        assert_eq!(nodes[2].effort, None);
    }

    fn write(path: &Path, value: &Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("pb.toml"),
            "[harness]\ngraph = \"harness/graph.json\"\nruntime = \"harness/runtime.json\"\n",
        )
        .unwrap();
        write(
            &root.join("harness/graph.json"),
            &json!({"root": {"kind": "step", "name": "solo", "worker": "w"}}),
        );
        write(
            &root.join("harness/runtime.json"),
            &json!({"nodes": {"solo": {"kind": "agent", "model": "claude-sonnet-5"}}}),
        );
        write(&root.join("harness/variants/clone-v1/graph.json"), &graph());
        write(
            &root.join("harness/variants/clone-v1/runtime.json"),
            &runtime("claude-sonnet-5"),
        );
        write(
            &root.join("harness/variants/clone-v1/runtime-haiku.json"),
            &runtime("claude-haiku-4-5"),
        );
        dir
    }

    #[test]
    fn batch_harnesses_name_the_variant_and_say_when_runs_differ() {
        let dir = project();
        let root = dir.path();
        // Two runs of clone-v1 as-is, one with its other runtime file, one with pb run --model.
        write(&root.join("runs/r1/harness/graph.json"), &graph());
        write(
            &root.join("runs/r1/harness/runtime.json"),
            &runtime("claude-sonnet-5"),
        );
        write(&root.join("runs/r2/harness/graph.json"), &graph());
        write(
            &root.join("runs/r2/harness/runtime.json"),
            &runtime("claude-sonnet-5"),
        );
        write(&root.join("runs/r3/harness/graph.json"), &graph());
        write(
            &root.join("runs/r3/harness/runtime.json"),
            &runtime("claude-haiku-4-5"),
        );
        let runs = vec![
            ("r1".to_owned(), None),
            ("r2".to_owned(), None),
            ("r3".to_owned(), None),
            ("missing".to_owned(), None),
            ("../etc".to_owned(), None),
        ];
        let hs = batch_harnesses(root, &runs);
        assert_eq!(
            hs.len(),
            2,
            "two distinct harnesses: the batch's runs differ"
        );
        assert_eq!(hs[0].runs, 2);
        assert_eq!(hs[0].source, "variant");
        assert_eq!(hs[0].variant.as_deref(), Some("clone-v1"));
        assert!(hs[0].runtime_matches);
        assert_eq!(hs[0].runtime_file, None);
        assert_eq!(hs[0].agent_harness.as_deref(), Some("claude"));
        assert_eq!(hs[1].runtime_file.as_deref(), Some("runtime-haiku.json"));
        assert_eq!(hs[1].nodes[0].model.as_deref(), Some("claude-haiku-4-5"));
    }

    #[test]
    fn a_model_override_still_matches_its_variant_and_a_stranger_is_custom() {
        let dir = project();
        let root = dir.path();
        // pb writes the effective runtime: every agent node on the override model.
        write(&root.join("runs/r1/harness/graph.json"), &graph());
        write(
            &root.join("runs/r1/harness/runtime.json"),
            &apply_override(&runtime("claude-sonnet-5"), Some("claude-opus-5")),
        );
        write(
            &root.join("runs/r2/harness/graph.json"),
            &json!({"root": {"kind": "step", "name": "other", "worker": "w"}}),
        );
        write(
            &root.join("runs/r2/harness/runtime.json"),
            &json!({"nodes": {}}),
        );
        let hs = batch_harnesses(
            root,
            &[
                ("r1".to_owned(), Some("claude-opus-5".to_owned())),
                ("r2".to_owned(), None),
            ],
        );
        let r1 = hs.iter().find(|h| h.variant.is_some()).unwrap();
        assert!(r1.runtime_matches);
        assert_eq!(r1.model_override.as_deref(), Some("claude-opus-5"));
        assert!(
            r1.nodes
                .iter()
                .all(|n| n.model.as_deref() == Some("claude-opus-5"))
        );
        let r2 = hs.iter().find(|h| h.variant.is_none()).unwrap();
        assert_eq!(r2.source, "custom");
        assert_eq!(r2.nodes[0].model, None);
    }

    #[test]
    fn the_default_harness_is_recognised() {
        let dir = project();
        let root = dir.path();
        write(
            &root.join("runs/r1/harness/graph.json"),
            &json!({"root": {"kind": "step", "name": "solo", "worker": "w"}}),
        );
        write(
            &root.join("runs/r1/harness/runtime.json"),
            &json!({"nodes": {"solo": {"kind": "agent", "model": "claude-sonnet-5"}}}),
        );
        let hs = batch_harnesses(root, &[("r1".to_owned(), None)]);
        assert_eq!(hs[0].source, "default");
        assert!(hs[0].runtime_matches);
    }

    #[test]
    fn harness_files_are_listed_by_what_they_contain() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("harness/notes.json"), "[1,2]").unwrap();
        std::fs::write(root.join("harness/broken.json"), "{").unwrap();
        let files = list_harness_files(root);
        assert_eq!(
            files.graphs,
            ["harness/graph.json", "harness/variants/clone-v1/graph.json"]
        );
        assert_eq!(
            files.runtimes,
            [
                "harness/runtime.json",
                "harness/variants/clone-v1/runtime-haiku.json",
                "harness/variants/clone-v1/runtime.json"
            ]
        );
    }

    #[test]
    fn paths_outside_the_project_are_refused_without_naming_the_server_directory() {
        let dir = project();
        let root = dir.path();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("g.json"), "{}").unwrap();
        for raw in [
            "../g.json",
            "harness/../../g.json",
            outside.path().join("g.json").to_str().unwrap(),
        ] {
            let err = resolve_in_root(root, raw).unwrap_err();
            assert!(!err.contains(&root.display().to_string()), "{err}");
        }
        let err = resolve_in_root(root, "harness/nope.json").unwrap_err();
        assert_eq!(err, "File not found: harness/nope.json.");
        assert!(resolve_in_root(root, "harness/graph.json").is_ok());
        // An absolute path is fine as long as it is inside the project.
        assert!(resolve_in_root(root, root.join("harness/graph.json").to_str().unwrap()).is_ok());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("g.json"),
                root.join("harness/link.json"),
            )
            .unwrap();
            assert!(
                resolve_in_root(root, "harness/link.json")
                    .unwrap_err()
                    .contains("outside")
            );
        }
    }

    #[test]
    fn preview_checks_both_files_and_lists_the_nodes() {
        let dir = project();
        let root = dir.path();
        let ok = preview(
            root,
            "harness/variants/clone-v1/graph.json",
            "harness/variants/clone-v1/runtime.json",
            None,
        );
        assert!(ok.errors.is_empty(), "{:?}", ok.errors);
        let info = ok.info.unwrap();
        assert_eq!(info.variant.as_deref(), Some("clone-v1"));
        assert_eq!(info.nodes.len(), 4);

        let swapped = preview(root, "harness/runtime.json", "harness/graph.json", None);
        assert!(swapped.info.is_none());
        assert!(
            swapped.errors.iter().any(|e| e.contains("has no `root`")),
            "{:?}",
            swapped.errors
        );

        let mismatch = preview(
            root,
            "harness/variants/clone-v1/graph.json",
            "harness/runtime.json",
            Some("claude-opus-5"),
        );
        assert!(mismatch.errors.is_empty());
        assert!(
            mismatch
                .warnings
                .iter()
                .any(|w| w.contains("no model to: worker, acceptance, code, repair")),
            "{:?}",
            mismatch.warnings
        );
        assert!(
            mismatch.warnings.iter().any(|w| w.contains("solo")),
            "{:?}",
            mismatch.warnings
        );

        let missing = preview(root, "harness/nope.json", "", None);
        assert_eq!(
            missing.errors,
            [
                "graph: File not found: harness/nope.json.",
                "runtime: No file chosen."
            ]
        );
    }
}

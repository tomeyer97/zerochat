//! The graph's nodes, flattened from the admitted graph definition, with each node's runtime
//! binding (model, effort) and lane (harness, provider).

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::model::{Execution, Lane};

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub name: String,
    pub kind: String,
    pub parent: Option<String>,
    /// Enclosing loops, outermost first.
    pub loops: Vec<String>,
    pub worker: Option<String>,
    pub instructions: Option<String>,
    pub attempts: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub max_iterations: Option<u64>,
    pub fail_reason: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The node's lane (`lane_field`): the agent CLI and the provider it runs on.
    pub harness: Option<String>,
    pub provider: Option<String>,
}

/// One field (`harness` or `provider`) of a node's lane, as Zeroshot 16.1 records it:
/// `runtime.nodes.<node>.lane.<key>`, else `legacy.<key>` (the older patched Zeroshot's
/// per-execution `provider` record; `Value::Null` when there is none), else the run's default
/// lane `runtime.<key>`. A value zerochat does not know is kept as it is; none at all is `None`.
pub(super) fn lane_field(runtime: &Value, node: &str, legacy: &Value, key: &str) -> Option<String> {
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    text(&runtime["nodes"][node]["lane"][key])
        .or_else(|| text(&legacy[key]))
        .or_else(|| text(&runtime[key]))
}

/// The lanes the run's agents ran on, in graph order, each with its nodes: an agent node's
/// latest execution's lane, else the one its runtime binding gives (a node that has not run).
/// Agent nodes are the ones the runtime binds (`runtime.nodes.<n>`) or that ran.
pub(super) fn lanes(
    graph_nodes: &[GraphNode],
    runtime: &Value,
    executions: &[Execution],
) -> Vec<Lane> {
    let mut lanes: Vec<Lane> = Vec::new();
    for node in graph_nodes {
        let latest = executions.iter().rev().find(|e| e.node == node.name);
        if latest.is_none() && !runtime["nodes"][&node.name].is_object() {
            continue;
        }
        let lane = match latest {
            Some(e) => Lane {
                harness: e.harness.clone(),
                provider: e.provider.clone(),
                model: e.model.clone(),
                nodes: Vec::new(),
            },
            None => Lane {
                harness: node.harness.clone(),
                provider: node.provider.clone(),
                model: node.model.clone(),
                nodes: Vec::new(),
            },
        };
        let same = |l: &&mut Lane| {
            l.harness == lane.harness && l.provider == lane.provider && l.model == lane.model
        };
        match lanes.iter_mut().find(same) {
            Some(l) => l.nodes.push(node.name.clone()),
            None => lanes.push(Lane {
                nodes: vec![node.name.clone()],
                ..lane
            }),
        }
    }
    lanes
}

/// Every node of the graph, parents before their children.
pub fn graph_nodes(graph: &Value, runtime: &Value) -> Vec<GraphNode> {
    let mut out = Vec::new();
    walk(&graph["root"], None, &mut Vec::new(), runtime, &mut out);
    out
}

fn walk(
    node: &Value,
    parent: Option<&str>,
    loops: &mut Vec<String>,
    runtime: &Value,
    out: &mut Vec<GraphNode>,
) {
    let Some(kind) = node.get("kind").and_then(Value::as_str) else {
        return;
    };
    let name = node
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let binding = &runtime["nodes"][&name];
    let text = |value: &Value| value.as_str().map(str::to_owned);
    out.push(GraphNode {
        name: name.clone(),
        kind: kind.to_owned(),
        parent: parent.map(str::to_owned),
        loops: loops.clone(),
        worker: text(&node["worker"]),
        instructions: text(&node["instructions"]),
        attempts: node["attempts"].as_u64(),
        timeout_ms: node["timeoutMs"].as_u64(),
        max_iterations: node["maxIterations"].as_u64(),
        fail_reason: text(&node["reason"]),
        model: text(&binding["model"]),
        effort: text(&binding["effort"]),
        harness: lane_field(runtime, &name, &Value::Null, "harness"),
        provider: lane_field(runtime, &name, &Value::Null, "provider"),
    });
    let is_loop = kind == "loop";
    if is_loop {
        loops.push(name.clone());
    }
    for child in node["children"].as_array().into_iter().flatten() {
        walk(child, Some(&name), loops, runtime, out);
    }
    for branch in node["branches"].as_array().into_iter().flatten() {
        let child = if branch.get("node").is_some() {
            &branch["node"]
        } else {
            branch
        };
        walk(child, Some(&name), loops, runtime, out);
    }
    for key in ["body", "otherwise"] {
        if node[key].is_object() {
            walk(&node[key], Some(&name), loops, runtime, out);
        }
    }
    if is_loop {
        loops.pop();
    }
}

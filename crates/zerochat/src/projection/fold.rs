//! The event fold: `Projector` takes in a run's ledger events one by one, with one handler per
//! event kind. What is still open when the last event is in (states, costs, the failure) is
//! settled by `Projector::finish` (finish.rs).

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::background;
use super::clock::{Clock, log_time};
use super::executions::{LoggedLine, StartedRef};
use super::graph::{GraphNode, graph_nodes, lane_field};
use super::model::{BackgroundAgent, CostSource, ExecState, Execution, Features};
use super::pressures::{ApiLine, Pressures};
use super::run_log::{self, RunLog};
use crate::pricing::Usage;
use crate::source::{Event, RunData};
use crate::transcript::{steer_receipts, tool_line};

/// The state of the fold: everything the events have said so far.
pub(super) struct Projector {
    pub(super) graph: Value,
    pub(super) runtime: Value,
    pub(super) graph_nodes: Vec<GraphNode>,
    /// Index into `graph_nodes` by node name.
    node_index: HashMap<String, usize>,
    pub(super) clock: Clock,
    /// The execution each steer receipt belongs to, by the receipt's ledger position.
    receipts: HashMap<u64, u64>,

    pub(super) executions: Vec<Execution>,
    /// Index into `executions` by execution id.
    by_id: HashMap<u64, usize>,
    /// Visits so far per (node, map indices): the round of the node's next first attempt.
    visits: HashMap<(String, String), u64>,

    pub(super) run_log: RunLog,
    pub(super) pressures: Pressures,
    pub(super) background: BTreeMap<String, BackgroundAgent>,
    /// Tokens of the agents outside the graph (run-level usage).
    pub(super) background_usage: Usage,
    pub(super) features: Features,

    /// Zeroshot's terminal record and its ledger position, once the run ended.
    pub(super) terminal: Value,
    pub(super) terminal_seq: Option<u64>,
    /// The ledger position of the run's outcome: the graph ending (`primary_graph_completed`),
    /// before Zeroshot's background agents run and it closes the run (`terminal`).
    pub(super) outcome_seq: Option<u64>,
    pub(super) force_stop_seq: Option<u64>,
    /// The first and last log-line times.
    pub(super) first_ts: Option<u64>,
    pub(super) last_ts: Option<u64>,
    pub(super) replay_of: Option<Value>,
}

impl Projector {
    pub(super) fn new(data: &RunData) -> Self {
        let admitted = &data.admitted;
        let graph = admitted["graph"].clone();
        let runtime = admitted["runtime"].clone();
        let graph_nodes = graph_nodes(&graph, &runtime);
        let node_index = graph_nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        Projector {
            graph,
            runtime,
            graph_nodes,
            node_index,
            clock: Clock::new(&data.events),
            receipts: steer_receipts(&data.events),
            executions: Vec::new(),
            by_id: HashMap::new(),
            visits: HashMap::new(),
            run_log: RunLog::default(),
            pressures: Pressures::default(),
            background: BTreeMap::new(),
            background_usage: Usage::default(),
            features: Features::default(),
            terminal: Value::Null,
            terminal_seq: None,
            outcome_seq: None,
            force_stop_seq: None,
            first_ts: None,
            last_ts: None,
            replay_of: admitted.get("replayOf").cloned(),
        }
    }

    /// Takes in one event. Lineage (`replayOf`) is read from every event the fold took in; one
    /// it skipped (it names no known execution, or says nothing to show) is skipped whole.
    pub(super) fn feed(&mut self, event: &Event) {
        let body = &event.body;
        if let Some(ts) = log_time(body) {
            self.first_ts.get_or_insert(ts);
            self.last_ts = Some(ts);
        }
        let taken = match body["kind"].as_str().unwrap_or("") {
            "node_started" | "prior_execution" => self.on_started(event),
            "node_completed" => self.on_completed(event),
            "execution_voided" => self.on_voided(event),
            "safe_log" => self.on_log(event),
            "token_usage_observed" => self.on_token_usage(event),
            "vector" => self.on_vector(event),
            "suspend_requested" => self.on_suspend_requested(event),
            "suspended" => self.on_suspended(event),
            "request_deferred" | "wait_deferred" => self.on_deferred(event),
            "run_token_usage_observed" => self.on_run_token_usage(event),
            "force_stop_requested" => {
                self.force_stop_seq = Some(event.seq);
                Some(())
            }
            "primary_graph_completed" => {
                if self.terminal.is_null() {
                    self.end(event);
                }
                Some(())
            }
            "terminal" => {
                self.end(event);
                Some(())
            }
            // Bookkeeping for resumes and delivery (`node_instances_reserved`, `delivery_state`),
            // and anything else: nothing a person reads.
            _ => Some(()),
        };
        if taken.is_some()
            && let Some(r) = body.get("replayOf")
        {
            self.replay_of = Some(r.clone());
        }
    }

    /// The graph node an execution runs.
    fn node(&self, name: &str) -> Option<&GraphNode> {
        self.node_index.get(name).map(|&i| &self.graph_nodes[i])
    }

    /// The index in `executions` of the execution an event refers to.
    fn index_of(&self, execution: &Value) -> Option<usize> {
        execution
            .as_u64()
            .and_then(|id| self.by_id.get(&id).copied())
    }

    /// 1-based visit number of a starting execution: a first attempt starts a new visit of its
    /// node (per map index); a retry belongs to the latest visit.
    fn round_of(&mut self, start: &StartedRef) -> u64 {
        let key = (start.node.clone(), start.map_indices.to_string());
        if start.attempt <= 1 {
            let n = self.visits.entry(key).or_insert(0);
            *n += 1;
            *n
        } else {
            *self.visits.get(&key).unwrap_or(&1)
        }
    }

    /// `node_started` (an execution runs here) or `prior_execution` (it ran in the original run
    /// and is reused).
    fn on_started(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let start = StartedRef::from_event(body)?;
        let id = start.id;
        let round = self.round_of(&start);
        let exact = body["at"].as_u64();
        self.features.exact_times |= exact.is_some();
        // Without exact times: a node starts right after whatever happened before it, except
        // the first one, which waits for the environment to be prepared.
        let first_to_run = self.executions.iter().all(|e| e.reused);
        let started_at = exact.or_else(|| {
            if first_to_run {
                self.clock.next(event.seq)
            } else {
                self.clock.at(event.seq)
            }
        });
        let info = self.node(&start.node);
        // The older patched Zeroshot recorded the lane it ran each execution on (`provider`);
        // Zeroshot 16.1 binds it per node (graph.rs `lane_field`).
        let legacy = &body["provider"];
        let model = legacy["model"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| info.and_then(|i| i.model.clone()));
        let harness = lane_field(&self.runtime, &start.node, legacy, "harness");
        let provider = lane_field(&self.runtime, &start.node, legacy, "provider");

        let mut execution = Execution::started(start, round, event.seq);
        execution.started_at = started_at;
        execution.model_turn_since = started_at;
        execution.times_approximate = exact.is_none();
        execution.model = model;
        execution.harness = harness;
        execution.provider = provider;
        execution.workspace_start = body.get("workspace").cloned();
        if execution.reused {
            execution.reuse(event.seq, &body["execution"]["state"]["Settled"]["outcome"]);
        }
        self.features.workspace_snapshots |= execution.workspace_start.is_some();
        // The run log is the run's timeline: a node starting is one of its facts, not only the
        // background agents Zeroshot runs outside the graph.
        if !execution.reused {
            self.run_log.note(
                event.seq,
                execution.started_at.unwrap_or(0),
                run_log::node_started(&execution),
            );
        }
        self.by_id.insert(id, self.executions.len());
        self.executions.push(execution);
        Some(())
    }

    fn on_completed(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let completion = &body["completion"];
        let i = self.index_of(&completion["reference"]["execution"])?;
        let exact = body["at"].as_u64().or_else(|| completion["at"].as_u64());
        let ended_at = exact.or_else(|| self.clock.at(event.seq));
        let e = &mut self.executions[i];
        e.end_seq = Some(event.seq);
        e.ended_at = ended_at;
        e.workspace_end = body
            .get("workspace")
            .or_else(|| completion.get("workspace"))
            .cloned();
        e.apply_outcome(&completion["outcome"]);
        self.run_log.note(
            event.seq,
            e.ended_at.unwrap_or(0),
            run_log::node_finished(e),
        );
        Some(())
    }

    fn on_voided(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let i = self.index_of(&body["reference"]["execution"])?;
        let ended_at = self.clock.at(event.seq);
        let e = &mut self.executions[i];
        e.state = ExecState::Voided;
        e.end_seq = Some(event.seq);
        e.ended_at = ended_at;
        e.void_reason = body["reason"].as_str().map(str::to_owned);
        Some(())
    }

    /// `safe_log`: a line of an execution counts for it; any other line is the run's (and maybe
    /// a background agent's). Either may say something about limits.
    fn on_log(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let text = body["line"].as_str().unwrap_or("");
        let kind = body["logKind"].as_str();
        let line = LoggedLine {
            at: log_time(body).unwrap_or(0),
            text,
            stream: body["stream"].as_str().unwrap_or("output"),
            kind,
            // Zeroshot 16.1 records tool calls as plain lines ("Claude tool call: …").
            tool: if kind.is_none() {
                tool_line(text)
            } else {
                None
            },
            diagnostic: body.get("diagnostic").is_some(),
        };
        self.features.tool_calls |= kind.is_some() || line.tool.is_some();
        let execution = self.index_of(&body["execution"]);
        match execution {
            Some(i) => {
                let e = &mut self.executions[i];
                e.note_log_line(&line);
                if line.shows_activity() {
                    self.pressures.clear_retry(e.id);
                }
            }
            None => {
                let (agent, rest) = background::split_agent(text);
                if let Some(name) = agent {
                    background::note_line(&mut self.background, name, line.at, line.stream, rest);
                }
                self.run_log
                    .logged(event.seq, line.at, line.stream, kind, agent, rest);
            }
        }
        let node = execution.map(|i| self.executions[i].node.clone());
        let api_line = ApiLine {
            text,
            stream: line.stream,
            kind,
            at: line.at,
        };
        self.pressures
            .observe(&api_line, body["execution"].as_u64(), node);
        Some(())
    }

    /// A model call answered: its tokens (and, from the older patched Zeroshot, its cost and
    /// model).
    fn on_token_usage(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let i = self.index_of(&body["execution"])?;
        let e = &mut self.executions[i];
        let delta = Usage::from_harness_json(&body["usage"], e.harness.as_deref());
        e.usage.add(&delta);
        let cost = body["costUsd"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| body["costUsd"].as_f64());
        if let Some(cost) = cost {
            self.features.provider_cost = true;
            *e.cost_usd.get_or_insert(0.0) += cost;
            e.cost_source = Some(CostSource::Provider);
        }
        if let Some(model) = body["model"].as_str() {
            e.model = Some(model.to_owned());
        }
        self.pressures.clear_retry(e.id);
        Some(())
    }

    /// A person's message to the run's agents (Zeroshot 16.1), and each agent taking it in.
    fn on_vector(&mut self, event: &Event) -> Option<()> {
        let v = &event.body["vector"];
        let at = v["createdAt"]
            .as_u64()
            .or_else(|| self.clock.at(event.seq))
            .unwrap_or(0);
        let text = match v["kind"].as_str() {
            Some("steer") if v["network"].is_null() => run_log::steer_sent(v),
            Some("steer_receipt") => {
                let i = self
                    .receipts
                    .get(&event.seq)
                    .and_then(|id| self.by_id.get(id));
                run_log::steer_received(i.map(|&i| self.executions[i].node.as_str()))
            }
            _ => return None,
        };
        self.run_log.note(event.seq, at, text);
        Some(())
    }

    // Zeroshot 16.1 suspends a run at a checkpoint (`zeroshot suspend`, a request node waiting
    // for an answer, a timed wait); `resume`/`answer` continue it as a successor.

    fn on_suspend_requested(&mut self, event: &Event) -> Option<()> {
        let at = self.clock.at(event.seq).unwrap_or(0);
        self.run_log
            .note(event.seq, at, run_log::SUSPEND_REQUESTED.to_owned());
        Some(())
    }

    fn on_suspended(&mut self, event: &Event) -> Option<()> {
        let at = self.clock.at(event.seq).unwrap_or(0);
        self.run_log
            .note(event.seq, at, run_log::suspended(&event.body["suspension"]));
        Some(())
    }

    fn on_deferred(&mut self, event: &Event) -> Option<()> {
        let body = &event.body;
        let i = self.index_of(&body["reference"]["execution"])?;
        let text = run_log::deferred(&self.executions[i].node, body["kind"] == "request_deferred");
        let at = self.clock.at(event.seq).unwrap_or(0);
        self.run_log.note(event.seq, at, text);
        Some(())
    }

    fn on_run_token_usage(&mut self, event: &Event) -> Option<()> {
        let usage =
            Usage::from_harness_json(&event.body["usage"], self.runtime["harness"].as_str());
        self.background_usage.add(&usage);
        Some(())
    }

    /// Zeroshot's result for the run: `terminal`, or `primary_graph_completed` when the graph
    /// ended before Zeroshot closed the run.
    fn end(&mut self, event: &Event) {
        self.terminal = event.body["result"].clone();
        self.terminal_seq = Some(event.seq);
        self.outcome_seq.get_or_insert(event.seq);
    }
}

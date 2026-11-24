//! The end of the fold: once every event is in, decide the run's status, settle each
//! execution, sum the totals, pick the pressures that still press, classify the failure and put
//! the run log in order.

use std::collections::HashMap;

use super::background;
use super::clock::event_time;
use super::executions::{self, RunEnd};
use super::failure::{Facts, classify};
use super::fold::Projector;
use super::graph::{self, GraphNode};
use super::model::RunView;
use super::status::{phase, reason, run_status};
use super::totals;
use crate::engine_lines::is_closing_line;
use crate::source::RunData;

/// The run log keeps this many lines (the latest).
const MAX_RUN_LOG_LINES: usize = 400;

impl Projector {
    pub(super) fn finish(mut self, target: &str, data: &RunData, now_ms: u64) -> RunView {
        if self.terminal.is_null()
            && let Some(t) = data.snapshot.get("terminal").filter(|t| !t.is_null())
        {
            self.terminal = t.clone();
        }

        let phase = data.snapshot["phase"].as_str().unwrap_or("").to_owned();
        let local = data.local.as_ref();
        let controller_dead = local.is_some_and(|l| !l.controller_liveness.is_alive());
        // A suspended run (16.1) has no live agent left: it continues only as a successor.
        let finished =
            !self.terminal.is_null() || phase == phase::FINISHED || phase == phase::SUSPENDED;
        let status = run_status(&self.terminal, &phase, controller_dead);
        // `zeroshot resume` closes a run whose controller died as `runtime_lost` when it starts
        // the successor; show the run as it was (controller lost when it was last active), not
        // as lost at the moment someone resumed it.
        let resumed_as = local.and_then(|l| l.resumed_as.clone());
        let lost_before_resume =
            resumed_as.is_some() && self.terminal["reason"] == reason::RUNTIME_LOST;

        // The run a replay comes from; a resumed run's parent otherwise.
        let replay_parent = self
            .replay_of
            .as_ref()
            .and_then(|r| r["runId"].as_str())
            .map(str::to_owned);
        let run_end = RunEnd {
            finished,
            stalled: status.is_stalled(),
            stopped: status.is_stopped(),
            lost: self.terminal["reason"] == reason::RUNTIME_LOST,
            force_stop_seq: self.force_stop_seq,
            live_usage: &data.live_usage,
            reused_from: replay_parent
                .clone()
                .or_else(|| local.and_then(|l| l.resumed_from.clone())),
            now_ms,
        };
        for e in &mut self.executions {
            executions::settle(e, &run_end);
        }

        let nodes = totals::node_stats(&self.executions);
        let lanes = graph::lanes(&self.graph_nodes, &self.runtime, &self.executions);
        let (node_usage, cost) = totals::node_totals(&self.executions);
        let mut usage = node_usage;
        usage.add(&self.background_usage);

        let node_info: HashMap<&str, &GraphNode> = self
            .graph_nodes
            .iter()
            .map(|n| (n.name.as_str(), n))
            .collect();
        let live = !finished && !status.is_stalled();
        let pressures = self
            .pressures
            .finish(live, &self.executions, &node_info, now_ms);

        let started_at = data
            .events
            .first()
            .and_then(|e| self.clock.at(e.seq))
            .or(self.first_ts);
        // When Zeroshot closed the run (after its background agents, such as a postmortem).
        let mut closed_at = self
            .terminal_seq
            .and_then(|s| self.clock.at(s))
            .or(if finished { self.last_ts } else { None });
        if lost_before_resume {
            closed_at = last_activity_before(data, self.terminal_seq).or(closed_at);
        }
        // The run's duration ends at its outcome: the graph ending, not Zeroshot closing the run
        // afterwards. The run list and the run header both count to here.
        let ended_at = if lost_before_resume {
            closed_at
        } else {
            self.outcome_seq
                .and_then(|s| self.clock.at(s))
                .or(closed_at)
        };
        let run_log = &mut self.run_log.lines;
        let postmortem = run_log
            .iter()
            .any(|l| l.text.starts_with(background::POSTMORTEM_PREFIX));
        let failure = classify(&Facts {
            runtime: &self.runtime,
            phase: &phase,
            status: status.as_str(),
            reason: self.terminal["reason"].as_str().unwrap_or(""),
            terminal: &self.terminal,
            executions: &self.executions,
            nodes: &node_info,
            started_at,
            ended_at: closed_at.or(self.last_ts),
            run_log,
            local,
            lost_before_resume,
            postmortem,
            now_ms,
        });

        // Background agents interleave; show the log in time order (joined lines keep their
        // first time).
        run_log.sort_by_key(|l| l.at);
        background::settle(&mut self.background, run_log);
        if run_log.len() > MAX_RUN_LOG_LINES {
            run_log.drain(..run_log.len() - MAX_RUN_LOG_LINES);
        }

        // A replay's lineage is its `replayOf`; Zeroshot also records it as `resumedFrom`, and
        // the original lists the replay as its successor. Name each link once.
        let replays = local.map(|l| l.replays.clone()).unwrap_or_default();
        let resumed_from = local
            .and_then(|l| l.resumed_from.clone())
            .filter(|id| Some(id) != replay_parent.as_ref());
        let resumed_as = resumed_as.filter(|id| !replays.contains(id));

        RunView {
            target: target.to_owned(),
            run_id: data.run_id.clone(),
            title: data.title.clone(),
            phase,
            status: status.as_str().to_owned(),
            created_at: crate::source::uuid_v7_millis(&data.run_id),
            started_at,
            ended_at,
            last_activity_at: self.last_ts,
            terminal: self.terminal,
            graph: self.graph,
            runtime: self.runtime,
            initial_input: data.admitted["initialInput"].clone(),
            source: data.admitted["source"].clone(),
            graph_nodes: self.graph_nodes,
            lanes,
            nodes,
            usage_live: totals::usage_live(&self.executions),
            usage_pending: totals::usage_pending(&self.executions),
            executions: self.executions,
            usage,
            cost_usd: cost.usd,
            cost_estimated: cost.estimated,
            failure,
            pressures,
            run_log: self.run_log.lines,
            background_agents: self.background.into_values().collect(),
            node_usage,
            background_usage: self.background_usage,
            cursor: data.events.last().map(|e| e.seq).unwrap_or(0),
            features: self.features,
            local: data.local.clone(),
            replay_of: self.replay_of,
            resumed_from,
            resumed_as,
            replays,
        }
    }
}

/// When the run was last active before Zeroshot closed it: the latest event before Zeroshot's
/// first closing line (`Run failed …`, `Node … failed: crash: run runtime_lost`). What follows
/// that line is the closing itself, stamped with the time Zeroshot wrote it.
fn last_activity_before(data: &RunData, terminal_seq: Option<u64>) -> Option<u64> {
    let terminal = terminal_seq?;
    let closing = |e: &&crate::source::Event| {
        e.body["kind"] == "safe_log" && e.body["line"].as_str().is_some_and(is_closing_line)
    };
    let end = data
        .events
        .iter()
        .filter(|e| e.seq < terminal)
        .find(closing)
        .map_or(terminal, |e| e.seq);
    data.events
        .iter()
        .filter(|e| e.seq < end)
        .filter_map(event_time)
        .max()
}

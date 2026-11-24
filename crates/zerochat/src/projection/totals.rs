//! Sums over executions: per node and for the run.

use std::collections::BTreeMap;

use super::model::{CostSource, ExecState, Execution, NodeStats, UsageSource};
use crate::pricing::Usage;

/// Dollars summed over executions, and whether part of them is estimated (shown with `~`).
/// `None` until an execution with a cost is added.
#[derive(Default)]
pub(super) struct CostTotal {
    pub(super) usd: Option<f64>,
    pub(super) estimated: bool,
}

impl CostTotal {
    pub(super) fn add(&mut self, e: &Execution) {
        add_cost(&mut self.usd, &mut self.estimated, e);
    }
}

/// Adds an execution's cost, if it has one, to a total and its "estimated" flag.
fn add_cost(usd: &mut Option<f64>, estimated: &mut bool, e: &Execution) {
    if let Some(cost) = e.cost_usd {
        *usd.get_or_insert(0.0) += cost;
        *estimated |= e.cost_source == Some(CostSource::Estimate);
    }
}

impl NodeStats {
    /// Adds the node's next execution; the latest one decides the node's state.
    fn add(&mut self, e: &Execution) {
        self.executions.push(e.id);
        self.rounds = self.rounds.max(e.round);
        self.usage.add(&e.usage);
        add_cost(&mut self.cost_usd, &mut self.cost_estimated, e);
        self.elapsed_ms += e.duration_ms.unwrap_or(0);
        self.state = e.state;
        self.active_since = (e.state == ExecState::Running)
            .then_some(e.started_at)
            .flatten();
    }
}

/// Each node's stats, by node name.
pub(super) fn node_stats(executions: &[Execution]) -> BTreeMap<String, NodeStats> {
    let mut nodes: BTreeMap<String, NodeStats> = BTreeMap::new();
    for e in executions {
        nodes.entry(e.node.clone()).or_default().add(e);
    }
    nodes
}

/// The graph's nodes together: their tokens and their cost.
pub(super) fn node_totals(executions: &[Execution]) -> (Usage, CostTotal) {
    let mut usage = Usage::default();
    let mut cost = CostTotal::default();
    for e in executions {
        usage.add(&e.usage);
        cost.add(e);
    }
    (usage, cost)
}

/// Some of the usage is live from a provider's session log, for an execution that is (or was
/// until the run stopped) still working.
pub(super) fn usage_live(executions: &[Execution]) -> bool {
    executions.iter().any(|e| {
        e.usage_source == UsageSource::SessionLog
            && matches!(e.state, ExecState::Running | ExecState::Interrupted)
    })
}

/// A running execution's usage is not known yet.
pub(super) fn usage_pending(executions: &[Execution]) -> bool {
    executions
        .iter()
        .any(|e| e.usage_source == UsageSource::Pending)
}

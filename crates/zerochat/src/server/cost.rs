//! Whose cost a benchmark run shows.
//!
//! A pb run's cost is pb's own figure (`runs.cost_usd`: pb's pricing table, the number its batch
//! page, budget cap and compare use), not Zeroshot's estimate from the same tokens. Zeroshot's
//! figure is kept aside (`zeroshotCostUsd`) for the tooltip. Until pb records the cost (when the
//! agent finishes), Zeroshot's estimate stays the run's cost.

use serde::Serialize;
use serde_json::{Value, json};

use crate::context::RunContext;

/// pb's cost of a benchmark run, once pb has recorded it.
pub(crate) fn pb_cost(context: &RunContext) -> Option<f64> {
    context.cost_usd.filter(|_| context.kind == "benchmark")
}

/// Zeroshot's own cost figure, kept next to pb's.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ZeroshotCost {
    pub(crate) zeroshot_cost_usd: Option<f64>,
    pub(crate) zeroshot_cost_estimated: bool,
    pub(crate) cost_source: &'static str,
}

/// [`pb_cost`] applied to a serialised run view: `costUsd`/`costEstimated` become pb's, and
/// Zeroshot's move to `zeroshotCostUsd`/`zeroshotCostEstimated`.
pub(crate) fn benchmark_cost(obj: &mut Value, context: &RunContext) {
    let Some(pb_cost) = pb_cost(context) else {
        return;
    };
    if !obj.is_object() {
        return;
    }
    obj["zeroshotCostUsd"] = obj.get("costUsd").cloned().unwrap_or(Value::Null);
    obj["zeroshotCostEstimated"] = obj
        .get("costEstimated")
        .cloned()
        .unwrap_or(Value::Bool(true));
    obj["costUsd"] = json!(pb_cost);
    obj["costEstimated"] = json!(false);
    obj["costSource"] = json!("pb");
}

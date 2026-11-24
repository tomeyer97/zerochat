//! Runs of configured sources (pb, zsf): whose cost a run shows, and whose controller this machine can see.

use serde_json::json;

use crate::context::RunContext;
use crate::discovery::{Target, TargetKind};
use crate::server::cost::benchmark_cost;
use crate::server::run_store::controller_on_this_host;

use super::observe_target;

fn pb_context(cost_usd: Option<f64>) -> RunContext {
    RunContext {
        kind: "benchmark",
        parent: None,
        label: None,
        task: None,
        seed: None,
        score: None,
        cost_usd,
        feature: None,
        slice: None,
        attempt: None,
        line: String::new(),
    }
}

#[test]
fn a_pb_runs_cost_is_pbs_own_with_zeroshots_estimate_kept_aside() {
    let mut view = json!({"costUsd": 1.256, "costEstimated": true});
    benchmark_cost(&mut view, &pb_context(Some(1.004)));
    assert_eq!(view["costUsd"], 1.004);
    assert_eq!(view["costEstimated"], false);
    assert_eq!(view["costSource"], "pb");
    assert_eq!(view["zeroshotCostUsd"], 1.256);
    assert_eq!(view["zeroshotCostEstimated"], true);

    // Not recorded by pb yet (the agent is still working): Zeroshot's estimate stays.
    let mut running = json!({"costUsd": 0.5, "costEstimated": true});
    benchmark_cost(&mut running, &pb_context(None));
    assert_eq!(running, json!({"costUsd": 0.5, "costEstimated": true}));
    // A zsf slice's context carries no cost and changes nothing.
    let mut slice = json!({"costUsd": 0.5});
    benchmark_cost(
        &mut slice,
        &RunContext {
            kind: "slice",
            ..pb_context(Some(9.0))
        },
    );
    assert_eq!(slice, json!({"costUsd": 0.5}));
}

#[test]
fn only_configured_sources_have_controllers_this_machine_cannot_see() {
    assert!(
        !controller_on_this_host(&observe_target(false)),
        "pb runs Zeroshot inside its Docker sandbox"
    );
    let zsf = Target {
        id: "zsf".into(),
        ..observe_target(false)
    };
    assert!(
        controller_on_this_host(&zsf),
        "zsf starts its runs on this machine"
    );
    let local = Target {
        id: "local".into(),
        kind: TargetKind::Local,
        ..observe_target(false)
    };
    assert!(controller_on_this_host(&local));
}

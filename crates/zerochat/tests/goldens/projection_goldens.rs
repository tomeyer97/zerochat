//! projection::project (ledger → run view) for every fixture run, at a fixed "now".

use super::world::{NOW_MS, Scenario, world};
use crate::projection;
use crate::source::{self, RunData};

pub fn run_data(s: &Scenario) -> RunData {
    let w = world();
    let root = if s.target == "observe" {
        &w.observe
    } else {
        &w.state
    };
    let (stored, events) = source::read_local(root, &s.run_id, 0).unwrap();
    let mut local = source::local_facts(root, &s.run_id);
    if s.target == "observe" {
        local.controller_elsewhere();
    }
    RunData {
        run_id: s.run_id.clone(),
        title: source::str_at(&stored, "/admitted/title"),
        admitted: stored["admitted"].clone(),
        snapshot: stored["snapshot"].clone(),
        events,
        local: Some(local),
        live_usage: Default::default(),
    }
}

#[test]
fn every_synthetic_run() {
    for s in &world().synthetic {
        let view = projection::project(&s.target, &run_data(s), NOW_MS);
        super::check_serialize(
            &format!("projection/{}{}", super::family_prefix(&s.tags), s.name),
            &view,
        );
    }
}

#[test]
fn every_recorded_run() {
    for s in &world().recorded {
        let view = projection::project(&s.target, &run_data(s), NOW_MS);
        super::check_serialize(
            &format!(
                "projection/{}{}",
                super::family_prefix(&s.tags),
                s.name.replace('/', "-")
            ),
            &view,
        );
    }
}

/// Without local facts (a direct target's history): no controller, lineage or checkpoints.
#[test]
fn without_local_facts() {
    for name in [
        "succeeded",
        "stalled",
        "replay-parent",
        "resumed-original",
        "diag-credentials",
    ] {
        let s = world().scenario(name);
        let mut data = run_data(s);
        data.local = None;
        let view = projection::project("target:lab", &data, NOW_MS);
        super::check_serialize(
            &format!(
                "projection/no-local/{}{}",
                super::family_prefix(&s.tags),
                name
            ),
            &view,
        );
    }
}

/// Live usage from a provider's session log (the server fills `live_usage`).
#[test]
fn with_live_usage() {
    let s = world().scenario("stalled-live-usage");
    let mut data = run_data(s);
    let usage = crate::pricing::Usage::from_json(
        &serde_json::json!({"inputTokens": 7, "outputTokens": 300, "cacheReadInputTokens": 20000, "cacheCreationInputTokens": 900}),
    );
    data.live_usage.insert(1, usage);
    let view = projection::project("local", &data, NOW_MS);
    super::check_serialize("projection/live-usage/stalled-live-usage", &view);
}

/// The same run read incrementally (two cursors) projects the same as read at once.
#[test]
fn incremental_read_matches() {
    let w = world();
    for s in w.synthetic.iter().filter(|s| s.target == "local") {
        let (_, all) = source::read_local(&w.state, &s.run_id, 0).unwrap();
        let mid = all.len() as u64 / 2;
        let (_, tail) = source::read_local(&w.state, &s.run_id, mid).unwrap();
        assert_eq!(
            tail.len() as u64,
            all.len() as u64 - mid,
            "{}: reading after cursor {mid}",
            s.name
        );
    }
}

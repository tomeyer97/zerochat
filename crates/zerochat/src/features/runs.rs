//! The Zeroshot runs behind slice attempts: where each one lives and what the run page's
//! projection says about it (status, cost, times), cached while its ledger is unchanged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;

use crate::projection;
use crate::source::{self, RunData};

/// Where a run can be read: `(zerochat target id, state root)`, in lookup order.
pub type Roots = Vec<(String, PathBuf)>;

/// The target whose state root holds `run_id`.
pub fn locate(roots: &Roots, run_id: &str) -> Option<(String, PathBuf)> {
    if !crate::validate::is_hex_id(run_id) {
        return None;
    }
    roots
        .iter()
        .find(|(_, root)| crate::paths::ledger_path(root, run_id).is_file())
        .cloned()
}

/// What a slice needs to know about one of its runs: where it is, its status and its cost
/// (the same projection as the run page).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunFacts {
    pub target: String,
    pub status: String,
    pub cost_usd: Option<f64>,
    pub cost_estimated: bool,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    /// The graph ended but Zeroshot is still finishing the run (its postmortem): the run's phase
    /// is not `finished` yet, which is what zsf waits for.
    pub closing: bool,
    /// The run's last event: after the postmortem, about when Zeroshot finished it wholly.
    pub last_activity_at: Option<u64>,
}

impl RunFacts {
    /// A run whose ledger cannot be read: status `unknown`, no cost (and so an estimate).
    fn unreadable(target: &str) -> RunFacts {
        RunFacts {
            target: target.to_owned(),
            status: "unknown".to_owned(),
            cost_usd: None,
            cost_estimated: true,
            started_at: None,
            ended_at: None,
            closing: false,
            last_activity_at: None,
        }
    }
}

/// Reads the run's ledger and projects it as the run page does.
fn load_run_facts(target: &str, root: &std::path::Path, run_id: &str, now: u64) -> RunFacts {
    let Ok((stored, events)) = source::read_local(root, run_id, 0) else {
        return RunFacts::unreadable(target);
    };
    let data = RunData {
        run_id: run_id.to_owned(),
        title: source::str_at(&stored, "/admitted/title"),
        admitted: stored["admitted"].clone(),
        snapshot: stored["snapshot"].clone(),
        events,
        local: Some(source::local_facts(root, run_id)),
        ..Default::default()
    };
    let view = projection::project(target, &data, now);
    let closing = view.phase == "running" && view.ended_at.is_some() && view.status != "stalled";
    RunFacts {
        target: target.to_owned(),
        status: view.status,
        cost_usd: view.cost_usd,
        cost_estimated: view.cost_estimated,
        started_at: view.started_at.or(view.created_at),
        ended_at: view.ended_at,
        closing,
        last_activity_at: view.last_activity_at,
    }
}

/// The cache is emptied when it holds more runs than this.
const MAX_CACHED_RUNS: usize = 1024;

type RunKey = (PathBuf, String);
/// The ledger files' sizes and modification times ([`source::ledger_stamp`]).
type Stamp = Vec<(u64, u128)>;

/// Run facts by run, valid while the run's ledger files are unchanged.
#[derive(Default)]
pub struct RunFactsCache {
    entries: Mutex<HashMap<RunKey, (Stamp, RunFacts)>>,
}

impl RunFactsCache {
    pub fn get(&self, roots: &Roots, run_id: &str, now: u64) -> Option<RunFacts> {
        let (target, root) = locate(roots, run_id)?;
        let stamp = source::ledger_stamp(&root, run_id);
        let key = (root.clone(), run_id.to_owned());
        if let Some((cached, facts)) = self.entries.lock().ok()?.get(&key)
            && *cached == stamp
        {
            return Some(facts.clone());
        }
        let facts = load_run_facts(&target, &root, run_id, now);
        let mut entries = self.entries.lock().ok()?;
        if entries.len() > MAX_CACHED_RUNS {
            entries.clear();
        }
        entries.insert(key, (stamp, facts.clone()));
        Some(facts)
    }
}

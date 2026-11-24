//! The Benchmarks page's data, read from pb's database (`<pb_root>/pb.sqlite`, opened read-only
//! through `pb_db`). pb, the "ProgramBench runner", is a separate benchmarking tool: a
//! ProgramBench loop for Zeroshot graphs. It runs a harness (a Zeroshot graph and runtime) on
//! benchmark tasks, several seeds each, and scores each run's code with hidden tests. In this
//! module `pb` always means that tool.
//!
//! - a **run** is one harness run on one (task, seed): a `runs` row (`run`);
//! - a **batch** is the runs one `pb run` started: a `batches` row. An older pb database has no
//!   `batches` table; its runs (and, on a newer one, runs with no `batch_id`) are grouped by
//!   `label` into pseudo-batches, `id = "label:<label>"`, the way `pb ls` groups them (`batch`);
//! - the batches list (`list`), one batch's task × seed matrix (`matrix`, `detail`, `replays`),
//!   batches pb has not recorded yet and live cells (`live`, `live_spend` priced by `pb_pricing`),
//!   one run's score card (`score`), two batches compared (`compare`), sweeps (`sweeps`), and the
//!   Start form's options from pb.toml (`harness_options`).
//!
//! pb's schema has versions: every query tolerates columns a database does not have yet
//! (`SELECT *` read with `db::get_opt`), so this reads old and new databases without a
//! migration.

mod batch;
mod compare;
mod db;
mod detail;
mod harness_options;
mod list;
mod lists;
mod live;
mod live_spend;
mod matrix;
mod pb_pricing;
mod replays;
mod run;
mod score;
mod sweeps;
#[cfg(test)]
mod tests;

pub use batch::running_batches_alive;
pub use compare::compare_batches;
pub use detail::batch_detail;
pub use harness_options::harness_options;
pub use list::list_batches;
pub use live::{StartedHere, fill_live_cells, pending_batch};
pub use run::{pb_run_ended, pb_run_of_state_root};
pub use score::run_score;
pub use sweeps::list_sweeps;

// The API's serialised types. Outside tests only `api_types` (the TypeScript generator) names
// them; the server serialises them without naming them.
#[allow(unused_imports)]
pub use self::{
    batch::BatchStatus,
    compare::{BatchCompare, NodeAgg, NodeCompareRow, Noise, TaskAgg, TaskCompareRow, TotalsRow},
    db::Limits,
    detail::BatchDetail,
    harness_options::{HarnessOptions, TaskSet},
    list::BatchSummary,
    matrix::{Aggregate, Cell, PriorAttempt},
    replays::ReplayContext,
    score::{NodeCost, RunScore, StepScore},
    sweeps::SweepSummary,
};

/// The types of this module the web reads (`web/src/generated/benchmarks.ts`, api_types.rs), in file order.
#[cfg(test)]
pub(crate) fn ts_module(cfg: &ts_rs::Config) -> crate::api_types::Module {
    crate::api_types::ts_types!(cfg, benchmarks: Limits, BatchStatus, BatchSummary, Cell, PriorAttempt, Aggregate, BatchDetail, ReplayContext,
        StepScore, NodeCost, RunScore, TaskAgg, Noise, TaskCompareRow, NodeAgg, NodeCompareRow, TotalsRow, BatchCompare,
        SweepSummary, TaskSet, HarnessOptions)
}

/// This module's value sets in `web/src/generated/shapes.ts` (api_types.rs `shape_fragments`).
#[cfg(test)]
pub(crate) const TS_SHAPES: &str = r#"/**
 * A benchmark cell's state (`Cell::state`, benchmarks.rs `cell_state`): `queued` / `not_started`
 * for a planned cell pb has not started (yet).
 */
export type BenchmarkCellState =
  | 'queued'
  | 'not_started'
  | 'running_agent'
  | 'evaluating'
  | 'scored'
  | 'interrupted'
  | 'timeout'
  | 'over_budget'
  | 'over_steps'
  | 'crashed'
  | 'failed';

/** Why a scored run's harness did not finish on its own (its score is of partial work). */
export type CutShort = 'timeout' | 'budget' | 'steps' | 'interrupted' | 'crashed';
"#;

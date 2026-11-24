# Golden tests

Golden (snapshot) tests pin **today's** output of every pure transform and every JSON API
response of zerochat, byte for byte, so a refactor cannot change what the UI receives without a
test saying so. They pin behaviour, they do not judge it: known bugs are pinned too and listed in
[KNOWN-BUGS.md](KNOWN-BUGS.md).

```sh
make goldens                      # cargo goldens (zerochat + zsf) and the web golden tests
UPDATE_GOLDENS=1 make goldens     # accept new output: rewrites the golden files, review the git diff
```

## Mechanism (one, both sides)

- A value is rendered as JSON with **object keys sorted**, two-space indent, trailing newline,
  and compared with a committed file. On a mismatch the test fails with the first differing
  lines and writes `<name>.actual.json` next to the golden (gitignored). A missing golden fails
  too. `UPDATE_GOLDENS=1` writes the goldens instead of comparing.
- Rust: `crates/zerochat/tests/goldens/golden.rs` (also used by zsf through `#[path]`), goldens in
  `crates/zerochat/testdata/golden/` and `crates/zsf/testdata/golden/`.
- Web: `web/src/testdata/golden.ts`, goldens in `web/src/testdata/golden/`. The web tests read
  the Rust side's API snapshots (`crates/zerochat/testdata/golden/api/**`) as their input, so the
  server and the UI models are pinned together: a server change shows up in both.
- Normalised before comparing, nothing else: the fixture's temporary root becomes `<root>`;
  floats are rounded to 9 decimals (some sums run in HashMap order, see KNOWN-BUGS); "now" is
  fixed (1795003600000) through the test-only clock `server::test_clock`. zsf goldens also zero
  `*_ms`/`ts` times, replace commit ids with `<commit>` and launch-key suffixes with `<time>`.
- Web goldens fix `TZ=UTC`, `Date.now()` (1795003600000) and the `en-US` locale in
  `web/src/testdata/golden.ts`, so they pass on any host.
- Error messages from the OS (`No such file or directory (os error 2)`) are Linux's wording.

## Seams added for testability (the only non-test source changes)

- `crates/zerochat/src/main.rs`: `#[cfg(test)] #[path = "../tests/goldens/mod.rs"] mod goldens;`
  (zerochat is a binary crate; the tests need its private modules).
- `crates/zerochat/src/server.rs`: `now_ms()` returns `test_clock::get()` when a test set it
  (`#[cfg(test)]` only).

## Fixtures (`crates/zerochat/testdata/`)

What each directory holds, where it came from and how to regenerate it:
[crates/zerochat/testdata/README.md](../crates/zerochat/testdata/README.md).

Built into one temporary "world" per test process (`tests/goldens/world.rs`): never the
machine's own state. HOME, the Zeroshot state root, the zerochat config, pb and zsf all point
into it.

| Fixture | What | Made by |
|---|---|---|
| `recorded/{fork,zeroshot16}/state` | 16 real runs on claude-haiku-4-5 on a throwaway copy of a tiny sample Python repository: fork happy path, reviewer never satisfied, worker timeout, bad credentials, unknown model, late failure, force stop, killed controller, steered, transient API errors, usage limit, resumed pair, replay; official Zeroshot 16.1 normal, steered (`vector`/`steer_receipt`, `recorded_at_ms`), unknown model. `manifest.json` says what happened in each. | `recorded/RECORDING.md`, sanitised by `tools/sanitize_state.py` |
| `recorded/*/home` | their Claude Code session logs (`user`/`assistant` records) | same |
| `ledgers/synthetic/*.json` | 52 generated ledgers in the fork's event shapes (copied from real ledgers): every execution state, failure reason and class, diagnosis kind, runtime cause, pressure kind, retries, steer records, voided and reused executions, replays and resumes, map indices, stock (no observe fields), Codex and Copilot harnesses, running runs on an observe source | `tools/make_synthetic.py` (regenerate, commit the JSON) |
| `ledgers/home` | Claude Code and Copilot session logs for synthetic runs (every record kind the readers handle) | same |
| `pb/` | a real pb data directory, shrunk: pb.sqlite (55 runs, 6 real batches incl. two whose runner died, pseudo batches by label, sweeps, step scores), pb.toml, harness variants, per-run small files and 8 Zeroshot ledgers | `tools/make_pb_fixture.py` |
| zsf features | built in `tests/goldens/zsf_fixture.rs` with real git (feature branch, squash commits, a live slice worktree): `calc` (13 slices: every status, waiting_for_approval, accepted, conflict re-run, queued retry, every attempt kind/result, every event kind), `paused`, `broken`, `fresh` | code |
| git workspace | the `succeeded` run's textkit repository (base, run commit, uncommitted edit) | `world.rs` |

Tags in golden paths: `fork-only/` (depends on the patched fork: steer records, replays,
observe fields) and `zeroshot-16/` (official 16.1 output fed through today's code; today's output
is partial on purpose, e.g. steers and 16.1 tool-call lines are not recognised). Moving zerochat
to official Zeroshot output changes exactly these.

## Inventory: JSON API (every route of `server::router`)

| Route | Method | Test | Goldens |
|---|---|---|---|
| `/api/overview` | GET | api::overview | `api/overview`, `api/overview-nav` |
| `/api/benchmarks/batches` | GET, POST | api::benchmarks; POST: api::post_validation | `api/benchmarks/batches`, `api/post/benchmark-start-invalid` |
| `/api/benchmarks/batches/{id}` | GET | api::benchmarks | `api/benchmarks/batch/*` (every real and pseudo batch, missing, invalid) |
| `/api/benchmarks/batches/{id}/stop` | POST | api::post_validation | `api/post/benchmark-stop-invalid` (stopping runs pb: not run) |
| `/api/benchmarks/compare` | GET | api::benchmarks | `api/benchmarks/compare/*` |
| `/api/benchmarks/start-options` | GET | api::benchmarks | `api/benchmarks/start-options` |
| `/api/benchmarks/slots` | GET | api::benchmarks | `api/benchmarks/slots` (pb not runnable: error shape) |
| `/api/benchmarks/harness-preview` | GET | api::benchmarks | `api/benchmarks/harness-preview/*` |
| `/api/benchmarks/sweeps` | GET | api::benchmarks | `api/benchmarks/sweeps` |
| `/api/benchmarks/sweep-report` | GET | api::benchmarks | `api/benchmarks/sweep-report-without-pb` (the report itself is pb's code: not run) |
| `/api/benchmarks/sweep-report/write-runtime` | POST | api::post_validation | `api/post/write-runtime-invalid` |
| `/api/benchmarks/runs/{run}/score` | GET | api::benchmarks | `api/benchmarks/score/<every pb run>`, `.../missing` |
| `/api/features` | GET | api::features | `api/features/list` |
| `/api/features/{name}` | GET | api::features | `api/features/{calc,paused,broken,fresh,missing}/detail` |
| `/api/features/{name}/events` | GET | api::features | `api/features/*/events`, `events-04` |
| `/api/features/{name}/actions` | POST | api::post_validation | `api/post/feature-*` (refusals; accepted actions spawn zsf: not run) |
| `/api/features/{name}/slices/{slice}/actions` | POST | api::post_validation | `api/post/slice-*` |
| `/api/features/{name}/slices/{slice}/task/{attempt}` | GET | api::features | `api/features/calc/task-*` |
| `/api/features/{name}/slices/{slice}/commit` | GET | api::features | `api/features/calc/commit-*` (real git) |
| `/api/features/{name}/slices/{slice}/worktree` | GET | api::features | `api/features/calc/worktree-*` (real git worktree) |
| `/api/features/{name}/changes` | GET | api::features | `api/features/*/changes`, `changes-diff` |
| `/api/features/{name}/slices/{slice}/artifacts/{*file}` | GET | api::features | `api/features/calc/artifact-*` (incl. traversal) |
| `/api/runs/{target}/{run}` | GET | api::runs_and_transcripts, api::run_errors | `api/runs/<every scenario>/run`, `api/errors/*` |
| `/api/runs/{target}/{run}/events` | GET (SSE) | — | UNCOVERED: a server-sent-event stream that only says `changed`; its payload is the run view pinned above. Needs a live run; not covered by the goldens. |
| `/api/runs/{target}/{run}/transcript/{execution}` | GET | api::runs_and_transcripts | `api/runs/<scenario>/transcript-<n>[-ledger]` for every execution of every run |
| `/api/runs/{target}/{run}/checkpoints` | GET | api::run_errors | `api/errors/checkpoints-without-zeroshot` (the list comes from `zeroshot checkpoints`: not run) |
| `/api/runs/{target}/{run}/changes` | GET | api::workspace_views | `api/workspace/changes*`, `observe-changes` |
| `/api/runs/{target}/{run}/diff` | GET | api::workspace_views | `api/workspace/diff*` |
| `/api/runs/{target}/{run}/file` | GET | api::workspace_views | `api/workspace/file*` |
| `/api/runs/{target}/{run}/tree` | GET | api::workspace_views | `api/workspace/tree*` |
| `/api/runs/{target}/{run}/steer` | POST | api::post_validation (fork-only) | `api/post/steer-*` (refusals; delivery needs a live controller: not covered) |
| `/api/runs/{target}/{run}/stop` | POST | api::post_validation | `api/post/stop-observe`, `invalid-json` |
| `/api/runs/{target}/{run}/resume` | POST | api::post_validation | `api/post/resume-*` |
| `/api/runs/{target}/{run}/replay` | POST | api::post_validation (fork-only) | `api/post/replay-*` |
| fallback (static UI) | GET | — | UNCOVERED: serves the built web bundle (bytes of `web/dist`), not JSON. |

## Inventory: Rust transforms

| Module | Function(s) | Test | Fixture → golden |
|---|---|---|---|
| projection | `project` | `projection_goldens::{every_synthetic_run, every_recorded_run, without_local_facts, with_live_usage}`; via API `runs_and_transcripts` | every ledger → `projection/**` |
| projection | `graph_nodes` | `pure::graph_walks` | synthetic graphs + pb harness → `pure/graph-nodes` |
| projection | `diagnose`, `http_statuses` | `pure::diagnose_and_http_statuses` (+ every `diag-*` run) | `pure/diagnose` |
| projection | `parse_reset`, `human_ms` | `pure::reset_times_and_durations` | `pure/reset-times`, `pure/human-ms` |
| projection | `verdict_accepted`, `humanize_retry`, `parse_rate_limit_line`, `state_name` | `pure::verdicts_retries_rate_limits` | `pure/verdicts`, `pure/retry-wording`, `pure/rate-limit-lines`, `pure/exec-state-names` |
| transcript | `from_ledger` | `transcript_goldens::from_ledger_every_execution` | every execution of every ledger → `transcript/ledger/**` |
| transcript | `from_claude_session`, `usage_from_claude_session`, `merge_native_with_ledger` | `transcript_goldens::{native_session_logs, merge_native_with_ledger}` | `transcript/claude/**`, `transcript/merge/*` |
| transcript | `find_claude_session`, `claude_project_slug`, `steer_body`, `parse_iso_millis` | API transcripts (native vs ledger source, `sourceNote`); `pure::small_helpers` | `api/runs/*/transcript-*`, `pure/helpers` |
| transcript/copilot | `from_copilot_session`, `find_copilot_session`, `copilot_home` | `transcript_goldens::native_session_logs`; API `copilot-session` | `transcript/copilot/session` |
| source | `read_local`, `local_facts`, `local_run_ids`, `local_listing(_from)`, `str_at`, `uuid_v7_millis`, `is_controller_of`, `controller_elsewhere` | every projection/API golden; `projection_goldens::incremental_read_matches`; `pure::small_helpers` | `api/overview`, `pure/helpers` |
| source | `never_started`, `is_transient` | — | UNCOVERED: need a ledger without a run row older than a minute / a locked database (timing) |
| source | `agent_processes`, `process_alive`, `hostname`, `ledger_stamp` | indirectly (stalled runs find no agent process) | UNCOVERED as values: read the live process table / host |
| source | `Remote::{probe, listings, read}` | — | UNCOVERED: needs a direct target (HTTP Zeroshot) |
| benchmarks | `list_batches`, `batch_detail`, `compare_batches`, `run_score`, `list_sweeps`, `harness_options`, `running_batches_alive`, `pb_run_ended`, `pb_run_of_state_root`, `pending_batch`, `fill_live_cells` | `benchmarks_goldens::{direct_readers, pending_and_live}`; API `benchmarks` | `benchmarks/direct/**`, `api/benchmarks/**` |
| features | `list`, `detail`, `events`, `task`, `artifacts`, `artifact`, `commit`, `worktree_diff`, `changes`, `needs_you_count`, `names`, `zeroshot_roots`, `locate`, `RunFactsCache::get`, `SliceIndex::build`, `context_line`, `context_label` | `features_goldens::list_and_detail_direct`; API `features`, `overview`; `pure::run_context` | `features/direct`, `api/features/**`, `pure/run-context` |
| features | `slice_actions`, `feature_actions`, `waiting_for_you`, `line_diff` | `features_goldens::{slice_action_table, feature_action_table, waiting_and_task_diff}` | `features/slice-actions` (every status × checkpoint × approval × job × worktree), `features/feature-actions` (every combination) |
| features | `act` | API `post_validation` (refusals) | `api/post/feature-*`, `slice-*`; accepted actions write zsf requests and spawn zsf: not covered |
| features | `zsf_root`, `zsf_bin`, `valid_run_id`, `safe_relative` | via config + API | `api/features/calc/artifact-*` |
| context | `providers`, `lookup`, `ZsfProvider::new` | `pure::run_context` | every pb run and synthetic run → `pure/run-context` |
| pricing | `Usage::{from_json, from_harness_json, add, total}`, `estimate_usd` | `pure::pricing_estimates` | 13 models × 4 harnesses → `pure/pricing` |
| config | `load`, `pb_command` | `pure::config_parsing` | 9 configs → `pure/config` |
| config | `config_root` | — | UNCOVERED: reads XDG/HOME of the process |
| harness | `graph_nodes`, `default_paths`, `list_harness_files`, `resolve_in_root`, `preview`, `batch_harnesses`, `runtime_model` | `pure::{graph_walks, harness_files_and_previews}`; API batch details | `pure/harness`, `api/benchmarks/**` |
| discovery | `targets` | API `overview` (no targets.json) | existing unit tests cover targets.json parsing |
| discovery | `local_state_root`, `config_root`, `home` | — | UNCOVERED: process environment |
| workspace | `read`, `read_blocking`, `safe_relative`, `line_counts` | API `workspace_views` (`file*`, outside/missing/option-like paths) | `api/workspace/file*` |
| actions | `changed_files`, `diff`, `file`, `tree`, `head`, `rev_exists`, `commit_at` | API `workspace_views` on a real git repository | `api/workspace/*` |
| actions | `Cli::{json, capabilities, version}` | API with a missing zeroshot program | `api/errors/checkpoints-without-zeroshot`, `api/overview` (`version: null`) |
| pb_exec | `start_batch_args`, `parse_batch_id`, `parse_detached_pid`, `parse_run_slots`, `sweep_report_args` | existing unit tests in `pb_exec.rs` (17) | UNCOVERED as goldens: argument builders, already pinned by those unit tests |
| pb_exec | `start_batch`, `stop_batch`, `run_slots`, `sweep_report` | API with a missing pb program | `api/benchmarks/slots`, `sweep-report-without-pb` |
| zsf::status | `build`, `render`, `one_line` | `crates/zsf/tests/goldens.rs::scheduler_walk`, `model_tables` | `zsf/walk/*` (report JSON and status text after each step) |
| zsf::engine | `open`, `tick`, `run_until_idle`, `apply` (approve, retry with notes, skip, refusals) | `goldens.rs::scheduler_walk` (state.json + events.jsonl after each step) | `zsf/walk/*`, `zsf/walk/05-refusals` |
| zsf::model | `slug`, `SliceStatus::as_str`, `display_status`, `AttemptKind::as_str`, serde of State/Event (unknown kinds) | `goldens.rs::model_tables` | `zsf/model-tables` |
| zsf::time | `duration`, `utc` | `goldens.rs::model_tables` | `zsf/model-tables` |

## Inventory: web models

See [`web/src/testdata/GOLDENS-web.md`](../web/src/testdata/GOLDENS-web.md): every export of
graph-model, features-model, benchmarks-model, transcript-model, replay-model, status, route,
format, markdown and diff, with its golden test and fixture.

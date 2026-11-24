# Test fixtures

Inputs and expected outputs of the golden tests ([tests/GOLDENS.md](../../../tests/GOLDENS.md)).
The tests copy them into one temporary "world" per test process (`tests/goldens/world.rs`); they
never read the machine's own state.

| Directory | What it holds | Where it came from | How to regenerate |
|---|---|---|---|
| `golden/` | The expected output: one JSON file per API response or transform result (`api/`, `projection/`, `transcript/`, `benchmarks/`, `features/`, `pure/`, `pb_exec/`). | Written by the tests. | `UPDATE_GOLDENS=1 make goldens`, then review the `git diff`. |
| `ledgers/synthetic/` | Generated Zeroshot ledgers, one JSON file per scenario: every execution state, failure reason, diagnosis, retry, steer, replay and resume. The tests load each into a `runs.sqlite3`. | `tools/make_synthetic.py`, in the event shapes of real ledgers. | `python3 tools/make_synthetic.py`, commit the JSON. |
| `ledgers/home/` | Claude Code and Copilot session logs for the synthetic runs (`dot-claude`, `dot-copilot`). | `tools/make_synthetic.py`, with the ledgers. | Same as `ledgers/synthetic/`. |
| `ledgers/zeroshot16/` | One reduced ledger of official Zeroshot 16.1 (steer receipts), read by a unit test in `src/transcript/ledger.rs`. | A real 16.1 run, reduced to the events the test needs. | Edit by hand. |
| `recorded/` | 16 real Zeroshot runs on `claude-haiku-4-5` (`fork/` and `zeroshot16/`): `state/runs/<id>/runs.sqlite3`, `workspace-recovery.json` and the Claude Code session logs under `home/`. `manifest.json` says what happened in each run. | Recorded on a maintainer's machine, then sanitised by `tools/sanitize_state.py`. | Follow [recorded/RECORDING.md](recorded/RECORDING.md). |
| `pb/` | A shrunk pb (benchmarks) data directory: `pb.sqlite` (runs, batches, sweeps, step scores), `pb.toml`, `harness/` and, per run in `runs/`, only the small files zerochat reads (some with a Zeroshot `runs.sqlite3`). | A real pb data directory, copied by `tools/make_pb_fixture.py`. | `tools/make_pb_fixture.py PB_DATA OUT` (needs a pb data directory). |
| `tools/` | The scripts above: `make_synthetic.py`, `make_pb_fixture.py`, `sanitize_state.py`. | — | — |

## Sanitising

`tools/sanitize_state.py` (also used by `make_pb_fixture.py`) rewrites every string in every
SQLite text column and JSON file before it lands here:

1. paths given with `--map`, then `$HOME` → `/home/user` and the user name → `user`;
2. secrets (`sk-ant-…`, `ghp_…`, `github_pat_…`, `*_TOKEN=`, `*_SECRET=`, `*_API_KEY=` values) → `REDACTED`;
3. strings longer than `--max-string` are cut.

It fails if the output still matches a forbidden pattern (a home path, a temporary agent
directory, a token). `make_pb_fixture.py` also replaces host names with `bench-host`.

## Why binary SQLite files are in git

Zeroshot and pb store their state in SQLite, and zerochat reads those databases directly. The
recorded fixtures pin what real programs wrote, byte for byte; a text dump would need a loader
that could hide differences. They are small (`pb.sqlite` about 2.5 MB, each `runs.sqlite3` under
300 KB) and change only when re-recorded. `.gitattributes` marks them `binary`.

To read one: `sqlite3 pb/pb.sqlite .schema` or `sqlite3 <run>/runs.sqlite3 'select * from v2_run_events limit 5'`.

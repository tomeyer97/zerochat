# zerochat — design

zerochat is a local web UI for observing, steering and replaying Zeroshot runs. One command,
`zerochat`, serves it on loopback and shows every run on this machine: the local controller's
runs and the runs of every direct (Docker) target in `targets.json`.

Goals, in priority order:
1. **No black box.** For every node: state, how long it has run, tokens, dollars, model, the
   exact prompt/input, full reasoning, messages, every tool call (commands, edits) and result,
   the files it changed, every round of a loop (reviewer verdicts and feedback), and every
   limit that applies (timeout, attempts, loop rounds, rate limits, retries).
2. **Clear failures.** Every failed run and node says *what* failed, *when* (how far into the
   run, after how much work), *why* (classified: the work, a reviewer, the agent/provider, or
   the orchestration) and *what you can do* (resume, start again from a checkpoint, look at the diff).
3. **Steering.** Send a message into a running agent; it arrives in the live turn.
4. **Resume and start again.** Continue a run from where it stopped, or start a new run from a
   saved checkpoint, so finished work is not thrown away. Changing the prompt, model or graph on
   the way (*Replay with changes*) needs a Zeroshot that can edit a replay; stock 16.1 cannot.

zerochat is an independent community project, not affiliated with The Open Engine Company.

**Zeroshot version policy** (the same in README, here and `zerochat doctor`):

| Zeroshot | What works |
|---|---|
| none, or older than 16.1 | reading runs (history, transcripts, failures). Stop, resume and steering are not supported: the UI's Zeroshot strip, the startup banner and `zerochat doctor` (exit 1) say so, with the install command |
| stock 16.1 or later (npm `@the-open-engine-company/zeroshot`) | everything above, plus stop, resume, start again from a checkpoint, and steering of the whole run |
| a build whose `zeroshot steer --help` lists `--node` | also steering of one agent |

The minimum is `ZEROSHOT_MINIMUM` in `crates/zerochat/src/discovery/setup.rs`; a Zeroshot that has
`zeroshot steer` counts as 16.1 whatever version it prints (a source build). See "What zerochat needs
from Zeroshot" below. Every panel that lacks data says why instead of showing nothing.

## Layout

```
crates/zerochat/   Rust (axum) server: discovery, ledger reader, transcript parsers, SSE, actions,
                   the Features and Benchmarks APIs; goldens in testdata/golden/
crates/zsf/        zsf (Zeroshot features): the slice scheduler, as the `zsf` CLI and a library
                   zerochat reads and drives it through (crates/zsf/README.md)
web/               React 19 + Vite + @xyflow/react + elkjs (Zeroshot UI stack), embedded at build;
                   web/src/generated/ is generated from the Rust API types (`make types`)
web/qa/            screenshot script for visual checks
docs/              user docs: install, configuration, threat model
tests/             what the golden tests pin (GOLDENS.md) and the bugs they pin as they are (KNOWN-BUGS.md)
Makefile           build, run and test entry points (`make help`); checks Node first (scripts/check-node.sh)
```

## Sources of truth (read-only)

| Data | Where |
|---|---|
| Local runs | `$ZEROSHOT_STATE_DIR` / `$XDG_STATE_HOME/zeroshot` / `~/.local/state/zeroshot` → `runs/<uuidv7>/runs.sqlite3` (tables `v2_runs`, `v2_run_events`), opened read-only |
| Local live status, actions | the run's controller: JSON-RPC over `runs/<id>/controller.sock` |
| Targets | `$ZEROSHOT_CONFIG_DIR` / `~/.config/zeroshot/targets.json` (v5). `direct` targets: GET `<origin>/.well-known/zeroshot-native-v2`, then JSON-RPC over WebSocket `<origin>/native-v2/oecp`. `hosted`: listed, marked unsupported in v1 |
| Checkpoints | `run/checkpoints` RPC (`runs/<id>/checkpoints/index.json` locally) |
| Native transcripts | local Claude: `~/.claude/projects/<cwd-slug>/<sessionId>.jsonl`; local Copilot: `$COPILOT_HOME` (default `~/.copilot`)`/session-state/<sessionId>/events.jsonl` (without a recorded id: the session whose `session.start` cwd and time match the execution); retained copies of older patched runs under `runs/<id>/transcripts/` |

zerochat never writes to a ledger and never starts a controller except through the
documented CLI (`zeroshot resume`, `zeroshot steer`, `zeroshot force-stop`).

## What zerochat needs from Zeroshot (16.1)

zerochat reads the ledger itself and drives runs only through documented `zeroshot` commands
(`actions::Cli`, always with `ZEROSHOT_ERROR_FORMAT=json`). It detects features from help text,
never from version numbers.

- **Steering** (`zeroshot steer`, Zeroshot 16.1): detected from `zeroshot steer --help`. A chat
  message is sent as `zeroshot steer --mode once --text-file F -- RUN` (≤ 256 KiB, no control
  characters). Stock 16.1 steers the whole run: every agent conversation receives it, and the
  composer says so. When `steer --help` lists `--node NAME` (upstream PR the-open-engine/zeroshot#1667,
  not yet released), the message goes only to the clicked agent's node (`--node NODE`). The
  command answers with a receipt (`steer.id`); delivery shows later in the ledger as `vector`
  events (`steer`, and `steer_receipt` whose `conversation` ends in `/execution/<n>`), which the
  chat shows as sent / received / not received.
- **Timing**: `v2_run_events.recorded_at_ms` (16.1) is the time of every event that has no time of
  its own, so node starts and ends are exact.
- **Tool calls**: 16.1 writes them as plain `safe_log` lines (`Claude tool call: NAME id=… status=…`
  then the input; `… tool completed: …` then the result); zerochat parses them for every harness.
  The native provider session log (local Claude and Copilot) stays the richer source when found.
- **Resume / Start again**: `zeroshot resume [--from-checkpoint ID] -- RUN`. *Resume* continues
  from where the run stopped (no checkpoint id). *Start again from…* names a checkpoint; a local one
  adds `--discard-workspace-changes` (16.1 requires it), never with `--target`. Because that rolls
  the retained workspace back, the dialog asks first, naming the checkpoint and what is discarded;
  Cancel sends nothing. A changed prompt, model or graph is not possible with 16.1, so
  *Replay with changes* is shown only for a Zeroshot that can edit a replay.
- **Suspended runs** (16.1 `suspend`, request and wait nodes) show as Suspended, not stalled; the
  new event kinds (`suspend_requested`, `suspended`, `request_deferred`, `wait_deferred`,
  `execution_voided`, `node_instances_reserved`, `delivery_state`) are read or skipped.
- **Not available from stock Zeroshot**: per-turn provider cost and model (cost is an estimate
  from tokens), per-node workspace snapshots (changes are compared with the run's base commit),
  and files or provider transcripts of runs on another machine.
- **Older ledgers**: runs recorded by an earlier patched Zeroshot (extra observe fields `logKind`,
  `provider`, `workspace`, `costUsd`, `model`, `replayOf`) are still displayed in full. Zeroshot 16.1
  cannot read those ledgers, so with a 16.1 Zeroshot zerochat refuses to stop, resume or message
  them and says why.

## Failure classification (shown first on every failure)

| Class | Means | Examples |
|---|---|---|
| **Work** | the agents did the work but it was not accepted | `change_attempts_exhausted`, `review_failed`, verifier `rejected` in the last round |
| **Agent** | an agent/provider failed, not the code | node `error` with `timeout`, `crash`, `malformed`, `refusal`, usage/rate limit, API retries exhausted |
| **Setup** | retrying unchanged fails the same way | rejected credentials (401), no access to a model (403), unknown model, agent CLI missing, disk full |
| **Orchestration** | Zeroshot itself | `runtime_lost`, `runtime_failed`, controller gone (stalled), history gap |
| **Delivery** | the work is done, delivering it failed | `delivery_unconfirmed`, `delivery_failed` |
| **Stopped** | a force stop | `force_stopped` (status `stopped`; the interrupted node's `refusal` code is Zeroshot's, not the model's) |

Each failure card shows, once each: the class, one short headline (the list shows the same
cause in 2-4 words), a fix when retrying unchanged would fail again, the reviewer's last
feedback for **Work** failures, when the deciding node failed (its end, not when Zeroshot
closed the run; a long Active Memory postmortem is named), which nodes finished before it,
one primary action that fits the cause (resume, start again from the reviewer, or the fix
itself; replay with another model or a longer timeout only when replay with changes exists)
plus the others as secondary, and the raw reason and error
lines collapsed. Tokens and cost are in the run header only.

## Numbers (one definition each, computed in the server)

- Run `usage` = graph nodes + background agents; `nodeUsage` = graph nodes only. The header
  shows `nodeUsage` next to the dollars (same scope; cache reads named unless they are all of
  it); background-agent tokens, which Zeroshot does not price, are in its tooltip and the Log.
- While a local Claude node runs, Zeroshot has no usage for it yet: the server sums
  `message.usage` (once per message id) from Claude's session log (`usageSource:
  session_log`, shown as *live*), else says *counted when the node finishes* (`pending`).
- An execution's duration is Zeroshot's own (`… failed: timeout after 45.4s`) when it reported
  one; otherwise inferred from log timestamps and marked ≈. A reused (prior) execution has no
  times in the replay; it names the run it ran in (`reusedFrom`).
- Times: one format. Lines say `2:21 PM` (today), `Oct 28, 2:21 PM`, `Oct 28, 2025`, or an
  offset into the run (`+48s`); tooltips say `Nov 4, 12:24:36 PM`.
- The run list's row summary (`status`, `cause`, `durationMs`, `costUsd`, lineage) comes from
  the same projection as the run page; rows are cached per run by ledger size/mtime (local)
  or history cursor (direct targets, refreshed in the background).

## UI

Simple without hiding what matters: one place per fact, no caveat paragraphs, red only for
failed, blue for running and actions, amber for stalled, grey for stopped.

Deliberate decisions (a change to one of these is a design change, not a cleanup):
- Success is muted; each scope (run, node, slice, cell) shows one status signal.
- The graph shows what ran: `choice` nodes are folded into edges, exits that were never reached
  are folded away, and a condition is shown only on the edge that was taken.
- The Details tab is where rarely needed facts live.
- Short "not recorded by this Zeroshot" notes stay: every panel that lacks data says why.
- The start time tells runs with the same title apart.
- `~` marks an estimated cost.

- **Home:** one run table (status dot, title, cause, start time, duration, cost, target) with
  chips for All / Running / Failed and a filter. The start time tells repeated titles apart.
  No sidebar here.
- **Sidebar (run pages):** targets (local first; no heading for a single target), runs newest
  first with cause, start time and duration. Under 900 px it is a drawer.
- **Run view:** header (title, status, one meta line: start, elapsed, cost, tokens, a
  *limited data* popover when this Zeroshot does not record something, copy id; lineage:
  resumed from/as, replayed with changes from/as, each run named by its start time), the failure
  card (a force stop is one line), limits close to running out (live runs only, one row), the
  **graph** (ELK layout, fitted to the canvas until the person moves it), the executions
  table, and one collapsed Log (run lines and background agents, filterable by agent).
- **Routes:** `#/run/<target>/<run>/node/<name>/round/<k>[/attempt/<a>]` (`/exec/<id>` for map
  items; a bare number is an execution id from older links); query strings are ignored.
- **Node chat (click a node):** the outcome (verdict and the reviewer's feedback, or the error)
  first, the prompt collapsed to one line, then reasoning, messages, tool calls with results,
  messages you sent (steer) and Zeroshot's automatic nudges (labelled as Zeroshot's, never
  "You"). The transcript source is in Details. Round picker for every visit of the node. Composer at
  the bottom when the node is live and steering is supported. Tabs: Chat · Files (changed
  files + diff + file browser at start/end; only when the node changed files) · Details.
- **Verbs:** one name per operation, the same in the button, dialog title, confirm button,
  notice and lineage line. *Resume* continues from where the run stopped (no checkpoint saved:
  *Start over*, from the beginning in the workspace as it is). *Start again from…* opens the
  checkpoint dialog (the failure card's button names the failed node, and the dialog marks its
  checkpoint "<node> failed here"). *Replay with changes* replaces it only when this Zeroshot can
  edit a replay. Zeroshot records a start again as a resume, so its lineage line says
  "Resumed from".
- **Start again dialog:** pick a checkpoint (node + round), then *Start again…* asks to confirm:
  the workspace goes back to that checkpoint and later changes are discarded. With replay with
  changes the same dialog also changes model, timeout, instructions and a loop's max rounds, or
  the graph/runtime/input JSON (admission keys such as `bounds` are stripped); a checkpoint in a
  loop's last round pre-raises that loop's max rounds. Its button validates with Zeroshot first
  (a conflict is shown), then asks to confirm; *Check* only shows what Zeroshot would reuse.

## Features (zsf)

The **Features** page shows work run by zsf (Zeroshot features, `crates/zsf`), and is in the nav only
when zsf is installed or has a feature on this machine. zsf's own model is in
[crates/zsf/README.md](crates/zsf/README.md); in short:

- A **feature** is a repository, a feature branch, its base and a plan: a spec cut into **slices**
  with dependencies between them.
- Each slice **attempt** is one Zeroshot run in the slice's own worktree. A finished slice is squashed
  into one commit and fast-forwarded onto the feature branch; a rebase conflict keeps the work under a
  backup ref and re-runs the slice once from the new tip.
- A **checkpoint** slice waits for a person to approve it once merged; a quality failure stops the slice
  for a person; infrastructure failures are retried and, repeated, pause the feature.
- zsf's daemon is the only writer of a feature's state. zerochat reads that state through the zsf
  library and queues every action to the daemon (`zsf::daemon::submit`).

Screens:
- **Feature list:** name, repository and branch, progress (`11 of 17 merged`), what runs now or what
  waits for you, whether the daemon is alive, cost so far. The nav badge counts what waits for you.
- **Feature page:** the slice graph (ELK, left to right, like the run graph): one card per slice (id,
  title, status, attempts, duration, cost), dependency edges, checkpoints marked; amber means waiting
  for you or paused. Under 640 px it is a list of the same cards.
- **Slice panel:** the spec; every attempt (kind, result, reason, a link to its run) with its exact task,
  diffed against the previous attempt's so retry notes stand out; the merged commit and its diff; the
  artifacts; the live worktree's changes.
- **Actions:** every feature and slice carries the actions zsf would accept in its state (`actions`,
  computed by `zsf::actions`). The UI offers exactly those; the server refuses anything else (409)
  before queueing it. *Start* runs the zsf daemon and is not offered without a `zsf` binary.
- Live by polling: every 3 s while the daemon runs or a slice is active, else every 15 s.
- A slice's run page names its feature, slice and attempt in the header and links back.

## Benchmarks (pb)

The **Benchmarks** page reads pb (ProgramBench runner), a separate tool that runs a harness (a
Zeroshot graph and runtime) on benchmark tasks, several seeds each, and scores each run's code with
hidden tests. The page needs pb: it is in the nav only when `pb_root` is set in zerochat's
`config.toml`, and everything else works without it. zerochat reads pb's database read-only and calls
pb's own command (`pb_command`) to start and stop batches and to build sweep reports.

- A **run** is one harness run on one (task, seed): a Zeroshot run in pb's sandbox, then scored.
- A **batch** is the runs one `pb run` started. Runs of older pb databases are grouped by label.
- **Batches** list: label, tasks × seeds, mean score, cost, status.
- **Batch page:** the task × seed matrix. A cell is one run: its score (the only coloured value), its
  state (agent → evaluating → scored), cost against the cap and wall time; totals per row and column
  with the min–max spread. Live while the batch runs.
- **Benchmark run page:** the run page plus a score card (hidden tests and regression tests passed,
  score after each step, cost per node). pb's cost is the definition here. Benchmark runs are
  observe-only: no messages, resume or replay from zerochat, which would break pb's sandbox and its
  accounting.
- **Compare** two batches: per task the score change, each side's seed spread and pb's noise verdict
  (defined once, as pb defines it: `compare_batches` in `crates/zerochat/src/benchmarks/compare.rs`);
  the cost and time change; per node the cost change.
- **Sweeps:** node × model score change against a baseline, pb's recommendation, and writing the
  recommended runtime file.
- **Start a batch:** tasks, seeds, label, harness, model, limits and parallelism, with the maximum spend
  (runs × cost cap) shown before *Start*. pb runs one batch at a time; a running batch can be stopped.

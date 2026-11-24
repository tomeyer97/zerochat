# zsf

zsf (Zeroshot features) builds one larger change (a **feature**) on its own git branch by splitting
it into steps (**slices**) and running each slice as one [Zeroshot](https://www.npmjs.com/package/@the-open-engine-company/zeroshot)
run: at most N at once, in dependency order, each finished slice squashed onto the feature branch.
It stops for a person at **checkpoints** (slices whose finished work waits, not merged, for review) and
at failures. zerochat's Features page shows and drives it; it also works on its own from the terminal.

Part of [zerochat](../../README.md), an independent community project, not affiliated with The Open
Engine Company.

## Requirements

- **Zeroshot 16.1 or later** (`npm i -g @the-open-engine-company/zeroshot@latest`): zsf starts and
  resumes every slice's run with the `zeroshot` command. The same policy as zerochat
  ([DESIGN.md](../../DESIGN.md)).
- git.

## Install

zsf is installed together with zerochat (the npm package and the release archives carry both; see
[docs/install.md](../../docs/install.md)). From a checkout of this repository:

```sh
make                         # builds zsf next to zerochat (target/debug/zsf)
cargo install --path crates/zsf
zsf install-service          # optional: systemd user unit, so the daemon survives reboots
```

Without systemd (containers, dev workspaces), put this in the shell's startup file so a restart
picks the features up again: `zsf autostart >/dev/null 2>&1`.

## Quickstart

```sh
zsf init checkout-v2 --repo ~/src/shop --branch checkout-v2 --base origin/main \
    --refined ~/specs/checkout-v2.refined --checkpoints 02,06 \
    --graph ~/zs/graph.json --runtime ~/zs/runtime.json
zsf start checkout-v2
zsf status                   # one line per slice; --json for tools
zsf log -f --slice 03
zsf approve 02               # a checkpoint waiting for review: check and merge it
zsf retry 02 --notes "Keep the old flag"         # request changes instead (re-runs it)
zsf retry 05 --notes "Use the existing parser"   # a stopped slice; or --fresh
zsf accept 05                # you finished it by hand in its worktree
zsf skip 07
zsf unpause                  # after repeated infrastructure failures
zsf edit 09 --deps 03,04 --before 08             # only slices that have not started
zsf max-parallel 3
zsf stop                     # running Zeroshot runs keep going
```

The feature name can be left out while there is only one feature.

## Files

`$ZSF_STATE_DIR`, else `$XDG_STATE_HOME/zsf`, else `~/.local/state/zsf`; one directory per feature:
`feature.toml`, `plan.toml`, `state.json` (written only by the daemon), `events.jsonl`,
`context/`, `tasks/<id>-a<n>.md` (the exact task of each attempt), `runs/`, `artifacts/<id>/`,
`requests/`, `worktrees/`.

## feature.toml

```toml
name = "checkout-v2"
repo = "/home/me/src/shop"
branch = "checkout-v2"
base = "origin/main"
# plan = "plan.toml"  worktrees = "…"  context = "…"  commit_prefix = "SHOP"
artifacts = [".zeroshot-pr/FOLLOW-UPS.md"]   # copied to artifacts/<id>/ before a worktree goes

[launch]
graph = "/home/me/zs/graph.json"
runtime = "/home/me/zs/runtime.json"
# zeroshot = "zeroshot"  args = []  context_in_worktree = ".zsf-context"  ("" = point at context/)

[hooks]          # all optional, run with bash -c
# launch = "…"            replaces `zeroshot run`; must print a {"runId":"…"} line
#                         (pass --submission-key "$ZSF_LAUNCH_KEY" so a crash cannot double-launch)
# prepare = "npm ci"      in a fresh worktree, before the launch
# gate = "make test"      must pass before merging (a failure re-runs the slice: gate_reruns)
# check_after_rebase = "cargo build"   failure counts as a conflict
# after_merge = "…"       in the repository; failure is only reported

[limits]         # defaults shown
max_parallel = 2           # launching, running or integrating slices
min_free_gb = 0
infra_retries = 2
conflict_reruns = 1        # re-runs after a rebase conflict
gate_reruns = 1            # re-runs after a failed gate, with its output as notes
pause_after_infra = 2
launch_timeout_secs = 180
start_grace_secs = 300
poll_secs = 30

[leases]         # counted resources a slice can require (plan: leases = ["e2e-box"])
# e2e-box = 3

[env]            # for hooks and runs, e.g. ZEROSHOT_STATE_DIR, PATH
```

## plan.toml

```toml
[[slice]]
id = "01"
title = "Add the limits table"
spec = "context/slices/01-limits-table.md"
deps = []
checkpoint = true          # waits, not merged, for your review when its run is done
# leases, commit_title, description, graph, runtime, args, [slice.hooks]
```

`zsf import <refined dir> [--feature NAME]` writes it from `slices/INDEX.md` and the slice files.

## Hook environment

`ZSF_FEATURE`, `ZSF_SLICE`, `ZSF_ATTEMPT`, `ZSF_WORKTREE`, `ZSF_BRANCH` (feature branch),
`ZSF_SLICE_BRANCH`, `ZSF_CONTEXT`, `ZSF_RUN_ID`, `ZSF_TASK_FILE`, `ZSF_LAUNCH_KEY`, `ZSF_STATE_DIR` (the feature
directory), plus `[env]`.

## How a slice goes

pending → launching → running → integrating → merged, or failed / interrupted / skipped.

A checkpoint slice whose run is done does not merge: it is **waiting for review**, not merged, with
its worktree, branch and diff kept (and its PR, when the run delivers one). Its dependents wait
until it is merged. Then:

- `zsf approve` = check and merge: the gate runs, the slice is squashed and merged, its dependents
  start. A conflict here gets the conflict re-run below and merges without a second review.
- `zsf retry --notes "…"` = request changes: the reviewed attempt ends `changes_requested` and the
  slice runs again in its worktree with the notes; when that run is done it waits for review again.
- `zsf skip`: drop it; its dependents no longer wait.

A run that ends `change_attempts_exhausted` stops the slice
(quality); `force_stopped` stops it; anything else is an infrastructure failure, retried in the
same worktree up to `infra_retries`, and `pause_after_infra` in a row pause the feature. A run
whose controller is gone (crash, reboot, or Zeroshot says `runtime_lost`) is first looked at with
`zeroshot status` (that records how it ended: it may have succeeded), then resumed once with
`zeroshot resume`, after its leftover agent processes are stopped. Merging squashes
the slice into one commit (trailers `Slice:` and `Zeroshot-Run:`), rebases it onto the branch
tip and fast-forwards the branch; a conflict keeps the work as `zsf-backup/<branch>/<id>-a<n>` and
runs the slice once more from the new tip with that work re-applied (the conflict names the slices
that moved the branch and the files; the re-run's task gets the previous work under its own heading,
apart from a person's notes). A failed gate re-runs the slice once (`gate_reruns`) in its worktree,
as a `gate-rerun` attempt whose task has the end of the gate's output under zsf's heading; a
checkpoint's gate re-run waits for review again. Only when the re-runs are used up does the slice
stop as failed. A `retry` while nothing may launch (paused, stopped, every slot busy)
waits as a pending slice and launches when the scheduler gets to it.

Every attempt has a launch key (`--submission-key`, saved before the launch): if zsf dies before
it recorded the run id, the next start finds the run by its key, and launching again with the key
returns the same run. A resume whose successor was not recorded is found the same way (Zeroshot's
`successorRunId`), so nothing runs twice or unrecorded.

Do not keep the feature branch checked out in the repository: zsf moves it with `git update-ref`.

# Recorded Zeroshot runs

Real runs on `claude-haiku-4-5`, sanitised for use as test fixtures. `manifest.json` lists every run
(family, scenario, run id, what happened).

- `fork/`: the zerochat fork of Zeroshot (branch `zerochat`, with the `zeroshot zerochat …` extensions).
- `zeroshot16/`: official `@the-open-engine-company/zeroshot@16.1.0` from npm.

Each family has `state/runs/<run-id>/` (`runs.sqlite3`, `workspace-recovery.json`) and
`home/dot-claude/projects/-work-<scenario>/` (renamed to `.claude` by the tests) (the Claude Code session logs of that run's workspace,
`user`/`assistant` records only).

## How to re-record

Scenarios, the task repository and the fault proxy come from the maintainers' end-to-end test lab, which
is not part of this repository: `scenarios.py`, `sample-repo` (a tiny Python package with tests) and
`faults.py` below refer to it.

1. Per family, use isolated directories and an isolated Claude Code and git home, so no personal
   instructions, MCP servers or identities end up in the session logs:

   ```sh
   export ZEROSHOT_STATE_DIR=$REC/<family>/state ZEROSHOT_CONFIG_DIR=$REC/<family>/config
   export CLAUDE_CONFIG_DIR=$REC/claude-home/<family>/.claude
   export GIT_CONFIG_GLOBAL=$REC/gitconfig            # user.name/email of a test identity
   export CLAUDE_CODE_OAUTH_TOKEN=...                  # never written to any file
   export ZEROSHOT_RESTIC=/path/to/restic              # fork built with cargo has no bundled restic
   ```

2. Per run: copy `sample-repo` to `$REC/<family>/work/<scenario>`, `git init -b main`, commit, write the
   scenario's `graph.json` (from `zeroshot template show <name> --delivery none --json` of the same
   binary, patched as in `scenarios.py`), `input.json` and `runtime.json`, then from the workspace:

   ```sh
   zeroshot run --title "<scenario> [<family>]" --graph graph.json --input input.json \
     --uniform-runtime-config runtime.json --detach     # --runtime-config for per-node plans
   ```

   Then poll `zeroshot status RUN` until `status.phase == "finished"`.

Scenario notes (differences from `scenarios.py`):

- happy-path: `ZEROSHOT_KEEP_CHECKPOINTS=1` so the replay has checkpoints.
- force-stopped / controller-killed: small task; `zeroshot force-stop RUN` or `kill -9` of the pid in
  `runs/RUN/controller.ready.json` 60 s in. The killed run gets `runtime_lost` on the next `status`.
- resumed: `zeroshot resume <force-stopped run>`.
- replay (fork): `zeroshot zerochat replay --from-checkpoint=<first id from zeroshot checkpoints RUN> --title "…" -- RUN`.
- steered: single-worker with the task "three `sleep 20` Bash calls, then write APPLE to out.txt unless told
  otherwise". About 30 s after `node_started`: fork
  `zeroshot zerochat steer --message="Change of plan: … BANANA …" -- RUN <node_started.reference.execution>`;
  16.1 `zeroshot steer RUN "<text>"`. A second message 15 s later.
- chaos-transient / chaos-usage-limit: single-worker small task with `ANTHROPIC_BASE_URL` pointing at
  the lab's fault proxy `faults.py` (`/p/<plan>`), with short plans:
  `[{afterRequests:1,count:2,overloaded},{afterRequests:4,count:2,rate_limit},{afterRequests:7,count:1,server_error}]`
  and `[{afterSeconds:15,forSeconds:60,usage_limit}]`.
- unknown-model on 16.1: uniform runtime with the bad model (16.1's local target rejects size `small` in exact plans).

## Sanitising

1. Keep only `user`/`assistant` records of `$REC/claude-home/<family>/.claude/projects/**/*.jsonl` in a
   staging home (Claude Code also logs its full system prompt, request shapes and skill listings).
2. `testdata/tools/sanitize_state.py $REC/<family>/state recorded/<family>/state RUN… \`
   `--map $REC/<family>/work=/work --map $REC/<family>/state=/state --map $REC/<family>=/rec \`
   `--claude-home <staging home> --dst-home recorded/<family>/home --max-string 1500`

To stay near 3 MB, the session logs of reviewer-never-satisfied and late-failure were dropped (their ledgers
are kept), and a recorded 16.1 force-stopped run was left out.

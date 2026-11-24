# Threat model of the local HTTP API

What zerochat's local server protects against, and what it does not. Read this before you change
`--listen`, `--allow-remote` or `--any-host`.

## What the API exposes

zerochat serves a web UI and an HTTP API under `/api` (route table: `crates/zerochat/src/server/mod.rs`).

Read (GET):

- runs: run list, run view, live events, transcripts, checkpoints;
- files of a run's workspace: changes, diff, file contents, tree;
- features and slices (zsf), benchmarks (pb), the setup check.

Change state (POST):

- runs: `steer` (send a message to an agent), `stop`, `resume`, `replay`
  (`/api/runs/{target}/{run}/…`); zerochat runs the `zeroshot` CLI for these;
- features (zsf): feature actions (start, stop, unpause, max parallel; start launches the zsf
  daemon) and slice actions (approve, retry, accept, skip, edit);
- benchmarks: start and stop a batch, write a runtime file (zerochat runs `pb`).

## Who can use it

- **No authentication.** Any request that passes the checks below reaches the handlers.
- **Loopback by default.** zerochat listens on `127.0.0.1:4380` (the next free port up to 4390).
  A non-loopback `--listen` address is refused without `--allow-remote` (`crates/zerochat/src/main.rs`).
- So **any local user or process on the machine can read your runs and steer, stop, resume
  and replay your agents**, e.g. another account on a shared machine, or a malicious package
  script running as you. This is a known limit, not a bug.

## The checks (`crates/zerochat/src/server/boundary.rs`)

Every request passes these, in order:

1. **Host header (DNS-rebinding defence).** Only the listen address, `localhost:<port>` (and
   `127.0.0.1:<port>` when listening on `0.0.0.0`) and any `--allowed-host` entries are
   answered; others get `403 host_rejected`. `*` alone is refused as an `--allowed-host`.
2. **Origin (CSRF defence), for every method except GET and HEAD:**
   - if `Sec-Fetch-Site` is present, it must be `same-origin` or `none`;
   - else, if `Origin` is present, it must name the Host (or `X-Forwarded-Host`);
   - else (**no `Origin` header**) the request passes. Browsers send `Origin` on cross-site
     POSTs, so this blocks other websites, but not curl or any local program.
   - Failure: `403 origin_rejected`.
3. **JSON only** for state changes: `Content-Type: application/json`, else `415 json_required`.
   A plain HTML form cannot send it.

Every answer also carries `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`,
`X-Frame-Options: DENY`, and the UI page a strict Content-Security-Policy.

## Flags that widen access

| Flag | What it changes | Use it |
|---|---|---|
| `--listen <addr>` / `ZEROCHAT_LISTEN` | The address. Non-loopback is refused without `--allow-remote`. | To pick a port. |
| `--allow-remote` | Allows a non-loopback listen address: anyone who can reach it can steer and stop runs. | Only behind an authenticating reverse proxy or your own network boundary (e.g. a container). |
| `--allowed-host <name[:port]>` | Adds Host names to the allow-list (exact, or `*.suffix`). | When a proxy or container reaches zerochat by another name. |
| `--any-host` / `ZEROCHAT_ANY_HOST` | Turns the Host check off (no DNS-rebinding defence). | Only behind a reverse proxy that logs you in, with no browser on this machine. |
| `make remote` | Runs `zerochat --any-host --no-open`: still loopback, no Host check. | Same as `--any-host`. |

Never expose zerochat to a network without a proxy that authenticates every request.

## Multi-user machines

- Every account on the machine can reach `127.0.0.1`, so every account can use your zerochat.
- On a shared machine, run zerochat only while you use it, or not at all.
- A container or VM of your own is a boundary; another user's shell is not.

## What zerochat reads and runs

Reads (as your user):

- Zeroshot's state directory (`$ZEROSHOT_STATE_DIR`, else `$XDG_STATE_HOME/zeroshot`, else
  `~/.local/state/zeroshot`) and config directory (`targets.json`) (`crates/zerochat/src/paths.rs`);
- its own `~/.config/zerochat/config.toml` (`crates/zerochat/src/config.rs`), and the state
  directories, `pb_root` and zsf state it names;
- files in a run's workspace, through `crates/zerochat/src/workspace/safe_read.rs`: it refuses
  paths that resolve outside the workspace, symlinked final components, and non-regular files,
  and caps the size, because agents write that directory.

Runs (as your user, as a program with arguments, not through a shell; run ids and other ids from
the URL are checked first, `crates/zerochat/src/server/args.rs`):

- `zeroshot` (`--zeroshot`, default from `PATH`): steer, stop, resume, replay, checkpoints,
  version (`crates/zerochat/src/actions/cli.rs`);
- `zsf` (`zsf_bin`, else beside zerochat, else `PATH`): starts a feature's daemon
  (`crates/zerochat/src/features/`);
- `pb` (`pb_command`, default `uv run --project <pb_root> pb`): batches, slots, sweep reports
  (`crates/zerochat/src/pb_exec.rs`);
- `git`, for read-only views of workspaces.

Whoever can write your `config.toml` or put a program first on your `PATH` chooses what
zerochat runs.

## Planned

A per-start token, required on every API request, so other local users and processes cannot use
the API. Not implemented yet.

## Reporting

Found a way around these checks? Report it privately: [SECURITY.md](../SECURITY.md).

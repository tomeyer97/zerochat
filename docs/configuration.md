# Configuration

zerochat works without any configuration. Everything below is optional.

## Flags and environment variables

Flags win over environment variables. `zerochat --help` lists them all.

| Flag | Environment | Default | What it does |
|---|---|---|---|
| `--listen <addr>` | `ZEROCHAT_LISTEN` | `127.0.0.1:4380`, else the next free port up to 4390 | Serve on exactly this address (fails if it is busy). Loopback only unless `--allow-remote`. |
| `--strict-port` | | off | Fail when 4380 is busy instead of trying the next ports. |
| `--zeroshot <path>` | `ZEROCHAT_ZEROSHOT` | `zeroshot` on `PATH` | The `zeroshot` program used to stop, resume and message runs. |
| `--no-open` | `BROWSER=none` | open when possible | Do not open a browser. zerochat also does not open one without an interactive terminal, without a display, over SSH or when `CI` is set. |
| `--allow-remote` | | off | Serve on a non-loopback address. Anyone who can reach it can stop and message runs ([threat model](threat-model.md)). |
| `--allowed-host <host>` | | none | Extra `Host` header value to accept, repeatable: `name[:port]` or `*.suffix[:port]`. |
| `--any-host` | `ZEROCHAT_ANY_HOST` | off | Accept any `Host` header (turns off the DNS-rebinding check). Only behind a proxy that logs you in. |

Other environment variables zerochat reads:

| Variable | Default | What it is |
|---|---|---|
| `ZEROSHOT_STATE_DIR` | `$XDG_STATE_HOME/zeroshot`, else `~/.local/state/zeroshot` | Zeroshot's state directory: where zerochat finds local runs. |
| `ZEROSHOT_CONFIG_DIR` | `$XDG_CONFIG_HOME/zeroshot`, else `~/.config/zeroshot` | Zeroshot's config directory: its `targets.json` (remote targets) is read from here. |
| `XDG_CONFIG_HOME` | `~/.config` | Where zerochat's own `config.toml` lives (below). |
| `ZSF_STATE_DIR` | `$XDG_STATE_HOME/zsf`, else `~/.local/state/zsf` | zsf's state (features), unless `zsf_state_dir` is set in `config.toml`. |
| `COPILOT_HOME` | `~/.copilot` | Where local Copilot session logs are read from. |

## config.toml

Location: `$XDG_CONFIG_HOME/zerochat/config.toml`, else `~/.config/zerochat/config.toml`. zerochat only
reads it, never writes it, and reads it again on every use, so an edit takes effect without a restart.
A missing file is fine. A problem in it is shown in the UI next to the run sources; the usable rest still
applies. The startup banner prints its path (`Sources`).

| Key | Default | What it does |
|---|---|---|
| `zsf_bin` | `zsf` beside the `zerochat` binary, else on `PATH` | The zsf program the Features page starts. |
| `zsf_state_dir` | zsf's own (`ZSF_STATE_DIR`, see above) | Where zsf keeps its features. |
| `pb_root` | not set | pb's project directory. Turns on the Benchmarks page and adds its runs as a read-only source named `pb`. pb is optional and not public yet. |
| `pb_command` | `uv run --project <pb_root> pb` | How to run pb (split on spaces; pb's subcommand is appended; it runs in `pb_root`). |
| `[[source]]` | none | Extra Zeroshot state directories to show runs from (below). |

`zsf_bin` and `zsf_state_dir` may start with `~`.

Each `[[source]]` table:

| Key | Required | What it does |
|---|---|---|
| `name` | yes | Shown in the sidebar; must be unique, not `local`, not `pb` while `pb_root` is set, and not start with `target:`. |
| `state_dir` | one of these two | One Zeroshot state directory. |
| `glob` | one of these two | A glob pattern; every matching directory is a state directory. |
| `actions` | no | `"all"` (default): runs can be stopped, resumed and messaged. `"observe"`: read-only. |

Example:

```toml
zsf_bin = "/home/you/.cargo/bin/zsf"
zsf_state_dir = "/home/you/.local/state/zsf"

# pb_root = "/home/you/zeroshot-benchmarking"
# pb_command = "uv run --project /home/you/zeroshot-benchmarking pb"

[[source]]
name = "ci"
state_dir = "/mnt/ci/zeroshot-state"
actions = "observe"

[[source]]
name = "sandboxes"
glob = "/home/you/sandboxes/*/zeroshot-state"
```

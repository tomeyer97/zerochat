# Install, upgrade, uninstall

Platforms: Linux (x64, arm64; static builds that run on any distribution) and macOS (x64, arm64).
Windows: use WSL.

Every route installs two programs: `zerochat` and `zsf` (Zeroshot features, used by the Features page).

**Zeroshot 16.1 or later** is needed to stop, resume and message runs:
`npm i -g @the-open-engine-company/zeroshot@latest`. Reading runs works without it. Messaging one agent
(not the whole run) needs a Zeroshot whose `zeroshot steer --help` lists `--node`.

## Install

### npm

Needs Node.js `^22.22.2 || ^24.15.0 || >=26.0.0` with npm.

```sh
npx @zerochat/cli          # try it without installing
npm i -g @zerochat/cli     # install `zerochat` and `zsf`
```

npm picks the package for your platform (`@zerochat/cli-<os>-<arch>`); there is no install script.
No `npx`, or Node too old (Ubuntu 24.04's apt has Node 18 and no npm): install Node from
[nodejs.org](https://nodejs.org/) or with [nvm](https://github.com/nvm-sh/nvm).

### Release archive

From [GitHub Releases](https://github.com/tomeyer97/zerochat/releases), download
`zerochat-<target>.tar.xz` and `zsf-<target>.tar.xz` for your platform (`x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`), check them
([verify.md](verify.md)), unpack and put both binaries in the same directory on your `PATH`.

### Shell installer

Each release also has `zerochat-installer.sh` and `zsf-installer.sh`. They download that release's
archive for your platform and install the binary into `~/.cargo/bin` (`$CARGO_HOME/bin`). Download the
script, read it and check it ([verify.md](verify.md)) before you run it:

```sh
curl -LO https://github.com/tomeyer97/zerochat/releases/latest/download/zerochat-installer.sh
sh zerochat-installer.sh
```

Same for `zsf-installer.sh`.

### From source

Prerequisites: [CONTRIBUTING.md](../CONTRIBUTING.md#prerequisites) (Rust 1.90.0, Node.js as above, a C
compiler, make).

```sh
git clone https://github.com/tomeyer97/zerochat.git && cd zerochat
make                 # build and run from the checkout (zerochat and zsf in target/debug)
make install         # install `zerochat` and `zsf` into ~/.cargo/bin
```

## Upgrade

| Installed with | Upgrade |
|---|---|
| npm | `npm i -g @zerochat/cli@latest` |
| archive | download the new archives and replace both binaries |
| shell installer | run the new release's installers again |
| source | `git pull`, then `make install` |

## Uninstall

| Installed with | Remove |
|---|---|
| npm | `npm rm -g @zerochat/cli` |
| archive | delete the `zerochat` and `zsf` binaries you unpacked |
| shell installer | `rm ~/.cargo/bin/zerochat ~/.cargo/bin/zsf` |
| source (`make install`) | `cargo uninstall zerochat zsf` |

What stays behind (delete it if you want it gone):

- `~/.config/zerochat/` (`$XDG_CONFIG_HOME/zerochat`): your `config.toml`, if you wrote one; zerochat
  itself writes nothing there. The shell installer may also keep its install receipt here.
- `~/.local/state/zsf/` (`$ZSF_STATE_DIR`): zsf's features, only if you used Features.

zerochat keeps no other state: it reads Zeroshot's runs and never writes to them. Zeroshot's own data
(`~/.local/state/zeroshot`) belongs to Zeroshot.

## Troubleshooting

Start with `zerochat doctor`: it checks Zeroshot, the run directory, zsf and pb, and each problem names
its fix.

| Problem | Fix |
|---|---|
| Port 4380 is busy | zerochat takes the next free port up to 4390 and prints it. If a zerochat already runs there, that one is opened. Use `--listen 127.0.0.1:<port>` for a fixed port, `--strict-port` to fail instead. |
| `Zeroshot not found` or `too old (needs 16.1+)` | `npm i -g @the-open-engine-company/zeroshot@latest`, or point at one with `--zeroshot <path>`. |
| Features says zsf is not installed | It comes with the npm package. From source: `make` builds it next to zerochat, or `cargo install --path crates/zsf`. Or set `zsf_bin` ([configuration.md](configuration.md)). |
| No browser opens | zerochat opens one only from an interactive terminal with a display, not over SSH or in CI. Open the printed URL yourself. |
| The wrong browser opens | zerochat uses your system default (`xdg-open` on Linux, `open` on macOS, `wslview` on WSL): change the default browser, or use `--no-open` (or `BROWSER=none`) and open the URL yourself. |
| No runs listed | zerochat reads `$ZEROSHOT_STATE_DIR`, else `~/.local/state/zeroshot`; `zerochat doctor` shows which directory and how many runs. |
| `make` says Node.js is too old | Install a Node in the range above (nodejs.org or `nvm install`, which reads `.nvmrc`). |

Still stuck: [SUPPORT.md](../SUPPORT.md).

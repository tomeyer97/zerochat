# Contributing

Thanks for helping. Read [DESIGN.md](DESIGN.md) first: it says what the UI promises (one place per fact,
every failure explained) and where each piece of data comes from.

- Questions: [SUPPORT.md](SUPPORT.md) (GitHub Discussions). Security problems: [SECURITY.md](SECURITY.md),
  never a public issue.
- Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).
- Who decides and merges: [MAINTAINERS.md](MAINTAINERS.md). How releases are made: [RELEASING.md](RELEASING.md).

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.90.0 | Pinned in `rust-toolchain.toml`; rustup installs it (with rustfmt and clippy) on the first `cargo` run. |
| Node.js with npm | `^22.22.2 \|\| ^24.15.0 \|\| >=26.0.0` | `.nvmrc` says `22.22.2`. |
| A C compiler (cc, gcc or clang) | any recent | rusqlite (`bundled`) and ring compile C. |
| make | any | |

C compiler and make: Debian/Ubuntu `sudo apt install build-essential`; macOS `xcode-select --install`.
Windows: use WSL.

## Work on it

```sh
make dev             # the server plus Vite hot reload on http://127.0.0.1:5174
make test            # Rust, web and npm-shim unit tests
make goldens         # golden tests (tests/GOLDENS.md)
make lint            # the format and lint checks CI runs; writes nothing
make fmt             # format the Rust code and the web UI (writes)
```

- **Rust**: `crates/zerochat` (the server) and `crates/zsf` (zsf, Zeroshot features). `cargo test --workspace`
  runs both crates' tests.
- **Web**: `web/` (React + Vite). `npm --prefix web test` runs the web tests, `npm --prefix web run typecheck`
  the TypeScript check.
- **API types**: the TypeScript types in `web/src/generated/` are generated from the Rust types. Do not
  edit them by hand; after changing a serialised Rust type run `make types` and commit the result.

### Format and lint

`make lint` runs exactly these; CI runs the same and fails on any warning:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
npm --prefix web run lint      # Biome: format and lint check, writes nothing
```

`make fmt` writes the fixes (`cargo fmt --all` and `npm --prefix web run fix`). Rust uses rustfmt defaults;
the web UI uses Biome (`web/biome.json`).

## Packaging

zerochat ships on npm as `zerochat` (JS shims for `zerochat` and `zsf`) plus one package per platform
holding both binaries; names live in `npm/zerochat/packages.json`, the version in the root `Cargo.toml`.
GitHub Releases archives come from dist (`dist-workspace.toml`, `.github/workflows/release.yml`).

```sh
make package         # scripts/package.sh: web UI, release binaries, the npm packages in target/npm, `npm pack --dry-run`
make check-targets   # scripts/check-targets.sh: `cargo check` for Windows and macOS
node --test npm/test/*.test.js   # the shim and layout tests (also in `make test`)
```

`scripts/package.sh` builds this machine's platform package, and another one only when its Rust target
(`rustup target add …`) and C cross compiler (`CC_<target>`) are installed; it says which it skipped.
Neither script publishes anything: a release starts only when a version tag is pushed.

## The test suite

The tests in this repository are the regression net: `make test` runs the Rust, web and npm-shim unit
tests, and `make goldens` runs the golden tests ([tests/GOLDENS.md](tests/GOLDENS.md)). The golden tests
pin every JSON API answer and every data transform, on the Rust side (`crates/*/tests/goldens`, data in
`crates/*/testdata`) and on the web side, from recorded and generated Zeroshot runs. When your change is meant to
change output, accept it with `UPDATE_GOLDENS=1 make goldens` and review the `git diff` of the golden
files: every changed line should be one you meant. A golden that changed by accident is a regression.
Known bugs are pinned as they are and listed in [tests/KNOWN-BUGS.md](tests/KNOWN-BUGS.md); fixing one
means updating its goldens and its entry there.

## Finding something to work on

- [`good first issue`](https://github.com/tomeyer97/zerochat/labels/good%20first%20issue): small, with the
  files to touch and how to test.
- [`help wanted`](https://github.com/tomeyer97/zerochat/labels/help%20wanted): the maintainer would welcome a
  pull request.
- Comment on the issue before you start, so two people do not do the same work. For larger work (a new
  feature, a new dependency, an API or UI change) agree on the approach in the issue or a
  [Discussion](https://github.com/tomeyer97/zerochat/discussions) first.

## Issues and triage

Labels are defined in [.github/labels.yml](.github/labels.yml) (applied with `scripts/labels.sh OWNER/REPO`).

| Group | Labels | Used for |
|---|---|---|
| Type | `bug`, `feature`, `docs`, `chore` | What kind of work. The issue forms set `bug` or `feature`. |
| Status | `needs-triage`, `needs-info`, `confirmed`, `blocked` | Where it stands. |
| Priority | `priority: high`, `priority: medium`, `priority: low` | Set when confirmed. |
| Area | `area: server`, `area: web`, `area: zsf`, `area: npm`, `area: release`, `area: docs` | `crates/zerochat`, `web/`, `crates/zsf`, `npm/`, release and packaging, docs. |
| Contributors | `good first issue`, `help wanted`, `dependencies` | Starter work, open for a PR, Dependabot updates. |

The flow:

1. A new issue gets `needs-triage` from its form. The maintainer responds within 7 days.
2. Triage: add an area and a priority and replace `needs-triage` with `confirmed`, or with `needs-info` and a
   question. Duplicates are closed with a link to the original; out-of-scope requests are closed with the
   reason.
3. A `needs-info` issue with no reply for 14 days may be closed; reopen it when you have the details.
4. `blocked` names what it waits for in a comment.

## Pull requests

- One change per pull request; small ones are reviewed faster. Open a draft PR to get early feedback.
- Run `make lint`, `make test` and `make goldens` before opening one.
- Keep UI text short and keep one place per fact (see DESIGN.md, UI).
- New behaviour gets a test: a unit test next to the code, a golden for a new API answer or data
  transform, or a web test for something the UI shows.
- Name every changed golden in the PR description, with why it changed.
- A user-visible change gets an entry in [CHANGELOG.md](CHANGELOG.md) under `Unreleased`.

### Commits

- Clear, imperative messages: `Show the run's cost in the list`, not `fixed stuff`. The first line is a
  short summary; the body says why.
- Sign off every commit (Developer Certificate of Origin):

  ```sh
  git commit -s
  ```

  This adds `Signed-off-by: Your Name <you@example.com>`. With it you certify that you wrote the change or
  have the right to submit it under the project's MIT licence ([developercertificate.org](https://developercertificate.org/)).
  A DCO check on every pull request flags commits without it; fix them with `git rebase --signoff main` and a
  force-push to your branch.

### After you open a PR

1. CI runs `.github/workflows/ci.yml` (see "CI on pull requests from forks" below). Your first pull request
   waits until the maintainer approves the workflow run.
2. The maintainer is asked for review automatically ([.github/CODEOWNERS](.github/CODEOWNERS)) and gives a
   first response within 7 days. No answer after 7 days: comment on the PR to ping.
3. Answer review comments with new commits; do not force-push during review unless asked (it hides what
   changed).

### How changes land

- **Squash merge only**: the PR becomes one commit on `main`, with the PR title as its message. `main` has
  a linear history; no direct pushes, every change goes through a PR. The branch is deleted on merge.
- **Required before merge**: these checks are green: `fmt`, `clippy`, `test-linux`, `web`, `goldens`;
  every review conversation is resolved; the maintainer approves. (`test-macos` and `audit` run too but do
  not block a merge.)
- **Who merges**: the maintainer. Known limit: with one maintainer, the maintainer's own PRs are merged
  after CI passes without a second reviewer. This ends when there is a second maintainer
  ([MAINTAINERS.md](MAINTAINERS.md)).

### CI on pull requests from forks

- A PR from a fork runs the same `ci.yml` as every other PR, triggered by `pull_request` (never
  `pull_request_target`), with a read-only token and no secrets.
- The runs belong to this repository (its Actions tab, its settings), so these are the checks that gate a
  merge. Checks a contributor runs in their own fork do not count.
- First-time contributors: their runs wait until the maintainer clicks **Approve and run** in the Actions
  tab. The workflow file comes from the PR's own code, so a fork PR can change what runs: the maintainer
  reads every change under `.github/` (and any script CI calls) before approving the run.

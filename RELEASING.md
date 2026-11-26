# Releasing

How a zerochat release is made. The pipeline is [.github/workflows/release.yml](.github/workflows/release.yml)
(built with dist, configured in [dist-workspace.toml](dist-workspace.toml)). Nothing is published from a laptop.

## Versioning

- zerochat follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).
- One version for everything: the `zerochat` and `zsf` binaries and all npm packages share
  `[workspace.package].version` in the root `Cargo.toml`. zsf does not version on its own.
- **Public API** (what SemVer protects):
  - CLI commands, subcommands and flags of `zerochat` and `zsf`, and their exit codes;
  - `~/.config/zerochat/config.toml` keys ([docs/configuration.md](docs/configuration.md));
  - the npm package names: `zerochat` and `@zerochat/cli-<os>-<arch>`;
  - zsf's feature and plan files (what `zsf init` reads and writes).
- **Not public API**: the HTTP API under `/api` (it is internal, between the binary and its embedded UI),
  unless a doc names a route as public; the web UI's layout; the crates' Rust items (not on crates.io).
- **0.x**: a minor bump (0.1 → 0.2) may break the public API; a patch bump (0.1.0 → 0.1.1) never does.
  From 1.0, only a major bump breaks.
- **A breaking change** needs a CHANGELOG entry under `Changed` or `Removed` that says what breaks and how
  to upgrade.
- Supported versions: the latest 0.x release only ([SECURITY.md](SECURITY.md)).

## Who can release

- The maintainer ([MAINTAINERS.md](MAINTAINERS.md)). Two guards:
  - a tag ruleset: only maintainers can create, move or delete `v*` tags;
  - the GitHub environment `release`: the jobs that publish (`host`, the GitHub Release; `publish-npm`,
    npm) wait for its required reviewer, the maintainer.
- npm publishing uses trusted publishing (OIDC) with provenance: there is no npm token to steal.
- **One person for now.** The same person writes the release PR, tags and approves. This is acceptable
  while there is one maintainer: every artifact is built by CI from a public tagged commit, attested,
  and the diff is public before the tag. **Plan:** once there is a second maintainer, the release PR needs
  their review and they become a required reviewer of the `release` environment (the person who pushed the
  tag does not approve their own release).

## Where the version lives

`scripts/bump-version.sh X.Y.Z` updates all of them:

| File | What |
|---|---|
| `Cargo.toml` | `[workspace.package] version` (the source) |
| `Cargo.lock` | the `zerochat` and `zsf` entries |
| `web/package.json` | `"version"` |
| `web/package-lock.json` | top-level `"version"` and `packages[""].version` |
| `CHANGELOG.md` | `## [Unreleased]` moved under `## [X.Y.Z] - YYYY-MM-DD`, compare links |

`npm/zerochat/package.json` stays at `0.0.0-dev`: `npm/build.js` writes the `Cargo.toml` version into the
packages it lays out, and `release.yml` checks it equals the tag.

## Checklist

1. **Pick the version** (see [Versioning](#versioning)). Check `## [Unreleased]` in
   [CHANGELOG.md](CHANGELOG.md) lists every user-visible change; the release notes come from it.
2. **Bump** on a branch from an up-to-date `main`, with a clean tree:
   ```sh
   git switch -c release/vX.Y.Z origin/main
   scripts/bump-version.sh X.Y.Z    # prints what changed; does not commit, tag or push
   git diff                         # read it; polish the CHANGELOG section
   cargo build                      # Cargo.lock must not change any further
   dist plan --tag vX.Y.Z           # optional: what the release will build
   ```
3. **PR**: commit (`git commit -s -m "Release vX.Y.Z"`), push, open a PR, wait for CI.
4. **Merge** (squash). Pull `main`.
5. **Signed tag** on the merged commit:
   ```sh
   git switch main && git pull --ff-only
   git tag -s vX.Y.Z -m "vX.Y.Z"
   git push origin vX.Y.Z
   ```
   Only tags matching `v[0-9]+.[0-9]+.[0-9]+` or `v[0-9]+.[0-9]+.[0-9]+-*` start the workflow.
6. **Watch the jobs** in Actions → Release:
   `test` → `notices`, `plan` → `build-local-artifacts` → `build-global-artifacts` → `npm-layout` →
   `host` → `publish-npm`.
7. **Approve the `release` environment** twice: for `host` (creates the GitHub Release with the
   archives, `sha256.sum`, `.sha256` files, installers, `THIRD_PARTY_NOTICES.md`, SBOMs), then for
   `publish-npm` (the 4 `@zerochat/cli-*` packages, then `zerochat`).
8. **Verify** as a user would ([docs/verify.md](docs/verify.md)):
   ```sh
   gh release download vX.Y.Z --repo tomeyer97/zerochat --dir /tmp/zc-vX.Y.Z
   cd /tmp/zc-vX.Y.Z && sha256sum -c --ignore-missing sha256.sum
   gh attestation verify zerochat-x86_64-unknown-linux-musl.tar.xz --repo tomeyer97/zerochat
   npx @zerochat/cli@X.Y.Z --version
   ```
9. **Announce**: a post in [Discussions](https://github.com/tomeyer97/zerochat/discussions)
   linking the GitHub Release.

If a job fails, fix it on `main` and release a new patch version; never move a tag. `publish-npm` skips
versions already on npm, so re-running a failed run is safe.

## Pre-release

- Version `X.Y.Z-rc.N` (or `-beta.N`); tag `vX.Y.Z-rc.N`.
- The bump script copies the Unreleased entries into the pre-release section and keeps them under
  `Unreleased`, so the final `X.Y.Z` collects everything.
- dist marks the GitHub Release as a pre-release; npm publishes with the dist-tag `next`, so
  `npm i @zerochat/cli` keeps getting `latest`.

## Rollback

A tag and a published version are never moved, reused or deleted. Instead:

1. **npm**: `npm deprecate @zerochat/cli@X.Y.Z "broken: use X.Y.Z+1"` (and each `@zerochat/cli-*@X.Y.Z`).
   Never `npm unpublish`: after 72 hours npm does not allow it, and before that it breaks lockfiles.
   If `latest` points at the bad version: `npm dist-tag add @zerochat/cli@<good> latest`.
2. **GitHub Release**: edit it, tick "Set as a pre-release" and put a warning at the top of the notes.
3. **Ship a patch** (X.Y.Z+1) with a `Fixed` entry, through the checklist above.

## Security release

Follow [docs/maintainers/security-response.md](docs/maintainers/security-response.md): the fix is
prepared privately, then released through this checklist with a `Security` entry in the CHANGELOG that
links the advisory.

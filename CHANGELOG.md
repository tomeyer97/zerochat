# Changelog

All notable changes to zerochat and zsf. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html) as described in
[RELEASING.md](RELEASING.md#versioning).

How to write entries:

- Every user-visible change adds a line under `## [Unreleased]`, in one of these groups:
  `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`.
- A breaking change goes under `Changed` or `Removed` and says how to upgrade.
- `Security` lists fixed vulnerabilities, with the advisory link.
- Keep the heading format `## [X.Y.Z] - YYYY-MM-DD`: the release workflow (dist) finds the section
  whose heading holds the tag's version and uses it as the GitHub Release notes.
  `scripts/bump-version.sh` moves `Unreleased` under the new version and dates it.

## [Unreleased]

## [0.1.0] - Unreleased

The first public release.

### Added

- **zerochat**: a local web UI for [Zeroshot](https://www.npmjs.com/package/@the-open-engine-company/zeroshot)
  runs. It shows every run on your machine and, per agent, what it did, what it cost and why it failed.
  - Runs view: history across runs, the run's graph and its executions.
  - Failure explanations: a failure card that says why a run stopped.
  - Steering: message a running agent; stop a run.
  - Replay: resume a failed run from a checkpoint.
  - Features: follow and steer features run by zsf.
  - `zerochat doctor` checks the setup; optional settings in `~/.config/zerochat/config.toml`.
  - Serves on loopback only (`http://127.0.0.1:4380/`), checks the Host header.
- **zsf (Zeroshot features)**: runs a feature cut into slices through Zeroshot, one run per slice, and
  merges each finished slice onto the feature branch. Works with zerochat or on its own.
- Platforms: Linux x64 and arm64 (static musl builds, any distribution), macOS x64 and arm64.
  No Windows build yet; use WSL.
- Install:
  - npm: `@zerochat/cli` (holds both binaries via `@zerochat/cli-<os>-<arch>`), published with provenance.
  - GitHub Release archives per platform, with sha256 checksums, build attestations, SBOMs, a shell
    installer and `THIRD_PARTY_NOTICES.md`. How to check them: [docs/verify.md](https://github.com/tomeyer97/zerochat/blob/main/docs/verify.md).
- Needs Zeroshot 16.1 or later to stop, resume and message runs; reading runs works without it.

### Known limitations

- Known bugs are pinned by the golden tests and listed in [tests/KNOWN-BUGS.md](https://github.com/tomeyer97/zerochat/blob/main/tests/KNOWN-BUGS.md).
- Benchmarks needs pb (ProgramBench runner), which is not public yet.
- The local HTTP API has no per-start token yet: see [docs/threat-model.md](https://github.com/tomeyer97/zerochat/blob/main/docs/threat-model.md).

## [0.1.0-rc.1] - 2026-11-26

A release candidate of 0.1.0 (the same contents) that runs the whole release pipeline once: the
release notes are under 0.1.0 below. npm: `npm i -g @zerochat/cli@next`.

[Unreleased]: https://github.com/tomeyer97/zerochat/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/tomeyer97/zerochat/releases/tag/v0.1.0-rc.1
[0.1.0]: https://github.com/tomeyer97/zerochat/releases/tag/v0.1.0

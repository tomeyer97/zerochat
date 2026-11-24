# Security policy

## Report a vulnerability privately

Never report a vulnerability in a public issue, pull request or Discussion.

Use GitHub's private vulnerability reporting:
[Report a vulnerability](https://github.com/tomeyer97/zerochat/security/advisories/new)
(Security tab → "Report a vulnerability"). Only the maintainers see the report.

## What to include

- The affected component (zerochat, zsf, an npm package, a release artifact) and version
  (`zerochat --version`, `zsf --version`).
- Your OS and how zerochat was started (flags such as `--listen`, `--allow-remote`, `--any-host`).
- Steps to reproduce, and what an attacker gains.
- A proof of concept, if you have one.
- How you want to be credited (name, handle, or anonymous).

## Supported versions

| Version | Supported |
|---|---|
| Latest 0.x release | Yes |
| Older releases | No: upgrade to the latest release |

## Response times

| Step | At the latest |
|---|---|
| Acknowledgement of your report | 3 days |
| A fix, or a plan with a date | 30 days |
| Public disclosure | 90 days after the report |

## Disclosure policy

- Coordinated disclosure: we agree a disclosure date with you, at most 90 days after the report.
- We publish a GitHub security advisory when the fixed release is out, and request a CVE where it applies.
- Please keep the details private until the advisory is published.

## Scope

In scope:

- zerochat (`crates/zerochat`) and its local HTTP API and web UI
- zsf (`crates/zsf`)
- the npm packages: `zerochat` and `@zerochat/cli-<os>-<arch>`
- release artifacts (archives, installers, checksums)

Out of scope:

- Zeroshot itself: report to the Zeroshot project.
- The documented non-goals of the local threat model, for example any local user or process
  using the unauthenticated API on 127.0.0.1. See [docs/threat-model.md](docs/threat-model.md).
  A way around the checks it does describe (Host check, Origin check, loopback-only listening)
  is in scope.

## Credit

We credit reporters in the advisory and the release notes, unless you ask to stay anonymous.

## Maintainers

How maintainers handle a report: [docs/maintainers/security-response.md](docs/maintainers/security-response.md).

# Access policy

Who holds which rights on GitHub and npm, and how they are granted and removed. The people are
listed in [MAINTAINERS.md](../../MAINTAINERS.md).

## Roles

| Role | Who | Can |
|---|---|---|
| Owner (Admin) | The maintainer's own account only | Settings, secrets, environments, transfer, delete |
| Maintain | Future maintainers | Merge, manage issues and releases, no settings |
| Write | Future maintainers, while they start | Push branches, merge pull requests |
| Triage | Helpers | Label, assign, close issues and pull requests |
| Read | Everyone else | Fork and open pull requests |

- Least privilege: the lowest role that does the job.
- Publishing a release needs approval in the GitHub `release` environment (required reviewer:
  the maintainer).
- Security reports are seen by the security contact in [SECURITY.md](../../SECURITY.md) and
  whoever the maintainer adds to an advisory.

## Two-factor authentication

- Required on GitHub for everyone with write access (Write, Maintain, Owner); recommended for
  Triage.
- npm: the `zerochat` organisation requires 2FA for every member, including for publishing.
- npm packages are published by CI through trusted publishing (OIDC) only. No long-lived npm
  tokens, on laptops or in repository secrets.

## Granting access

1. After sustained, good contributions over several months (reviews, fixes, triage).
2. The maintainer proposes it; the person agrees.
3. A pull request adds them to MAINTAINERS.md with their role.
4. After it is merged: grant the GitHub role, and the npm organisation membership if needed.

## Revoking access

When someone asks, or after 12 months without activity (they move to "Emeritus" in
MAINTAINERS.md).

- [ ] Pull request: move them to Emeritus in MAINTAINERS.md.
- [ ] Remove their GitHub role and team memberships.
- [ ] Remove them from the npm organisation.
- [ ] Rotate anything they had access to that is shared (secrets, tokens, environment reviewers).

On a suspected account compromise: remove access first, then do the rest.

## Bus factor

Today zerochat has **one maintainer**, who is the only Owner. If that account is lost, nobody can
change settings or publish. Adding a second maintainer as backup Owner is the plan once someone
has earned it.

# Handling a security report

The maintainer side of [SECURITY.md](../../SECURITY.md). Deadlines count from the day the report
arrives. Work in private until step 7.

## 1. Acknowledge (within 3 days)

- [ ] Reports arrive as a draft advisory (Security tab → Advisories) or by email to the security
      contact. GitHub notifies repository admins of new private reports: keep those
      notifications on, and check the email inbox.
- [ ] Reply to the reporter: received, who handles it, when they hear next.
- [ ] Report came by email or a public issue? Move it into a draft advisory (step 3). For a
      public issue, see [If the details are already public](#if-the-details-are-already-public).

## 2. Triage and severity

- [ ] Reproduce on the latest release and on `main`.
- [ ] In scope? Check the scope in SECURITY.md and the non-goals in
      [docs/threat-model.md](../threat-model.md). Out of scope: tell the reporter why; close.
- [ ] Rate the severity: low, medium, high or critical (a CVSS score is optional).
- [ ] Zeroshot's code involved? Contact Zeroshot's maintainers privately and agree who fixes what.

## 3. Fix in private

- [ ] Open (or accept) a draft GitHub security advisory: affected versions, severity.
- [ ] Add the reporter as a collaborator on the advisory, if they want to follow along.
- [ ] Create the advisory's temporary private fork; fix it there, with a test. Never push the fix
      to a public branch or open a public pull request before release.
- [ ] Ask the reporter to confirm the fix.

## 4. Embargo and coordinate

- [ ] Agree the disclosure date with the reporter: as soon as the release is ready, at most 90 days
      after the report. A fix or a plan is due within 30 days.
- [ ] Include Zeroshot's maintainers in the date when their code is involved.
- [ ] Keep the reporter updated at least every 14 days.

## 5. Request a CVE

- [ ] In the draft advisory: "Request CVE" (GitHub is the CNA). Not needed for low-impact issues
      nobody can be harmed by.

## 6. Prepare the patched release

- [ ] Merge the private fork into `main` from the advisory ("Merge pull request").
- [ ] Release as described in [RELEASING.md](../../RELEASING.md): binaries and npm packages
      come from the same tagged CI run.
- [ ] Add the fix to CHANGELOG.md under `### Security`, without exploit details.
- [ ] Deprecate affected npm versions:
      `npm deprecate zerochat@"<x.y.z" "Security fix in x.y.z; see <advisory URL>"`.

## 7. Publish

- [ ] Publish the advisory once the release is out; set the patched version.
- [ ] Credit the reporter (advisory credits) unless they asked to stay anonymous.

## 8. Announce

- [ ] Link the advisory in the release notes.
- [ ] Post in GitHub Discussions (Announcements): what is affected, upgrade to x.y.z.

## If the details are already public

The reporter or someone else went public before the fix:

- [ ] Do not argue in public; thank them and say a fix is in progress.
- [ ] Hide or lock a public issue that has exploit details; move the report into a draft advisory.
- [ ] Skip the embargo: fix, release and publish the advisory as fast as possible.
- [ ] Document a workaround (for example: stop zerochat, or do not use `--allow-remote`) in the
      advisory until the release is out.

## Who

The maintainers in [MAINTAINERS.md](../../MAINTAINERS.md). Today that is one person; see
[access.md](access.md).

'use strict';
// The release workflow's guarantees, checked on the file (actionlint checks the syntax).
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '../..');
const workflow = fs.readFileSync(path.join(ROOT, '.github/workflows/release.yml'), 'utf8');
const buildSetup = fs.readFileSync(path.join(ROOT, '.github/build-setup.yml'), 'utf8');

/** The text of each job, by id. */
function jobs(text) {
  const body = text.split(/^jobs:\n/m)[1];
  const result = {};
  const parts = body.split(/^ {2}([a-z][a-z0-9-]*):\n/m);
  for (let i = 1; i < parts.length; i += 2) result[parts[i]] = parts[i + 1];
  return result;
}
const all = jobs(workflow);
const permission = (job, name) => new RegExp(`^ {6}${name}: write$`, 'm').test(all[job]);

test('the workflow says who can release and that it is edited by hand', () => {
  const header = workflow.split(/^name:/m)[0];
  assert.match(header, /allow-dirty = \["ci"\]/);
  assert.match(header, /tag ruleset/);
  assert.match(header, /`release`\s+(#\s+)?environment/);
});

test('only version tags start it', () => {
  const on = workflow.split(/^on:\n/m)[1].split(/^\S/m)[0];
  assert.deepStrictEqual(
    [...on.matchAll(/^ {6}- "(.*)"$/gm)].map((m) => m[1]),
    ['v[0-9]+.[0-9]+.[0-9]+', 'v[0-9]+.[0-9]+.[0-9]+-*'],
  );
  assert.doesNotMatch(on, /pull_request|workflow_dispatch|branches/);
});

test('a read-only token by default; write permissions only where needed', () => {
  assert.match(workflow, /^permissions:\n {2}contents: read\n\n/m);
  const writers = (name) => Object.keys(all).filter((job) => permission(job, name)).sort();
  assert.deepStrictEqual(writers('contents'), ['host']);
  assert.deepStrictEqual(writers('id-token'), ['build-local-artifacts', 'notices', 'publish-npm']);
  assert.deepStrictEqual(writers('attestations'), ['build-local-artifacts', 'notices']);
});

test('every action is pinned by commit SHA with its version, and nothing is piped into a shell', () => {
  for (const text of [workflow, buildSetup]) {
    const uses = [...text.matchAll(/uses:\s*"?([^\s"]+)"?(.*)$/gm)];
    assert.ok(uses.length > 0);
    for (const [, ref, comment] of uses) {
      assert.match(ref, /^[\w.-]+\/[\w.-]+@[0-9a-f]{40}$/, ref);
      assert.match(comment, /^ # v\d+\.\d+\.\d+$/, ref);
    }
    // Outside comments: no `curl ... | sh`.
    assert.doesNotMatch(text, /^[^#\n]*\|\s*(sh|bash)\b/m);
  }
});

test('publishing jobs wait for the release environment; tests gate the build', () => {
  const publishing = Object.keys(all).filter((job) => /gh release create|npm publish/.test(all[job])).sort();
  assert.deepStrictEqual(publishing, ['host', 'publish-npm']);
  for (const job of publishing) assert.match(all[job], /^ {4}environment: release$/m, job);
  for (const job of Object.keys(all).filter((j) => !publishing.includes(j))) {
    assert.doesNotMatch(all[job], /environment:/, job);
  }
  assert.match(all.test, /make test/);
  assert.match(all.test, /make goldens/);
  assert.match(all.plan, /^ {4}needs: test$/m);
  assert.match(all.notices, /^ {4}needs: test$/m);
});

test('Linux binaries are checked static; npm is laid out from the same archives and checked before publishing', () => {
  assert.match(all['build-local-artifacts'], /musl-tools/);
  assert.match(all['build-local-artifacts'], /statically linked/);
  assert.match(all['npm-layout'], /node npm\/build\.js from-dist --dist target\/distrib/);
  assert.match(all['npm-layout'], /node npm\/build\.js check target\/npm --version/);
  assert.match(all['host'], /- npm-layout$/m, 'no GitHub Release before the npm layout passed');
  const publish = all['publish-npm'];
  assert.match(publish, /- host\n\s+- npm-layout/);
  assert.ok(publish.indexOf('build.js check') < publish.indexOf('npm publish'), 'the check runs before npm publish');
  assert.match(publish, /publish-order/);
  assert.match(publish, /npm publish --provenance --access public/);
  assert.doesNotMatch(workflow, /NPM_TOKEN|NODE_AUTH_TOKEN|secrets\.(?!GITHUB_TOKEN)/);
});

test('notices and SBOMs go into the release', () => {
  assert.match(all.notices, /scripts\/third-party-notices\.sh THIRD_PARTY_NOTICES\.md/);
  assert.match(all.notices, /cargo cyclonedx/);
  assert.match(all.notices, /cyclonedx-npm/);
  assert.match(all.notices, /actions\/attest-build-provenance@/);
  assert.match(all.notices, /name: artifacts-notices/);
  assert.match(all.notices, /name: artifacts-sbom/);
  assert.match(all.host, /pattern: artifacts-\*/);
  assert.match(all['build-local-artifacts'], /name: artifacts-notices/);
});

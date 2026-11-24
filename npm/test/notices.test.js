'use strict';
// THIRD_PARTY_NOTICES.md: scripts/third-party-notices.sh and npm/web-licenses.js.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const { describe, render, productionPackages } = require('../web-licenses');

const ROOT = path.resolve(__dirname, '../..');
const tmp = (prefix) => fs.mkdtempSync(path.join(os.tmpdir(), prefix));

function writePackage(dir, manifest, files = {}) {
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest));
  for (const [name, text] of Object.entries(files)) fs.writeFileSync(path.join(dir, name), text);
}

/** A web project: one bundled package (with a dependency), one type-only package, one dev dependency. */
function fakeWeb() {
  const web = tmp('zerochat-web-');
  writePackage(web, { name: 'web', version: '1.0.0', private: true, dependencies: { lib: '1.0.0', '@types/lib': '1.0.0', 'types-only': '1.0.0' }, devDependencies: { tool: '1.0.0' } });
  const nm = path.join(web, 'node_modules');
  writePackage(path.join(nm, 'lib'), { name: 'lib', version: '1.0.0', license: 'MIT', main: 'index.js', dependencies: { dep: '2.0.0' } }, { LICENSE: 'MIT text of lib' });
  writePackage(path.join(nm, 'dep'), { name: 'dep', version: '2.0.0', license: 'EPL-2.0', main: 'index.js', repository: { url: 'git+https://example.com/dep.git' } }, { 'LICENSE.md': 'EPL text' });
  writePackage(path.join(nm, '@types/lib'), { name: '@types/lib', version: '1.0.0', license: 'MIT', types: 'index.d.ts' });
  writePackage(path.join(nm, 'types-only'), { name: 'types-only', version: '1.0.0', license: 'MIT', types: 'index.d.ts' });
  writePackage(path.join(nm, 'tool'), { name: 'tool', version: '1.0.0', license: 'GPL-3.0', main: 'index.js' });
  return web;
}

test('the web licences: production packages that reach the bundle, with their licence texts', () => {
  const web = fakeWeb();
  const packages = productionPackages(web).map(describe).filter(Boolean);
  assert.deepStrictEqual(packages.map((p) => `${p.name}@${p.version} ${p.license}`).sort(), ['dep@2.0.0 EPL-2.0', 'lib@1.0.0 MIT']);
  const md = render(packages);
  assert.match(md, /^## Web UI\n/);
  assert.match(md, /^- \[lib 1\.0\.0\]\(https:\/\/www\.npmjs\.com\/package\/lib\/v\/1\.0\.0\): MIT$/m);
  assert.match(md, /### dep 2\.0\.0 \(EPL-2\.0\)\n\nSource code: https:\/\/example\.com\/dep\.git \(unmodified\)\.\n\n```text\nEPL text\n```/);
  assert.match(md, /```text\nMIT text of lib\n```/);
  assert.doesNotMatch(md, /tool|@types|types-only/);
  assert.throws(() => render([{ name: 'x', version: '1', license: 'UNKNOWN', texts: [] }]), /no licence declared by x@1/);
});

test('third-party-notices.sh writes the Rust and web sections, and refuses another cargo-about', { skip: process.platform === 'win32' }, () => {
  const bin = tmp('zerochat-stub-');
  const stub = path.join(bin, 'cargo-about');
  fs.writeFileSync(
    stub,
    `#!/bin/sh
if [ "$1" = --version ]; then echo "cargo-about \${STUB_VERSION:-0.9.3}"; exit 0; fi
out=""; prev=""
for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done
printf '## Rust crates\\n\\nstub: %s\\n' "$*" > "$out"
`,
    { mode: 0o755 },
  );
  const out = path.join(tmp('zerochat-notices-'), 'THIRD_PARTY_NOTICES.md');
  const env = { ...process.env, CARGO_ABOUT: stub, WEB_DIR: fakeWeb() };
  const run = spawnSync(path.join(ROOT, 'scripts/third-party-notices.sh'), [out], { env, encoding: 'utf8' });
  assert.strictEqual(run.status, 0, run.stderr);
  const md = fs.readFileSync(out, 'utf8');
  assert.match(md, /^# Third-party notices\n/);
  assert.match(md, /^stub: generate --workspace --fail about\.hbs -o /m);
  assert.match(md, /^## Web UI$/m);
  assert.match(md, /^- \[dep 2\.0\.0\]/m);

  const old = spawnSync(path.join(ROOT, 'scripts/third-party-notices.sh'), [out], { env: { ...env, STUB_VERSION: '0.1.0' }, encoding: 'utf8' });
  assert.strictEqual(old.status, 1);
  assert.match(old.stderr, /needs cargo-about 0\.9\.3 \(found: 0\.1\.0\)/);

  const version = spawnSync(path.join(ROOT, 'scripts/third-party-notices.sh'), ['--tool-version'], { encoding: 'utf8' });
  assert.strictEqual(version.stdout, '0.9.3\n');
});

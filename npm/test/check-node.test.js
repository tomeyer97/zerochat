// scripts/check-node.sh: `make` checks node and npm first and fails clearly outside the one
// supported range (web/package.json engines), e.g. on the Node 18 Ubuntu 24.04 ships.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = path.resolve(__dirname, '../..');
const script = path.join(root, 'scripts/check-node.sh');
const range = JSON.parse(fs.readFileSync(path.join(root, 'web/package.json'), 'utf8')).engines.node;

// The system tools the script and the Makefile use, linked into a directory of their own: the
// system's PATH may have a real node or npm (/usr/bin/node on Ubuntu), which must not be found.
const TOOLS = ['sh', 'sed', 'cut', 'head', 'tr', 'cat', 'dirname', 'make', 'find', 'grep', 'awk'];
function whichTool(name) {
  return spawnSync('/bin/sh', ['-c', `command -v ${name}`], { encoding: 'utf8' }).stdout.trim();
}

/** A PATH with a fake `node` reporting `version` (none when null), a fake `npm` (unless `npm` is false), a fake `cargo` and the system tools above, nothing else. */
function fakePath(version, { npm = true } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'check-node-'));
  for (const name of TOOLS) {
    const found = whichTool(name);
    if (found) fs.symlinkSync(found, path.join(dir, name));
  }
  const tool = (name, body) => {
    fs.writeFileSync(path.join(dir, name), `#!/bin/sh\n${body}\n`);
    fs.chmodSync(path.join(dir, name), 0o755);
  };
  if (version !== null) tool('node', `echo v${version}`);
  if (npm) tool('npm', 'exit 0');
  tool('cargo', 'echo "fake cargo ran" >&2; exit 3');
  return dir;
}

function check(version, options) {
  return spawnSync('/bin/sh', [script], { env: { PATH: fakePath(version, options) }, encoding: 'utf8' });
}

test('the range is the one in web/package.json and npm/zerochat/package.json', () => {
  assert.equal(range, '^22.22.2 || ^24.15.0 || >=26.0.0');
  const shim = JSON.parse(fs.readFileSync(path.join(root, 'npm/zerochat/package.json'), 'utf8'));
  assert.equal(shim.engines.node, range);
  assert.equal(fs.readFileSync(path.join(root, '.nvmrc'), 'utf8').trim(), '22.22.2');
});

test('Node 18 (Ubuntu 24.04) fails with the required range and how to get Node', () => {
  const r = check('18.19.1');
  assert.equal(r.status, 1);
  assert.match(r.stderr, /zerochat needs Node\.js \^22\.22\.2 \|\| \^24\.15\.0 \|\| >=26\.0\.0 with npm; found node 18\.19\.1\./);
  assert.match(r.stderr, /nodejs\.org/);
  assert.match(r.stderr, /nvm/);
});

test('versions inside the range pass, versions outside fail', () => {
  for (const v of ['22.22.2', '22.30.0', '24.15.0', '24.20.1', '26.0.0', '27.1.0']) {
    const r = check(v);
    assert.equal(r.status, 0, `${v}: ${r.stderr}`);
    assert.equal(r.stderr, '');
  }
  for (const v of ['20.19.0', '22.22.1', '22.0.0', '23.11.0', '24.14.9', '25.9.0', 'garbage']) {
    assert.equal(check(v).status, 1, v);
  }
});

test('missing node or npm fails with the range', () => {
  const noNode = check(null);
  assert.equal(noNode.status, 1);
  assert.match(noNode.stderr, /node was not found on PATH/);
  assert.ok(noNode.stderr.includes(range));
  const noNpm = check('22.22.2', { npm: false });
  assert.equal(noNpm.status, 1);
  assert.match(noNpm.stderr, /npm was not found on PATH/);
});

test('`make` on Node 18 stops at the check, before building anything', (t) => {
  const make = whichTool('make');
  if (!make) return t.skip('make is not installed');
  const env = { PATH: fakePath('18.19.1'), HOME: os.tmpdir() };
  const r = spawnSync(make, ['-C', root, '--no-print-directory', 'run'], { env, encoding: 'utf8', timeout: 60_000 });
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /zerochat needs Node\.js .* found node 18\.19\.1/);
  assert.doesNotMatch(r.stderr, /fake cargo ran/);
});

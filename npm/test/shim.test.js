'use strict';
// node --test npm/test   (also part of `make test`)
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');

const { platformPackage, binaryPath } = require('../zerochat/lib/shim');
const spec = require('../zerochat/packages.json');

const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), 'zerochat-shim-'));

test('each supported host resolves to <platformPrefix><platform>-<arch>', () => {
  assert.strictEqual(platformPackage('linux', 'x64'), '@zerochat/cli-linux-x64');
  assert.strictEqual(platformPackage('linux', 'arm64'), '@zerochat/cli-linux-arm64');
  assert.strictEqual(platformPackage('darwin', 'x64'), '@zerochat/cli-darwin-x64');
  assert.strictEqual(platformPackage('darwin', 'arm64'), '@zerochat/cli-darwin-arm64');
  const scoped = { ...spec, platformPrefix: '@scope/zerochat-' };
  assert.strictEqual(platformPackage('darwin', 'arm64', scoped), '@scope/zerochat-darwin-arm64');
});

test('an unsupported host names the supported ones (and WSL on Windows)', () => {
  assert.throws(() => platformPackage('freebsd', 'x64'), {
    message: 'UNSUPPORTED_ZEROCHAT_HOST: no prebuilt binary for freebsd/x64; supported hosts: linux/x64, linux/arm64, darwin/x64, darwin/arm64',
  });
  assert.throws(() => platformPackage('win32', 'x64'), /supported hosts: .*; on Windows, use WSL$/);
});

test('a missing platform package says how to reinstall', () => {
  const resolve = (request) => {
    const error = new Error(`Cannot find module '${request}'`);
    error.code = 'MODULE_NOT_FOUND';
    throw error;
  };
  assert.throws(() => binaryPath('zsf', { platform: 'linux', arch: 'x64', resolve }), {
    message:
      'NATIVE_BINARY_MISSING: @zerochat/cli-linux-x64 is not installed; reinstall zerochat with optional dependencies enabled (npm i -g zerochat, without --omit=optional or --no-optional)',
  });
});

test('both binaries come from the same platform package directory', () => {
  const dir = tmp();
  fs.mkdirSync(path.join(dir, 'bin'));
  for (const bin of ['zerochat', 'zsf']) fs.writeFileSync(path.join(dir, 'bin', bin), '');
  const resolve = (request) => {
    assert.strictEqual(request, '@zerochat/cli-darwin-arm64/package.json');
    return path.join(dir, 'package.json');
  };
  const options = { platform: 'darwin', arch: 'arm64', resolve };
  assert.strictEqual(binaryPath('zerochat', options), path.join(dir, 'bin', 'zerochat'));
  assert.strictEqual(path.dirname(binaryPath('zsf', options)), path.dirname(binaryPath('zerochat', options)));
  assert.throws(() => binaryPath('zerochat', { ...options, platform: 'darwin' , resolve: () => path.join(tmp(), 'package.json') }), /^Error: NATIVE_BINARY_MISSING: .*bin\/zerochat; reinstall zerochat/);
});

// An installed layout: node_modules/zerochat (the shims) beside node_modules/@zerochat/cli-<host>.
function installed(script) {
  const root = tmp();
  const modules = path.join(root, 'node_modules');
  const main = path.join(modules, 'zerochat');
  fs.cpSync(path.join(__dirname, '../zerochat'), main, { recursive: true });
  const host = path.join(modules, platformPackage());
  fs.mkdirSync(path.join(host, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(host, 'package.json'), '{}');
  for (const bin of ['zerochat', 'zsf']) fs.writeFileSync(path.join(host, 'bin', bin), `#!/bin/sh\n${script}\n`, { mode: 0o755 });
  return { shim: (bin) => path.join(main, 'bin', `${bin}.js`), host };
}

const unix = process.platform !== 'win32' && spec.targets.some((t) => t.platform === process.platform && t.arch === process.arch);

test('the shim runs the binary with its arguments and returns its exit code', { skip: !unix }, () => {
  const { shim } = installed('echo "$(basename "$0") $*"; exit 7');
  const zerochat = spawnSync(process.execPath, [shim('zerochat'), 'doctor', '--json'], { encoding: 'utf8' });
  assert.strictEqual(zerochat.stdout, 'zerochat doctor --json\n');
  assert.strictEqual(zerochat.status, 7);
  const zsf = spawnSync(process.execPath, [shim('zsf'), 'status'], { encoding: 'utf8' });
  assert.strictEqual(zsf.stdout, 'zsf status\n');
});

test('the shim dies by the signal that killed the binary', { skip: !unix }, () => {
  const { shim } = installed('kill -TERM $$');
  const result = spawnSync(process.execPath, [shim('zerochat')], { encoding: 'utf8' });
  assert.strictEqual(result.signal, 'SIGTERM');
});

test('a SIGTERM sent to the shim reaches the binary', { skip: !unix }, async () => {
  const marker = path.join(tmp(), 'got');
  const { shim } = installed(`trap 'echo TERM > "${marker}"; exit 3' TERM; echo ready; while :; do sleep 0.05; done`);
  const child = spawn(process.execPath, [shim('zerochat')], { stdio: ['ignore', 'pipe', 'inherit'] });
  await new Promise((resolve) => child.stdout.once('data', resolve));
  child.kill('SIGTERM');
  const [code] = await new Promise((resolve) => child.once('exit', (...a) => resolve(a)));
  assert.strictEqual(fs.readFileSync(marker, 'utf8'), 'TERM\n');
  assert.strictEqual(code, 3);
});

test('a missing platform package fails with the message, exit 1', () => {
  const root = tmp();
  const main = path.join(root, 'node_modules', 'zerochat');
  fs.cpSync(path.join(__dirname, '../zerochat'), main, { recursive: true });
  const result = spawnSync(process.execPath, [path.join(main, 'bin', 'zsf.js')], { encoding: 'utf8', cwd: root });
  assert.strictEqual(result.status, 1);
  assert.match(result.stderr, /^zsf: (NATIVE_BINARY_MISSING: @zerochat\/cli-\w+-\w+ is not installed|UNSUPPORTED_ZEROCHAT_HOST)/);
});

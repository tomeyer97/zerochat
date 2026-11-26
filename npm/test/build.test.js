'use strict';
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFileSync, spawnSync } = require('child_process');

const { layout, fromDist, check, publishOrder, binaryInfo, workspacePackage, parseArgs, packList } = require('../build');
const { platformPackage } = require('../zerochat/lib/shim');
const spec = require('../zerochat/packages.json');

const ROOT = path.resolve(__dirname, '../..');
const BUILD = path.join(ROOT, 'npm/build.js');
const tmp = (prefix) => fs.mkdtempSync(path.join(os.tmpdir(), prefix));

// Minimal binaries: the headers binaryInfo reads, nothing else.
const MACHINE = { x64: 62, arm64: 183 };
const CPU = { x64: 0x01000007, arm64: 0x0100000c };

/** A 64-bit little-endian ELF. `link`: 'static', 'static-pie' (PT_DYNAMIC, no DT_NEEDED) or 'glibc'. */
function elf(arch, link = 'static') {
  const phdrs = link === 'static' ? [] : link === 'static-pie' ? [2] : [3, 2];
  const dynOffset = 64 + phdrs.length * 56;
  const buf = Buffer.alloc(dynOffset + 32);
  buf.writeUInt32BE(0x7f454c46, 0);
  buf[4] = 2; // ELFCLASS64
  buf[5] = 1; // little-endian
  buf.writeUInt16LE(MACHINE[arch], 18);
  buf.writeBigUInt64LE(64n, 32); // e_phoff
  buf.writeUInt16LE(56, 54); // e_phentsize
  buf.writeUInt16LE(phdrs.length, 56);
  phdrs.forEach((type, i) => {
    const ph = 64 + i * 56;
    buf.writeUInt32LE(type, ph);
    if (type === 2) {
      buf.writeBigUInt64LE(BigInt(dynOffset), ph + 8);
      buf.writeBigUInt64LE(32n, ph + 32);
    }
  });
  // The dynamic section: DT_NEEDED (libc.so.6) for glibc, then DT_NULL.
  if (link === 'glibc') buf.writeBigInt64LE(1n, dynOffset);
  return buf;
}

function macho(arch) {
  const buf = Buffer.alloc(32);
  buf.writeUInt32LE(0xfeedfacf, 0);
  buf.writeUInt32LE(CPU[arch], 4);
  return buf;
}

/** A directory with zerochat and zsf built for target `t` (or the given bytes). */
function bins(t, bytes) {
  const dir = tmp('zerochat-bins-');
  const content = bytes ?? (t.platform === 'darwin' ? macho(t.arch) : elf(t.arch));
  for (const b of spec.binaries) fs.writeFileSync(path.join(dir, b), content);
  return dir;
}

const target = (key) => spec.targets.find((t) => `${t.platform}-${t.arch}` === key);
const allBinaries = () => Object.fromEntries(spec.targets.map((t) => [`${t.platform}-${t.arch}`, bins(t)]));
function noticesFile() {
  const file = path.join(tmp('zerochat-notices-'), 'THIRD_PARTY_NOTICES.md');
  fs.writeFileSync(file, '# Third-party notices\n');
  return file;
}
const readJson = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));

test('the version and repository come from the workspace Cargo.toml', () => {
  const cargo = workspacePackage('[workspace]\nmembers = []\n\n[workspace.package]\nversion = "1.2.3"\nrepository = "https://example.com/r"\n\n[profile.dist]\nversion = "9"\n');
  assert.deepStrictEqual(cargo, { version: '1.2.3', repository: 'https://example.com/r' });
  assert.match(workspacePackage().version, /^\d+\.\d+\.\d+/);
  const zerochat = fs.readFileSync(path.join(ROOT, 'crates/zerochat/Cargo.toml'), 'utf8');
  const zsf = fs.readFileSync(path.join(ROOT, 'crates/zsf/Cargo.toml'), 'utf8');
  for (const crate of [zerochat, zsf]) assert.match(crate, /^version\.workspace = true$/m);
});

test('neither crate can be published to crates.io, and dist still releases both', () => {
  const workspace = fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8');
  const section = workspace.split(/^\[workspace\.package\]\s*$/m)[1].split(/^\[/m)[0];
  assert.match(section, /^publish = false$/m);
  for (const crate of ['zerochat', 'zsf']) {
    const manifest = fs.readFileSync(path.join(ROOT, 'crates', crate, 'Cargo.toml'), 'utf8');
    assert.match(manifest, /^publish\.workspace = true$/m, crate);
    assert.match(manifest, /^\[package\.metadata\.dist\]\ndist = true$/m, crate);
  }
});

test('the npm names: @zerochat/cli, platform packages beside it in the @zerochat scope', () => {
  assert.strictEqual(spec.name, '@zerochat/cli');
  assert.deepStrictEqual(
    spec.targets.map((t) => platformPackage(t.platform, t.arch)),
    ['@zerochat/cli-linux-x64', '@zerochat/cli-linux-arm64', '@zerochat/cli-darwin-x64', '@zerochat/cli-darwin-arm64'],
  );
  assert.doesNotMatch(JSON.stringify(spec), /DECISIONS/);
});

test('binaryInfo tells ELF from Mach-O, the CPU, and static from dynamically linked', () => {
  assert.deepStrictEqual(binaryInfo(elf('x64')), { format: 'elf', arch: 'x64', static: true });
  assert.deepStrictEqual(binaryInfo(elf('arm64', 'static-pie')), { format: 'elf', arch: 'arm64', static: true });
  assert.deepStrictEqual(binaryInfo(elf('x64', 'glibc')), { format: 'elf', arch: 'x64', static: false });
  assert.deepStrictEqual(binaryInfo(macho('arm64')), { format: 'macho', arch: 'arm64' });
  assert.strictEqual(binaryInfo(Buffer.from('binary')), null);
  // A real dynamically linked binary: the node running this test.
  if (process.platform === 'linux' && (process.arch === 'x64' || process.arch === 'arm64')) {
    const node = binaryInfo(fs.readFileSync(process.execPath));
    assert.deepStrictEqual([node.format, node.arch], ['elf', process.arch]);
    assert.strictEqual(node.static, false, 'node links libc dynamically');
  }
});

test('the layout: shims and optional platform packages, both binaries per platform, no postinstall', () => {
  const out = tmp('zerochat-npm-');
  const darwin = bins(target('darwin-arm64'));
  const notices = noticesFile();
  const dirs = layout({ out, binaries: { 'darwin-arm64': darwin }, notices, cargo: { version: '1.2.3', repository: 'https://example.com/r' } });
  assert.deepStrictEqual(
    dirs.map((d) => path.relative(out, d)),
    [path.join('@zerochat', 'cli'), path.join('@zerochat', 'cli-darwin-arm64')],
  );

  const main = readJson(path.join(out, '@zerochat/cli/package.json'));
  assert.strictEqual(main.version, '1.2.3');
  assert.strictEqual(main.license, 'MIT');
  assert.deepStrictEqual(main.bin, { zerochat: 'bin/zerochat.js', zsf: 'bin/zsf.js' });
  assert.deepStrictEqual(main.optionalDependencies, {
    '@zerochat/cli-linux-x64': '1.2.3',
    '@zerochat/cli-linux-arm64': '1.2.3',
    '@zerochat/cli-darwin-x64': '1.2.3',
    '@zerochat/cli-darwin-arm64': '1.2.3',
  });
  assert.deepStrictEqual(main.peerDependencies, { '@the-open-engine-company/zeroshot': '>=16.1' });
  assert.deepStrictEqual(main.peerDependenciesMeta, { '@the-open-engine-company/zeroshot': { optional: true } });
  assert.strictEqual(main.scripts, undefined, 'no lifecycle scripts');
  assert.strictEqual(main.private, true, 'one platform of four: the main package cannot be published');
  assert.deepStrictEqual(main.files, ['bin', 'lib', 'packages.json', 'THIRD_PARTY_NOTICES.md']);
  for (const f of ['bin/zerochat.js', 'bin/zsf.js', 'lib/shim.js', 'packages.json', 'README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md']) {
    assert.ok(fs.existsSync(path.join(out, '@zerochat/cli', f)), f);
  }

  const dir = path.join(out, '@zerochat/cli-darwin-arm64');
  const platform = readJson(path.join(dir, 'package.json'));
  assert.strictEqual(platform.name, '@zerochat/cli-darwin-arm64');
  assert.deepStrictEqual([platform.os, platform.cpu, platform.libc, platform.private], [['darwin'], ['arm64'], undefined, undefined]);
  assert.deepStrictEqual(platform.files, ['bin', 'THIRD_PARTY_NOTICES.md']);
  assert.strictEqual(platform.scripts, undefined);
  for (const b of ['zerochat', 'zsf']) {
    const file = path.join(dir, 'bin', b);
    assert.deepStrictEqual(fs.readFileSync(file), macho('arm64'));
    if (process.platform !== 'win32') assert.ok(fs.statSync(file).mode & 0o100, `${b} is executable`);
  }
  for (const f of ['README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md']) assert.ok(fs.existsSync(path.join(dir, f)), f);
  assert.match(fs.readFileSync(path.join(dir, 'README.md'), 'utf8'), /^Internal package of \[`@zerochat\/cli`\].*Install `@zerochat\/cli` instead/m);

  layout({ out, binaries: { 'linux-x64': bins(target('linux-x64')) }, cargo: { version: '1.2.3' } });
  const linux = readJson(path.join(out, '@zerochat/cli-linux-x64/package.json'));
  assert.deepStrictEqual([linux.os, linux.cpu, linux.libc, linux.private], [['linux'], ['x64'], ['glibc', 'musl'], undefined]);
  assert.match(linux.description, /\(x86_64-unknown-linux-musl\)/);
  assert.throws(() => layout({ out, binaries: { 'win32-x64': darwin }, cargo: { version: '1' } }), /unknown platform win32-x64/);
});

test('the npm README is written for npm: absolute links, no contributor commands', () => {
  const out = tmp('zerochat-npm-');
  layout({ out, cargo: { version: '1.0.0' } });
  const readme = fs.readFileSync(path.join(out, '@zerochat/cli/README.md'), 'utf8');
  assert.strictEqual(readme, fs.readFileSync(path.join(ROOT, 'npm/zerochat/README.md'), 'utf8'));
  assert.notStrictEqual(readme, fs.readFileSync(path.join(ROOT, 'README.md'), 'utf8'));
  assert.match(readme, /zerochat is an independent community project, not affiliated with The Open Engine Company\./);
  assert.match(readme, /!\[[^\]]*\]\(https:\/\/raw\.githubusercontent\.com\/tomeyer97\/zerochat\/[^/]+\/docs\/img\/failed-run\.png\)/);
  assert.match(readme, /npx @zerochat\/cli/);
  assert.match(readme, /npm i -g @zerochat\/cli/);
  assert.match(readme, /\^22\.22\.2 \|\| \^24\.15\.0 \|\| >=26\.0\.0/);
  for (const doc of ['docs/install.md', 'SECURITY.md', 'docs/verify.md']) {
    assert.ok(readme.includes(`https://github.com/tomeyer97/zerochat/blob/main/${doc}`), doc);
  }
  assert.doesNotMatch(readme, /nothing is published|make install|make test|make goldens/i);
  // Every link and image is absolute.
  for (const [, url] of readme.matchAll(/\]\(([^)]+)\)/g)) assert.match(url, /^https:\/\//, url);
});

test('a dynamically linked Linux binary is refused, or labelled a local glibc build that cannot be published', () => {
  const out = tmp('zerochat-npm-');
  const glibc = bins(target('linux-x64'), elf('x64', 'glibc'));
  assert.throws(() => layout({ out, binaries: { 'linux-x64': glibc }, cargo: { version: '1.0.0' } }), /is dynamically linked: a Linux package holds the static x86_64-unknown-linux-musl build/);
  assert.throws(() => layout({ out, binaries: { 'linux-x64': bins(target('linux-x64'), elf('arm64')) }, cargo: { version: '1.0.0' } }), /not a 64-bit ELF x64 binary/);
  assert.throws(() => layout({ out, binaries: { 'darwin-x64': bins(target('darwin-x64'), elf('x64')) }, cargo: { version: '1.0.0' } }), /not a 64-bit Mach-O x64 binary/);

  layout({ out, binaries: { 'linux-x64': glibc }, allowGlibc: true, cargo: { version: '1.0.0' } });
  const pkg = readJson(path.join(out, '@zerochat/cli-linux-x64/package.json'));
  assert.strictEqual(pkg.private, true);
  assert.deepStrictEqual(pkg.libc, ['glibc']);
  assert.match(pkg.description, /^LOCAL TEST BUILD, not publishable: .* built against glibc \(dynamically linked\), not x86_64-unknown-linux-musl$/);
  assert.doesNotMatch(pkg.description, /\(x86_64-unknown-linux-musl\)/);
  // --allow-glibc changes nothing for a static build.
  layout({ out, binaries: { 'linux-x64': bins(target('linux-x64')) }, allowGlibc: true, cargo: { version: '1.0.0' } });
  assert.strictEqual(readJson(path.join(out, '@zerochat/cli-linux-x64/package.json')).private, undefined);
});

test('the completeness check passes only a full, consistent release set', () => {
  const out = tmp('zerochat-npm-');
  const notices = noticesFile();
  const full = () => layout({ out, binaries: allBinaries(), notices, cargo: { version: '1.2.3' } });
  full();
  assert.deepStrictEqual(check(out, { version: '1.2.3' }), []);
  assert.strictEqual(readJson(path.join(out, '@zerochat/cli/package.json')).private, undefined, 'complete: publishable');

  // The tag says another version.
  assert.ok(check(out, { version: '1.2.4' }).some((p) => p === '@zerochat/cli: version 1.2.3, not 1.2.4'));

  // A platform package is missing: the main package is private, and check names the gap.
  const partial = tmp('zerochat-npm-');
  layout({ out: partial, binaries: { 'darwin-arm64': bins(target('darwin-arm64')) }, notices, cargo: { version: '1.2.3' } });
  const problems = check(partial, { version: '1.2.3' });
  assert.ok(problems.includes('@zerochat/cli: private (a local or incomplete build)'), problems.join('\n'));
  for (const name of ['@zerochat/cli-linux-x64', '@zerochat/cli-linux-arm64', '@zerochat/cli-darwin-x64']) {
    assert.ok(problems.includes(`missing package ${name} (no package.json)`), name);
  }

  // A binary is missing, or replaced by a dynamically linked one.
  fs.rmSync(path.join(out, '@zerochat/cli-linux-arm64/bin/zsf'));
  fs.writeFileSync(path.join(out, '@zerochat/cli-linux-x64/bin/zerochat'), elf('x64', 'glibc'));
  const bad = check(out, { version: '1.2.3' });
  assert.ok(bad.some((p) => /^@zerochat\/cli-linux-arm64: missing .*bin\/zsf$/.test(p)), bad.join('\n'));
  assert.ok(bad.some((p) => /^@zerochat\/cli-linux-x64: .*zerochat is dynamically linked/.test(p)), bad.join('\n'));

  // optionalDependencies that do not pin exactly the four platform packages at the version.
  full();
  const mainFile = path.join(out, '@zerochat/cli/package.json');
  const main = readJson(mainFile);
  delete main.optionalDependencies['@zerochat/cli-darwin-x64'];
  fs.writeFileSync(mainFile, JSON.stringify(main));
  assert.ok(check(out, { version: '1.2.3' }).some((p) => p.startsWith('@zerochat/cli: optionalDependencies ')));

  // No notices file.
  layout({ out, binaries: allBinaries(), cargo: { version: '1.2.3' } });
  assert.ok(check(out, { version: '1.2.3' }).includes('@zerochat/cli: no THIRD_PARTY_NOTICES.md'));

  // The command exits 1 and says nothing may be published.
  const cli = spawnSync(process.execPath, [BUILD, 'check', out, '--version', '1.2.3'], { encoding: 'utf8' });
  assert.strictEqual(cli.status, 1);
  assert.match(cli.stderr, /nothing may be published/);
});

test('publish order: the four platform packages first, @zerochat/cli last', () => {
  const out = '/o';
  assert.deepStrictEqual(publishOrder(out), [
    '/o/@zerochat/cli-linux-x64',
    '/o/@zerochat/cli-linux-arm64',
    '/o/@zerochat/cli-darwin-x64',
    '/o/@zerochat/cli-darwin-arm64',
    '/o/@zerochat/cli',
  ].map((p) => path.join(p)));
});

test('from-dist lays out all five packages from dist archives, checking each sha256', { skip: process.platform === 'win32' }, () => {
  const dist = tmp('zerochat-distrib-');
  for (const t of spec.targets) {
    for (const bin of spec.binaries) {
      const stage = tmp('zerochat-stage-');
      const name = `${bin}-${t.target}`;
      fs.mkdirSync(path.join(stage, name));
      fs.writeFileSync(path.join(stage, name, bin), t.platform === 'darwin' ? macho(t.arch) : elf(t.arch, 'static-pie'), { mode: 0o755 });
      fs.writeFileSync(path.join(stage, name, 'README.md'), '');
      const archive = path.join(dist, `${name}.tar.xz`);
      execFileSync('tar', ['-cJf', archive, '-C', stage, name]);
      const sum = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
      fs.writeFileSync(`${archive}.sha256`, `${sum} *${name}.tar.xz\n`);
    }
  }
  const out = tmp('zerochat-npm-');
  const notices = noticesFile();
  const dirs = fromDist({ dist, out, notices, cargo: { version: '2.0.0' } });
  assert.strictEqual(dirs.length, 5);
  assert.deepStrictEqual(check(out, { version: '2.0.0' }), []);

  // The CLI does the same.
  const cliOut = tmp('zerochat-npm-');
  const cli = spawnSync(process.execPath, [BUILD, 'from-dist', '--dist', dist, '--out', cliOut, '--notices', notices], { encoding: 'utf8' });
  assert.strictEqual(cli.status, 0, cli.stderr);
  assert.strictEqual(cli.stdout.trim().split('\n').length, 5);

  // A tampered archive fails its checksum; a missing one fails.
  const archive = path.join(dist, 'zsf-aarch64-apple-darwin.tar.xz');
  fs.appendFileSync(archive, 'x');
  assert.throws(() => fromDist({ dist, out, cargo: { version: '2.0.0' } }), /sha256 mismatch for .*zsf-aarch64-apple-darwin\.tar\.xz/);
  fs.rmSync(archive);
  assert.throws(() => fromDist({ dist, out, cargo: { version: '2.0.0' } }), /missing .*zsf-aarch64-apple-darwin\.tar\.xz$/);
});

test('npm and dist build the same targets', () => {
  const dist = fs.readFileSync(path.join(ROOT, 'dist-workspace.toml'), 'utf8');
  const targets = JSON.parse(dist.match(/^targets = (\[.*\])$/m)[1]);
  assert.deepStrictEqual([...targets].sort(), spec.targets.map((t) => t.target).sort());
  // Every archive carries the notices; release.yml is edited by hand, so dist must not regenerate it.
  assert.match(dist, /^include = \["THIRD_PARTY_NOTICES\.md"\]$/m);
  assert.match(dist, /^allow-dirty = \["ci"\]$/m);
});

test('--bin, --out and the other flags are parsed', () => {
  const args = parseArgs(['--out', 'o', '--bin', 'linux-x64=target/release', '--allow-glibc', '--notices', 'n.md']);
  assert.strictEqual(args.out, path.resolve('o'));
  assert.deepStrictEqual(args.binaries, { 'linux-x64': path.resolve('target/release') });
  assert.strictEqual(args.allowGlibc, true);
  assert.strictEqual(args.notices, path.resolve('n.md'));
  assert.strictEqual(parseArgs(['--out', 'o']).allowGlibc, undefined);
  assert.throws(() => parseArgs(['--bin', 'linux-x64']), /--bin wants/);
});

test('layout names packages with the shim\'s rule (one place)', () => {
  const out = tmp('zerochat-npm-');
  const scoped = { ...spec, name: 'other', platformPrefix: '@scope/other-' };
  const dirs = layout({ out, spec: scoped, binaries: { 'linux-arm64': bins(target('linux-arm64')) }, cargo: { version: '1.0.0' } });
  assert.strictEqual(path.relative(out, dirs[1]), path.join(platformPackage('linux', 'arm64', scoped)));
  assert.strictEqual(platformPackage('linux', 'arm64', scoped), '@scope/other-linux-arm64');
  const main = readJson(path.join(dirs[0], 'package.json'));
  assert.deepStrictEqual(Object.keys(main.optionalDependencies), spec.targets.map((t) => platformPackage(t.platform, t.arch, scoped)));
});

test('pack-list prints each file and a total', () => {
  const json = JSON.stringify([{ name: '@zerochat/cli', version: '1.0.0', entryCount: 2, size: 4500, files: [{ path: 'bin/zsf.js', size: 100 }, { path: 'bin/zsf', size: 5 << 20 }] }]);
  assert.deepStrictEqual(packList(json), ['      0.1 kB  bin/zsf.js', '      5.0 MB  bin/zsf', '   @zerochat/cli@1.0.0: 2 files, 4.4 kB packed']);
});

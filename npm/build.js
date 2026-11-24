#!/usr/bin/env node
'use strict';
// Lays out and checks the npm packages (never publishes; .github/workflows/release.yml does).
//
//   node npm/build.js --out <dir> [--bin <platform>-<arch>=<dir>]... [--notices <file>] [--allow-glibc]
//   node npm/build.js from-dist --dist <dir> --out <dir> [--notices <file>]
//   node npm/build.js check <dir> [--version <version>]   # the completeness check; exit 1 on a problem
//   node npm/build.js publish-order <dir>                 # platform packages first, the main one last
//   node npm/build.js targets        # `<rust target> <platform>-<arch>` per line
//   node npm/build.js version        # the workspace version
//   npm pack --dry-run --json | node npm/build.js pack-list   # a packed file list, sizes readable
//
// Writes <out>/<name> (the main package: shims, optionalDependencies on every platform package) and
// <out>/<platformPrefix><platform>-<arch> for each platform given (both binaries in bin/). Names come
// from npm/zerochat/packages.json; the version and repository from the root Cargo.toml.
//
// A release publishes exactly what `from-dist` lays out from dist's archives (the attested binaries),
// after `check` passed. A Linux package must hold a static binary; --allow-glibc (scripts/package.sh,
// local test builds only) lays out a dynamically linked one, labelled as such and marked private so
// npm refuses to publish it. The main package is private unless all its platform packages are laid out.

const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { execFileSync } = require('child_process');

const { platformPackage } = require('./zerochat/lib/shim');

const ROOT = path.resolve(__dirname, '..');
const MAIN = path.join(__dirname, 'zerochat');
const NOTICES = 'THIRD_PARTY_NOTICES.md';

/** `version` and `repository` of [workspace.package] in the root Cargo.toml. */
function workspacePackage(cargoToml = fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8')) {
  const section = cargoToml.split(/^\[workspace\.package\]\s*$/m)[1]?.split(/^\[/m)[0] ?? '';
  const field = (key) => section.match(new RegExp(`^${key}\\s*=\\s*"([^"]*)"`, 'm'))?.[1];
  const version = field('version');
  if (!version) throw new Error('no version in [workspace.package] of Cargo.toml');
  return { version, repository: field('repository') };
}

/**
 * What a binary is: `{ format: 'elf' | 'macho', arch: 'x64' | 'arm64' | <number>, static }`, or
 * null for anything else. `static` (ELF only) means no program interpreter and no needed shared
 * library, as `file` says "statically linked" / "static-pie linked".
 */
function binaryInfo(buf) {
  if (buf.length >= 8 && buf.readUInt32LE(0) === 0xfeedfacf) {
    const cpu = buf.readUInt32LE(4);
    return { format: 'macho', arch: { 0x01000007: 'x64', 0x0100000c: 'arm64' }[cpu] ?? cpu };
  }
  // 64-bit little-endian ELF only: every Linux target is x86_64 or aarch64.
  if (buf.length < 64 || buf.readUInt32BE(0) !== 0x7f454c46 || buf[4] !== 2 || buf[5] !== 1) return null;
  const machine = buf.readUInt16LE(18);
  const phoff = Number(buf.readBigUInt64LE(32));
  const phentsize = buf.readUInt16LE(54);
  const phnum = buf.readUInt16LE(56);
  let interp = false;
  let needed = false;
  for (let i = 0; i < phnum; i++) {
    const ph = phoff + i * phentsize;
    if (ph + 56 > buf.length) return null;
    const type = buf.readUInt32LE(ph);
    if (type === 3) interp = true; // PT_INTERP
    if (type === 2) {
      // PT_DYNAMIC: a DT_NEEDED entry names a shared library (static-pie has PT_DYNAMIC without one).
      const offset = Number(buf.readBigUInt64LE(ph + 8));
      const size = Number(buf.readBigUInt64LE(ph + 32));
      for (let d = offset; d + 16 <= Math.min(offset + size, buf.length); d += 16) {
        const tag = buf.readBigInt64LE(d);
        if (tag === 0n) break;
        if (tag === 1n) needed = true;
      }
    }
  }
  return { format: 'elf', arch: { 62: 'x64', 183: 'arm64' }[machine] ?? machine, static: !interp && !needed };
}

/** Why `file` cannot be the binary for target `t` (packages.json), or null when it can. */
function binaryProblem(file, t, { allowGlibc = false } = {}) {
  if (!fs.existsSync(file)) return `missing ${file}`;
  const info = binaryInfo(fs.readFileSync(file));
  const format = t.platform === 'darwin' ? 'macho' : 'elf';
  const kind = format === 'macho' ? 'Mach-O' : 'ELF';
  if (!info || info.format !== format || info.arch !== t.arch) {
    return `${file} is not a 64-bit ${kind} ${t.arch} binary (wanted for ${t.target})`;
  }
  if (format === 'elf' && !info.static && !allowGlibc) {
    return `${file} is dynamically linked: a Linux package holds the static ${t.target} build`;
  }
  return null;
}

function write(file, text) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
}

function writeJson(file, value) {
  write(file, JSON.stringify(value, null, 2) + '\n');
}

function copy(from, to, mode) {
  fs.mkdirSync(path.dirname(to), { recursive: true });
  fs.copyFileSync(from, to);
  if (mode !== undefined) fs.chmodSync(to, mode);
}

const readJson = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));
const defaultSpec = () => require('./zerochat/packages.json');

/**
 * Lays the packages out under `out`. `binaries` maps `<platform>-<arch>` to a directory holding
 * the built binaries; `notices` is the THIRD_PARTY_NOTICES.md to ship in every package.
 * Returns the package directories, main package first.
 */
function layout({ out, binaries = {}, spec = defaultSpec(), cargo = workspacePackage(), notices, allowGlibc = false }) {
  const { version, repository } = cargo;
  const repo = repository ? { type: 'git', url: `git+${repository}.git` } : undefined;
  const dirs = [];

  const known = new Set(spec.targets.map((t) => `${t.platform}-${t.arch}`));
  for (const key of Object.keys(binaries)) {
    if (!known.has(key)) throw new Error(`unknown platform ${key}; known: ${[...known].join(', ')}`);
  }
  const shared = (dir, files) => {
    copy(path.join(ROOT, 'LICENSE'), path.join(dir, 'LICENSE'));
    if (notices) {
      copy(notices, path.join(dir, NOTICES));
      files.push(NOTICES);
    }
    return files;
  };

  const mainManifest = readJson(path.join(MAIN, 'package.json'));

  // One package per platform that has binaries: zerochat and zsf side by side in bin/, so
  // zerochat finds zsf beside itself (Features needs no second install).
  const platforms = [];
  let local = false;
  for (const t of spec.targets) {
    const key = `${t.platform}-${t.arch}`;
    if (!binaries[key]) continue;
    const name = platformPackage(t.platform, t.arch, spec);
    const dir = path.join(out, name);
    fs.rmSync(dir, { recursive: true, force: true });
    let dynamic = false;
    for (const bin of spec.binaries) {
      const from = path.join(binaries[key], bin);
      const problem = binaryProblem(from, t, { allowGlibc });
      if (problem) throw new Error(problem);
      dynamic ||= t.platform === 'linux' && !binaryInfo(fs.readFileSync(from)).static;
      copy(from, path.join(dir, 'bin', bin), 0o755);
    }
    const binNames = spec.binaries.join(' and ');
    const pkg = {
      name,
      version,
      // A dynamically linked Linux build is a local glibc test build: never call it musl, never publish it.
      description: dynamic
        ? `LOCAL TEST BUILD, not publishable: the ${binNames} binaries for ${t.platform} ${t.arch}, built against glibc (dynamically linked), not ${t.target}`
        : `The ${binNames} binaries for ${t.platform} ${t.arch} (${t.target}); installed by ${spec.name}`,
      ...(dynamic ? { private: true } : {}),
      license: mainManifest.license,
      ...(repo ? { repository: repo } : {}),
      os: [t.platform],
      cpu: [t.arch],
      // Static musl runs on glibc and musl systems alike, so neither is excluded.
      ...(t.platform === 'linux' ? { libc: dynamic ? ['glibc'] : ['glibc', 'musl'] } : {}),
      files: shared(dir, ['bin']),
      preferUnplugged: true,
    };
    writeJson(path.join(dir, 'package.json'), pkg);
    write(
      path.join(dir, 'README.md'),
      `# ${name}\n\nInternal package of [\`${spec.name}\`](https://www.npmjs.com/package/${spec.name}): the \`${spec.binaries.join('` and `')}\` binaries for ${t.platform} ${t.arch}. Install \`${spec.name}\` instead; it picks this package for you.\n`,
    );
    local ||= dynamic;
    platforms.push(dir);
  }

  // The main package; private (unpublishable) unless every platform package it pins is laid out
  // as a release build.
  const main = path.join(out, spec.name);
  fs.rmSync(main, { recursive: true, force: true });
  const manifest = mainManifest;
  manifest.name = spec.name;
  manifest.version = version;
  if (platforms.length < spec.targets.length || local) manifest.private = true;
  if (repo) manifest.repository = repo;
  manifest.optionalDependencies = Object.fromEntries(spec.targets.map((t) => [platformPackage(t.platform, t.arch, spec), version]));
  manifest.files = shared(main, [...manifest.files]);
  writeJson(path.join(main, 'package.json'), manifest);
  for (const file of ['bin/zerochat.js', 'bin/zsf.js']) copy(path.join(MAIN, file), path.join(main, file), 0o755);
  copy(path.join(MAIN, 'lib/shim.js'), path.join(main, 'lib/shim.js'));
  copy(path.join(MAIN, 'packages.json'), path.join(main, 'packages.json'));
  copy(path.join(MAIN, 'README.md'), path.join(main, 'README.md'));
  dirs.push(main, ...platforms);
  return dirs;
}

/**
 * Lays the packages out from dist's archives in `dist` (`<binary>-<target>.tar.xz` beside its
 * `.sha256`): checks each checksum, unpacks, and lays out every platform. Missing archives fail.
 */
function fromDist({ dist, out, spec = defaultSpec(), cargo = workspacePackage(), notices }) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'zerochat-dist-'));
  const binaries = {};
  try {
    for (const t of spec.targets) {
      const dir = path.join(tmp, t.target);
      fs.mkdirSync(dir);
      for (const bin of spec.binaries) {
        const archive = path.join(dist, `${bin}-${t.target}.tar.xz`);
        if (!fs.existsSync(archive)) throw new Error(`missing ${archive}`);
        const sumFile = `${archive}.sha256`;
        if (!fs.existsSync(sumFile)) throw new Error(`missing ${sumFile}`);
        const want = fs.readFileSync(sumFile, 'utf8').trim().split(/\s+/)[0].toLowerCase();
        const got = crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex');
        if (got !== want) throw new Error(`sha256 mismatch for ${archive}: ${got}, ${path.basename(sumFile)} says ${want}`);
        const unpacked = path.join(tmp, `${bin}-${t.target}.unpacked`);
        fs.mkdirSync(unpacked);
        execFileSync('tar', ['-xJf', archive, '-C', unpacked]);
        // dist's archive layout: <binary>-<target>/<binary>
        const file = path.join(unpacked, `${bin}-${t.target}`, bin);
        if (!fs.existsSync(file)) throw new Error(`${archive} has no ${bin}-${t.target}/${bin}`);
        fs.copyFileSync(file, path.join(dir, bin));
      }
      binaries[`${t.platform}-${t.arch}`] = dir;
    }
    return layout({ out, binaries, spec, cargo, notices });
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

/**
 * The completeness check before anything is published: all packages laid out, each platform package
 * holding every binary for its target (static on Linux), one version everywhere (`version` when
 * given, e.g. the tag's), the main package pinning exactly the platform packages at that version,
 * nothing private, notices in every package. Returns the problems; empty means publishable.
 */
function check(out, { spec = defaultSpec(), version } = {}) {
  const problems = [];
  const read = (dir) => {
    const file = path.join(dir, 'package.json');
    if (!fs.existsSync(file)) {
      problems.push(`missing package ${path.relative(out, dir)} (no package.json)`);
      return null;
    }
    return readJson(file);
  };
  const main = read(path.join(out, spec.name));
  const want = version ?? main?.version;
  const common = (pkg, dir, name) => {
    if (pkg.name !== name) problems.push(`${dir}: name is ${pkg.name}, not ${name}`);
    if (pkg.version !== want) problems.push(`${name}: version ${pkg.version}, not ${want}`);
    if (pkg.private) problems.push(`${name}: private (a local or incomplete build)`);
    for (const f of ['README.md', 'LICENSE', NOTICES]) {
      if (!fs.existsSync(path.join(out, dir, f))) problems.push(`${name}: no ${f}`);
    }
  };
  if (main) {
    common(main, spec.name, spec.name);
    const pins = Object.fromEntries(spec.targets.map((t) => [platformPackage(t.platform, t.arch, spec), want]));
    if (JSON.stringify(main.optionalDependencies ?? {}) !== JSON.stringify(pins)) {
      problems.push(`${spec.name}: optionalDependencies ${JSON.stringify(main.optionalDependencies)}, not ${JSON.stringify(pins)}`);
    }
  }
  for (const t of spec.targets) {
    const name = platformPackage(t.platform, t.arch, spec);
    const pkg = read(path.join(out, name));
    if (!pkg) continue;
    common(pkg, name, name);
    for (const bin of spec.binaries) {
      const problem = binaryProblem(path.join(out, name, 'bin', bin), t);
      if (problem) problems.push(`${name}: ${problem}`);
    }
  }
  return problems;
}

/** The package directories in publishing order: every platform package, then the main package. */
function publishOrder(out, spec = defaultSpec()) {
  return [...spec.targets.map((t) => path.join(out, platformPackage(t.platform, t.arch, spec))), path.join(out, spec.name)];
}

const BOOLEAN_FLAGS = new Set(['--allow-glibc']);

function parseArgs(argv) {
  const args = { binaries: {} };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (BOOLEAN_FLAGS.has(flag)) {
      args.allowGlibc = true;
      continue;
    }
    const value = argv[++i];
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === '--out') args.out = path.resolve(value);
    else if (flag === '--dist') args.dist = path.resolve(value);
    else if (flag === '--notices') args.notices = path.resolve(value);
    else if (flag === '--version') args.version = value;
    else if (flag === '--bin') {
      const [key, dir] = value.split(/=(.*)/s);
      if (!dir) throw new Error(`--bin wants <platform>-<arch>=<dir>, got ${value}`);
      args.binaries[key] = path.resolve(dir);
    } else throw new Error(`unknown flag ${flag}`);
  }
  return args;
}

function requireOut(args) {
  if (!args.out) throw new Error('--out <dir> is required');
  return args;
}

/** Bytes as `1.0 kB` / `17.5 MB`, right-aligned. */
function size(n) {
  return (n < 1 << 20 ? `${(n / 1024).toFixed(1)} kB` : `${(n / (1 << 20)).toFixed(1)} MB`).padStart(9);
}

/** The lines `npm pack --dry-run --json` output becomes: one per file, then a total. */
function packList(json) {
  const lines = [];
  for (const p of JSON.parse(json)) {
    for (const f of p.files) lines.push(`   ${size(f.size)}  ${f.path}`);
    lines.push(`   ${p.name}@${p.version}: ${p.entryCount} files, ${size(p.size).trim()} packed`);
  }
  return lines;
}

function command(argv) {
  const spec = defaultSpec();
  switch (argv[0]) {
    case 'targets':
      for (const t of spec.targets) console.log(t.target, `${t.platform}-${t.arch}`);
      return;
    case 'version':
      console.log(workspacePackage().version);
      return;
    case 'pack-list': {
      let input = '';
      process.stdin.on('data', (d) => (input += d)).on('end', () => console.log(packList(input).join('\n')));
      return;
    }
    case 'from-dist': {
      const args = requireOut(parseArgs(argv.slice(1)));
      if (!args.dist) throw new Error('--dist <dir> is required');
      for (const dir of fromDist(args)) console.log(dir);
      return;
    }
    case 'check': {
      if (!argv[1]) throw new Error('check <dir> [--version <version>]');
      const args = parseArgs(argv.slice(2));
      const out = path.resolve(argv[1]);
      const problems = check(out, { version: args.version });
      if (problems.length) {
        for (const p of problems) console.error(`incomplete: ${p}`);
        throw new Error(`${problems.length} problem(s): nothing may be published from ${out}`);
      }
      console.log(`complete: ${publishOrder(out).length} packages in ${out}`);
      return;
    }
    case 'publish-order':
      if (!argv[1]) throw new Error('publish-order <dir>');
      for (const dir of publishOrder(path.resolve(argv[1]))) console.log(dir);
      return;
    default:
      for (const dir of layout(requireOut(parseArgs(argv)))) console.log(dir);
  }
}

if (require.main === module) {
  try {
    command(process.argv.slice(2));
  } catch (error) {
    console.error(`npm/build.js: ${error.message}`);
    process.exit(1);
  }
}

module.exports = { layout, fromDist, check, publishOrder, binaryInfo, workspacePackage, parseArgs, packList };

'use strict';
// scripts/bump-version.sh on a temp copy of the files that carry the version: every place is
// updated, nothing else changes, and it refuses what it must refuse.
const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');

const ROOT = path.resolve(__dirname, '../..');
const DATE = '2031-02-03';
// Every file the script edits; npm/zerochat/package.json is copied to prove it stays untouched.
const EDITED = ['Cargo.toml', 'Cargo.lock', 'web/package.json', 'web/package-lock.json', 'CHANGELOG.md'];
const FILES = [...EDITED, 'npm/zerochat/package.json', 'scripts/bump-version.sh'];
const CURRENT = /^version = "([^"]+)"$/m.exec(/^\[workspace\.package\][^[]*/m.exec(fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8'))[0])[1];

const git = (dir, ...args) =>
  execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@example.com', '-c', 'commit.gpgsign=false', '-c', 'core.hooksPath=/dev/null', ...args], {
    cwd: dir,
    encoding: 'utf8',
  });

/** A git repo holding a copy of FILES; `changelog` replaces CHANGELOG.md when given. */
function scratch(changelog) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-version-'));
  for (const f of FILES) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.copyFileSync(path.join(ROOT, f), path.join(dir, f));
  }
  if (changelog !== undefined) fs.writeFileSync(path.join(dir, 'CHANGELOG.md'), changelog);
  git(dir, 'init', '-q');
  git(dir, 'add', '-A');
  git(dir, 'commit', '-qm', 'scratch');
  return dir;
}

const bump = (dir, ...args) =>
  spawnSync('bash', [path.join(dir, 'scripts/bump-version.sh'), ...args], {
    cwd: dir,
    encoding: 'utf8',
    env: { ...process.env, RELEASE_DATE: DATE },
  });
const read = (dir, f) => fs.readFileSync(path.join(dir, f), 'utf8');
const snapshot = (dir) => Object.fromEntries(FILES.map((f) => [f, read(dir, f)]));

/** The 0-based line numbers where `a` and `b` differ (same line count required). */
function changedLines(a, b) {
  const [x, y] = [a.split('\n'), b.split('\n')];
  assert.strictEqual(x.length, y.length, 'line count changed');
  return x.flatMap((l, i) => (l === y[i] ? [] : [i]));
}
const lineOf = (text, re) => text.split('\n').findIndex((l) => re.test(l));

const CHANGELOG = `# Changelog

Intro.

## [Unreleased]

### Fixed

- A fix.

## [0.1.0] - 2030-01-01

### Added

- The first.

[Unreleased]: https://github.com/tomeyer97/zerochat/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tomeyer97/zerochat/releases/tag/v0.1.0
`;

test('a release sets every version place, moves Unreleased, and changes nothing else', () => {
  const dir = scratch(CHANGELOG);
  const before = snapshot(dir);
  const r = bump(dir, '0.2.0');
  assert.strictEqual(r.status, 0, r.stderr);
  assert.match(r.stdout, new RegExp(`version: ${CURRENT.replace(/\./g, '\\.')} -> 0\\.2\\.0`));
  assert.match(r.stdout, /Cargo\.toml/);
  const after = snapshot(dir);

  // Cargo.toml: only the [workspace.package] version line.
  assert.deepStrictEqual(changedLines(before['Cargo.toml'], after['Cargo.toml']), [lineOf(before['Cargo.toml'], /^version = /)]);
  assert.match(after['Cargo.toml'], /^\[workspace\.package\]\n(?:[^\n]*\n)*?version = "0\.2\.0"$/m);

  // Cargo.lock: exactly the zerochat and zsf entries.
  const lockLines = before['Cargo.lock'].split('\n');
  const entry = (name) => lockLines.indexOf(`name = "${name}"`) + 1;
  assert.deepStrictEqual(changedLines(before['Cargo.lock'], after['Cargo.lock']), [entry('zerochat'), entry('zsf')].sort((a, b) => a - b));
  for (const name of ['zerochat', 'zsf']) assert.match(after['Cargo.lock'], new RegExp(`^name = "${name}"\\nversion = "0\\.2\\.0"$`, 'm'));

  // web/package.json and its lock: the top-level version and packages[""].version only.
  assert.strictEqual(changedLines(before['web/package.json'], after['web/package.json']).length, 1);
  assert.strictEqual(JSON.parse(after['web/package.json']).version, '0.2.0');
  assert.strictEqual(changedLines(before['web/package-lock.json'], after['web/package-lock.json']).length, 2);
  const lock = JSON.parse(after['web/package-lock.json']);
  assert.strictEqual(lock.version, '0.2.0');
  assert.strictEqual(lock.packages[''].version, '0.2.0');

  // Not touched: the npm template (npm/build.js writes the version) and the script.
  assert.strictEqual(after['npm/zerochat/package.json'], before['npm/zerochat/package.json']);
  assert.strictEqual(after['scripts/bump-version.sh'], before['scripts/bump-version.sh']);

  assert.strictEqual(
    after['CHANGELOG.md'],
    `# Changelog

Intro.

## [Unreleased]

## [0.2.0] - ${DATE}

### Fixed

- A fix.

## [0.1.0] - 2030-01-01

### Added

- The first.

[Unreleased]: https://github.com/tomeyer97/zerochat/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tomeyer97/zerochat/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tomeyer97/zerochat/releases/tag/v0.1.0
`,
  );

  // No commit, no tag, nothing staged; no file besides the edited ones.
  assert.strictEqual(git(dir, 'rev-list', '--count', 'HEAD').trim(), '1');
  assert.strictEqual(git(dir, 'tag').trim(), '');
  assert.strictEqual(git(dir, 'diff', '--cached', '--name-only').trim(), '');
  assert.ok(git(dir, 'status', '--porcelain', '--untracked-files=all').split('\n').filter(Boolean).every((l) => l.startsWith(' M ')), 'only modified files');
  assert.deepStrictEqual(git(dir, 'diff', '--name-only').trim().split('\n').sort(), [...EDITED].sort());
});

test("the repository's own CHANGELOG: the first release dates the pending section", () => {
  const dir = scratch();
  const before = read(dir, 'CHANGELOG.md');
  assert.match(before, new RegExp(`^## \\[${CURRENT.replace(/\./g, '\\.')}\\] - Unreleased$`, 'm'));
  const r = bump(dir, CURRENT);
  assert.strictEqual(r.status, 0, r.stderr);
  const after = read(dir, 'CHANGELOG.md');
  assert.strictEqual(after, before.replace(`## [${CURRENT}] - Unreleased`, `## [${CURRENT}] - ${DATE}`));
  // The version is unchanged, so only the CHANGELOG changed.
  assert.strictEqual(git(dir, 'status', '--porcelain'), ' M CHANGELOG.md\n');
});

test('a pre-release copies the Unreleased entries (the final release collects them)', () => {
  const dir = scratch(CHANGELOG);
  const r = bump(dir, '0.2.0-rc.1');
  assert.strictEqual(r.status, 0, r.stderr);
  const after = read(dir, 'CHANGELOG.md');
  assert.match(after, /## \[Unreleased\]\n\n### Fixed\n\n- A fix\.\n\n## \[0\.2\.0-rc\.1\] - 2031-02-03\n\n### Fixed\n\n- A fix\.\n\n## \[0\.1\.0\]/);
  assert.match(after, /^\[0\.2\.0-rc\.1\]: https:\/\/github\.com\/tomeyer97\/zerochat\/compare\/v0\.1\.0\.\.\.v0\.2\.0-rc\.1$/m);
  assert.strictEqual(JSON.parse(read(dir, 'web/package.json')).version, '0.2.0-rc.1');
});

test('refuses: invalid versions, a dirty tree, going backwards, a dated section, no notes', () => {
  const refuses = (dir, args, why) => {
    const before = snapshot(dir);
    const r = bump(dir, ...args);
    assert.notStrictEqual(r.status, 0, `${args.join(' ')} should fail`);
    assert.match(r.stderr, why);
    assert.deepStrictEqual(snapshot(dir), before, `${args.join(' ')} changed files`);
  };
  const dir = scratch(CHANGELOG);
  for (const v of ['v0.2.0', '0.2', '01.2.0', '0.2.0+build.1', '0.2.0-', '0.2.0-01', 'latest']) refuses(dir, [v], /not a SemVer version/);
  refuses(dir, [], /usage/);
  refuses(dir, ['0.2.0', '0.3.0'], /usage/);
  refuses(dir, ['0.0.9'], /lower than the current/);
  refuses(dir, ['0.1.0'], /already has a dated 0\.1\.0 section/);

  fs.appendFileSync(path.join(dir, 'Cargo.toml'), '\n');
  refuses(dir, ['0.2.0'], /not clean/);

  const empty = scratch(CHANGELOG.replace('### Fixed\n\n- A fix.\n\n', ''));
  refuses(empty, ['0.2.0'], /nothing under Unreleased/);
});

test('the script knows every tracked file that carries the version', () => {
  // A new package.json or Cargo.toml with its own version must be added to the script (or here).
  const exempt = new Map([
    ['crates/zerochat/Cargo.toml', /^version\.workspace = true$/m],
    ['crates/zsf/Cargo.toml', /^version\.workspace = true$/m],
    ['npm/zerochat/package.json', /^ {2}"version": "0\.0\.0-dev",$/m], // npm/build.js sets the real one
  ]);
  const tracked = execFileSync('git', ['ls-files', '*Cargo.toml', '*package.json', '*package-lock.json'], { cwd: ROOT, encoding: 'utf8' })
    .split('\n')
    .filter(Boolean);
  for (const f of tracked) {
    if (EDITED.includes(f)) continue;
    assert.ok(exempt.has(f), `${f} is not handled by scripts/bump-version.sh`);
    assert.match(fs.readFileSync(path.join(ROOT, f), 'utf8'), exempt.get(f), `${f}: expected no version of its own`);
  }
  // The script's header lists the same places.
  const script = fs.readFileSync(path.join(ROOT, 'scripts/bump-version.sh'), 'utf8');
  for (const f of EDITED.filter((f) => f !== 'CHANGELOG.md')) assert.ok(script.includes(`#   ${f} `), `${f} missing from the header`);
});

test('bash -n accepts the script', () => {
  const r = spawnSync('bash', ['-n', path.join(ROOT, 'scripts/bump-version.sh')], { encoding: 'utf8' });
  assert.strictEqual(r.status, 0, r.stderr);
});

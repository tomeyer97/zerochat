#!/usr/bin/env bash
# Sets the release version everywhere it lives and moves CHANGELOG.md's Unreleased entries under it.
#
#   scripts/bump-version.sh X.Y.Z          # e.g. 0.2.0, or a pre-release such as 0.2.0-rc.1
#
# Where the version lives (RELEASING.md lists the same):
#   Cargo.toml             [workspace.package] version (zerochat, zsf and the npm packages use it)
#   Cargo.lock             the zerochat and zsf entries
#   web/package.json       "version"
#   web/package-lock.json  the top-level "version" and packages[""].version
# npm/zerochat/package.json keeps "0.0.0-dev": npm/build.js writes the Cargo.toml version into the
# packages it lays out, so it is not touched here.
#
# CHANGELOG.md: a release moves the `## [Unreleased]` entries under `## [X.Y.Z] - <today>`; a
# pre-release copies them (Unreleased keeps them for the final release). A `## [X.Y.Z] - Unreleased`
# section that already exists gets today's date and the Unreleased entries on top. The compare links
# at the bottom follow. RELEASE_DATE=YYYY-MM-DD overrides today's date (UTC).
#
# Refuses a dirty working tree and an invalid version. Prints what changed. Never commits, tags or
# pushes: that is the release PR (RELEASING.md).
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
  echo "bump-version: $*" >&2
  exit 1
}

[ $# -eq 1 ] || die "usage: scripts/bump-version.sh X.Y.Z (SemVer 2.0.0, no leading v)"
new=$1

# SemVer 2.0.0 without build metadata (the release tags are vX.Y.Z and vX.Y.Z-<pre>).
num='(0|[1-9][0-9]*)'
ident='(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
[[ $new =~ ^$num\.$num\.$num(-$ident(\.$ident)*)?$ ]] ||
  die "'$new' is not a SemVer version X.Y.Z or X.Y.Z-<pre> (no leading v, no +build)"

date=${RELEASE_DATE:-$(date -u +%Y-%m-%d)}
[[ $date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || die "RELEASE_DATE '$date' is not YYYY-MM-DD"

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not a git checkout"
[ -z "$(git status --porcelain)" ] || die "the working tree is not clean: commit or stash first"

command -v node >/dev/null || die "needs node"

node - "$new" "$date" <<'JS'
'use strict';
const fs = require('fs');
const [newVersion, date] = process.argv.slice(2);
const die = (msg) => { console.error(`bump-version: ${msg}`); process.exit(1); };
const read = (file) => fs.readFileSync(file, 'utf8');
const edits = []; // [file, text], written only when every check passed

/** Replaces the one match of `re` (group 1: the version) in `text`; fails unless exactly one matches. */
function setVersion(file, text, re, what) {
  const matches = [...text.matchAll(new RegExp(re.source, `${re.flags.replace('g', '')}g`))];
  if (matches.length !== 1) die(`${file}: expected one ${what}, found ${matches.length}`);
  const m = matches[0];
  const start = m.index + m[0].indexOf(m[1]);
  return { old: m[1], text: text.slice(0, start) + newVersion + text.slice(start + m[1].length) };
}

// Cargo.toml: the version line of [workspace.package].
const cargoToml = read('Cargo.toml');
const section = /^\[workspace\.package\]\n(?:(?!\[)[^\n]*\n)*/m.exec(cargoToml);
if (!section) die('Cargo.toml: no [workspace.package]');
const inSection = setVersion('Cargo.toml', section[0], /^version = "([^"]+)"$/m, '[workspace.package] version');
const current = inSection.old;
edits.push(['Cargo.toml', cargoToml.slice(0, section.index) + inSection.text + cargoToml.slice(section.index + section[0].length)]);

// Cargo.lock: the zerochat and zsf entries (workspace members: no `source` line).
let lock = read('Cargo.lock');
for (const name of ['zerochat', 'zsf']) {
  const r = setVersion('Cargo.lock', lock, new RegExp(`^name = "${name}"\\nversion = "([^"]+)"$`, 'm'), `${name} entry`);
  if (r.old !== current) die(`Cargo.lock: ${name} is ${r.old}, Cargo.toml says ${current}: run cargo build first`);
  lock = r.text;
}
edits.push(['Cargo.lock', lock]);

// web/package.json and its lock: only the top-level version lines (2 spaces in; packages[""] 6).
const pkg = setVersion('web/package.json', read('web/package.json'), /^ {2}"version": "([^"]+)",$/m, 'top-level "version"');
edits.push(['web/package.json', pkg.text]);
let pkgLock = setVersion('web/package-lock.json', read('web/package-lock.json'), /^ {2}"version": "([^"]+)",$/m, 'top-level "version"').text;
pkgLock = setVersion('web/package-lock.json', pkgLock, /^ {4}"": \{\n {6}"name": "[^"]+",\n {6}"version": "([^"]+)",$/m, 'packages[""].version').text;
edits.push(['web/package-lock.json', pkgLock]);

// Refuse going backwards (by X.Y.Z; a pre-release of the current version is fine).
const core = (v) => v.split('-')[0].split('.').map(Number);
const [a, b] = [core(newVersion), core(current)];
const cmp = a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
if (cmp < 0) die(`${newVersion} is lower than the current ${current}`);

// CHANGELOG.md.
const changelog = read('CHANGELOG.md');
const lines = changelog.split('\n');
const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const unreleasedAt = lines.findIndex((l) => l === '## [Unreleased]');
if (unreleasedAt < 0) die('CHANGELOG.md: no "## [Unreleased]" heading');
let nextAt = lines.findIndex((l, i) => i > unreleasedAt && (l.startsWith('## ') || /^\[[^\]]+\]: /.test(l)));
if (nextAt < 0) nextAt = lines.length;
const trim = (ls) => {
  const out = [...ls];
  while (out.length && out[0].trim() === '') out.shift();
  while (out.length && out[out.length - 1].trim() === '') out.pop();
  return out;
};
const entries = trim(lines.slice(unreleasedAt + 1, nextAt));
const heading = new RegExp(`^## \\[${esc(newVersion)}\\](?: - (.*))?$`);
const existingAt = lines.findIndex((l) => heading.test(l));
const pre = newVersion.includes('-');
if (existingAt >= 0 && heading.exec(lines[existingAt])[1] !== 'Unreleased') die(`CHANGELOG.md already has a dated ${newVersion} section`);
if (newVersion === current && existingAt < 0) die(`the version is already ${current}`);
if (existingAt >= 0 && existingAt !== nextAt) die(`CHANGELOG.md: the ${newVersion} section must follow Unreleased`);
if (!entries.length && existingAt < 0) die('CHANGELOG.md: nothing under Unreleased; add the release notes first');

let out;
const newHeading = `## [${newVersion}] - ${date}`;
if (existingAt >= 0) {
  let end = lines.findIndex((l, i) => i > existingAt && (l.startsWith('## ') || /^\[[^\]]+\]: /.test(l)));
  if (end < 0) end = lines.length;
  const body = trim(lines.slice(existingAt + 1, end));
  const merged = entries.length ? [...entries, '', ...body] : body;
  if (entries.length && body.length) console.log(`note: CHANGELOG.md: the Unreleased entries now sit on top of ${newVersion}; merge repeated groups by hand`);
  out = [...lines.slice(0, unreleasedAt + 1), '', ...(pre ? [...entries, ''] : []), newHeading, '', ...merged, '', ...lines.slice(end)];
} else {
  out = [...lines.slice(0, unreleasedAt + 1), '', ...(pre ? [...entries, ''] : []), newHeading, '', ...entries, '', ...lines.slice(nextAt)];
}

// Compare links: [Unreleased] compares from the new tag; the new version gets its own link.
let text = out.join('\n').replace(/\n{3,}/g, '\n\n');
const unreleasedLink = /^\[Unreleased\]: (https:\/\/github\.com\/[^/]+\/[^/]+)\/compare\/v(.+)\.\.\.HEAD$/m.exec(text);
if (unreleasedLink) {
  const [line, repo, from] = unreleasedLink;
  let replacement = `[Unreleased]: ${repo}/compare/v${newVersion}...HEAD`;
  if (!new RegExp(`^\\[${esc(newVersion)}\\]: `, 'm').test(text)) {
    replacement += from === newVersion
      ? `\n[${newVersion}]: ${repo}/releases/tag/v${newVersion}`
      : `\n[${newVersion}]: ${repo}/compare/v${from}...v${newVersion}`;
  }
  text = text.replace(line, replacement);
} else {
  console.log('note: CHANGELOG.md has no "[Unreleased]: <repo>/compare/vX...HEAD" link; add the links by hand');
}
edits.push(['CHANGELOG.md', text]);

for (const [file, content] of edits) fs.writeFileSync(file, content);
console.log(`version: ${current} -> ${newVersion}`);
JS

echo "changed:"
git diff --stat
echo
echo "Not committed, tagged or pushed. Next: review 'git diff', run 'cargo build' (Cargo.lock must stay as is),"
echo "then open the release PR (RELEASING.md)."

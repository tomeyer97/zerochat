#!/usr/bin/env node
'use strict';
// The licences of the npm packages bundled into the web UI that the zerochat binary embeds
// (web/dist), as the second half of THIRD_PARTY_NOTICES.md (scripts/third-party-notices.sh).
//
//   node npm/web-licenses.js [web dir]     # Markdown on stdout; default: web/
//
// The packages are web/'s production dependencies, as installed (`npm ls --omit=dev --all`), minus
// type-only packages (@types/*, and packages with types but no code), which never reach the bundle.

const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const LICENSE_FILE = /^(licen[cs]e|copying|notice)(\.(md|txt))?$/i;

/** Installed production dependency directories of the npm project in `dir`. */
function productionPackages(dir) {
  const out = execFileSync('npm', ['ls', '--omit=dev', '--all', '--parseable'], { cwd: dir, encoding: 'utf8' });
  // npm prints real paths: compare real paths, or a symlinked dir would list itself.
  const root = fs.realpathSync(dir);
  return [...new Set(out.split('\n').map((l) => l.trim()).filter((l) => l && fs.realpathSync(l) !== root))];
}

/** `{ name, version, license, texts, url }` of the package in `dir`, or null for a type-only package. */
function describe(dir) {
  const pkg = JSON.parse(fs.readFileSync(path.join(dir, 'package.json'), 'utf8'));
  const code = pkg.main || pkg.module || pkg.exports || pkg.style || pkg.browser;
  if (pkg.name.startsWith('@types/') || (!code && (pkg.types || pkg.typings))) return null;
  const license = typeof pkg.license === 'string' ? pkg.license : (pkg.license?.type ?? (pkg.licenses ?? []).map((l) => l.type).join(' OR '));
  const texts = fs
    .readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => fs.readFileSync(path.join(dir, f), 'utf8').trim());
  const repo = typeof pkg.repository === 'string' ? pkg.repository : pkg.repository?.url;
  const url = `https://www.npmjs.com/package/${pkg.name}/v/${pkg.version}`;
  return { name: pkg.name, version: pkg.version, license: license || 'UNKNOWN', texts, url, repo };
}

/** The Markdown section for the described packages. Fails on a package without a licence. */
function render(packages) {
  const sorted = [...packages].sort((a, b) => a.name.localeCompare(b.name));
  const unknown = sorted.filter((p) => p.license === 'UNKNOWN');
  if (unknown.length) throw new Error(`no licence declared by ${unknown.map((p) => `${p.name}@${p.version}`).join(', ')}`);
  const lines = [
    '## Web UI',
    '',
    'The npm packages bundled into the web UI that the zerochat binary embeds.',
    '',
    ...sorted.map((p) => `- [${p.name} ${p.version}](${p.url}): ${p.license}`),
    '',
  ];
  for (const p of sorted) {
    lines.push(`### ${p.name} ${p.version} (${p.license})`, '');
    if (p.license.includes('EPL')) {
      lines.push(`Source code: ${p.repo ? p.repo.replace(/^git\+/, '') : p.url} (unmodified).`, '');
    }
    if (p.texts.length === 0) lines.push(`No licence file in the package; its declared licence is ${p.license}.`, '');
    for (const text of p.texts) lines.push('```text', text, '```', '');
  }
  return lines.join('\n');
}

if (require.main === module) {
  try {
    const dir = path.resolve(process.argv[2] ?? path.join(__dirname, '..', 'web'));
    const packages = productionPackages(dir).map(describe).filter(Boolean);
    process.stdout.write(render(packages));
  } catch (error) {
    console.error(`npm/web-licenses.js: ${error.message}`);
    process.exit(1);
  }
}

module.exports = { productionPackages, describe, render };

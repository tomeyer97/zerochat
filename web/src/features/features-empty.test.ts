// The empty Features page when zsf is missing says in one line how to get it (the npm package
// includes it; from source `make` builds it, or `cargo install --path crates/zsf`), and its docs
// link points at the public repository's main branch.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Setup } from '../generated/discovery';
import { FeaturesEmpty } from './FeaturesEmpty';
import { REPO_URL, ZSF_DOCS_URL, ZSF_INSTALL } from './features-empty';

const text = (html: string) =>
  html
    .replace(/<[^>]+>/g, ' ')
    .replace(/&#x27;|&#39;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ');

const missing = { zsf: { found: false, program: null, version: null, features: 0 } } as unknown as Setup;

test('a missing zsf: one line names the npm package, `make` and cargo install', () => {
  const html = renderToStaticMarkup(createElement(FeaturesEmpty, { setup: missing }));
  const line = text(html.match(/<p class="tone-text-warn">[\s\S]*?<\/p>/)?.[0] ?? '').trim();
  assert.equal(line, 'zsf is not installed. It comes with the zerochat npm package; from source, `make` builds it next to zerochat, or install it with cargo:');
  assert.ok(text(html).includes(ZSF_INSTALL.npm));
  assert.ok(text(html).includes('cargo install --path crates/zsf'));
});

test('the zsf docs link is the README on main of the repository in Cargo.toml', () => {
  const cargo = readFileSync(new URL('../../../Cargo.toml', import.meta.url), 'utf8');
  assert.ok(cargo.includes(`repository = "${REPO_URL}"`), 'one public home');
  assert.ok(cargo.includes(`homepage = "${REPO_URL}"`));
  assert.equal(ZSF_DOCS_URL, `${REPO_URL}/blob/main/crates/zsf/README.md`);
});

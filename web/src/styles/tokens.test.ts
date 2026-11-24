import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

// The dark palette is written twice in tokens.css (system dark, and data-theme="dark"); CSS has no
// way to share one block between a media query and an attribute selector. These checks keep the two
// copies equal and every token used by the stylesheets defined.

const css = readFileSync(new URL('./tokens.css', import.meta.url), 'utf8');

function block(opening: string): Map<string, string> {
  const start = css.indexOf(opening);
  assert.ok(start >= 0, `tokens.css has no block opening with ${opening}`);
  const body = css.slice(start + opening.length, css.indexOf('}', start));
  return new Map([...body.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}

const light = block(':root {');
const systemDark = block(':root:not([data-theme]) {');
const chosenDark = block(":root[data-theme='dark'] {");

test('both dark palettes declare the same tokens with the same values', () => {
  assert.deepEqual([...chosenDark], [...systemDark]);
});

test('every dark token overrides a light token', () => {
  for (const name of systemDark.keys()) assert.ok(light.has(name), `${name} is dark-only`);
});

test('every token the stylesheets read is defined', () => {
  const sheets = ['base', 'components', 'run', 'graph', 'chat', 'markdown', 'benchmarks', 'features', 'slice-graph', 'benchmarks-forms'];
  const defined = new Set([...light.keys(), ...systemDark.keys()]);
  for (const sheet of sheets) {
    const text = readFileSync(new URL(`./${sheet}.css`, import.meta.url), 'utf8');
    // Custom properties declared inside a stylesheet (e.g. the responsive layout widths) count too.
    for (const m of text.matchAll(/(--[\w-]+):/g)) defined.add(m[1]);
    for (const m of text.matchAll(/var\((--[\w-]+)/g)) assert.ok(defined.has(m[1]), `${sheet}.css reads undefined ${m[1]}`);
  }
});

test('the theme-color metas in index.html are the canvas colours of both palettes', () => {
  const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
  const darkMeta = html.match(/<meta name="theme-color" content="([^"]+)" media="\(prefers-color-scheme: dark\)"/);
  const lightMeta = html.match(/<meta name="theme-color" content="([^"]+)" \/>/);
  assert.equal(darkMeta?.[1], systemDark.get('--canvas'));
  assert.equal(lightMeta?.[1], light.get('--canvas'));
});

// Screen-reader-only spans (.visually-hidden, position: absolute) inside a scroll container must
// stay inside it: else they escape its clip and give the whole page a scrollbar (a 891 px page in
// a 526 px window on a slice page with a diff, measured with Playwright at 1366x526).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const css = (name: string) => readFileSync(new URL(`./styles/${name}`, import.meta.url), 'utf8');
/** The declarations of the first rule whose selector list is exactly `selector`. */
const rule = (text: string, selector: string) => {
  const at = text.search(new RegExp(`(^|\\n)${selector.replace(/[.*]/g, '\\$&')} \\{`));
  assert.ok(at >= 0, `no ${selector} rule`);
  return text.slice(at, text.indexOf('}', at));
};

test('the panels’ scroll container is a containing block', () => {
  assert.match(rule(css('run.css'), '.panel-body'), /position: relative;/);
});

test('a diff, a scroll container holding hidden line labels, is a containing block too', () => {
  // Without it, an open diff low in a long Files list lengthened the page (9656 px in a 526 px
  // window with 300 changed files, measured with Playwright, when .panel-body was not one either).
  assert.match(rule(css('run.css'), '.diff'), /position: relative;/);
});

test('visually hidden text sits at its containing block’s corner, not deep in the flow', () => {
  const hidden = rule(css('base.css'), '.visually-hidden');
  assert.match(hidden, /position: absolute;/);
  assert.match(hidden, /top: 0;/);
  assert.match(hidden, /left: 0;/);
});

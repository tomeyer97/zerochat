// The limits strip, rendered from the API snapshots of the server's runs
// (crates/zerochat/testdata/golden/api/runs/*/run.json): only the agent's own API events make
// a pressure, and the strip never shows a raw log line.
import { FIXED_NOW, apiFile } from './testdata/golden';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { PressureStrip, pressureTitle } from './PressureStrip';
import type { Pressure, RunView } from './types';

const run = (name: string) => apiFile<RunView>(`runs/${name}/run.json`).body;

function strip(view: RunView, pressures: Pressure[] = view.pressures): string {
  return renderToStaticMarkup(createElement(PressureStrip, { pressures, view, now: FIXED_NOW }));
}

/** The strip's text, tags dropped. */
const text = (html: string) =>
  html
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const RAW = 'Claude tool completed: Bash id=toolu_013Q1Cc1gwLu5V3gMSPuHaJu status=success\nHTTP 429 rate_limit_error: You have hit your usage limit. Rate limit exceeded, retry-after 60';

test('tool output with rate-limit words is no pressure; the real notice names the agent and links to its log', () => {
  const view = run('tool-output-rate-limit');
  assert.deepEqual(
    view.pressures.map((p) => [p.kind, p.text]),
    [['rate_limit', '5-hour limit hit']]
  );
  const html = strip(view);
  assert.match(text(html), /^worker: 5-hour limit hit · resets in /);
  assert.match(html, /<a class="pressure-node" href="#\/run\/observe\/[^"]+\/node\/worker[^"]*" title="Open worker&#x27;s log">/);
  assert.doesNotMatch(html, /rate_limit_error|toolu_/);
});

test('a raw log line never reaches the strip: its kind titles it, with the agent and its link', () => {
  const view = run('tool-output-rate-limit');
  const raw: Pressure = { ...view.pressures[0], text: RAW, resetsAt: null };
  const html = strip(view, [raw]);
  assert.equal(text(html), 'worker: rate limit hit');
  assert.match(html, /href="#\/run\/observe\/[^"]+\/node\/worker/);
  assert.equal(pressureTitle({ kind: 'retry', text: 'Claude API retry 2/10: overloaded_error' }), "retrying the provider's API");
  assert.equal(pressureTitle({ kind: 'retry', text: 'retried 2 of 10 (rate limit)' }), 'retried 2 of 10 (rate limit)');
  assert.equal(pressureTitle({ kind: 'rate_limit', text: 'x'.repeat(120) }), 'rate limit hit');
});

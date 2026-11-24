// Onboarding: the first-run card, the Zeroshot strip, the logo tooltip, the glossary,
// the first-visit hint, Home's visible headers and status legend, and the context line of rows
// whose titles repeat. Setup fixtures are shaped like the `api/setup` golden
// (crates/zerochat/testdata/golden/api/setup.json).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import type { Setup } from '../generated/discovery';
import type { Overview, RunListing } from '../types';
import type { OverviewState } from '../overview-poll';
import { Home } from '../Home';
import { setFetch } from '../api';
import { FirstRunCard } from './FirstRunCard';
import { ZeroshotStrip } from './ZeroshotStrip';
import { FirstVisitHint, FirstVisitHintView, HINT_KEY } from './FirstVisitHint';
import { GlossaryButton, GlossaryList } from './Glossary';
import { GLOSSARY } from './glossary-terms';
import { brandTitle, EXAMPLE_COMMAND, homePath, setupLine, stripText } from './setup-model';
import { fetchSetup } from './setup-api';
import { repeatedTitles, rowSubtitle } from './run-rows';

const INSTALL = 'npm i -g @the-open-engine-company/zeroshot@latest';

function setup(zeroshot: Partial<Setup['zeroshot']>, problems: Setup['problems'] = []): Setup {
  return {
    app: 'zerochat',
    version: '0.1.0',
    ok: problems.length === 0,
    zeroshot: {
      found: true,
      program: '/usr/bin/zeroshot',
      version: '16.1.0',
      minimum: '16.1.0',
      meetsMinimum: true,
      stopResume: true,
      steer: true,
      steerNode: false,
      ...zeroshot,
    },
    runs: { stateDir: '/home/u/.local/state/zeroshot', exists: true, readable: true, count: 0 },
    zsf: { features: 0, found: false, program: null, version: null },
    pb: { configured: false, root: null },
    problems,
  };
}

const ok = setup({});
const missing = setup({ found: false, program: '/root/no-such-zeroshot', version: null, meetsMinimum: false, stopResume: false, steer: false }, [
  { check: 'zeroshot', message: 'Zeroshot not found at /root/no-such-zeroshot', fix: INSTALL },
]);
const tooOld = setup({ version: '11.0.0', meetsMinimum: false, stopResume: false, steer: false }, [{ check: 'zeroshot', message: 'Zeroshot 11.0.0 is too old (needs 16.1+)', fix: INSTALL }]);

const html = (el: Parameters<typeof renderToStaticMarkup>[0]) => renderToStaticMarkup(el);

// ---------- first-run card ----------

test('first-run card, setup ok: the watched dir in plain text, ✓ Zeroshot version, the example, zerochat vs zeroshot ui', () => {
  const out = html(createElement(FirstRunCard, { setup: ok, stateRoot: '/ignored' }));
  assert.match(out, /No runs yet/);
  assert.match(out, /<code class="onb-path">\/home\/u\/\.local\/state\/zeroshot<\/code>/);
  assert.match(out, /class="onb-ok"/);
  assert.match(out, /16\.1\.0/);
  assert.match(out, /\/usr\/bin\/zeroshot/);
  assert.ok(!out.includes('onb-bad'), 'no problem shown');
  assert.ok(!out.includes(INSTALL), 'no install command when all is well');
  assert.ok(out.includes('zeroshot run --template single-worker'), 'the example command');
  assert.match(out, /aria-label="Copy the example"/);
  assert.match(out, /<code>zeroshot ui<\/code>/);
});

test('first-run card, Zeroshot missing: ✗ with the copyable install command', () => {
  const out = html(createElement(FirstRunCard, { setup: missing, stateRoot: '/ignored' }));
  assert.match(out, /class="onb-bad"/);
  assert.match(out, /Zeroshot not found at \/root\/no-such-zeroshot/);
  assert.ok(out.includes(INSTALL));
  assert.match(out, /aria-label="Copy the install command"/);
  assert.ok(!out.includes('onb-ok'));
});

test('first-run card, Zeroshot too old: ✗ says the version and the minimum, with the install command', () => {
  const out = html(createElement(FirstRunCard, { setup: tooOld, stateRoot: '/ignored' }));
  assert.match(out, /Zeroshot 11\.0\.0 is too old \(needs 16\.1\+\)/);
  assert.ok(out.includes(INSTALL));
});

test('first-run card without the setup check: the overview’s state dir, no Zeroshot line', () => {
  const out = html(createElement(FirstRunCard, { setup: null, stateRoot: '/srv/state' }));
  assert.match(out, /\/srv\/state/);
  assert.ok(!out.includes('<dt>Zeroshot</dt>'));
  assert.ok(out.includes('single-worker'));
});

test('first-run card names an unreadable state dir', () => {
  const s = { ...ok, ok: false, problems: [{ check: 'runs' as const, message: 'Cannot read /x: denied', fix: null }] };
  assert.match(html(createElement(FirstRunCard, { setup: s, stateRoot: '/x' })), /Cannot read \/x: denied/);
});

test('the example uses stock Zeroshot 16.1 syntax: a title, the single-worker template, the task in --input', () => {
  assert.match(EXAMPLE_COMMAND, /zeroshot run --template single-worker --title "[^"]+" --input task\.json/);
  assert.match(EXAMPLE_COMMAND, /\{"task": "[^"]+"\}' > task\.json/);
});

test('the logo tooltip names both versions', () => {
  assert.equal(brandTitle(ok, 'zeroshot 16.1.0'), 'zerochat 0.1.0 · Zeroshot 16.1.0');
  assert.equal(brandTitle(missing, null), 'zerochat 0.1.0 · Zeroshot not found');
  assert.equal(brandTitle(null, 'zeroshot 11.0.0'), 'Zeroshot 11.0.0');
  assert.equal(brandTitle(null, null), undefined);
});

test('the top bar names the Zeroshot and the runs folder; a source build says so', () => {
  assert.equal(setupLine(ok)?.text, 'Zeroshot 16.1.0 · ~/.local/state/zeroshot');
  assert.match(setupLine(ok)?.title ?? '', /\/usr\/bin\/zeroshot[\s\S]*\/home\/u\/\.local\/state\/zeroshot/);
  const source = setup({ version: '0.1.0' });
  assert.equal(setupLine(source)?.text, 'Zeroshot built from source · ~/.local/state/zeroshot');
  assert.equal(brandTitle(source, null), 'zerochat 0.1.0 · Zeroshot built from source');
  assert.equal(setupLine(tooOld)?.text, 'Zeroshot 11.0.0 · ~/.local/state/zeroshot');
  assert.equal(setupLine(missing)?.text, 'Zeroshot not found · ~/.local/state/zeroshot');
  assert.equal(setupLine(null), null);
  assert.equal(homePath('/Users/me/x'), '~/x');
  assert.equal(homePath('/root'), '~');
  assert.equal(homePath('/srv/zeroshot'), '/srv/zeroshot');
});

test('the setup fetcher reads GET /api/setup', async () => {
  let asked = '';
  setFetch((async (path: string) => {
    asked = path;
    return new Response(JSON.stringify(ok), { status: 200 });
  }) as typeof fetch);
  try {
    assert.deepEqual(await fetchSetup(), ok);
    assert.equal(asked, '/api/setup');
  } finally {
    setFetch((...args) => fetch(...args));
  }
});

// ---------- Zeroshot strip ----------

test('the strip shows only when the Zeroshot check fails', () => {
  assert.equal(html(createElement(ZeroshotStrip, { setup: ok })), '');
  assert.equal(html(createElement(ZeroshotStrip, { setup: null })), '');
  const gone = html(createElement(ZeroshotStrip, { setup: missing }));
  assert.match(gone, /class="zeroshot-strip"/);
  assert.match(gone, /Zeroshot missing: stop, resume and steer are off/);
  assert.match(gone, /<summary>Fix<\/summary>/);
  assert.ok(gone.includes(INSTALL));
  assert.match(html(createElement(ZeroshotStrip, { setup: tooOld })), /Zeroshot 11\.0\.0 is too old \(needs 16\.1\+\)/);
  assert.equal(stripText(ok), null);
});

// ---------- glossary and first-visit hint ----------

test('the glossary lists the twelve core terms in order, each in one short line', () => {
  const terms = ['Run', 'Node', 'Round', 'Execution', 'Steer', 'Checkpoint', 'Background agents', 'Feature', 'Slice', 'Attempt', 'Approval slice', 'Gate'];
  assert.deepEqual(
    GLOSSARY.map((g) => g.term),
    terms
  );
  for (const g of GLOSSARY) assert.ok(g.meaning.length <= 90, `${g.term} is short`);
  const out = html(createElement(GlossaryList));
  for (const t of terms) assert.ok(out.includes(`<dt>${t}</dt>`), t);
  assert.match(out, /Checkpoint<\/dt><dd>A snapshot of the run’s workspace/);
});

test('the "?" in the top bar is a keyboard-reachable button that opens the glossary dialog', () => {
  const out = html(createElement(GlossaryButton));
  assert.match(out, /<button type="button"[^>]*aria-label="Glossary"[^>]*aria-haspopup="dialog"/);
  const app = readFileSync(new URL('../App.tsx', import.meta.url), 'utf8');
  assert.match(app, /<GlossaryButton setup=\{setup\} \/>/, 'it gets the setup, shown in the dialog (the top bar hides it on a phone)');
});

test('the first-visit hint shows until dismissed, and the dismissal is remembered', () => {
  const store = new Map<string, string>();
  const g = globalThis as { window?: unknown };
  const before = g.window;
  g.window = { localStorage: { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v), removeItem: (k: string) => store.delete(k) } };
  try {
    assert.match(html(createElement(FirstVisitHint)), /A run is one <code>zeroshot run<\/code>; click a node to read its chat or message it\./);
    let dismissed = false;
    const view = createElement(FirstVisitHintView, { onDismiss: () => (dismissed = true) });
    assert.match(html(view), /aria-label="Dismiss hint"/);
    view.props.onDismiss();
    assert.equal(dismissed, true);
    store.set(HINT_KEY, 'true');
    assert.equal(html(createElement(FirstVisitHint)), '', 'hidden after a dismissal');
  } finally {
    g.window = before;
  }
});

test('blocked storage: the hint still shows', () => {
  const g = globalThis as { window?: unknown };
  const before = g.window;
  g.window = {
    get localStorage(): never {
      throw new Error('blocked');
    },
  };
  try {
    assert.match(html(createElement(FirstVisitHint)), /first-visit-hint/);
  } finally {
    g.window = before;
  }
});

// ---------- Home: first-run card in place, headers and legend, context line ----------

const listing = (id: string, title: string, extra: Partial<RunListing> = {}): RunListing => ({
  runId: id,
  title,
  phase: 'finished',
  terminal: { status: 'succeeded' } as RunListing['terminal'],
  createdAt: Number(id.replace(/\D/g, '')) || 1,
  source: {},
  ...extra,
});

function homeHtml(runs: RunListing[], s: Setup | null = ok): string {
  const data = {
    targets: [{ id: 'local', name: 'This machine', kind: 'local', status: 'ok', runs }],
    issue: null,
    zeroshot: { program: 'zeroshot', version: null },
    stateRoot: '/srv/state',
    now: 0,
  } as unknown as Overview;
  const overview: OverviewState = { data, nav: data, loading: false, reloading: false, reload: () => undefined };
  return html(createElement(Home, { overview, now: 10_000, setup: s }));
}

test('Home with no runs shows the first-run card', () => {
  const out = homeHtml([]);
  assert.match(out, /class="onb-card"/);
  assert.match(out, /\/home\/u\/\.local\/state\/zeroshot/);
  assert.ok(!out.includes('run-table'));
});

test('Home’s run table shows its column headers and a status legend', () => {
  const out = homeHtml([listing('r1', 'One')]);
  assert.ok(!out.includes('quiet-head'), 'the headers are not visually hidden');
  for (const h of ['Run', 'Started', 'Duration', 'Cost']) assert.match(out, new RegExp(`<th scope="col"[^>]*>${h}</th>`), h);
  assert.match(out, /class="status-legend"/);
  for (const s of ['Running', 'Failed', 'Stalled', 'Stopped', 'Succeeded']) assert.ok(out.includes(`</span>${s}</li>`), s);
  assert.match(out, /first-visit-hint/);
});

test('rows with the same title show their context line; a failure cause wins; unique titles show none', () => {
  const sliceCtx = (line: string) => ({ kind: 'slice' as const, line });
  const runs = [
    listing('r1', 'zc 04: Add farewell()', { context: sliceCtx('Slice 04 of zc · attempt 1 of 2 · failed') }),
    listing('r2', 'zc 04: Add farewell()', { context: sliceCtx('Slice 04 of zc · attempt 2 of 2 · merged') }),
    listing('r3', 'Unique', { context: sliceCtx('Slice 05 of zc · attempt 1 of 1 · merged') }),
  ];
  const repeated = repeatedTitles(runs);
  assert.deepEqual([...repeated], ['zc 04: Add farewell()']);
  assert.equal(rowSubtitle(runs[1], '', repeated), 'Slice 04 of zc · attempt 2 of 2 · merged');
  assert.equal(rowSubtitle(runs[0], 'credentials rejected', repeated), 'credentials rejected');
  assert.equal(rowSubtitle(runs[2], '', repeated), '');
  assert.equal(rowSubtitle(listing('r4', 'zc 04: Add farewell()'), '', repeated), '', 'no context, no line');
  const out = homeHtml(runs);
  assert.ok(out.includes('Slice 04 of zc · attempt 2 of 2 · merged'));
  assert.ok(out.includes('Slice 04 of zc · attempt 1 of 2 · failed'));
  assert.ok(!out.includes('Slice 05 of zc'));
});

test('onboarding.css reads only defined tokens', () => {
  const tokens = readFileSync(new URL('../styles/tokens.css', import.meta.url), 'utf8');
  const defined = new Set([...tokens.matchAll(/(--[\w-]+):/g)].map((m) => m[1]));
  const css = readFileSync(new URL('../styles/onboarding.css', import.meta.url), 'utf8');
  for (const m of css.matchAll(/var\((--[\w-]+)/g)) assert.ok(defined.has(m[1]), m[1]);
  assert.match(readFileSync(new URL('../styles.css', import.meta.url), 'utf8'), /@import '\.\/styles\/onboarding\.css';/);
});

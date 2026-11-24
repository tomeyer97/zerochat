// Visual QA: screenshots of the UI, in light and dark, written next to this file as <shot>-<theme>.png.
//   npm run qa -- [baseUrl] [authBaseUrl]        (baseUrl default http://127.0.0.1:4317)
//
// Two kinds of shot:
// - mocked: a live run (rich transcript, diff, steering) built from src/fixtures/review-failed.json and
//   qa/fixtures/live-transcript.json, served by request interception. Work against any zerochat.
// - real: runs in the zerochat at baseUrl (GET only). They need the ids of runs on your machine, from
//   the environment; a shot whose ids are not set is skipped:
//     QA_TARGET            the target of the runs below (default: local)
//     QA_FAILED_RUN        a run that failed in review; with QA_REVIEW_VISIT (NODE/EXECUTION of a rejected
//                          review visit) and QA_FAILED_VISIT (NODE/EXECUTION of a failed visit)
//     QA_REJECTED_RUN      a run whose change was not accepted
//     QA_STOPPED_RUN       a force-stopped run
//     QA_LOST_RUN          a run whose controller was lost
//     QA_AUTH_FAILURE_RUN, QA_UNKNOWN_MODEL_RUN   local runs in the zerochat at authBaseUrl that failed on
//                          credentials and on a model that does not exist
//   The maintainers' end-to-end test lab starts runs like these.
// SHOTS=name,name takes only those shots; NOCLICK=1 skips the `click` step.
// Needs a Chromium: set CHROMIUM or have Playwright's cache in ~/.cache/ms-playwright.
import { chromium } from 'playwright-core';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const base = process.argv[2] ?? 'http://127.0.0.1:4317';
const authBase = process.argv[3];
const only = process.env.SHOTS ? process.env.SHOTS.split(',') : null;
const env = process.env;

/** Waits for the page to settle, in ms. */
const WAIT = { load: 1800, click: 700, dialog: 900, scroll: 300 };

function findChromium() {
  if (env.CHROMIUM) return env.CHROMIUM;
  const cache = join(homedir(), '.cache/ms-playwright');
  const dirs = existsSync(cache)
    ? readdirSync(cache).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse()
    : [];
  for (const d of dirs) {
    const p = join(cache, d, 'chrome-headless-shell-linux64/chrome-headless-shell');
    if (existsSync(p)) return p;
  }
  return undefined;
}

function readJson(path) {
  return JSON.parse(readFileSync(join(here, path), 'utf8'));
}

// ---------------------------------------------------------------- the mocked live run

const MOCK_RUN = '01a0ffff-0000-7000-8000-000000000001';
const MOCK_VIEW_PATH = `/api/runs/local/${MOCK_RUN}`;
const fixture = readJson('../src/fixtures/review-failed.json');
const transcriptFixture = readJson('fixtures/live-transcript.json');
/** The mocked run is at its 19th execution, which is running. */
const LIVE_EXECUTIONS = 19;

/** A running copy of the review-failed run on this machine. */
function liveView() {
  const now = Date.now();
  const copy = structuredClone(fixture);
  const executions = copy.executions.slice(0, LIVE_EXECUTIONS);
  const running = {
    ...executions.at(-1),
    state: 'running',
    endedAt: null,
    outcome: null,
    startedAt: now - 125_000,
    timesApproximate: false,
    workspaceStart: { commit: 'a'.repeat(40) },
    usageSource: 'session_log',
  };
  executions[executions.length - 1] = running;
  const nodes = Object.fromEntries(
    Object.entries(copy.nodes).map(([name, stats]) => {
      const visits = executions.filter((e) => e.node === name);
      const lastVisit = visits.at(-1);
      return [
        name,
        {
          ...stats,
          executions: stats.executions.filter((id) => id <= LIVE_EXECUTIONS),
          rounds: Math.max(0, ...visits.map((e) => e.round)),
          state: lastVisit ? lastVisit.state : 'idle',
          activeSince: lastVisit?.state === 'running' ? lastVisit.startedAt : null,
        },
      ];
    }),
  );
  const backgroundRetry = 'Claude API retry 1/10: overloaded_error';
  return {
    ...copy,
    runId: MOCK_RUN,
    target: 'local',
    targetKind: 'local',
    targetName: 'This machine',
    title: 'frontend-dashboard (live, mocked)',
    status: 'running',
    phase: 'running',
    terminal: null,
    failure: null,
    startedAt: now - 3_600_000,
    endedAt: null,
    capabilities: ['steer', 'replay', 'transcript', 'diff', 'workspace'],
    features: { exactTimes: true, toolCalls: true, providerCost: true, workspaceSnapshots: true },
    executions,
    nodes,
    usageLive: true,
    pressures: [
      {
        kind: 'timeout',
        level: 'warning',
        text: '4m 10s left of its 30m 0s timeout',
        execution: LIVE_EXECUTIONS,
        node: running.node,
        at: null,
        resetsAt: null,
      },
      {
        kind: 'rate_limit',
        level: 'warning',
        text: 'Claude five hour limit nearly used up',
        execution: LIVE_EXECUTIONS,
        node: running.node,
        at: now - 30_000,
        resetsAt: now + 50 * 60_000,
      },
    ],
    backgroundAgents: [
      {
        name: 'active_memory_graph',
        lines: 3,
        firstAt: now - 50_000,
        lastAt: now - 40_000,
        lastLine: backgroundRetry,
        retries: 1,
        errors: 0,
      },
    ],
    runLog: [{ seq: 1, at: now - 50_000, stream: 'system', text: `[active_memory_graph] ${backgroundRetry}` }],
  };
}

/** The live run after its review loop gave up (the Replay dialog's starting point). */
function failedView() {
  const view = liveView();
  const last = view.executions.at(-1);
  last.state = 'rejected';
  return {
    ...view,
    status: 'failed',
    failure: {
      ...fixture.failure,
      class: 'work',
      cause: 'not accepted after 3 rounds',
      title: 'Not accepted after 3 of 10 rounds',
      primary: 'replay',
      resetsAt: null,
      node: last.node,
      execution: LIVE_EXECUTIONS,
    },
  };
}

/** The running execution's transcript, with the fixture's `ago` offsets turned into times. */
function liveTranscript(view) {
  const now = Date.now();
  const at = ({ ago, ...rest }) => ({ ...rest, at: now - ago });
  const items = transcriptFixture.items.map((item) => {
    const timed = at(item);
    if (item.tool?.result) timed.tool = { ...item.tool, result: at(item.tool.result) };
    return timed;
  });
  return {
    execution: view.executions.at(-1),
    instructions: transcriptFixture.instructions,
    transcript: { source: 'native', sourcePath: transcriptFixture.sourcePath, limitations: [], items },
  };
}

const sampleDiff = `diff --git a/admin-dashboard/src/stores/metrics.js b/admin-dashboard/src/stores/metrics.js
index 1111111..2222222 100644
--- a/admin-dashboard/src/stores/metrics.js
+++ b/admin-dashboard/src/stores/metrics.js
@@ -38,9 +38,12 @@ export const useMetrics = defineStore('metrics', () => {
   function onReconnect(snapshot) {
-    buffer.push(...pending);
-    points.value = merge(points.value, buffer);
+    // Server truth wins after a reconnect: drop what was buffered before it.
+    buffer.length = 0;
+    points.value = snapshot.points.slice(-MAX_POINTS);
+    generation.value += 1;
   }

   function push(point) {
-    buffer.push(point);
+    if (point.generation !== generation.value) return;
+    buffer.push(point);
   }
`;

const changedFiles = {
  files: [
    { path: 'admin-dashboard/src/stores/metrics.js', status: 'M', additions: 5, deletions: 2 },
    { path: 'adversarial/repro.test.js', status: 'A', additions: 14, deletions: 0 },
  ],
  from: 'aaaaaaaaaaaa',
  to: null,
  note: '',
};

function checkpoints() {
  return [1, 2, 3, 4].map((i) => ({
    checkpointId: `cp${i}`,
    sequence: i,
    node: i === 1 ? 'worker' : 'parallel_reviews',
    mapIndices: [],
    loopIterations: i === 1 ? [] : [i - 1],
    createdAt: Date.now() - (5 - i) * 600_000,
  }));
}

/** Answers every request about the mocked run; `failed` serves its run view as failed. */
async function mockRun(page, { failed = false } = {}) {
  const live = liveView();
  const runView = failed ? failedView() : live;
  const transcript = liveTranscript(live);
  await page.route(`**${MOCK_VIEW_PATH}**`, async (route) => {
    const path = new URL(route.request().url()).pathname;
    const json = (body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
    if (path.endsWith('/events'))
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: 'event: ping\ndata: {}\n\n' });
    if (path.includes('/transcript/')) return json(transcript);
    if (path.endsWith('/changes')) return json(changedFiles);
    if (path.endsWith('/diff')) return json({ diff: sampleDiff });
    if (path.endsWith('/checkpoints')) return json({ checkpoints: checkpoints() });
    return json(path === MOCK_VIEW_PATH ? runView : live);
  });
}

// ---------------------------------------------------------------- the shots

/**
 * @typedef {object} Shot
 * @property {string} name        file name: <name>-<theme>.png
 * @property {string} hash        the route, after `#`
 * @property {string[]} [needs]   environment variables the hash uses; the shot is skipped without them
 * @property {string} [base]      another zerochat (default: baseUrl)
 * @property {boolean} [mock]     serve the mocked live run
 * @property {boolean} [mockFailed]  ... as failed
 * @property {string} [click]     a selector to click once loaded (skipped with NOCLICK=1)
 * @property {string} [clickText] then a button to click, by its name
 * @property {string} [then]      then a selector to click
 * @property {string} [scroll]    then a selector to scroll into view
 * @property {number} [width]     viewport (default 1440 x 920)
 * @property {number} [height]
 */

const target = encodeURIComponent(env.QA_TARGET ?? 'local');
const realRun = (variable, rest = '') => `#/run/${target}/${env[variable]}${rest}`;
const failed = 'QA_FAILED_RUN';
const reviewVisit = `/node/${env.QA_REVIEW_VISIT}`;
const failedVisit = `/node/${env.QA_FAILED_VISIT}`;
const live = `#/run/local/${MOCK_RUN}`;
const liveVisit = `${live}/node/review_repair_heavy/${LIVE_EXECUTIONS}`;

/** @type {Shot[]} */
const shots = [
  { name: 'overview', hash: '#/' },
  { name: 'overview-375', hash: '#/', width: 375, height: 800 },
  { name: 'failed-run', hash: realRun(failed), needs: [failed] },
  { name: 'failed-run-timeline', hash: realRun(failed), needs: [failed], scroll: '.timeline' },
  { name: 'rejected-run', hash: realRun('QA_REJECTED_RUN'), needs: ['QA_REJECTED_RUN'] },
  { name: 'stopped-run', hash: realRun('QA_STOPPED_RUN'), needs: ['QA_STOPPED_RUN'] },
  { name: 'runtime-lost', hash: realRun('QA_LOST_RUN'), needs: ['QA_LOST_RUN'] },
  { name: 'node-chat-rejected', hash: realRun(failed, reviewVisit), needs: [failed, 'QA_REVIEW_VISIT'] },
  { name: 'node-chat-failed', hash: realRun(failed, failedVisit), needs: [failed, 'QA_FAILED_VISIT'] },
  {
    name: 'node-details',
    hash: realRun(failed, failedVisit),
    needs: [failed, 'QA_FAILED_VISIT'],
    click: 'button[role=tab]:has-text("Details")',
  },
  { name: 'files', hash: realRun(failed, '/files'), needs: [failed] },
  { name: 'live-mock', hash: live, mock: true },
  { name: 'live-mock-chat', hash: liveVisit, mock: true, click: '.tool-group-head' },
  {
    name: 'live-mock-files',
    hash: liveVisit,
    mock: true,
    click: 'button[role=tab]:has-text("Files")',
    then: '.changed-file',
  },
  { name: 'replay-dialog', hash: live, mock: true, mockFailed: true, clickText: 'Replay' },
  { name: 'run-375', hash: realRun('QA_REJECTED_RUN'), needs: ['QA_REJECTED_RUN'], width: 375, height: 800 },
  {
    name: 'node-chat-375',
    hash: realRun(failed, reviewVisit),
    needs: [failed, 'QA_REVIEW_VISIT'],
    width: 375,
    height: 800,
  },
  { name: 'narrow-1100', hash: realRun(failed, reviewVisit), needs: [failed, 'QA_REVIEW_VISIT'], width: 1100 },
];
if (authBase) {
  const authRun = (variable, rest = '') => `#/run/local/${env[variable]}${rest}`;
  shots.push(
    { name: 'auth-failure', base: authBase, hash: authRun('QA_AUTH_FAILURE_RUN'), needs: ['QA_AUTH_FAILURE_RUN'] },
    { name: 'unknown-model', base: authBase, hash: authRun('QA_UNKNOWN_MODEL_RUN'), needs: ['QA_UNKNOWN_MODEL_RUN'] },
    {
      name: 'unknown-model-chat',
      base: authBase,
      hash: authRun('QA_UNKNOWN_MODEL_RUN', '/node/worker'),
      needs: ['QA_UNKNOWN_MODEL_RUN'],
    },
  );
}

/** The shot's clicks and scrolls, each followed by its wait. */
async function act(page, shot) {
  if (shot.click && !env.NOCLICK) {
    await page.locator(shot.click).first().click();
    await page.waitForTimeout(WAIT.click);
  }
  if (shot.clickText) {
    await page.getByRole('button', { name: shot.clickText }).first().click();
    await page.waitForTimeout(WAIT.dialog);
  }
  if (shot.then) {
    await page.locator(shot.then).first().click();
    await page.waitForTimeout(WAIT.click);
  }
  if (shot.scroll) {
    await page.locator(shot.scroll).first().scrollIntoViewIfNeeded();
    await page.waitForTimeout(WAIT.scroll);
  }
}

/** Takes one screenshot; returns the problems seen (console errors, sideways scrolling). */
async function takeShot(browser, shot, theme) {
  const problems = [];
  const context = await browser.newContext({
    viewport: { width: shot.width ?? 1440, height: shot.height ?? 920 },
    colorScheme: theme,
    deviceScaleFactor: 1,
  });
  await context.addInitScript((t) => localStorage.setItem('zerochat.theme.v1', t), theme);
  const page = await context.newPage();
  const where = `${shot.name}/${theme}`;
  page.on('console', (m) => m.type() === 'error' && problems.push(`${where}: console: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`${where}: pageerror: ${e.message}`));
  if (shot.mock) await mockRun(page, { failed: shot.mockFailed });
  await page.goto(`${shot.base ?? base}/${shot.hash}`);
  await page.waitForTimeout(WAIT.load);
  await act(page, shot);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  if (overflow > 0) problems.push(`${where}: page scrolls sideways by ${overflow}px`);
  await page.screenshot({ path: join(here, `${shot.name}-${theme}.png`) });
  await context.close();
  return problems;
}

const wanted = shots.filter((shot) => !only || only.includes(shot.name));
const skipped = wanted.filter((shot) => (shot.needs ?? []).some((variable) => !env[variable]));
for (const shot of skipped) console.log(`skipped ${shot.name}: set ${shot.needs.filter((v) => !env[v]).join(', ')}`);

const browser = await chromium.launch({ executablePath: findChromium() });
const problems = [];
for (const theme of ['light', 'dark']) {
  for (const shot of wanted) {
    if (!skipped.includes(shot)) problems.push(...(await takeShot(browser, shot, theme)));
  }
}
await browser.close();
console.log(problems.length ? problems.join('\n') : 'no console errors');

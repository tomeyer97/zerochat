import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  capabilityHint,
  diagnosticText,
  duplicateTitleSuffixes,
  isLive,
  isWatchable,
  listingStatus,
  matchesFilter,
  mergeOverview,
  missingData,
  nodeState,
  observeOnly,
  observeOnlyNote,
  overviewDelay,
  runStatus,
  runStatusInfo,
  runSubtitle,
  sortRuns,
  statusGroup,
  verdictText,
} from './status';
import type { Overview, RunListing, RunView } from './types';

const run = (id: string, createdAt: number, phase: string, status?: string, reason?: string): RunListing => ({
  runId: id,
  title: `title ${id}`,
  phase,
  terminal: status ? { status, reason } : null,
  createdAt,
  source: { repository: 'acme/app', branch: 'main' },
});

type Features = RunView['features'];
const allFeatures: Features = { exactTimes: true, toolCalls: true, providerCost: true, workspaceSnapshots: true };
const noFeatures: Features = { exactTimes: false, toolCalls: false, providerCost: false, workspaceSnapshots: false };

/** The part of a run view `missingData` and `observeOnly` read; each test names what differs. */
const viewWith = (view: Partial<Pick<RunView, 'capabilities' | 'features' | 'targetKind'>> & { executions?: unknown[] }) => ({
  capabilities: [] as string[],
  features: allFeatures,
  targetKind: 'local' as RunView['targetKind'],
  ...view,
  executions: (view.executions ?? []) as RunView['executions'],
});

test('listing status prefers the terminal status, else the live phase', () => {
  assert.equal(listingStatus(run('a', 1, 'finished', 'failed', 'review_failed')), 'failed');
  assert.equal(listingStatus(run('a', 1, 'running')), 'running');
  assert.equal(listingStatus(run('a', 1, 'stalled')), 'stalled');
  assert.equal(listingStatus(run('a', 1, 'weird')), 'unknown');
});

test('newest first, whatever the state', () => {
  const sorted = sortRuns([run('old', 1, 'finished', 'succeeded'), run('live', 0, 'running'), run('new', 5, 'finished', 'failed')]);
  assert.deepEqual(
    sorted.map((r) => r.runId),
    ['new', 'old', 'live']
  );
});

test('force-stopped runs are stopped; the summary wins; red is only for failures', () => {
  assert.equal(listingStatus(run('a', 1, 'finished', 'failed', 'force_stopped')), 'stopped');
  assert.equal(listingStatus({ ...run('a', 1, 'running'), summary: { status: 'stalled', cause: 'controller lost' } as never }), 'stalled');
  assert.equal(runStatusInfo.running.tone, 'running');
  assert.notEqual(runStatusInfo.stalled.tone, 'bad');
  assert.equal(runStatusInfo.failed.tone, 'bad');
});

test('the list subtitle is the cause', () => {
  const r = { ...run('a', 1, 'finished', 'failed', 'worker_failed'), summary: { status: 'failed', cause: 'credentials rejected' } as never };
  assert.equal(runSubtitle(r), 'credentials rejected');
  assert.equal(runSubtitle(run('b', 1, 'running')), '', 'the status dot already says running');
  assert.ok(matchesFilter(r, 'credentials'));
  assert.ok(matchesFilter({ ...run('c', 1, 'running'), summary: { status: 'stalled' } as never }, 'failed'), 'stalled runs are in the failed group');
});

test('the two causes long enough to crowd a sidebar row are shortened', () => {
  const withCause = (cause: string) => ({ ...run('a', 1, 'finished', 'failed', 'worker_failed'), summary: { status: 'failed', cause } as never });
  assert.equal(runSubtitle(withCause('not accepted after 2 rounds')), 'rejected ×2');
  assert.equal(runSubtitle(withCause('not accepted after 1 round')), 'rejected ×1');
  assert.equal(runSubtitle(withCause('timed out after 45s')), 'timed out');
  assert.equal(runSubtitle(withCause('usage limit')), 'usage limit', 'already short causes are untouched');
});

test('identical titles get an id tail', () => {
  const a = { ...run('01a1a64d-26ce-77f3-acf2-032ea516c52c', 1, 'running'), title: 'Same' };
  const b = { ...run('01a1a64d-26ce-77f3-acf2-032ea516beef', 2, 'running'), title: 'Same' };
  const c = { ...run('01a1a64d-0000-0000-0000-000000000000', 3, 'running'), title: 'Other' };
  const tails = duplicateTitleSuffixes([a, b, c]);
  assert.deepEqual([tails.get(a.runId), tails.get(b.runId), tails.get(c.runId)], ['c52c', 'beef', undefined]);
});

test('a run that cannot be read for a moment keeps its last good row', () => {
  const good = run('x', 1, 'running');
  const overview = (r: typeof good): Overview => ({
    targets: [{ id: 'local', name: 'This machine', kind: 'local', status: 'ok', runs: [r] }],
    issue: null,
    zeroshot: { program: 'z', version: null },
    stateRoot: '/',
    now: 0,
  });
  const blip = { ...good, title: '', phase: 'unavailable', issue: 'database is locked' };
  assert.equal(mergeOverview(overview(good), overview(blip)).targets[0].runs[0].title, good.title);
  assert.equal(mergeOverview(undefined, overview(blip)).targets[0].runs[0].phase, 'unavailable');
});

test('filter matches title, status, reason and repository', () => {
  const r = run('01ab', 1, 'finished', 'failed', 'change_attempts_exhausted');
  assert.ok(matchesFilter(r, 'failed'));
  assert.ok(matchesFilter(r, 'exhausted acme'));
  assert.ok(matchesFilter(r, ''));
  assert.ok(!matchesFilter(r, 'succeeded'));
});

test('filter also matches what the row actually displays, not just raw fields', () => {
  const r = {
    ...run('01ac', 1_700_000_000_000, 'finished', 'failed', 'change_attempts_exhausted'),
    summary: { status: 'failed', cause: 'not accepted after 3 rounds', failureNode: 'acceptance' } as never,
  };
  // The sidebar/home row shows "rejected ×3" (the shortened cause), not the raw cause text.
  assert.ok(matchesFilter(r, 'rejected'));
  assert.ok(matchesFilter(r, 'x3') || matchesFilter(r, '×3'));
  // The failing node, even though it's only shown on the run page itself.
  assert.ok(matchesFilter(r, 'acceptance'));
  assert.ok(!matchesFilter(r, 'worker'));
});

test('isWatchable keeps polling/SSE alive for a stalled run, not just a live one', () => {
  assert.ok(isWatchable('running'));
  assert.ok(isWatchable('stalled'));
  assert.ok(!isLive('stalled'), 'stalled is not counted as actively live elsewhere (ticking, badges)');
  assert.ok(!isWatchable('failed'));
  assert.ok(!isWatchable('succeeded'));
});

test('execution states and verdicts', () => {
  assert.equal(nodeState('rejected'), 'rejected');
  assert.equal(nodeState(undefined), 'idle');
  assert.equal(nodeState('bogus'), 'idle');
  assert.equal(verdictText({ signals: { verdict: 'rejected' } }), 'verdict: rejected');
  assert.equal(diagnosticText({ message: 'Fix the race' }), 'Fix the race');
  assert.equal(diagnosticText(null), '');
});

test('missing data is listed only when missing', () => {
  const view = viewWith({ features: noFeatures, executions: [{}], targetKind: 'local' });
  const missing = missingData(view);
  assert.ok(missing.some((m) => m.startsWith('exact node times')));
  assert.ok(!missing.includes('tool calls'), "local Claude runs get tool calls from Claude's own log");
  const full = viewWith({ capabilities: ['steer', 'replay'], features: allFeatures, targetKind: 'direct' });
  assert.deepEqual(missingData(full), []);
});

// pb is the ProgramBench runner: zerochat shows its benchmark runs but never drives them.
test('an observe-only source (pb) offers no actions and says why, instead of blaming Zeroshot', () => {
  const view = viewWith({ capabilities: [], features: allFeatures, targetKind: 'observe' });
  assert.deepEqual(missingData(view), [], 'no steer/replay is the source policy, not missing Zeroshot data');
  assert.equal(observeOnly(view), true);
  assert.equal(observeOnly({ targetKind: 'local' }), false);
  const batchRun = observeOnlyNote({ targetName: 'pb', context: { kind: 'benchmark', parent: '20261107-1', line: '' } });
  assert.match(batchRun, /^Observed only: pb runs this/);
  assert.match(batchRun, /stop its batch/);
  assert.doesNotMatch(observeOnlyNote({ targetName: 'pb', context: { kind: 'benchmark', line: '' } }), /stop its batch/, 'a label group has no batch to stop');
});

test('a page with no run list polls only the nav, and rarely (perf: /api/overview every 4 s everywhere)', () => {
  assert.equal(overviewDelay('full', false), 4_000);
  assert.equal(overviewDelay('slow', false), 5_000);
  assert.equal(overviewDelay('full', true), 15_000);
  assert.equal(overviewDelay('nav', false), 30_000);
  assert.equal(overviewDelay('nav', true), 60_000);
});

test('a suspended run (Zeroshot 16.1) is neither live nor failed, and the hints name no fork', () => {
  assert.equal(runStatus({ status: 'suspended' }), 'suspended');
  assert.equal(isLive('suspended'), false);
  assert.equal(statusGroup('suspended'), 'stopped');
  assert.equal(listingStatus({ runId: 'r', phase: 'suspended', terminal: null } as never), 'suspended');
  for (const c of ['steer', 'replay'] as const) assert.doesNotMatch(capabilityHint(c), /zerochat|branch|patch/);
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { homeRows, runRowFacts, sidebarRunLabel, sortRuns } from './run-list';
import { runElapsed } from './run-model';
import { FIXED_NOW, apiFile } from './testdata/golden';
import type { Overview, RunListing, RunView, TargetOverview } from './types';

const run = (id: string, createdAt: number, phase: string, summary?: Partial<NonNullable<RunListing['summary']>>): RunListing => ({
  runId: id,
  title: `title ${id}`,
  phase,
  terminal: null,
  createdAt,
  source: {},
  ...(summary ? { summary: { cause: null, startedAt: null, durationMs: null, ...summary } as NonNullable<RunListing['summary']> } : {}),
});

const target = (id: string, runs: RunListing[]): TargetOverview => ({ id, name: `Target ${id}`, kind: 'local', status: 'ok', runs });

test('runRowFacts: a live run ticks from its start; a finished one shows its recorded duration', () => {
  const live = runRowFacts(run('a', 1_000, 'running', { status: 'running', startedAt: 2_000, durationMs: 5 }), 10_000);
  assert.deepEqual({ live: live.live, started: live.started, ms: live.ms }, { live: true, started: 2_000, ms: 8_000 });
  const done = runRowFacts(run('b', 1_000, 'finished', { status: 'failed', cause: 'credentials rejected', durationMs: 42 }), 10_000);
  assert.deepEqual(done, { status: 'failed', live: false, started: 1_000, ms: 42, cause: 'credentials rejected' });
  assert.equal(runRowFacts(run('c', 1_000, 'finished'), 10_000).ms, null, 'no summary: no duration');
});

test('sidebarRunLabel: the recognised task, else the title', () => {
  assert.equal(sidebarRunLabel({ ...run('pb-1', 1, 'finished'), context: { task: 'fix the parser' } as never }), 'fix the parser');
  assert.equal(sidebarRunLabel(run('a', 1, 'finished')), 'title a');
});

test('homeRows: chip counts follow the text filter, the rows follow the chip too', () => {
  const targets = [
    target('local', [run('r1', 3, 'running', { status: 'running' }), run('f1', 2, 'finished', { status: 'failed' })]),
    target('box', [run('f2', 1, 'finished', { status: 'failed' }), run('s1', 4, 'finished', { status: 'succeeded' })]),
  ];
  const all = homeRows(targets, '', 'all');
  assert.deepEqual(
    all.all.map((r) => r.runId),
    ['s1', 'r1', 'f1', 'f2'],
    'newest first, across targets'
  );
  assert.deepEqual(all.counts, { all: 4, running: 1, failed: 2, succeeded: 1 });
  assert.equal(all.manyTargets, true);
  assert.equal(all.rows.find((r) => r.runId === 'f2')?.targetName, 'Target box');

  const failed = homeRows(targets, '', 'failed');
  assert.deepEqual(
    failed.rows.map((r) => r.runId),
    ['f1', 'f2']
  );
  assert.deepEqual(failed.counts, all.counts, 'the chip does not change the counts');

  const filtered = homeRows(targets, 'f1', 'all');
  assert.deepEqual(filtered.counts, { all: 1, failed: 1 });
  assert.equal(filtered.all.length, 4, '`all` ignores the filter');

  assert.equal(homeRows([targets[0]], '', 'all').manyTargets, false);
});

// ---------- F19, F31: one duration; sorted by Started ----------

test('sortRuns: newest first by the Started time the row shows, not the import time', () => {
  // A pb run imported (created) today that started on Nov 4 sorts after today's runs.
  const today1 = run('today-1', 10_000, 'finished', { status: 'succeeded', startedAt: 10_500 });
  const today2 = run('today-2', 20_000, 'finished', { status: 'succeeded', startedAt: 20_500 });
  const pb = run('pb', 15_000, 'finished', { status: 'succeeded', startedAt: 1_000 });
  const notStarted = run('admitted', 25_000, 'admitted');
  assert.deepEqual(
    sortRuns([today1, pb, today2, notStarted]).map((r) => r.runId),
    ['admitted', 'today-2', 'today-1', 'pb']
  );
});

/** A run's listing and its page, from the same server snapshot. */
function listedAndViewed(name: string, runId: string) {
  const overview = apiFile<Overview>('overview.json').body;
  const listing = overview.targets.flatMap((t) => t.runs).find((r) => r.runId === runId)!;
  return { listing, view: apiFile<RunView>(`runs/${name}/run.json`).body };
}

test('one duration: the list row and the run header agree, to the outcome', () => {
  for (const name of ['fork-only/fork-late-failure', 'fork-only/fork-happy-path', 'closing-postmortem', 'stalled-then-closed']) {
    const view = apiFile<RunView>(`runs/${name}/run.json`).body;
    const { listing } = listedAndViewed(name, view.runId);
    assert.equal(runRowFacts(listing, FIXED_NOW + 600_000).ms, runElapsed(view, FIXED_NOW + 600_000), name);
  }
  // A failed run whose postmortem kept Zeroshot closing it: the duration ends at the failure.
  const late = apiFile<RunView>('runs/fork-only/fork-late-failure/run.json').body;
  assert.ok(late.failure?.closedAfterMs, 'the closing is told apart on the card');
  assert.ok(runElapsed(late, FIXED_NOW)! < late.lastActivityAt! - late.startedAt!);
});

test('a run whose outcome is in stops counting while Zeroshot closes it', () => {
  // Zeroshot still closing: the phase says running, the summary has the outcome.
  const closing = run('c', 1_000, 'running', { status: 'succeeded', startedAt: 2_000, endedAt: 9_000, durationMs: 7_000 });
  assert.deepEqual(pick(runRowFacts(closing, 60_000)), { live: false, ms: 7_000 });
  // A stale row (its status not known yet) that already has an outcome stops too.
  const stale = run('s', 1_000, 'running', { status: undefined, startedAt: 2_000, endedAt: 9_000, durationMs: 7_000 });
  assert.deepEqual(pick(runRowFacts(stale, 60_000)), { live: false, ms: 7_000 });
  const live = run('l', 1_000, 'running', { status: 'running', startedAt: 2_000, endedAt: null });
  assert.deepEqual(pick(runRowFacts(live, 60_000)), { live: true, ms: 58_000 });
});

const pick = ({ live, ms }: { live: boolean; ms: number | null }) => ({ live, ms });

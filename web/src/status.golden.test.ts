// Goldens for status.ts over the overview and every run the API snapshots hold.
// golden.ts pins TZ=UTC, Date.now() and the en-US locale first (matchesFilter matches on formatted times).
import { apiBodies, apiFile, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import {
  diagnosticText,
  duplicateTitleSuffixes,
  execStateInfo,
  executionLabel,
  failureClass,
  failureClassInfo,
  idTail,
  isLive,
  isWatchable,
  listingStatus,
  matchesFilter,
  mergeOverview,
  missingData,
  nodeState,
  observeOnly,
  observeOnlyNote,
  orderTargets,
  overviewDelay,
  positionText,
  runStatus,
  runStatusInfo,
  runSubtitle,
  sortRuns,
  stateMark,
  statusGroup,
  stepExecution,
  verdictText,
  type RunStatus,
} from './status';
import type { Overview, RunListing, RunView } from './types';

const overview = apiFile<Overview>('overview.json').body;
const overviewNav = apiFile<Overview>('overview-nav.json').body;
const listings: [string, RunListing][] = overview.targets.flatMap((t) => t.runs.map((r) => [`${t.id}/${r.runId}`, r] as [string, RunListing]));
const allListings = listings.map(([, r]) => r);
const runs = apiBodies<RunView>('runs', /(^|\/)run\.json$/).map(([name, view]) => [name.replace(/\/run$/, ''), view] as [string, RunView]);
const executions = runs.flatMap(([name, view]) => view.executions.map((e) => [`${name}#${e.id}`, e, view] as const));
const byListing = <T>(f: (r: RunListing) => T) => Object.fromEntries(listings.map(([n, r]) => [n, f(r)]));
const byRun = <T>(f: (v: RunView) => T) => Object.fromEntries(runs.map(([n, v]) => [n, f(v)]));

const STATUSES: RunStatus[] = ['running', 'succeeded', 'failed', 'stalled', 'stopped', 'stopping', 'admitted', 'unknown'];
const NODE_VALUES = [null, undefined, '', 'idle', 'running', 'succeeded', 'accepted', 'rejected', 'failed', 'interrupted', 'voided', 'pending', 'constructor', 'toString', '__proto__'];

test('golden: listingStatus over the overview and edge listings', () => {
  const base = { runId: 'r', title: 't', phase: 'running', terminal: null, createdAt: null, source: null } as RunListing;
  const edge: [string, RunListing][] = [
    ['summary wins', { ...base, summary: { status: 'stalled' } as RunListing['summary'] }],
    ['unknown summary falls through', { ...base, summary: { status: 'weird' } as RunListing['summary'] }],
    ['force_stopped', { ...base, terminal: { status: 'failed', reason: 'force_stopped' } }],
    ['terminal succeeded', { ...base, terminal: { status: 'succeeded' } }],
    ['terminal other', { ...base, terminal: { status: 'cancelled' } }],
    ['phase finished', { ...base, phase: 'finished' }],
    ['phase admitted', { ...base, phase: 'admitted' }],
    ['phase odd', { ...base, phase: 'unavailable' }],
  ];
  checkGolden('status/listingStatus', { overview: byListing(listingStatus), edge: Object.fromEntries(edge.map(([n, r]) => [n, listingStatus(r)])) });
});

test('golden: runStatus, missingData, observeOnly, observeOnlyNote over every run', () => {
  checkGolden('status/runStatus', byRun(runStatus));
  checkGolden('status/missingData', byRun(missingData));
  checkGolden('status/observeOnly', byRun(observeOnly));
  const notes = byRun(observeOnlyNote);
  const edge = [
    { targetName: '', context: null },
    { targetName: 'pb-sandbox', context: { kind: 'benchmark', line: '', parent: '20261108-142204-d474ae' } },
    { targetName: 'pb-sandbox', context: { kind: 'benchmark', line: '', parent: 'label:smoke' } },
    { targetName: 'zsf', context: { kind: 'slice', line: '' } },
  ] as Pick<RunView, 'targetName' | 'context'>[];
  checkGolden('status/observeOnlyNote', { runs: notes, edge: edge.map(observeOnlyNote) });
});

test('golden: status tables (isLive, isWatchable, statusGroup, runStatusInfo)', () => {
  checkGolden('status/isLive', Object.fromEntries(STATUSES.map((s) => [s, isLive(s)])));
  checkGolden('status/isWatchable', Object.fromEntries(STATUSES.map((s) => [s, isWatchable(s)])));
  checkGolden('status/statusGroup', Object.fromEntries(STATUSES.map((s) => [s, statusGroup(s)])));
  checkGolden('status/runStatusInfo', runStatusInfo);
  checkGolden('status/execStateInfo', execStateInfo);
  checkGolden('status/failureClassInfo', failureClassInfo);
});

test('golden: sortRuns, duplicateTitleSuffixes, idTail over the overview', () => {
  const refs = refsOf({ runs: allListings });
  checkGolden('status/sortRuns', sortRuns(allListings), refs);
  checkGolden('status/duplicateTitleSuffixes', duplicateTitleSuffixes(allListings));
  checkGolden('status/idTail', { overview: byListing((r) => idTail(r.runId)), table: ['', 'abc', 'a-b-c-d-e', '0190c3a2-7b1e-7c00-8000-00000000abcd'].map((id) => [id, idTail(id)]) });
});

test('golden: matchesFilter and runSubtitle over the overview', () => {
  const filters = [
    '',
    '   ',
    'failed',
    'FAILED',
    'running',
    'stalled',
    'stopped',
    'succeeded',
    'timed out',
    'rejected',
    'setup',
    'agent',
    'work',
    'credentials',
    'AM',
    'PM',
    'nov',
    'Nov 18',
    '2026',
    's',
    'm',
    'not-a-word',
    'main',
    'force',
    'unknown',
  ];
  checkGolden(
    'status/matchesFilter',
    byListing((r) => filters.filter((f) => matchesFilter(r, f)))
  );
  checkGolden('status/runSubtitle', byListing(runSubtitle));
});

test('golden: mergeOverview', () => {
  const unreadable: Overview = {
    ...overview,
    targets: overview.targets.map((t, ti) => ({
      ...t,
      runs: t.runs.map((r, i) => (i % 3 === 0 ? { ...r, phase: 'unavailable', title: '' } : i % 3 === 1 && ti === 0 ? { ...r, issue: 'mid-write', title: '' } : r)),
    })),
  };
  const refs = refsOf({ overview, overviewNav, unreadable });
  checkGolden(
    'status/mergeOverview',
    {
      'no previous': mergeOverview(undefined, overview),
      'same twice': mergeOverview(overview, overview),
      'unreadable keeps last good rows': mergeOverview(overview, unreadable),
      'unreadable stays without a previous': mergeOverview(unreadable, unreadable),
      'nav after full': mergeOverview(overview, overviewNav),
    },
    refs
  );
});

test('golden: overviewDelay and orderTargets', () => {
  checkGolden('status/overviewDelay', Object.fromEntries((['full', 'slow', 'nav'] as const).flatMap((m) => [false, true].map((h) => [`${m} hidden=${h}`, overviewDelay(m, h)]))));
  const refs = refsOf({ overview, overviewNav });
  checkGolden('status/orderTargets', { overview: orderTargets(overview.targets), overviewNav: orderTargets(overviewNav.targets), reversed: orderTargets([...overview.targets].reverse()) }, refs);
});

test('golden: nodeState and stateMark', () => {
  checkGolden('status/nodeState', {
    table: NODE_VALUES.map((v) => [String(v), nodeState(v)]),
    executions: Object.fromEntries(executions.map(([n, e]) => [n, nodeState(e.state)])),
    nodes: byRun((v) => Object.fromEntries(Object.entries(v.nodes).map(([k, s]) => [k, nodeState(s.state)]))),
  });
  checkGolden('status/stateMark', Object.fromEntries((Object.keys(execStateInfo) as (keyof typeof execStateInfo)[]).concat(['other' as never]).map((s) => [s, stateMark(s)])));
});

test('golden: verdictText, diagnosticText over every execution', () => {
  checkGolden('status/verdictText', {
    executions: Object.fromEntries(executions.map(([n, e]) => [n, verdictText(e)])),
    table: [null, {}, { verdict: 'accepted' }, { verdict: 'rejected', score: 3, nested: { a: [1] } }].map((s) => verdictText({ signals: s as never })),
  });
  const table = [null, undefined, '', 'plain', { message: 'only' }, { message: 'with', code: 'x' }, { code: 'x' }, ['a'], 42, true, { message: 5 }];
  checkGolden('status/diagnosticText', {
    executions: Object.fromEntries(executions.map(([n, e]) => [n, diagnosticText(e.diagnostic)])),
    table: table.map((d) => diagnosticText(d)),
  });
});

test('golden: executionLabel, positionText over every execution', () => {
  checkGolden('status/executionLabel', Object.fromEntries(executions.map(([n, e]) => [n, [executionLabel(e), executionLabel(e, false)]])));
  checkGolden(
    'status/positionText',
    Object.fromEntries(
      executions.map(([n, e, v]) => {
        const info = v.graphNodes.find((g) => g.name === e.node);
        const loop = info?.loops.length ? v.graphNodes.find((g) => g.name === info.loops[info.loops.length - 1]) : undefined;
        return [n, [positionText(e, loop?.maxIterations, info?.attempts, !!info?.loops.length), positionText(e, null, null, true), positionText(e, 3, 2, false)]];
      })
    )
  );
});

test('golden: failureClass', () => {
  checkGolden('status/failureClass', {
    runs: byRun((v) => (v.failure ? failureClass(v.failure.class) : null)),
    overview: byListing((r) => failureClass(r.summary?.failureClass ?? '')),
    table: ['', 'work', 'agent', 'setup', 'orchestration', 'delivery', 'stopped', 'novel', 'toString'].map((c) => [c, failureClass(c)]),
  });
});

test('golden: stepExecution over every run', () => {
  const out = byRun((v) => {
    const ids = [undefined, ...v.executions.map((e) => e.id), -1];
    return Object.fromEntries(ids.map((id) => [String(id), [-1, 1, 5].map((d) => stepExecution(v, id, d)?.id ?? null)]));
  });
  checkGolden('status/stepExecution', { runs: out, empty: stepExecution({ executions: [] }, undefined, 1) ?? null });
});

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { documentTitle, formatRoute, hashFromPath, pageTitle, parseRoute, visitFallback, visitOf } from './route';

test('routes round-trip, including target ids with colons', () => {
  const routes = [
    { kind: 'home' as const },
    { kind: 'run' as const, target: 'target:lab', run: '01a0c5cf-e835' },
    { kind: 'run' as const, target: 'local', run: 'abc', node: 'code', round: 2 },
    { kind: 'run' as const, target: 'local', run: 'abc', node: 'code', round: 2, attempt: 2 },
    { kind: 'run' as const, target: 'local', run: 'abc', node: 'item', execution: 26 },
    { kind: 'run' as const, target: 'local', run: 'abc', files: true },
  ];
  for (const route of routes) assert.deepEqual(parseRoute(formatRoute(route)), route);
  assert.equal(formatRoute(routes[1]), '#/run/target%3Alab/01a0c5cf-e835');
  assert.equal(formatRoute(routes[2]), '#/run/local/abc/node/code/round/2');
});

test('an empty hash goes home; node without a visit is kept', () => {
  assert.deepEqual(parseRoute(''), { kind: 'home' });
  assert.deepEqual(parseRoute('#/run/local/r/node/x'), { kind: 'run', target: 'local', run: 'r', node: 'x' });
  assert.deepEqual(parseRoute('#/run/local/r/node/x/nope'), { kind: 'run', target: 'local', run: 'r', node: 'x' });
  assert.deepEqual(parseRoute('#/run/target:lab/r'), { kind: 'run', target: 'target:lab', run: 'r' });
});

test('an unrecognised or incomplete path is "unknown", not silently home', () => {
  // A typo, a stale link or a route from a future version must not quietly show the run list.
  assert.deepEqual(parseRoute('#/run/local'), { kind: 'unknown', path: 'run/local' });
  assert.deepEqual(parseRoute('#/settings'), { kind: 'unknown', path: 'settings' });
  assert.equal(formatRoute({ kind: 'unknown', path: 'settings' }), '#/settings');
});

test('query strings are ignored and old numeric links mean an execution id', () => {
  assert.deepEqual(parseRoute('#/run/local/01a1-2243?x=1&utm_source=chat'), { kind: 'run', target: 'local', run: '01a1-2243' });
  assert.deepEqual(parseRoute('#/run/local/r/node/acceptance/5'), { kind: 'run', target: 'local', run: 'r', node: 'acceptance', execution: 5 });
});

// `pb` below is the ProgramBench runner, a configured source whose runs zerochat observes.
test('a configured source (pb, or any other non-local/non-direct target id) routes exactly like local did', () => {
  // One run route for every target kind, not a second route per source. Configured sources are addressed the same way targets are today: `#/run/<source>/<run>`.
  // Old links to a local run (`target: "local"`) must keep working byte-for-byte.
  assert.deepEqual(parseRoute('#/run/local/01a1-2243'), { kind: 'run', target: 'local', run: '01a1-2243' });
  assert.deepEqual(parseRoute('#/run/pb/20261103-120654-160a0e'), { kind: 'run', target: 'pb', run: '20261103-120654-160a0e' });
  assert.equal(formatRoute({ kind: 'run', target: 'pb', run: '20261103-120654-160a0e' }), '#/run/pb/20261103-120654-160a0e');
  assert.deepEqual(parseRoute('#/run/pb/r/node/worker/round/1'), { kind: 'run', target: 'pb', run: 'r', node: 'worker', round: 1 });
});

test('benchmarks routes: the list and one batch, including a label pseudo-batch id with colons and a hash', () => {
  assert.deepEqual(parseRoute('#/benchmarks'), { kind: 'benchmarks' });
  assert.deepEqual(parseRoute('#/benchmarks/batch/b1'), { kind: 'benchmarksBatch', id: 'b1' });
  const labelId = 'label:sweep:sweep-triage-repair:repair#1:claude-sonnet-5';
  const route = { kind: 'benchmarksBatch' as const, id: labelId };
  assert.deepEqual(parseRoute(formatRoute(route)), route);
  assert.deepEqual(parseRoute('#/benchmarks/batch'), { kind: 'unknown', path: 'benchmarks/batch' });
  assert.deepEqual(parseRoute('#/benchmarks/other'), { kind: 'unknown', path: 'benchmarks/other' });
});

test('benchmarks compare carries its state in the query string (unlike every other route)', () => {
  assert.deepEqual(parseRoute('#/benchmarks/compare?a=bA&b=bB'), { kind: 'benchmarksCompare', a: 'bA', b: 'bB' });
  assert.deepEqual(parseRoute('#/benchmarks/compare'), { kind: 'benchmarksCompare', a: undefined, b: undefined });
  assert.equal(formatRoute({ kind: 'benchmarksCompare', a: 'bA', b: 'bB' }), '#/benchmarks/compare?a=bA&b=bB');
  assert.equal(formatRoute({ kind: 'benchmarksCompare' }), '#/benchmarks/compare');
  // Label pseudo-batch ids contain ':' and '#', which must survive the round trip.
  const withColons = { kind: 'benchmarksCompare' as const, a: 'label:base', b: 'label:candidate:repair#1' };
  assert.deepEqual(parseRoute(formatRoute(withColons)), withColons);
});

test('benchmarks start and sweeps are plain routes; sweep-report carries filters in the query', () => {
  assert.deepEqual(parseRoute('#/benchmarks/start'), { kind: 'benchmarksStart' });
  assert.equal(formatRoute({ kind: 'benchmarksStart' }), '#/benchmarks/start');
  assert.deepEqual(parseRoute('#/benchmarks/sweeps'), { kind: 'benchmarksSweeps' });
  assert.equal(formatRoute({ kind: 'benchmarksSweeps' }), '#/benchmarks/sweeps');

  assert.deepEqual(parseRoute('#/benchmarks/sweep-report'), { kind: 'benchmarksSweepReport', ids: [], label: undefined, task: undefined, budget: undefined });
  assert.deepEqual(parseRoute('#/benchmarks/sweep-report?ids=sw-1,sw-2&label=L&task=gron&budget=4.5'), {
    kind: 'benchmarksSweepReport',
    ids: ['sw-1', 'sw-2'],
    label: 'L',
    task: 'gron',
    budget: '4.5',
  });
  const route = { kind: 'benchmarksSweepReport' as const, ids: ['sw-1', 'sw-2'], label: 'L', task: undefined, budget: undefined };
  assert.deepEqual(parseRoute(formatRoute(route)), route);
});

test('links name a visit by round, or by execution for map items', () => {
  assert.deepEqual(visitOf({ id: 5, round: 2, attempt: 1, mapIndices: [] }), { round: 2, attempt: undefined });
  assert.deepEqual(visitOf({ id: 6, round: 1, attempt: 2, mapIndices: [] }), { round: 1, attempt: 2 });
  assert.deepEqual(visitOf({ id: 7, round: 1, attempt: 1, mapIndices: [3] }), { execution: 7 });
});

test('every page names the tab, except the run and feature pages, which name it from their data', () => {
  assert.equal(pageTitle(parseRoute('')), 'zerochat');
  assert.equal(pageTitle(parseRoute('#/nope')), 'Page not found · zerochat');
  assert.equal(pageTitle(parseRoute('#/features')), 'Features · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks')), 'Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks/batch/b1')), 'b1 · Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks/compare?a=x&b=y')), 'Compare · Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks/start')), 'Start a batch · Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks/sweeps')), 'Sweeps · Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/benchmarks/sweep-report?ids=a,b')), 'Sweep report · Benchmarks · zerochat');
  assert.equal(pageTitle(parseRoute('#/run/local/r1')), null);
  assert.equal(pageTitle(parseRoute('#/features/f1')), null);
  assert.equal(documentTitle('build', 'My run'), 'build · My run · zerochat');
  assert.equal(documentTitle(null, 'My run'), 'My run · zerochat');
});

test("a stale visit falls back to the node's latest execution, with a notice naming what was missing", () => {
  const view = {
    executions: [
      { id: 1, node: 'build', round: 1, attempt: 1 },
      { id: 2, node: 'build', round: 2, attempt: 1 },
    ],
  };
  assert.deepEqual(visitFallback(view, { node: 'build', round: 7 }), { fallback: view.executions[1], notice: 'build has no round 7 here; showing its latest.' });
  assert.equal(visitFallback(view, { node: 'build', execution: 9 })?.notice, 'build has no execution 9 here; showing its latest.');
  assert.equal(visitFallback(view, { node: 'build', round: 2 }), null, 'the visit exists');
  assert.equal(visitFallback(view, { node: 'review', round: 1 }), null, 'the node never ran');
  assert.equal(visitFallback(view, {}), null);
});

test('a path link opens the route it names (F34)', () => {
  const id = '01a20478-9733-73c1-97af-c4e78d1b1928';
  assert.equal(hashFromPath(`/runs/local/${id}`, '', ''), `#/run/local/${id}`);
  assert.deepEqual(parseRoute(hashFromPath(`/runs/local/${id}/`, '', '')!), { kind: 'run', target: 'local', run: id });
  assert.equal(hashFromPath(`/run/local/${id}/node/worker`, '', ''), `#/run/local/${id}/node/worker`);
  assert.equal(hashFromPath('/features/calc/slice/04', '', ''), '#/features/calc/slice/04');
  assert.equal(hashFromPath('/benchmarks/compare', '?a=1&b=2', ''), '#/benchmarks/compare?a=1&b=2');
  assert.equal(parseRoute(hashFromPath('/nope', '', '')!).kind, 'unknown');
  assert.equal(hashFromPath('/', '', ''), null);
  // Every top-level page as a path: the "Runs" tab (the run list is home), Features, Benchmarks.
  for (const path of ['/runs', '/runs/', '/run', '/run/']) {
    assert.equal(hashFromPath(path, '', ''), '#/', path);
  }
  assert.deepEqual(parseRoute('#/runs'), { kind: 'home' });
  assert.deepEqual(parseRoute('#/run'), { kind: 'home' });
  assert.deepEqual(parseRoute(hashFromPath('/features/', '', '')!), { kind: 'features' });
  assert.deepEqual(parseRoute(hashFromPath('/benchmarks', '', '')!), { kind: 'benchmarks' });
  assert.equal(hashFromPath('/index.html', '', ''), null);
  assert.equal(hashFromPath('/runs/local/x', '', '#/features'), null, 'a hash route wins');
});

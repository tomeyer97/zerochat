// Goldens for route.ts: a table of hashes (odd ones included), their round trip, the href helpers,
// and every execution of every run the API snapshots hold.
import { apiBodies, attempt, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import {
  benchmarksBatchHref,
  benchmarksCompareHref,
  benchmarksHref,
  benchmarksStartHref,
  benchmarksSweepReportHref,
  benchmarksSweepsHref,
  featureHref,
  featuresHref,
  formatRoute,
  navigate,
  nodeHref,
  parseRoute,
  resolveVisit,
  runHref,
  visitOf,
  type Route,
} from './route';
import type { RunView } from './types';

const runs = apiBodies<RunView>('runs', /(^|\/)run\.json$/).map(([name, view]) => [name.replace(/\/run$/, ''), view] as [string, RunView]);

const HASHES = [
  '',
  '#',
  '#/',
  '/',
  '#//',
  '#/?utm_source=x',
  '#/run',
  '#/run/local',
  '#/run/local/r1',
  '#/run/local/r1/',
  'run/local/r1',
  '#run/local/r1',
  '#/run/local/r1?x=1',
  '#/run/local/r1/files',
  '#/run/local/r1/files/extra',
  '#/run/local/r1/node',
  '#/run/local/r1/node/worker',
  '#/run/local/r1/node/worker/round/2',
  '#/run/local/r1/node/worker/round/2/attempt/3',
  '#/run/local/r1/node/worker/round/2/attempt/1',
  '#/run/local/r1/node/worker/round/0/attempt/0',
  '#/run/local/r1/node/worker/round/x/attempt/3',
  '#/run/local/r1/node/worker/round/-1',
  '#/run/local/r1/node/worker/round/1.5',
  '#/run/local/r1/node/worker/round/',
  '#/run/local/r1/node/worker/round/2/attempt',
  '#/run/local/r1/node/worker/exec/7',
  '#/run/local/r1/node/worker/exec/',
  '#/run/local/r1/node/worker/exec/abc',
  '#/run/local/r1/node/worker/5',
  '#/run/local/r1/node/worker/05',
  '#/run/local/r1/node/worker/1e3',
  '#/run/local/r1/node/worker/0x10',
  '#/run/local/r1/node/worker/ 5',
  '#/run/local/r1/node/worker/round/2/../3',
  '#/run/a%2Fb/r%20id/node/n%23x',
  '#/run/a%2/r1',
  '#/run/%E0%A4%A/r1/node/%ZZ',
  '#/run/local/r1/other/thing',
  '#/run//r1',
  '#/run/local//node/x',
  '#/benchmarks',
  '#/benchmarks/',
  '#/benchmarks/batch',
  '#/benchmarks/batch/20261108-142204-d474ae',
  '#/benchmarks/batch/label%3Asmoke-haiku',
  '#/benchmarks/batch/a/b',
  '#/benchmarks/compare',
  '#/benchmarks/compare?a=x&b=y',
  '#/benchmarks/compare?a=&b=y',
  '#/benchmarks/compare?b=y&a=x&a=z',
  '#/benchmarks/compare/extra?a=x',
  '#/benchmarks/compare?a=x?b=y',
  '#/benchmarks/start',
  '#/benchmarks/start?x=1',
  '#/benchmarks/start/now',
  '#/benchmarks/sweeps',
  '#/benchmarks/sweeps/1',
  '#/benchmarks/sweep-report',
  '#/benchmarks/sweep-report?ids=a,b,%20c,,&label=L&task=t&budget=2.5',
  '#/benchmarks/sweep-report?ids=',
  '#/benchmarks/sweep-report?label=%E2%9C%93',
  '#/benchmarks/unknown',
  '#/features',
  '#/features/',
  '#/features/calc',
  '#/features/calc/slice/04',
  '#/features/calc/slice',
  '#/features/calc/other/04',
  '#/features/calc/slice/04/x',
  '#/features/a%20b/slice/c%2Fd',
  '#/nope',
  '#/nope/deeper?q=1',
  '#/RUN/local/r1',
];

test('golden: parseRoute over a table of hashes', () => {
  checkGolden(
    'route/parseRoute',
    HASHES.map((h) => [h, attempt(() => parseRoute(h))])
  );
});

test('golden: formatRoute of each parsed hash, and of hand-made routes', () => {
  const made: Route[] = [
    { kind: 'home' },
    { kind: 'unknown', path: 'a/b c' },
    { kind: 'run', target: 'a/b', run: 'r 1' },
    { kind: 'run', target: 't', run: 'r', files: true },
    { kind: 'run', target: 't', run: 'r', node: 'n', files: true },
    { kind: 'run', target: 't', run: 'r', node: 'n#1', round: 0 },
    { kind: 'run', target: 't', run: 'r', node: 'n', round: 2, attempt: 1 },
    { kind: 'run', target: 't', run: 'r', node: 'n', round: 2, attempt: 2, execution: 9 },
    { kind: 'run', target: 't', run: 'r', node: 'n', attempt: 2 },
    { kind: 'run', target: 't', run: 'r', node: 'n', execution: 0 },
    { kind: 'run', target: 't', run: 'r', node: '', execution: 3 },
    { kind: 'benchmarksBatch', id: 'label:x/y' },
    { kind: 'feature', name: 'f', slice: '' },
    { kind: 'feature', name: 'f?', slice: '0 1' },
    { kind: 'benchmarksCompare' },
    { kind: 'benchmarksCompare', b: 'only b' },
    { kind: 'benchmarksSweepReport', ids: [] },
    { kind: 'benchmarksSweepReport', ids: ['a', 'b,c'], label: 'l&m', task: '', budget: '0' },
  ];
  checkGolden('route/formatRoute', {
    roundTrip: HASHES.map((h) => {
      const route = parseRoute(h);
      const hash = formatRoute(route);
      return [h, hash, formatRoute(parseRoute(hash)) === hash];
    }),
    made: made.map((r) => [r, formatRoute(r)]),
  });
});

test('golden: href helpers', () => {
  const names = ['', 'plain', 'a/b', 'a b', 'ü#?&%', 'label:smoke'];
  checkGolden('route/hrefs', {
    runHref: names.map((n) => runHref(n, n)),
    benchmarksHref: benchmarksHref(),
    benchmarksBatchHref: names.map(benchmarksBatchHref),
    featuresHref: featuresHref(),
    featureHref: names.flatMap((n) => [featureHref(n), featureHref(n, n)]),
    benchmarksCompareHref: [benchmarksCompareHref(), benchmarksCompareHref('a'), benchmarksCompareHref(undefined, 'b'), benchmarksCompareHref('a b', 'c&d')],
    benchmarksStartHref: benchmarksStartHref(),
    benchmarksSweepsHref: benchmarksSweepsHref(),
    benchmarksSweepReportHref: [
      benchmarksSweepReportHref([]),
      benchmarksSweepReportHref(['s1']),
      benchmarksSweepReportHref(['s1', 's2'], { label: 'x y', task: 't', budget: '1.5' }),
      benchmarksSweepReportHref(['s1'], { label: '' }),
    ],
    nodeHref: [
      nodeHref('t', 'r', 'n'),
      nodeHref('t', 'r', 'n', { round: 1 }),
      nodeHref('t', 'r', 'n', { round: 1, attempt: 2 }),
      nodeHref('t', 'r', 'n', { execution: 4 }),
      nodeHref('a/b', 'r#', 'n n', { round: 0, execution: 4 }),
    ],
  });
});

test('golden: visitOf and nodeHref over every execution', () => {
  const out = Object.fromEntries(runs.map(([name, v]) => [name, v.executions.map((e) => [e.id, visitOf(e), nodeHref(v.target, v.runId, e.node, visitOf(e))])]));
  const table = [
    { id: 1, round: 1, attempt: 1 },
    { id: 2, round: 2, attempt: 3, mapIndices: [] },
    { id: 3, round: 1, attempt: 1, mapIndices: [0] },
    { id: 4, round: 1, attempt: 2, mapIndices: null },
    { id: 5, round: 1, attempt: 1, mapIndices: { 0: 1 } },
  ];
  checkGolden('route/visitOf', { runs: out, table: table.map(visitOf) });
});

test('golden: resolveVisit over every run', () => {
  const refs = refsOf({ runs: Object.fromEntries(runs) });
  const out = Object.fromEntries(
    runs.map(([name, v]) => {
      const routes: Parameters<typeof resolveVisit>[1][] = [{}, { node: 'no_such_node' }, { node: 'no_such_node', round: 1 }];
      for (const e of v.executions) {
        routes.push(
          { node: e.node },
          { node: e.node, ...visitOf(e) },
          { node: e.node, round: e.round },
          { node: e.node, round: e.round, attempt: e.attempt + 1 },
          { node: e.node, execution: e.id },
          { node: e.node, execution: e.id + 1000 }
        );
      }
      return [name, routes.map((r) => [r, resolveVisit(v, r)])];
    })
  );
  checkGolden('route/resolveVisit', out, refs);
});

test('golden: navigate (location/history stubbed)', () => {
  const calls: unknown[] = [];
  const g = globalThis as Record<string, unknown>;
  const saved = { history: g.history, location: g.location, window: g.window, HashChangeEvent: g.HashChangeEvent };
  const location = {
    _hash: '#/',
    get hash() {
      return this._hash;
    },
    set hash(v: string) {
      calls.push(['location.hash =', v]);
      this._hash = v;
    },
  };
  g.history = {
    replaceState: (...args: unknown[]) => {
      calls.push(['history.replaceState', ...args]);
      location._hash = String(args[2]);
    },
  };
  g.location = location;
  g.window = { dispatchEvent: (e: { type: string }) => calls.push(['window.dispatchEvent', e.type]) };
  g.HashChangeEvent = class {
    type: string;
    constructor(type: string) {
      this.type = type;
    }
  };
  try {
    const steps: [Route, boolean][] = [
      [{ kind: 'features' }, false],
      [{ kind: 'features' }, false],
      [{ kind: 'run', target: 't', run: 'r', node: 'n', round: 2 }, true],
      [{ kind: 'home' }, false],
      [{ kind: 'home' }, true],
    ];
    const out = steps.map(([route, replace]) => {
      calls.length = 0;
      navigate(route, replace);
      return { route, replace, calls: [...calls], hash: location.hash };
    });
    checkGolden('route/navigate', out);
  } finally {
    Object.assign(g, saved);
  }
});

// Goldens for graph-model.ts over every run the API snapshots hold: the flow model, the ELK
// graph built from it and ELK's layout of it (both directions), plus the condition helpers over
// every `when` in every graph.
import { apiBodies, attempt, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import ELK from 'elkjs/lib/elk.bundled.js';
import {
  buildElkGraph,
  buildFlowModel,
  commonModel,
  conditionLabel,
  guardNames,
  humanId,
  itemSize,
  labelSize,
  pickReachedFail,
  readElkLayout,
  shortLabel,
  showEveryModel,
  type FlowItem,
  type FlowModel,
} from './graph-model';
import type { RunView } from './types';

const runs = apiBodies<RunView>('runs', /(^|\/)run\.json$/).map(([name, view]) => [name.replace(/\/run$/, ''), view] as [string, RunView]);
const byRun = <T>(f: (v: RunView, name: string) => T) => Object.fromEntries(runs.map(([n, v]) => [n, f(v, n)]));
// What a flow item passes through from the run (its graph node info, stats and latest
// execution) is pinned by identity: `<ref:runs["name"].executions[3]>`.
const refs = refsOf({ runs: Object.fromEntries(runs) });
const models = new Map<string, FlowModel>(runs.map(([n, v]) => [n, buildFlowModel(v)]));

/** Every `when` (and the node it sits in) anywhere in a graph. */
function whens(graph: unknown): { node: string; when: unknown }[] {
  const out: { node: string; when: unknown }[] = [];
  const walk = (v: unknown, node: string) => {
    if (!v || typeof v !== 'object') return;
    if (Array.isArray(v)) {
      for (const x of v) walk(x, node);
      return;
    }
    const o = v as Record<string, unknown>;
    const here = typeof o.name === 'string' && typeof o.kind === 'string' ? o.name : node;
    if ('when' in o) out.push({ node: here, when: o.when });
    for (const [k, x] of Object.entries(o)) if (k !== 'when') walk(x, here);
  };
  walk(graph, '');
  return out;
}

const GUARD_TABLE: unknown[] = [
  undefined,
  null,
  'text',
  {},
  { kind: 'in', labels: ['accepted'], value: { name: 'review' } },
  { kind: 'in', labels: ['accepted'], value: { name: 'review', field: 'verdict' } },
  { kind: 'in', labels: ['high', 'very_high'], value: { name: 'triage', field: 'risk_level' } },
  { kind: 'in', labels: ['crash', 'malformed', 'refusal', 'timeout'], value: { name: 'w', source: 'error' } },
  { kind: 'in', labels: ['timeout', 'odd_label'], value: { name: 'w', source: 'error' } },
  { kind: 'in', labels: [], value: { name: 'w' } },
  { kind: 'in', labels: ['ok'] },
  {
    kind: 'all',
    guards: [
      { kind: 'in', labels: ['accepted'], value: { name: 'a' } },
      { kind: 'in', labels: ['accepted'], value: { name: 'b' } },
    ],
  },
  {
    kind: 'any',
    guards: [
      { kind: 'in', labels: ['rejected'], value: { name: 'a' } },
      { kind: 'in', labels: ['rejected'], value: { name: 'b' } },
      { kind: 'in', labels: ['rejected'], value: { name: 'c' } },
    ],
  },
  {
    kind: 'all',
    guards: [
      { kind: 'in', labels: ['accepted'], value: { name: 'a' } },
      { kind: 'in', labels: ['rejected'], value: { name: 'b' } },
    ],
  },
  { kind: 'all', guards: [{ kind: 'in', labels: ['accepted'], value: { name: 'a' } }] },
  { kind: 'all', guards: [] },
  { kind: 'not', guard: { kind: 'in', labels: ['accepted'], value: { name: 'a' } } },
  { kind: 'not' },
  {
    kind: 'any',
    guards: [
      { kind: 'not', guard: { kind: 'in', labels: ['x'], value: { name: 'a' } } },
      { kind: 'in', labels: ['y'], value: { name: 'a' } },
    ],
  },
  { kind: 'mystery', value: { name: 'z' } },
];
const LOCALS: [string, Set<string>][] = [
  ['none', new Set()],
  ['a,b', new Set(['a', 'b'])],
  ['a,b,c,review,w', new Set(['a', 'b', 'c', 'review', 'w'])],
];

test('golden: humanId over every node name', () => {
  const names = [...new Set(runs.flatMap(([, v]) => v.graphNodes.map((g) => g.name)))].sort();
  checkGolden('graph-model/humanId', {
    nodes: Object.fromEntries(names.map((n) => [n, humanId(n)])),
    table: ['', 'build#1', 'a#b#2', '__x--y__', ' padded_ ', 'review#', 'x#12y'].map((n) => [n, humanId(n)]),
  });
});

test('golden: guardNames and conditionLabel over every condition', () => {
  const graphs = runs.flatMap(([n, v]) => whens(v.graph).map((w, i) => [`${n}#${i}@${w.node}`, w] as const));
  checkGolden('graph-model/guardNames', {
    graphs: Object.fromEntries(graphs.map(([n, w]) => [n, guardNames(w.when)])),
    table: GUARD_TABLE.map((g) => [g, guardNames(g)]),
  });
  checkGolden('graph-model/conditionLabel', {
    graphs: Object.fromEntries(graphs.map(([n, w]) => [n, [conditionLabel(w.when), conditionLabel(w.when, new Set([w.node]))]])),
    table: GUARD_TABLE.map((g) => [g, Object.fromEntries(LOCALS.map(([k, local]) => [k, conditionLabel(g, local)]))]),
  });
});

test('golden: shortLabel', () => {
  const labels = [...new Set([...models.values()].flatMap((m) => m.edges.map((e) => e.label ?? '')))].sort();
  checkGolden('graph-model/shortLabel', {
    edges: labels.map((l) => [l, shortLabel(l), shortLabel(l, 10)]),
    table: ['', 'x'.repeat(26), 'x'.repeat(27), 'é😀'.repeat(10)].map((l) => [l, shortLabel(l), shortLabel(l, 1), shortLabel(l, 0)]),
  });
});

test('golden: buildFlowModel over every run', () => {
  checkGolden(
    'graph-model/buildFlowModel',
    byRun((_, n) => models.get(n)),
    refs
  );
});

test('golden: buildFlowModel with no graph and with odd graphs', () => {
  const base = runs[0][1];
  const odd: Record<string, unknown> = {
    'no graph': null,
    'empty object': {},
    'root not a node': { root: 'x' },
    'lone step': { root: { kind: 'step', name: 'only' } },
    'lone succeed': { root: { kind: 'succeed', name: 'done' } },
    'empty seq': { root: { kind: 'seq', name: 's', children: [] } },
    'par one branch': { root: { kind: 'par', name: 'p', branches: [{ kind: 'step', name: 'a' }] } },
    'loop with nothing': { root: { kind: 'loop', name: 'l', body: { kind: 'seq', name: 'b', children: [] } } },
    'choice no branches': { root: { kind: 'choice', name: 'c' } },
  };
  checkGolden(
    'graph-model/buildFlowModel-odd',
    Object.fromEntries(
      Object.entries(odd).map(([n, graph]) => [
        n,
        attempt(() => buildFlowModel({ ...base, graph: graph as RunView['graph'], graphNodes: [], nodes: {}, executions: [], terminal: null, failure: null })),
      ])
    ),
    refs
  );
});

test('golden: showEveryModel and commonModel over every run', () => {
  checkGolden(
    'graph-model/showEveryModel',
    byRun((_, n) => [showEveryModel(models.get(n)!.items, false), showEveryModel(models.get(n)!.items, true)])
  );
  checkGolden(
    'graph-model/commonModel',
    byRun((_, n) => commonModel(models.get(n)!.items) ?? null)
  );
});

test('golden: pickReachedFail over every run', () => {
  // The guards map is buildFlowModel's own internal; here: each fail item read by the nodes its
  // id names, by every graph node, or by none.
  checkGolden(
    'graph-model/pickReachedFail',
    byRun((v, n) => {
      const fails = models.get(n)!.items.filter((i) => i.kind === 'fail');
      const all = new Map(fails.map((f) => [f.id, v.graphNodes.map((g) => g.name)]));
      const pick = (list: FlowItem[], guards: Map<string, string[]>) => pickReachedFail(list, v, guards)?.id ?? null;
      const withReason: FlowItem[] = v.graphNodes
        .filter((g) => g.kind === 'fail')
        .map((g) => ({ id: g.name, kind: 'fail', graphKind: 'fail', state: 'idle', visited: false, failReason: g.failReason }));
      return {
        model: pick(fails, new Map()),
        modelAllGuards: pick(fails, all),
        graphFails: pick(withReason, new Map()),
        graphFailsAllGuards: pick(withReason, new Map(withReason.map((f) => [f.id, v.executions.map((e) => e.node)]))),
      };
    })
  );
});

test('golden: itemSize and labelSize', () => {
  checkGolden(
    'graph-model/itemSize',
    byRun((_, n) => Object.fromEntries(models.get(n)!.items.map((i) => [i.id, itemSize(i)])))
  );
  const labels = [...new Set([...models.values()].flatMap((m) => m.edges.map((e) => e.label ?? '')))].sort();
  checkGolden(
    'graph-model/labelSize',
    [...labels, 'x'.repeat(40)].map((l) => [l, labelSize(l)])
  );
});

test('golden: buildElkGraph over every run, both directions', () => {
  checkGolden(
    'graph-model/buildElkGraph',
    byRun((_, n) => ({ RIGHT: buildElkGraph(models.get(n)!), DOWN: buildElkGraph(models.get(n)!, 'DOWN') }))
  );
});

test('golden: readElkLayout of ELK layouts of every run, both directions', async () => {
  const elk = new ELK();
  const out: Record<string, unknown> = {};
  for (const [n] of runs) {
    const model = models.get(n)!;
    out[n] = {
      RIGHT: readElkLayout(await elk.layout(buildElkGraph(model, 'RIGHT'))),
      DOWN: readElkLayout(await elk.layout(buildElkGraph(model, 'DOWN'))),
    };
  }
  const hand = { id: 'root', children: [{ id: 'a', x: 5, y: 6, width: 10, height: 12, children: [{ id: 'b', x: 1, y: 2 }] }, { id: 'c' }] };
  checkGolden('graph-model/readElkLayout', { runs: out, hand: readElkLayout(hand), empty: readElkLayout({ id: 'root' }) });
});

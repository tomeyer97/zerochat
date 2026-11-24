// Goldens for replay-model.ts over every run the API snapshots hold, plus tables of edge inputs.
// The snapshots have no checkpoint listing (the fake Zeroshot answers `checkpoints` with an
// error), so each run gets one checkpoint per execution, derived from its executions (below).
import { apiBodies, apiFiles, apiFile, attempt, checkGolden, jsonChanges, refsOf } from './testdata/golden';
import { test } from 'node:test';
import {
  AGENT_KINDS,
  ancestors,
  applyEdits,
  checkpointIteration,
  editableNodes,
  parseJsonField,
  positiveInt,
  preselectCheckpoint,
  replayVerbs,
  resumeText,
  rollbackText,
  submissionGraph,
  validationLines,
  validationSummary,
  type NodeEdit,
} from './replay-model';
import type { Checkpoint, JsonObject, RunView } from './types';

const runs = apiBodies<RunView>('runs', /(^|\/)run\.json$/).map(([name, view]) => [name.replace(/\/run$/, ''), view] as [string, RunView]);
const byRun = <T>(f: (v: RunView) => T) => Object.fromEntries(runs.map(([n, v]) => [n, f(v)]));

/** A checkpoint after every execution: sequence = execution id, iteration = its round when in a loop. */
function checkpointsOf(v: RunView): Checkpoint[] {
  return v.executions.map((e) => {
    const info = v.graphNodes.find((g) => g.name === e.node);
    return {
      checkpointId: `cp-${e.id}`,
      sequence: e.id,
      node: e.node,
      mapIndices: Array.isArray(e.mapIndices) ? (e.mapIndices as number[]) : [],
      loopIterations: info?.loops.length ? [e.round] : [],
      createdAt: e.endedAt ?? e.startedAt ?? 0,
    };
  });
}

test('golden: AGENT_KINDS', () => {
  checkGolden('replay-model/AGENT_KINDS', AGENT_KINDS);
});

test('golden: ancestors of every graph node', () => {
  checkGolden('replay-model/ancestors', {
    runs: byRun((v) => Object.fromEntries([...v.graphNodes.map((g) => g.name), 'no_such_node'].map((n) => [n, ancestors(n, v.graphNodes)]))),
  });
});

test('golden: checkpointIteration', () => {
  checkGolden(
    'replay-model/checkpointIteration',
    [[], [3], [1, 2], undefined as unknown as number[]].map((loopIterations) => checkpointIteration({ loopIterations }))
  );
});

test('golden: preselectCheckpoint over every run', () => {
  const out = byRun((v) => {
    const cps = checkpointsOf(v);
    const pick = (list: Checkpoint[], view: Pick<RunView, 'failure' | 'graphNodes' | 'executions'>) => preselectCheckpoint(list, view)?.checkpointId ?? null;
    return {
      all: pick(cps, v),
      reversed: pick([...cps].reverse(), v),
      none: pick([], v),
      noFailure: pick(cps, { ...v, failure: null }),
      // Only checkpoints of nodes the failure is not in: falls back to the time of the failure.
      othersOnly: pick(
        cps.filter((c) => c.node !== v.failure?.node),
        v
      ),
      unrelated: pick(
        [
          { checkpointId: 'x-1', sequence: 1, node: 'elsewhere', mapIndices: [], loopIterations: [], createdAt: 0 },
          { checkpointId: 'x-2', sequence: 2, node: 'elsewhere', mapIndices: [], loopIterations: [], createdAt: 9e15 },
        ],
        v
      ),
    };
  });
  checkGolden('replay-model/preselectCheckpoint', out);
});

test('golden: editableNodes over every run', () => {
  const refs = refsOf({ runs: Object.fromEntries(runs) });
  checkGolden(
    'replay-model/editableNodes',
    byRun((v) => Object.fromEntries([undefined, 'no_such_node', ...v.graphNodes.map((g) => g.name)].map((n) => [String(n), editableNodes(n, v.graphNodes)]))),
    refs
  );
});

test('golden: positiveInt', () => {
  const texts = [undefined, '', '  ', '0', '1', ' 7 ', '-3', '1.5', '2.0', '1e3', '0x10', 'abc', '9007199254740993', 'Infinity'];
  checkGolden(
    'replay-model/positiveInt',
    texts.map((t) => [String(t), positiveInt(t)])
  );
});

test('golden: submissionGraph over every run', () => {
  const refs = refsOf({ runs: Object.fromEntries(runs) });
  checkGolden(
    'replay-model/submissionGraph',
    { runs: byRun((v) => submissionGraph(v.graph)), null: submissionGraph(null), extra: submissionGraph({ root: 1, bounds: 2, profile: 3, other: 4 } as JsonObject) },
    refs
  );
});

test('golden: applyEdits over every run', () => {
  const out = byRun((v) => {
    const info = (name: string) => v.graphNodes.find((g) => g.name === name);
    const original = {
      instructions: (n: string) => info(n)?.instructions ?? null,
      model: (n: string) => info(n)?.model ?? null,
      timeoutMs: (n: string) => info(n)?.timeoutMs ?? null,
      maxIterations: (n: string) => info(n)?.maxIterations ?? null,
    };
    const agents = v.graphNodes.filter((g) => AGENT_KINDS.has(g.kind));
    const loop = v.graphNodes.find((g) => g.kind === 'loop');
    const first = agents[0]?.name ?? 'none';
    const cases: Record<string, Record<string, NodeEdit>> = {
      none: {},
      unchanged: Object.fromEntries(agents.map((g) => [g.name, { instructions: g.instructions ?? '', model: g.model ?? '', timeout: g.timeoutMs ? String(g.timeoutMs / 1000) : '' }])),
      instructions: { [first]: { instructions: 'New instructions.' } },
      model: { [first]: { model: '  claude-sonnet-5  ' } },
      blankModel: { [first]: { model: '   ' } },
      timeout: { [first]: { timeout: '90' } },
      badTimeout: { [first]: { timeout: '-5' } },
      rounds: loop ? { [loop.name]: { maxRounds: '7' } } : { [first]: { maxRounds: '7' } },
      unknownNode: { no_such_node: { instructions: 'x', model: 'm' } },
      everything: Object.fromEntries(agents.map((g, i) => [g.name, { instructions: `step ${i}`, model: `model-${i}`, timeout: String(30 + i), maxRounds: '2' }])),
    };
    // The edited graph and runtime are copies of the submitted graph and the runtime: pinned as
    // their changes (`"<unchanged>"` when the part is left out of the request).
    const summary = (r: ReturnType<typeof applyEdits>) => ({
      graph: r.graph === undefined ? '<unchanged>' : jsonChanges(submissionGraph(v.graph) ?? {}, r.graph),
      runtime: r.runtime === undefined ? '<unchanged>' : jsonChanges(v.runtime ?? {}, r.runtime),
      inputUntouched: JSON.stringify(v.graph) === graphBefore && JSON.stringify(v.runtime) === runtimeBefore,
    });
    const graphBefore = JSON.stringify(v.graph);
    const runtimeBefore = JSON.stringify(v.runtime);
    return Object.fromEntries(Object.entries(cases).map(([k, edits]) => [k, attempt(() => summary(applyEdits(v.graph, v.runtime, edits, original)))]));
  });
  const minimal = { instructions: () => null, model: () => null };
  checkGolden('replay-model/applyEdits', {
    runs: out,
    noGraph: applyEdits(null, null, { a: { instructions: 'x', model: 'm', timeout: '5', maxRounds: '2' } }, minimal),
  });
});

test('golden: parseJsonField', () => {
  const table: [string, unknown][] = [
    ['', { a: 1 }],
    ['   ', null],
    ['{"a":1}', { a: 1 }],
    ['{ "a" : 1 }', { a: 1 }],
    ['{"b":1,"a":2}', { a: 2, b: 1 }],
    ['[1,2]', [1, 2]],
    ['null', null],
    ['null', undefined],
    ['{bad', null],
    ['{"a":1', { a: 1 }],
    ['1 2', null],
  ];
  checkGolden('replay-model/parseJsonField', {
    table: table.map(([text, original]) => [text, parseJsonField(text, original, 'Runtime')]),
    runs: byRun((v) => ({
      runtimeUnchanged: parseJsonField(JSON.stringify(v.runtime, null, 2) ?? '', v.runtime, 'Runtime'),
      inputUnchanged: parseJsonField(JSON.stringify(v.initialInput) ?? '', v.initialInput, 'Initial input'),
      inputChanged: parseJsonField('{"changed":true}', v.initialInput, 'Initial input'),
    })),
  });
});

test('golden: validationSummary and validationLines over the replay responses and a table', () => {
  const posts = apiFiles('post').map((rel) => [rel, apiFile(rel).body] as const);
  const table: unknown[] = [
    null,
    'text',
    42,
    {},
    { valid: true, replayOf: { reusedExecutions: 1, changedNodes: [] } },
    { valid: true, replayOf: { reusedExecutions: 0, changedNodes: ['worker', 'review'] }, completedNodes: ['plan', 'worker'] },
    { valid: true, replayOf: { reusedExecutions: 3 }, completedNodes: 'not a list' },
    { valid: true, replayOf: { changedNodes: ['x'] } },
    { valid: false, message: 'no such checkpoint' },
    { valid: true, message: 'ignored' },
    { errors: ['plain', { path: 'root.children[0]', message: 'bad kind' }, { node: 'worker', field: 'model', message: 'unknown model' }, { code: 7 }, 5, null] },
    { issues: [{ message: 'from issues', path: '' }] },
    { problems: 'not an array', message: 'm', valid: false },
    { conflict: { node: 'worker', message: 'already running' } },
    { conflict: 'conflicting run' },
    { errors: [], valid: false, message: 'm' },
  ];
  for (const [name, f] of [
    ['validationSummary', validationSummary],
    ['validationLines', validationLines],
  ] as const) {
    checkGolden(`replay-model/${name}`, { post: Object.fromEntries(posts.map(([rel, body]) => [rel, f(body)])), table: table.map((t) => [t, f(t)]) });
  }
});

// F01/F02/F13: the words of each operation, for stock Zeroshot (start again unchanged) and for a
// Zeroshot that replays with changes: button, title, confirm, notice and lineage use one verb.
test('golden: verbs', () => {
  const both = (canEdit: boolean) => ({
    plain: replayVerbs(canEdit),
    node: replayVerbs(canEdit, 'acceptance', 'In parallel (round 2)'),
    resume: resumeText(canEdit),
  });
  checkGolden('replay-model/verbs', { stock: both(false), editable: both(true), rollback: rollbackText('In parallel (round 2)') });
});

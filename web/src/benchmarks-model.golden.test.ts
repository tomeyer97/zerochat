// Goldens for benchmarks-model.ts over every benchmark response the API snapshots hold
// (`api/benchmarks/**`: batches, each batch, compares, scores, harness previews, slots and
// start options), plus tables of edge inputs.
import { FIXED_NOW, apiBodies, apiFile, checkGolden, refsOf } from './testdata/golden';
import { test } from 'node:test';
import {
  DEFAULT_MAX_CONCURRENT_RUNS,
  START_FIELD_LABEL,
  START_PATIENCE_MS,
  aggregateByKey,
  aggregateText,
  batchIsLive,
  batchMaxSpend,
  batchStartPhase,
  cellKey,
  cellMap,
  cellStateTone,
  cellTitle,
  concurrencyNote,
  cutShortBadge,
  defaultHarnessOptions,
  defaultParallel,
  deltaTone,
  detailIsLive,
  harnessSegments,
  liveNodeText,
  maxSpend,
  nodeModelText,
  noiseText,
  noiseTone,
  noteStarted,
  replayModelText,
  replaySegmentText,
  resolveTasks,
  retriedText,
  scoreColor,
  scoreText,
  seedHeader,
  setsIncluding,
  signedDuration,
  signedPp,
  signedUsd,
  startedRecently,
  stateLabel,
  taskAggText,
  tasksArg,
  totalsVerdict,
  validateStart,
  type StartFormValues,
} from './benchmarks-model';
import type { BatchCompare, BatchDetail, BatchSummary, BenchmarkCell, HarnessOptions, HarnessPreview, NoiseVerdict, RunScore, RunSlots, TaskSet } from './types';

const summaries = apiFile<{ batches: BatchSummary[] }>('benchmarks/batches.json').body.batches;
const batches = apiBodies<BatchDetail>('benchmarks/batch');
const compares = apiBodies<BatchCompare>('benchmarks/compare');
const scores = apiBodies<RunScore>('benchmarks/score');
const previews = apiBodies<HarnessPreview>('benchmarks/harness-preview');
const cells: [string, BenchmarkCell, BatchDetail][] = batches.flatMap(([n, b]) => b.cells.map((c) => [`${n}/${c.task}#${c.seed}`, c, b] as [string, BenchmarkCell, BatchDetail]));
const byCell = <T>(f: (c: BenchmarkCell, b: BatchDetail) => T) => Object.fromEntries(cells.map(([n, c, b]) => [n, f(c, b)]));
const byBatch = <T>(f: (b: BatchDetail) => T) => Object.fromEntries(batches.map(([n, b]) => [n, f(b)]));
const bySummary = <T>(f: (b: BatchSummary) => T) => Object.fromEntries(summaries.map((b) => [b.id, f(b)]));
/** The start-options snapshot is an error (no pb in the snapshot's sandbox): its fallback, with the batches' own task lists as task sets. */
const startOptions = apiFile<HarnessOptions>('benchmarks/start-options.json');
const options: HarnessOptions = startOptions.status === 200 ? startOptions.body : defaultHarnessOptions();
const taskSets: TaskSet[] = [...options.taskSets, ...batches.map(([, b]) => ({ name: b.label ?? b.id, tasks: b.tasks }))];
const slotsResponse = apiFile<RunSlots>('benchmarks/slots.json');

const STATES = ['queued', 'not_started', 'running_agent', 'evaluating', 'scored', 'interrupted', 'timeout', 'over_budget', 'over_steps', 'crashed', 'failed', 'brand_new'];
const NOISE: NoiseVerdict[] = ['within_noise', 'beyond_spread', 'needs_seeds', 'n_a', 'novel'];
const NUMS = [null, undefined, -1.234, -0.5, -0.0001, 0, 0.0001, 0.004, 0.005, 0.5, 0.715, 1, 1.5, 59.9, 3600, 12345.678];
const label = (v: unknown) => (v === undefined ? 'undefined' : JSON.stringify(v));

/** A cell with every field set to "nothing", for the tables. */
const blank: BenchmarkCell = {
  task: 't',
  seed: 0,
  runId: 'r',
  zeroshotRunId: null,
  state: 'scored',
  exitReason: null,
  overLimit: null,
  score: null,
  testsPassed: null,
  testsTotal: null,
  regressionPassed: null,
  regressionTotal: null,
  costUsd: null,
  capUsd: null,
  wallS: null,
  attempts: 1,
  priorAttempts: [],
};
const CELL_TABLE: [string, BenchmarkCell][] = [
  ['blank', blank],
  ['queued', { ...blank, state: 'queued' }],
  ['not started', { ...blank, state: 'not_started' }],
  ['running, nothing known', { ...blank, state: 'running_agent' }],
  [
    'running, live',
    { ...blank, state: 'running_agent', startedAt: FIXED_NOW - 125_000, liveNodes: ['coverage', 'bugfinder'], liveCostUsd: 0.4321, liveCostUnpriced: ['gpt-x'], capUsd: 5, zeroshotRunId: 'z' },
  ],
  ['running, agent done', { ...blank, state: 'running_agent', liveNodes: [], liveCostUsd: 0 }],
  ['failed with reason', { ...blank, state: 'failed', exitReason: 'exit 2' }],
  ['crashed', { ...blank, state: 'crashed' }],
  ['timeout cap', { ...blank, cutShort: 'timeout', capMinutes: 45 }],
  ['timeout fractional cap', { ...blank, cutShort: 'timeout', capMinutes: 2.5 }],
  ['timeout no cap', { ...blank, overLimit: 'timeout' }],
  ['budget cap', { ...blank, overLimit: 'budget', capUsd: 1.5, costUsd: 1.62 }],
  ['budget no cap', { ...blank, cutShort: 'budget' }],
  ['steps cap', { ...blank, overLimit: 'steps', capSteps: 12 }],
  ['steps no cap', { ...blank, cutShort: 'steps' }],
  ['interrupted', { ...blank, cutShort: 'interrupted' }],
  ['crashed cut short', { ...blank, cutShort: 'crashed' }],
  ['odd cut short', { ...blank, cutShort: 'something' }],
  ['odd over limit', { ...blank, overLimit: 'memory' }],
  ['full scored', { ...blank, score: 0.8125, testsPassed: 13, testsTotal: 16, regressionPassed: null, regressionTotal: 4, costUsd: 0.004, capUsd: 5, wallS: 754.2 }],
  [
    'retried',
    {
      ...blank,
      attempts: 3,
      priorAttempts: [
        { runId: 'a', zeroshotRunId: null, state: 'interrupted', exitReason: null, costUsd: 1, wallS: 2 },
        { runId: 'b', zeroshotRunId: null, state: 'crashed', exitReason: 'oom', costUsd: null, wallS: null },
      ],
    },
  ],
];

test('golden: stateLabel and cellStateTone', () => {
  checkGolden('benchmarks-model/stateLabel', {
    table: STATES.map((s) => [s, stateLabel(s), stateLabel(s, 'why'), stateLabel(s, '')]),
    cells: byCell((c) => stateLabel(c.state, c.exitReason)),
    scores: Object.fromEntries(scores.map(([n, s]) => [n, stateLabel(s.state, s.exitReason)])),
  });
  checkGolden('benchmarks-model/cellStateTone', {
    table: Object.fromEntries(STATES.map((s) => [s, cellStateTone(s)])),
    cells: byCell((c) => cellStateTone(c.state)),
    scores: Object.fromEntries(scores.map(([n, s]) => [n, cellStateTone(s.state)])),
  });
});

test('golden: cutShortBadge and cellTitle', () => {
  checkGolden('benchmarks-model/cutShortBadge', {
    table: CELL_TABLE.map(([n, c]) => [n, cutShortBadge(c)]),
    cells: byCell(cutShortBadge),
    scores: Object.fromEntries(scores.map(([n, s]) => [n, cutShortBadge(s)])),
  });
  checkGolden('benchmarks-model/cellTitle', {
    table: CELL_TABLE.map(([n, c]) => [n, cellTitle(c), cellTitle(c, FIXED_NOW)]),
    cells: byCell((c) => [cellTitle(c), cellTitle(c, FIXED_NOW)]),
  });
});

test('golden: scoreColor, scoreText, aggregateText', () => {
  const numbers = NUMS.filter((n): n is number => typeof n === 'number');
  checkGolden('benchmarks-model/scoreColor', { table: [...numbers, NaN].map((s) => [label(s), scoreColor(s)]), cells: byCell((c) => (c.score == null ? null : scoreColor(c.score))) });
  checkGolden('benchmarks-model/scoreText', {
    table: NUMS.map((s) => [label(s), scoreText(s)]),
    cells: byCell((c) => scoreText(c.score)),
    scores: Object.fromEntries(scores.map(([n, s]) => [n, [scoreText(s.score), s.steps.map((st) => scoreText(st.score))]])),
  });
  checkGolden('benchmarks-model/aggregateText', {
    batches: byBatch((b) => ({ rows: b.rowAggregates.map((a) => [a.key, aggregateText(a)]), cols: b.colAggregates.map((a) => [a.key, aggregateText(a)]) })),
    table: [
      undefined,
      { key: 'k', mean: null, min: null, max: null, n: 0 },
      { key: 'k', mean: 0.5, min: null, max: null, n: 0 },
      { key: 'k', mean: 0.5, min: 0.5, max: 0.504, n: 2 },
      { key: 'k', mean: 0.5, min: 0.5, max: 0.505, n: 2 },
      { key: 'k', mean: 0.71, min: 0.58, max: 0.84, n: 3 },
      { key: 'k', mean: 0.71, min: null, max: 0.84, n: 3 },
    ].map((a) => aggregateText(a)),
  });
});

test('golden: seedHeader over every batch', () => {
  checkGolden('benchmarks-model/seedHeader', {
    batches: byBatch((b) => b.seeds.map((s) => seedHeader(s, !!b.replays?.length, b.seeds.length > 8))),
    table: [0, 1, 9].flatMap((s) => [false, true].flatMap((r) => [false, true].map((m) => [s, r, m, seedHeader(s, r, m)]))),
  });
});

test('golden: replaySegmentText and replayModelText over every replay', () => {
  const replays = batches.flatMap(([n, b]) => (b.replays ?? []).map((r, i) => [`${n}#${i}`, r] as const));
  const table = [
    { node: 'build#1', through: null, models: [], recordedModel: null },
    { node: 'build#1', through: 'build#1', models: [], recordedModel: 'claude-haiku-4-5' },
    { node: 'triage#1', through: 'repair#1', models: ['claude-sonnet-5'], recordedModel: 'claude-sonnet-5' },
    { node: 'triage#1', through: 'repair#1', models: ['claude-sonnet-5'], recordedModel: 'claude-haiku-4-5' },
    { node: 'x', through: '', models: ['claude-sonnet-5', 'gpt-5'], recordedModel: 'claude-sonnet-5' },
    { node: 'x', through: null, models: ['gpt-5'], recordedModel: null },
  ];
  checkGolden('benchmarks-model/replaySegmentText', { replays: Object.fromEntries(replays.map(([n, r]) => [n, replaySegmentText(r)])), table: table.map(replaySegmentText) });
  checkGolden('benchmarks-model/replayModelText', { replays: Object.fromEntries(replays.map(([n, r]) => [n, replayModelText(r)])), table: table.map(replayModelText) });
});

test('golden: liveNodeText, cellKey, cellMap, aggregateByKey', () => {
  checkGolden('benchmarks-model/liveNodeText', { table: [undefined, [], ['build'], ['coverage', 'bugfinder']].map((n) => liveNodeText(n)), cells: byCell((c) => liveNodeText(c.liveNodes)) });
  checkGolden('benchmarks-model/cellKey', { table: [cellKey('t', 0), cellKey('', -1), cellKey('a\u0000b', 2)], cells: byCell((c) => cellKey(c.task, c.seed)) });
  const inputs = Object.fromEntries(batches);
  const refs = refsOf({ batches: inputs });
  checkGolden('benchmarks-model/cellMap', { batches: byBatch((b) => cellMap(b.cells)), duplicates: cellMap([blank, { ...blank, runId: 'second' }]) }, refs);
  checkGolden(
    'benchmarks-model/aggregateByKey',
    byBatch((b) => ({ rows: aggregateByKey(b.rowAggregates), cols: aggregateByKey(b.colAggregates) })),
    refs
  );
});

test('golden: batch summaries (batchIsLive, retriedText, batchMaxSpend)', () => {
  checkGolden('benchmarks-model/batchIsLive', {
    batches: bySummary(batchIsLive),
    table: [
      ['running', 0],
      ['done', 0],
      ['done', 1],
      ['stopped', -1],
    ].map(([status, n]) => batchIsLive({ status: status as string, runsInFlight: n as number })),
  });
  checkGolden('benchmarks-model/retriedText', {
    batches: bySummary(retriedText),
    details: byBatch((b) => retriedText({ retriedAttempts: b.retriedAttempts ?? 0, retriedCostUsd: b.retriedCostUsd ?? null })),
    table: [
      [0, 5],
      [1, null],
      [1, 0.5],
      [3, 3.5],
      [-1, 1],
    ].map(([n, c]) => retriedText({ retriedAttempts: n as number, retriedCostUsd: c })),
  });
  checkGolden('benchmarks-model/batchMaxSpend', {
    details: byBatch((b) => batchMaxSpend(b.limits?.maxCostUsd, b.cells.length, b.retriedAttempts ?? 0)),
    table: [
      [null, 3, 0],
      [undefined, 3, 0],
      [5, 0, 2],
      [5, -1, 0],
      [5, 1, 3],
      [5, 2, -4],
      [0, 2, 0],
    ].map(([cap, planned, retried]) => batchMaxSpend(cap, planned as number, retried as number)),
  });
});

test('golden: detailIsLive and batchStartPhase over every batch', () => {
  const missing = apiFile('benchmarks/batch/missing.json');
  checkGolden('benchmarks-model/detailIsLive', {
    batches: byBatch(detailIsLive),
    table: [
      { status: 'starting', cells: [] },
      { status: 'done', cells: [{ state: 'evaluating' }] },
      { status: 'done', cells: [{ state: 'scored' }] },
    ].map(detailIsLive),
  });
  // The tab's memory of batches it started: in-memory (no sessionStorage under node).
  noteStarted('started-here', FIXED_NOW - 60_000);
  noteStarted('started-long-ago', FIXED_NOW - START_PATIENCE_MS);
  noteStarted('started-now');
  checkGolden('benchmarks-model/batchStartPhase', {
    batches: byBatch((b) => batchStartPhase(b, undefined, b.id, FIXED_NOW)),
    missing: ['started-here', 'started-long-ago', 'started-now', 'never'].map((id) => [
      id,
      missing.status,
      batchStartPhase(undefined, missing.status, id, FIXED_NOW),
      batchStartPhase(undefined, 500, id, FIXED_NOW),
      batchStartPhase(undefined, missing.status, id),
    ]),
    table: [
      batchStartPhase({ status: 'failed_to_start' }, 404, 'started-here', FIXED_NOW),
      batchStartPhase({ status: 'starting' }, undefined, 'x', FIXED_NOW),
      batchStartPhase({ status: 'running', starting: true }, undefined, 'x', FIXED_NOW),
    ],
  });
  checkGolden(
    'benchmarks-model/startedRecently',
    ['started-here', 'started-long-ago', 'started-now', 'never'].map((id) => [id, startedRecently(id, FIXED_NOW), startedRecently(id), startedRecently(id, FIXED_NOW + START_PATIENCE_MS)])
  );
});

test('golden: compare helpers over every compare', () => {
  checkGolden('benchmarks-model/noiseText', Object.fromEntries(NOISE.map((n) => [n, noiseText(n)])));
  checkGolden('benchmarks-model/noiseTone', Object.fromEntries(NOISE.map((n) => [n, noiseTone(n)])));
  checkGolden('benchmarks-model/deltaTone', {
    table: NOISE.map((n) => [n, NUMS.map((d) => deltaTone(n, d))]),
    compares: Object.fromEntries(compares.map(([n, c]) => [n, c.tasks.map((t) => deltaTone(t.noise, t.deltaScore))])),
  });
  const row = (noise: NoiseVerdict, deltaScore: number | null) => ({ noise, deltaScore });
  const verdictTable: Record<string, { noise: NoiseVerdict; deltaScore: number | null }[]> = {
    none: [],
    'only n/a': [row('n_a', null)],
    'one up': [row('beyond_spread', 0.2)],
    'one down': [row('beyond_spread', -0.2)],
    'one within': [row('within_noise', 0.01)],
    'one needs seeds': [row('needs_seeds', 0.3)],
    'beyond, zero delta': [row('beyond_spread', 0)],
    'both up': [row('beyond_spread', 0.2), row('beyond_spread', 0.1)],
    'up and down': [row('beyond_spread', 0.2), row('beyond_spread', -0.1)],
    'three up': [row('beyond_spread', 0.2), row('beyond_spread', 0.1), row('beyond_spread', 0.3)],
    'one of three up': [row('beyond_spread', 0.2), row('within_noise', 0.1), row('within_noise', -0.3)],
    'up, within, needs': [row('beyond_spread', 0.2), row('within_noise', 0.1), row('needs_seeds', null), row('n_a', null)],
    'within and needs': [row('within_noise', 0.2), row('needs_seeds', 0.1)],
    'all within': [row('within_noise', 0.2), row('within_noise', 0.1)],
    'all need seeds': [row('needs_seeds', 0.2), row('needs_seeds', 0.1), row('needs_seeds', 0)],
    'two need seeds of three': [row('needs_seeds', 0.2), row('needs_seeds', 0.1), row('within_noise', 0)],
    'odd verdict': [row('novel', 0.2)],
  };
  checkGolden('benchmarks-model/totalsVerdict', {
    compares: Object.fromEntries(compares.map(([n, c]) => [n, totalsVerdict(c.tasks)])),
    table: Object.fromEntries(Object.entries(verdictTable).map(([n, t]) => [n, totalsVerdict(t)])),
  });
  checkGolden('benchmarks-model/signed', {
    table: NUMS.map((v) => [label(v), signedPp(v), signedPp(v, 0), signedUsd(v), signedDuration(v)]),
    compares: Object.fromEntries(
      compares.map(([n, c]) => [
        n,
        {
          tasks: c.tasks.map((t) => [signedPp(t.deltaScore), signedUsd(t.deltaCostUsd), signedDuration(t.deltaTimeS)]),
          totals: c.totals ? [signedPp(c.totals.deltaScore), signedUsd(c.totals.deltaCostUsd), signedDuration(c.totals.deltaTimeS)] : null,
          nodes: c.nodes.map((nd) => [nd.node, signedUsd(nd.deltaMeanCostUsd), signedDuration(nd.deltaMeanTimeS)]),
        },
      ])
    ),
  });
  checkGolden('benchmarks-model/taskAggText', {
    compares: Object.fromEntries(compares.map(([n, c]) => [n, c.tasks.map((t) => [taskAggText(t.a), taskAggText(t.b)])])),
    table: [null, { n: 0, meanScore: 0, minScore: 0, maxScore: 0 }, { n: 3, meanScore: 0.71, minScore: 0.58, maxScore: 0.84 }, { n: 2, meanScore: 0.5, minScore: 0.5, maxScore: 0.504 }].map(
      taskAggText
    ),
  });
});

test('golden: start-form helpers (resolveTasks, tasksArg, maxSpend, setsIncluding)', () => {
  const allTasks = [...new Set(taskSets.flatMap((s) => s.tasks))].sort();
  const firstTwo = taskSets.slice(0, 2).map((s) => s.name);
  const selections: [string, string[], string[]][] = [
    ['nothing', [], []],
    ['one set', firstTwo.slice(0, 1), []],
    ['two sets', firstTwo, []],
    ['sets and ids', firstTwo, [allTasks[0] ?? 't', 'unknown-task', allTasks[0] ?? 't']],
    ['unknown set name', ['no-such-set'], []],
    ['every set', taskSets.map((s) => s.name), allTasks],
  ];
  checkGolden('benchmarks-model/resolveTasks', { taskSets, out: selections.map(([n, s, t]) => [n, resolveTasks(s, t, taskSets)]) });
  checkGolden(
    'benchmarks-model/tasksArg',
    selections.map(([n, s, t]) => [n, tasksArg(s, t)])
  );
  checkGolden(
    'benchmarks-model/maxSpend',
    [
      [0, 1, 5],
      [2, 0, 5],
      [2, 3, null],
      [2, 3, undefined],
      [2, 3, 0],
      [2, 3, -1],
      [2, 3, 5],
      [1, 1, 0.01],
      [options.tasks.length, 1, options.defaultMaxCostUsd],
    ].map(([t, s, c]) => maxSpend(t as number, s as number, c))
  );
  checkGolden(
    'benchmarks-model/setsIncluding',
    Object.fromEntries(
      allTasks.map((t) => [
        t,
        [
          setsIncluding(
            t,
            taskSets.map((s) => s.name),
            taskSets
          ),
          setsIncluding(t, firstTwo, taskSets),
          setsIncluding(t, [], taskSets),
        ],
      ])
    )
  );
});

test('golden: validateStart', () => {
  const ok: StartFormValues = {
    taskCount: 2,
    label: 'run',
    seeds: 1,
    parallel: '',
    harnessKind: 'default',
    variant: '',
    customGraph: '',
    customRuntime: '',
    maxCost: '',
    maxMinutes: '',
    maxSteps: '',
  };
  const cases: Record<string, StartFormValues> = {
    ok,
    'everything wrong': { ...ok, taskCount: 0, label: '  ', seeds: 0, parallel: '1.5', harnessKind: 'variant', maxCost: '-1', maxMinutes: 'abc', maxSteps: '2.5' },
    'fractional seeds': { ...ok, seeds: 1.5 },
    'NaN seeds': { ...ok, seeds: NaN },
    'parallel 0': { ...ok, parallel: '0' },
    'parallel padded': { ...ok, parallel: ' 3 ' },
    'parallel hex': { ...ok, parallel: '0x10' },
    'variant chosen': { ...ok, harnessKind: 'variant', variant: 'v1' },
    'custom missing runtime': { ...ok, harnessKind: 'custom', customGraph: 'g.json', customRuntime: ' ' },
    'custom ok': { ...ok, harnessKind: 'custom', customGraph: 'g.json', customRuntime: 'r.json' },
    'harness errors': { ...ok, harnessErrors: ['graph: File not found.', 'runtime: bad.'] },
    'harness errors and variant missing': { ...ok, harnessKind: 'variant', harnessErrors: ['x'] },
    'empty harness errors': { ...ok, harnessErrors: [] },
    'cost fractional': { ...ok, maxCost: '0.5', maxMinutes: '1e2', maxSteps: '12' },
    'cost zero': { ...ok, maxCost: '0', maxMinutes: '0', maxSteps: '0' },
    'cost Infinity': { ...ok, maxCost: 'Infinity', maxMinutes: '  ', maxSteps: '1e1' },
  };
  checkGolden('benchmarks-model/validateStart', Object.fromEntries(Object.entries(cases).map(([n, v]) => [n, validateStart(v)])));
});

test('golden: harnessSegments and nodeModelText over every harness', () => {
  const harnesses = [
    ...batches.flatMap(([n, b]) => (b.harnesses ?? []).map((h, i) => [`batch/${n}#${i}`, h] as const)),
    ...previews.flatMap(([n, p]) => (p.info ? [[`preview/${n}`, p.info] as const] : [])),
  ];
  const refs = refsOf({ harnesses: Object.fromEntries(harnesses) });
  checkGolden('benchmarks-model/harnessSegments', Object.fromEntries(harnesses.map(([n, h]) => [n, harnessSegments(h.nodes)])), refs);
  checkGolden('benchmarks-model/nodeModelText', {
    harnesses: Object.fromEntries(harnesses.map(([n, h]) => [n, h.nodes.map(nodeModelText)])),
    table: [
      { model: null, effort: null },
      { model: null, effort: 'high' },
      { model: 'claude-sonnet-5', effort: null },
      { model: 'claude-sonnet-5', effort: 'high' },
      { model: 'gpt-claude-x', effort: '' },
    ].map(nodeModelText),
  });
});

test('golden: defaultParallel, concurrencyNote, defaults and constants', () => {
  checkGolden(
    'benchmarks-model/defaultParallel',
    [undefined, null, 0, -2, 1, 2, 4, 8, 2.5].map((m) => [label(m), defaultParallel(m)])
  );
  const running = summaries.filter((b) => batchIsLive(b));
  const slots: (RunSlots | null)[] = [
    slotsResponse.status === 200 ? slotsResponse.body : null,
    null,
    { maxConcurrentRuns: null, running: 0, queued: 0 },
    { maxConcurrentRuns: 4, running: 4, queued: 1 },
    { maxConcurrentRuns: 2, running: 1, queued: 3 },
    { maxConcurrentRuns: null, running: 0, queued: 2 },
  ];
  checkGolden('benchmarks-model/concurrencyNote', {
    snapshotSlots: { status: slotsResponse.status },
    out: [[], running.slice(0, 1), running].flatMap((r) => slots.flatMap((s) => [undefined, null, 6].map((m) => [r.map((b) => b.id), s, label(m), concurrencyNote(r, s, m)]))),
  });
  checkGolden('benchmarks-model/defaultHarnessOptions', { defaultHarnessOptions: defaultHarnessOptions(), startOptionsStatus: startOptions.status });
  checkGolden('benchmarks-model/constants', { START_PATIENCE_MS, DEFAULT_MAX_CONCURRENT_RUNS, START_FIELD_LABEL });
});

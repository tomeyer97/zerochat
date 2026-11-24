import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  aggregateByKey,
  aggregateText,
  batchIsLive,
  batchStartPhase,
  batchMaxSpend,
  retriedText,
  noteStarted,
  START_PATIENCE_MS,
  cellKey,
  cellMap,
  cellStateTone,
  cellTitle,
  concurrencyNote,
  cutShortBadge,
  defaultHarnessOptions,
  defaultParallel,
  harnessSegments,
  liveNodeText,
  replayModelText,
  replaySegmentText,
  seedHeader,
  nodeModelText,
  setsIncluding,
  validateStart,
  type StartFormValues,
  deltaTone,
  detailIsLive,
  maxSpend,
  noiseText,
  noiseTone,
  resolveTasks,
  scoreColor,
  scoreText,
  signedDuration,
  signedPp,
  signedUsd,
  stateLabel,
  taskAggText,
  tasksArg,
  totalsVerdict,
} from './benchmarks-model';
import type { BenchmarkAggregate, BenchmarkCell, TaskSet } from './types';

const cell = (over: Partial<BenchmarkCell> = {}): BenchmarkCell => ({
  task: 'gron',
  seed: 1,
  runId: 'r1',
  zeroshotRunId: 'zs-1',
  state: 'scored',
  exitReason: null,
  overLimit: null,
  score: 0.85,
  testsPassed: 8,
  testsTotal: 10,
  regressionPassed: null,
  regressionTotal: null,
  costUsd: 0.42,
  capUsd: 5,
  wallS: 120,
  attempts: 1,
  priorAttempts: [],
  ...over,
});

test('state labels are one word, failed/crashed carry their reason, a stop and a limit are not failures', () => {
  assert.equal(stateLabel('running_agent'), 'running agent');
  assert.equal(stateLabel('evaluating'), 'evaluating');
  assert.equal(stateLabel('scored'), 'scored');
  assert.equal(stateLabel('failed', 'timeout'), 'failed: timeout');
  assert.equal(stateLabel('failed', null), 'failed');
  assert.equal(stateLabel('interrupted', 'stopped by pb stop'), 'interrupted');
  assert.equal(stateLabel('timeout', 'wall-time limit 45 min'), 'over time limit');
  assert.equal(stateLabel('over_budget', 'cost limit $6.00'), 'over budget');
  assert.equal(stateLabel('crashed', 'pb process died'), 'crashed: pb process died');
  assert.equal(stateLabel('queued'), 'queued');
  assert.equal(stateLabel('not_started'), 'not started');
  assert.equal(stateLabel('over_steps', 'step_limit after 900s'), 'over step limit');
});

test('cell tone: red only for failed/crashed, amber for a limit, grey for a stop, blue for running, nothing for scored/evaluating', () => {
  assert.equal(cellStateTone('failed'), 'bad');
  assert.equal(cellStateTone('crashed'), 'bad');
  assert.equal(cellStateTone('timeout'), 'warn');
  assert.equal(cellStateTone('over_budget'), 'warn');
  assert.equal(cellStateTone('interrupted'), 'muted');
  assert.equal(cellStateTone('running_agent'), 'running');
  assert.equal(cellStateTone('scored'), null);
  assert.equal(cellStateTone('evaluating'), null);
  assert.equal(cellStateTone('over_steps'), 'warn');
  assert.equal(cellStateTone('queued'), 'muted');
  assert.equal(cellStateTone('not_started'), 'muted');
});

test('a cut-short cell gets a visible marker naming the limit it hit, and says it is in the mean', () => {
  const caps = { capUsd: 6, capMinutes: 45, capSteps: 24 };
  assert.deepEqual(cutShortBadge({ ...caps, cutShort: 'budget', overLimit: 'budget' })?.short, '$6 cap');
  assert.match(cutShortBadge({ ...caps, cutShort: 'budget', overLimit: 'budget' })!.long, /\$6\.00 budget cap, scored on the code it had; counted in the mean/);
  assert.equal(cutShortBadge({ ...caps, cutShort: 'timeout', overLimit: 'timeout' })?.short, '⏱ 45m');
  assert.equal(cutShortBadge({ ...caps, cutShort: 'steps', overLimit: 'steps' })?.short, '24 steps');
  assert.equal(cutShortBadge({ ...caps, cutShort: 'interrupted', overLimit: null })?.short, 'stopped');
  assert.equal(cutShortBadge({ ...caps, cutShort: 'crashed', overLimit: null })?.short, 'crashed');
  assert.equal(cutShortBadge({ ...caps, cutShort: null, overLimit: null }), null);
  // An older server without `cutShort` still marks its over-limit cells.
  assert.equal(cutShortBadge({ capUsd: 2.5, overLimit: 'budget' })?.short, '$2.5 cap');
  assert.equal(cutShortBadge({ capUsd: null, overLimit: 'timeout' })?.short, '⏱ time');
});

test('queued, not-started and running cells explain themselves in the tooltip', () => {
  assert.match(cellTitle(cell({ state: 'queued', runId: '', score: null })), /^queued: pb starts it/);
  assert.match(cellTitle(cell({ state: 'not_started', runId: '', score: null })), /^not started/);
  const running = cellTitle(cell({ state: 'running_agent', score: null, startedAt: 1_000_000, testsTotal: null, costUsd: null, wallS: null }), 1_000_000 + 125_000);
  assert.match(running, /running for 2m 05s/);
  assert.match(running, /click to watch it live/);
  assert.match(cellTitle(cell({ state: 'running_agent', zeroshotRunId: null, score: null })), /Zeroshot run has not started yet/);
});

test('a running cell names its current node and its live, estimated spend', () => {
  assert.equal(liveNodeText(['build']), 'in build');
  assert.equal(liveNodeText(['coverage', 'bugfinder']), 'in coverage ‖ bugfinder');
  assert.equal(liveNodeText([]), null);
  assert.equal(liveNodeText(undefined), null);
  const title = cellTitle(cell({ state: 'running_agent', score: null, costUsd: null, capUsd: 5, liveNodes: ['build'], liveCostUsd: 1.234, liveCostUnpriced: ['gpt-x'] }));
  assert.match(title, /in build/);
  assert.match(title, /spent so far ~\$1\.23 of \$5\.00 cap: live estimate/);
  assert.match(title, /not counting gpt-x/);
  assert.match(cellTitle(cell({ state: 'running_agent', score: null, liveNodes: [] })), /agent finished; pb is wrapping up/);
});

test('score formatting', () => {
  assert.equal(scoreText(0.8457), '0.85');
  assert.equal(scoreText(null), '—');
  assert.equal(scoreText(undefined), '—');
});

test('score colour is a single hue scaled by score, never a different hue for a low score', () => {
  const low = scoreColor(0);
  const high = scoreColor(1);
  assert.match(low, /^color-mix\(in srgb, var\(--accent-text\)/);
  assert.match(high, /^color-mix\(in srgb, var\(--accent-text\)/);
  assert.notEqual(low, high);
  // Out-of-range inputs are clamped, not reflected as a negative or >100% mix.
  assert.doesNotMatch(scoreColor(-1), /-\d+%/);
  assert.doesNotMatch(scoreColor(5), /1\d\d%/);
});

test('aggregate text shows the spread only when scores actually differ', () => {
  const spread: BenchmarkAggregate = { key: 'gron', mean: 0.71, min: 0.58, max: 0.84, n: 2 };
  assert.equal(aggregateText(spread), '0.71 (0.58–0.84)');
  const flat: BenchmarkAggregate = { key: 'gron', mean: 0.85, min: 0.85, max: 0.85, n: 2 };
  assert.equal(aggregateText(flat), '0.85');
  assert.equal(aggregateText(undefined), '—');
  assert.equal(aggregateText({ key: 'gron', mean: null, min: null, max: null, n: 0 }), '—');
});

test('cell tooltip lists every fact the matrix cell itself has no room for', () => {
  const title = cellTitle(cell());
  assert.match(title, /^scored/);
  assert.match(title, /tests 8\/10/);
  assert.match(title, /cost \$0\.42 of \$5\.00 cap/);
  assert.match(title, /wall time 2m 00s/);
  assert.doesNotMatch(title, /regression/);
});

test('cell tooltip flags a scored run that hit a limit, and a cost over the cap, without hiding the score', () => {
  const overLimit = cellTitle(cell({ overLimit: 'budget', cutShort: 'budget' }));
  assert.match(overLimit, /^scored/);
  assert.match(overLimit, /stopped at the \$5\.00 budget cap, scored on the code it had; counted in the mean/);
  const overCap = cellTitle(cell({ costUsd: 12.82, capUsd: 6 }));
  assert.match(overCap, /cost \$12\.82, over the \$6\.00 cap/);
  assert.doesNotMatch(overCap, / of \$6\.00 cap/);
});

test('cell tooltip names every earlier attempt instead of letting them vanish', () => {
  const title = cellTitle(
    cell({
      attempts: 3,
      priorAttempts: [
        { runId: 'r-a', zeroshotRunId: null, state: 'interrupted', exitReason: 'interrupted after 985s', costUsd: 2.68, wallS: 985 },
        { runId: 'r-b', zeroshotRunId: null, state: 'crashed', exitReason: 'restic executable is unavailable', costUsd: null, wallS: null },
      ],
    })
  );
  assert.match(title, /attempt 3 \(earlier: interrupted, crashed: restic executable is unavailable\)/);
});

test('cell map and aggregate map index by task+seed / key with no collisions', () => {
  const cells = [cell({ task: 'gron', seed: 0 }), cell({ task: 'gron', seed: 1 }), cell({ task: 'dupl', seed: 0 })];
  const map = cellMap(cells);
  assert.equal(map.size, 3);
  assert.equal(map.get(cellKey('gron', 0))?.seed, 0);
  assert.equal(map.get(cellKey('dupl', 0))?.task, 'dupl');
  assert.equal(map.get(cellKey('gron', 2)), undefined);

  const aggs = aggregateByKey([{ key: '0', mean: 0.5, min: 0.5, max: 0.5, n: 1 }]);
  assert.equal(aggs.get('0')?.mean, 0.5);
  assert.equal(aggs.get('1'), undefined);
});

test('a batch is live while anything in it is still running or unscored', () => {
  assert.equal(batchIsLive({ status: 'running', runsInFlight: 0 }), true);
  assert.equal(batchIsLive({ status: 'done', runsInFlight: 1 }), true);
  assert.equal(batchIsLive({ status: 'done', runsInFlight: 0 }), false);
  // A stopped batch with cells pb never started (1/2 done) is not live.
  assert.equal(batchIsLive({ status: 'stopped', runsInFlight: 0 }), false);
});

test('a replay batch labels its columns as replays and says what was replayed on which model', () => {
  assert.deepEqual(seedHeader(0, true, false), { text: 'replay 1', title: 'replay 1 (pb seed 0)' });
  assert.deepEqual(seedHeader(1, false, false), { text: 'seed 1', title: 'seed 1' });
  assert.equal(seedHeader(9, true, true).text, '10');
  assert.equal(replaySegmentText({ node: 'build#1', through: null }), 'build#1');
  assert.equal(replaySegmentText({ node: 'triage#1', through: 'repair#1' }), 'triage#1 → repair#1');
  assert.equal(replayModelText({ models: ['claude-sonnet-5'], recordedModel: 'claude-haiku-4-5' }), 'on sonnet-5 (recorded: haiku-4-5)');
  assert.equal(replayModelText({ models: ['claude-haiku-4-5'], recordedModel: 'claude-haiku-4-5' }), 'on haiku-4-5 (as recorded)');
  assert.equal(replayModelText({ models: [], recordedModel: null }), null);
});

test('retried attempts are counted apart and the maximum spend covers them', () => {
  assert.equal(retriedText({ retriedAttempts: 0, retriedCostUsd: null }), null);
  assert.equal(retriedText({ retriedAttempts: 3, retriedCostUsd: 3.5 }), '+3 retried attempts, $3.50');
  assert.equal(retriedText({ retriedAttempts: 1, retriedCostUsd: null }), '+1 retried attempt');
  // 1 planned cell + 3 retries, each with its own $5 cap.
  assert.equal(batchMaxSpend(5, 1, 3), 20);
  assert.equal(batchMaxSpend(null, 1, 3), null);
});

test('a batch detail is live while any cell is still running or evaluating', () => {
  assert.equal(detailIsLive({ status: 'done', cells: [{ state: 'scored' }, { state: 'evaluating' }] }), true);
  assert.equal(detailIsLive({ status: 'done', cells: [{ state: 'scored' }, { state: 'failed' }] }), false);
  assert.equal(detailIsLive({ status: 'running', cells: [] }), true);
});

// ---------- compare ----------

test('noise verdicts are shown in words, matching pb\'s rule but friendlier than "unknown (n<2)"', () => {
  assert.equal(noiseText('within_noise'), 'within noise');
  assert.equal(noiseText('beyond_spread'), 'beyond seed spread');
  assert.equal(noiseText('needs_seeds'), 'needs 2 seeds');
  assert.equal(noiseText('n_a'), '—');
  assert.equal(noiseText('something_future'), 'something_future'); // forward compatible: shown verbatim, not hidden
});

test('only a real, measurable change draws the eye; "needs seeds" and "n/a" are muted', () => {
  assert.equal(noiseTone('beyond_spread'), 'ok');
  assert.equal(noiseTone('within_noise'), 'muted');
  assert.equal(noiseTone('needs_seeds'), 'muted');
  assert.equal(noiseTone('n_a'), 'muted');
});

test('delta tone points green/red only for a beyond-spread change, never for noise', () => {
  assert.equal(deltaTone('beyond_spread', 0.13), 'pos');
  assert.equal(deltaTone('beyond_spread', -0.13), 'neg');
  assert.equal(deltaTone('beyond_spread', 0), null);
  assert.equal(deltaTone('within_noise', 0.058), null, 'a within-noise Δ must stay neutral even though it is non-zero');
  assert.equal(deltaTone('needs_seeds', 0.5), null);
  assert.equal(deltaTone('n_a', null), null);
});

test('the totals verdict counts tasks instead of letting one beyond-spread task speak for all of them', () => {
  const up = { noise: 'beyond_spread', deltaScore: 0.2 };
  const down = { noise: 'beyond_spread', deltaScore: -0.2 };
  const within = { noise: 'within_noise', deltaScore: 0.01 };
  const needs = { noise: 'needs_seeds', deltaScore: 0.3 };
  const na = { noise: 'n_a', deltaScore: null };
  // The reported case: 1 of 3 tasks beyond spread used to read "beyond seed spread" overall.
  const mixed = totalsVerdict([up, within, within]);
  assert.equal(mixed.verdict, 'mixed');
  assert.equal(mixed.text, '1 of 3 beyond spread (B ahead), 2 within noise');
  assert.deepEqual([mixed.up, mixed.down, mixed.within, mixed.compared], [1, 0, 2, 3]);
  // A real change only when every compared task moved beyond spread the same way.
  const real = totalsVerdict([up, up, up]);
  assert.equal(real.verdict, 'beyond_spread');
  assert.equal(real.text, 'beyond seed spread on all 3 tasks (B ahead)');
  assert.equal(totalsVerdict([down, down]).text, 'beyond seed spread on both tasks (B behind)');
  const opposite = totalsVerdict([up, down]);
  assert.equal(opposite.verdict, 'mixed');
  assert.equal(opposite.text, 'beyond seed spread on both tasks (B ahead on 1, behind on 1)');
  assert.equal(totalsVerdict([within, within]).verdict, 'within_noise');
  assert.equal(totalsVerdict([within, within]).text, 'within noise on both tasks');
  // Never settled while a task still needs seeds.
  const unsettled = totalsVerdict([needs, up]);
  assert.equal(unsettled.verdict, 'mixed');
  assert.equal(unsettled.text, '1 of 2 beyond spread (B ahead), 1 needs 2 seeds');
  assert.equal(totalsVerdict([needs, within]).verdict, 'needs_seeds');
  assert.equal(totalsVerdict([needs, needs]).text, 'both tasks need 2 seeds');
  assert.equal(totalsVerdict([na, within]).verdict, 'within_noise', 'a task only one side ran does not count either way');
  assert.equal(totalsVerdict([na, within]).text, 'within noise');
  assert.equal(totalsVerdict([na]).verdict, 'n_a');
  assert.equal(totalsVerdict([up]).text, 'beyond seed spread (B ahead)');
});

test('signed formatters show a +/- sign and never a double negative', () => {
  assert.equal(signedPp(0.12), '+12.0pp');
  assert.equal(signedPp(-0.02), '-2.0pp');
  assert.equal(signedPp(0), '0.0pp');
  assert.equal(signedPp(null), '—');
  assert.equal(signedUsd(1.5), '+$1.50');
  assert.equal(signedUsd(-0.5), '-$0.50');
  assert.equal(signedUsd(undefined), '—');
  assert.equal(signedDuration(30), '+30s');
  assert.equal(signedDuration(-30), '-30s');
  assert.equal(signedDuration(null), '—');
});

test('task aggregate text shows the seed count and only shows spread when scores actually differ', () => {
  assert.equal(taskAggText({ n: 2, meanScore: 0.71, minScore: 0.58, maxScore: 0.84 }), '0.71 (0.58–0.84), n=2');
  assert.equal(taskAggText({ n: 2, meanScore: 0.85, minScore: 0.85, maxScore: 0.85 }), '0.85, n=2');
  assert.equal(taskAggText(null), '—');
  assert.equal(taskAggText({ n: 0, meanScore: 0, minScore: 0, maxScore: 0 }), '—');
});

// ---------- start a batch ----------

const SETS: TaskSet[] = [
  { name: 'dev', tasks: ['gron', 'nomino'] },
  { name: 'smoke', tasks: ['gron'] },
];

test("resolving a selection expands task sets and dedups, matching pb's Config.resolve_tasks", () => {
  assert.deepEqual(resolveTasks(['dev'], [], SETS), ['gron', 'nomino']);
  assert.deepEqual(resolveTasks(['dev', 'smoke'], [], SETS), ['gron', 'nomino']); // smoke's gron is already counted
  assert.deepEqual(resolveTasks(['dev'], ['extra-task'], SETS), ['gron', 'nomino', 'extra-task']);
  assert.deepEqual(resolveTasks([], ['gron'], SETS), ['gron']); // a raw id that happens to also be in a set: used as-is
  assert.deepEqual(resolveTasks([], [], SETS), []);
});

test('the tasks arg sent to pb is the raw selection (set names and ids), comma separated', () => {
  assert.equal(tasksArg(['dev'], ['extra-task']), 'dev,extra-task');
  assert.equal(tasksArg([], []), '');
});

test('max spend is runs x cap, and null while nothing would run or no cap applies', () => {
  assert.equal(maxSpend(2, 3, 5), 30);
  assert.equal(maxSpend(0, 3, 5), null);
  assert.equal(maxSpend(2, 0, 5), null);
  assert.equal(maxSpend(2, 3, null), null);
  assert.equal(maxSpend(2, 3, 0), null);
});

const form = (over: Partial<StartFormValues> = {}): StartFormValues => ({
  taskCount: 2,
  label: 'try-x',
  seeds: 3,
  parallel: '2',
  harnessKind: 'default',
  variant: '',
  customGraph: '',
  customRuntime: '',
  maxCost: '6',
  maxMinutes: '45',
  maxSteps: '24',
  ...over,
});

test('start form validation reports every problem at once, by field', () => {
  assert.deepEqual(validateStart(form()), {});
  const all = validateStart(form({ taskCount: 0, label: ' ', parallel: '0', maxCost: '-1', maxMinutes: 'x', maxSteps: '1.5', harnessKind: 'variant' }));
  assert.deepEqual(Object.keys(all), ['tasks', 'label', 'parallel', 'harness', 'maxCost', 'maxMinutes', 'maxSteps']);
  assert.match(all.label!, /label is required/);
  // Empty limits fall back to pb.toml's defaults; that is not an error.
  assert.deepEqual(validateStart(form({ maxCost: '', maxMinutes: '', maxSteps: '', parallel: '' })), {});
  assert.equal(validateStart(form({ harnessKind: 'custom', customGraph: 'harness/graph.json' })).harness, 'Choose both a graph and a runtime file.');
  // The harness preview's own findings block Start too.
  assert.equal(validateStart(form({ harnessKind: 'custom', customGraph: 'g', customRuntime: 'r', harnessErrors: ['graph: g has no `root`.'] })).harness, 'graph: g has no `root`.');
});

test('a ticked task set shows up on its member tasks', () => {
  const sets: TaskSet[] = [
    { name: 'dev', tasks: ['gron', 'nomino'] },
    { name: 'smoke', tasks: ['gron'] },
  ];
  assert.deepEqual(setsIncluding('gron', ['dev', 'smoke'], sets), ['dev', 'smoke']);
  assert.deepEqual(setsIncluding('gron', ['smoke'], sets), ['smoke']);
  assert.deepEqual(setsIncluding('nomino', ['smoke'], sets), []);
});

test('harness nodes group into one segment per loop, in graph order', () => {
  const n = (name: string, loopName: string | null, model = 'claude-sonnet-5', effort: string | null = 'high') => ({
    name,
    kind: 'step',
    model,
    effort,
    loopName,
    loopMax: loopName ? 3 : null,
    parallel: null,
  });
  const segs = harnessSegments([n('explore', null), n('build', null), n('coverage', 'review_loop'), n('repair', 'review_loop')]);
  assert.deepEqual(
    segs.map((s) => [s.loopName, s.loopMax, s.nodes.map((x) => x.name)]),
    [
      [null, null, ['explore', 'build']],
      ['review_loop', 3, ['coverage', 'repair']],
    ]
  );
  assert.equal(nodeModelText(n('x', null)), 'sonnet-5 · high');
  assert.equal(nodeModelText({ model: null, effort: null }), 'no model');
});

test('default harness options are a safe empty shape before the form has loaded', () => {
  const opts = defaultHarnessOptions();
  assert.deepEqual(opts.taskSets, []);
  assert.equal(opts.defaultMaxCostUsd, 5);
});

test('a batch just started reads as starting, not "not found", until pb records it', () => {
  const now = 1_800_000_000_000;
  // A 404 for an id this tab never started is a real "not found".
  assert.equal(batchStartPhase(undefined, 404, 'b-unknown', now), null);
  noteStarted('b-new', now - 5_000);
  assert.equal(batchStartPhase(undefined, 404, 'b-new', now), 'starting');
  // Patience runs out: a 404 long after Start is "not found" again.
  assert.equal(batchStartPhase(undefined, 404, 'b-new', now + START_PATIENCE_MS), null);
  // The server's own answer wins either way.
  assert.equal(batchStartPhase({ status: 'starting' }, undefined, 'b-other', now), 'starting');
  assert.equal(batchStartPhase({ status: 'failed_to_start' }, undefined, 'b-new', now), 'failed_to_start');
  assert.equal(batchStartPhase({ status: 'running', starting: true }, undefined, 'b-new', now), null);
  assert.equal(detailIsLive({ status: 'starting', cells: [] }), true);
});

test('Parallel defaults to 4, capped by the host run-slot ceiling', () => {
  assert.equal(defaultParallel(undefined), 4);
  assert.equal(defaultParallel(8), 4);
  assert.equal(defaultParallel(2), 2);
  assert.equal(defaultParallel(0), 4, "a nonsense ceiling falls back to pb's default");
});

test('other running batches do not block Start: the note says how busy the host run slots are', () => {
  assert.equal(concurrencyNote([], null, 4), null, 'nothing running, nothing to say');
  assert.equal(concurrencyNote([], { maxConcurrentRuns: 4, running: 0, queued: 0 }, 4), null);
  const two = [
    { id: 'b1', label: 'a' },
    { id: 'b2', label: null },
  ];
  assert.equal(
    concurrencyNote(two, { maxConcurrentRuns: 4, running: 4, queued: 3 }, 4),
    '2 batches are already running; 4 of 4 run slots on this host are busy, 3 runs queued. A new batch starts now; its runs queue for a free slot (at most 4 runs at once across all batches).'
  );
  // an older pb without `pb slots`: still says a batch runs, from the batches list alone
  assert.match(concurrencyNote([two[0]], null, 3) ?? '', /^1 batch is already running\. .*at most 3 runs/);
  // runs from another checkout on the same host hold slots even when this pb has no running batch
  assert.match(concurrencyNote([], { maxConcurrentRuns: 4, running: 1, queued: 0 }, 4) ?? '', /^1 of 4 run slots on this host are busy\. /);
});

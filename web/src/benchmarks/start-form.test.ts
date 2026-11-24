import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyDefaults, defaultHarnessOptions, emptyStartForm, harnessPaths, spendLine, startFormReducer, toStartRequest, DEFAULT_SEEDS } from './start-form';
import { toggleCompare, toggleIn } from './batch-list';

const options = { ...defaultHarnessOptions(), defaultGraph: 'harness/graph.json', defaultRuntime: 'harness/runtime.json', maxConcurrentRuns: 2 };

test('the form starts empty with 3 seeds, and fills only the limits still empty once pb.toml loads', () => {
  const form = emptyStartForm();
  assert.equal(form.seeds, DEFAULT_SEEDS);
  const filled = applyDefaults({ ...form, maxCost: '2' }, options);
  assert.deepEqual([filled.maxCost, filled.maxMinutes, filled.maxSteps, filled.parallel], ['2', '45', '12', '2']);
});

test('the reducer edits fields and ticks sets and tasks', () => {
  let form = startFormReducer(emptyStartForm(), { kind: 'edit', patch: { label: 'x' } });
  form = startFormReducer(form, { kind: 'toggleSet', name: 'dev' });
  form = startFormReducer(form, { kind: 'toggleTask', id: 't1' });
  form = startFormReducer(form, { kind: 'toggleTask', id: 't1' });
  assert.deepEqual([form.label, form.setNames, form.taskIds], ['x', ['dev'], []]);
});

test('a harness choice resolves to pb’s graph and runtime paths', () => {
  const form = emptyStartForm();
  assert.deepEqual(harnessPaths(form, options), { graph: 'harness/graph.json', runtime: 'harness/runtime.json' });
  assert.equal(harnessPaths(form, defaultHarnessOptions()), null);
  assert.equal(harnessPaths({ ...form, harnessKind: 'variant' }, options), null);
  assert.deepEqual(harnessPaths({ ...form, harnessKind: 'variant', variant: 'v2' }, options), { graph: 'harness/variants/v2/graph.json', runtime: 'harness/variants/v2/runtime.json' });
  assert.equal(harnessPaths({ ...form, harnessKind: 'custom', customGraph: ' g.json ' }, options), null);
  assert.deepEqual(harnessPaths({ ...form, harnessKind: 'custom', customGraph: ' g.json ', customRuntime: 'r.json' }, options), { graph: 'g.json', runtime: 'r.json' });
});

test('Start sends the trimmed form, with empty numbers left to pb', () => {
  const form = { ...emptyStartForm(), setNames: ['dev'], taskIds: ['t9'], label: ' try ', model: ' ', maxCost: '2.5', parallel: '' };
  assert.deepEqual(JSON.stringify(toStartRequest(form)), JSON.stringify({ tasks: 'dev,t9', seeds: 3, label: 'try', harness: { kind: 'default' }, maxCostUsd: 2.5 }));
  assert.deepEqual(toStartRequest({ ...form, harnessKind: 'variant', variant: 'v2' }).harness, { kind: 'variant', name: 'v2' });
  assert.deepEqual(toStartRequest({ ...form, harnessKind: 'custom', customGraph: ' g ', customRuntime: 'r ' }).harness, { kind: 'custom', graph: 'g', runtime: 'r' });
});

test('the spend line names its cap, or why there is no maximum yet', () => {
  assert.deepEqual(spendLine(3, 2, 5, 5), { runs: '3 tasks × 2 seeds', spend: { max: '$30.00', cap: '$5.00 cap from pb.toml' }, hint: null });
  assert.deepEqual(spendLine(1, 1, 2, 5).spend, { max: '$2.00', cap: '$2.00 cap' });
  assert.equal(spendLine(1, 1, 2, 5).runs, '1 task × 1 seed');
  assert.equal(spendLine(0, 3, 5, 5).hint, '(pick a task to see the maximum spend)');
  assert.equal(spendLine(2, 3, 0, 5).hint, '(set a positive cost cap to see the maximum spend)');
});

test('toggleIn ticks and unticks; toggleCompare keeps at most two', () => {
  assert.deepEqual(toggleIn(['a'], 'b'), ['a', 'b']);
  assert.deepEqual(toggleIn(['a', 'b'], 'a'), ['b']);
  assert.deepEqual(toggleCompare(['a', 'b'], 'c'), ['b', 'c']);
  assert.deepEqual(toggleCompare(['a', 'b'], 'a'), ['b']);
});

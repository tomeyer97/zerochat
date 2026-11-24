import { test } from 'node:test';
import assert from 'node:assert/strict';
import { batchCostText, batchCostTitle, batchCounts, isReplayBatch, limitsText, spendCeiling, startedAtText } from './batches';
import { maxSpend } from './start-form';
import type { BenchmarkCell } from '../types';

test('both maximum-spend figures are runs × the per-run cap', () => {
  assert.equal(spendCeiling(6, 5), 30);
  assert.equal(maxSpend(2, 3, 5), spendCeiling(6, 5));
});

test('the batch cost reads against its maximum, and its tooltip counts retried attempts', () => {
  assert.equal(batchCostText(4.1, 12), '$4.10 of $12.00 max');
  assert.equal(batchCostText(4.1, null), '$4.10');
  assert.equal(batchCostTitle(1, 0), 'summed over every run');
  assert.equal(batchCostTitle(1, 3), 'summed over every attempt, the 3 retried included; the max is the per-run cap × 4 attempts (1 cell + 3 retried)');
  assert.match(batchCostTitle(2, 1), /\(2 cells \+ 1 retried\)$/);
});

test('limits read as one line, and only the ones set', () => {
  assert.equal(limitsText({ maxCostUsd: 6, maxWallMinutes: 45, maxSteps: 24 }), 'cap $6.00 / 45m / 24 steps');
  assert.equal(limitsText({ maxCostUsd: null, maxWallMinutes: 45, maxSteps: null }), '45m');
  assert.equal(limitsText(null), null);
});

test('an unknown start time is a dash with no tooltip', () => {
  assert.deepEqual(startedAtText(null, 0), { text: '—', title: '' });
  assert.notEqual(startedAtText(1_800_000_000_000, 1_800_000_000_000).title, '');
});

test('counts: groups, cut-short scored cells and planned cells', () => {
  const cells = [
    { state: 'scored', cutShort: 'budget', overLimit: 'budget', capUsd: 5 },
    { state: 'scored', cutShort: null, overLimit: null, capUsd: 5 },
    { state: 'running_agent', cutShort: null, overLimit: null, capUsd: 5 },
  ] as BenchmarkCell[];
  const counts = batchCounts({ cells, tasks: ['a', 'b'], seeds: [0, 1] });
  assert.equal(counts.byGroup.scored, 2);
  assert.equal(counts.byGroup.running, 1);
  assert.equal(counts.cutShort, 1);
  assert.equal(counts.planned, 4);
});

test('a batch is a replay batch when every attempt is a replay', () => {
  const replay = { runs: 2 } as never;
  assert.equal(isReplayBatch({ replays: [replay], cells: [{ attempts: 1 }, { attempts: 1 }] }), true);
  assert.equal(isReplayBatch({ replays: [replay], cells: [{ attempts: 2 }, { attempts: 1 }] }), false);
  assert.equal(isReplayBatch({ replays: [], cells: [] }), false);
});

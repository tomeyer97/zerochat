import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { BatchSummary } from '../types';
import { batchCompactText, batchDoneTitle, batchIdentity, batchNoCostReason, batchNoScoreReason, comparePair, toggleCompare } from './batch-list';

test('at most two batches stay picked; a third tick drops the first', () => {
  assert.deepEqual(toggleCompare([], 'a'), ['a']);
  assert.deepEqual(toggleCompare(['a'], 'b'), ['a', 'b']);
  assert.deepEqual(toggleCompare(['a', 'b'], 'c'), ['b', 'c']);
  assert.deepEqual(toggleCompare(['a', 'b'], 'a'), ['b']);
});

test('the older batch is always A, whichever row was ticked first', () => {
  const batches = [
    { id: 'new', startedAt: 2000 },
    { id: 'old', startedAt: 1000 },
  ];
  assert.deepEqual(comparePair(['new', 'old'], batches), ['old', 'new']);
  assert.deepEqual(comparePair(['old', 'new'], batches), ['old', 'new']);
  assert.equal(comparePair(['new'], batches), null);
});

test('a row names its harness version and models, and its Done tooltip counts retries apart', () => {
  assert.equal(batchIdentity({ harnessVersion: 'v3', models: ['a', 'b'] }), 'harness v3 · a, b');
  assert.equal(batchIdentity({ harnessVersion: null, models: ['a'] }), 'a');
  assert.equal(batchIdentity({ harnessVersion: null, models: [] }), '');
  assert.equal(batchDoneTitle({ runsDone: 1, runsTotal: 2, retriedAttempts: 0, retriedCostUsd: null }), '1 of 2 cells (task × seed) done');
  assert.equal(batchDoneTitle({ runsDone: 1, runsTotal: 1, retriedAttempts: 2, retriedCostUsd: 1 }), '1 of 1 cells (task × seed) done; +2 retried attempts, $1.00, not counted as cells');
});

const batch: BatchSummary = {
  id: 'b1',
  label: 'lab-clone-v1',
  harnessVersion: '1cbce021',
  zeroshotVersion: null,
  models: ['claude-haiku-4-5'],
  taskCount: 3,
  seedCount: 3,
  runsTotal: 9,
  runsDone: 9,
  runsInFlight: 0,
  retriedAttempts: 0,
  retriedCostUsd: null,
  meanScore: 0.76,
  minScore: 0.61,
  maxScore: 0.91,
  totalCostUsd: 83.24,
  startedAt: Date.UTC(2026, 10, 18, 13, 44),
  endedAt: null,
  status: 'done',
  real: true,
};

test('the phone-width row line labels every number and spaces the score like the rest', () => {
  const text = batchCompactText(batch, Date.UTC(2026, 10, 18, 15));
  assert.match(text, /^3 tasks × 3 seeds · 9\/9 done · score 0\.76 \(0\.61–0\.91\) · \$83\.24 · started /);
  assert.doesNotMatch(text, / ·\S/);
});

test('a stopped or crashed batch with nothing scored says why its score and cost are "—"', () => {
  const unscored = { ...batch, runsDone: 1, meanScore: null, minScore: null, maxScore: null, totalCostUsd: null };
  assert.equal(batchNoScoreReason({ ...unscored, status: 'stopped' }), 'stopped before a run was scored');
  assert.equal(batchNoScoreReason({ ...unscored, status: 'crashed' }), 'crashed before a run was scored');
  assert.equal(batchNoScoreReason(batch), null);
  assert.equal(batchNoCostReason(unscored), 'pb recorded no cost for its runs');
  assert.equal(batchNoCostReason(batch), null);
  assert.match(batchCompactText({ ...unscored, status: 'crashed' }, 0), / · no score \(crashed before a run was scored\) · no cost recorded · /);
});

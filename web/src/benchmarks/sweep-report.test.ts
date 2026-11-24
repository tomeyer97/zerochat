import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { SweepModelStat, SweepSuggestion } from '../types';
import { mappingSummary, recommendationTone, sweepReportKey, sweepRowCells, writeDisabledReason } from './sweep-report';
import { durationS, signalClass } from './batch-compare';

const stat = (over: Partial<SweepModelStat>): SweepModelStat => ({
  model: 'm',
  isBaseline: false,
  n: 2,
  replays: 2,
  states: 1,
  mean: 0.5,
  min: 0.4,
  max: 0.6,
  nodeCost: 0.1,
  segCost: 0.2,
  delta: 0.05,
  spread: 0.1,
  dcost: 0.01,
  dnode: -0.02,
  noise: 'beyond noise',
  verdicts: {},
  perUsd: 0.5,
  ...over,
});

test('a sweep row: Δs blank on the baseline, endings instead of a missing score', () => {
  assert.deepEqual(sweepRowCells(stat({})), { n: '2', score: '0.50', delta: '+5.0pp', noiseClass: 'bm-signal-pos', dnode: '-$0.02', dcost: '+$0.01', perUsd: '50.0pp/$' });
  const base = sweepRowCells(stat({ isBaseline: true, n: 1, replays: 3, mean: null, verdicts: { timeout: 2, crash: 1 }, noise: 'baseline', perUsd: null }));
  assert.deepEqual(base, { n: '1/3', score: 'timeout ×2, crash ×1', delta: '', noiseClass: 'muted', dnode: '', dcost: '', perUsd: '' });
  assert.equal(sweepRowCells(stat({ mean: null })).score, '—');
});

test('recommendation and signal colours', () => {
  assert.deepEqual(['upgrade', 'unknown', 'keep'].map(recommendationTone), ['bm-signal-pos', 'muted', '']);
  assert.deepEqual([signalClass('pos'), signalClass('neg'), signalClass(null), signalClass(null, '')], ['bm-signal-pos', 'bm-signal-neg', 'muted', '']);
});

test('the mapping summary and when Write runtime is offered', () => {
  const s: SweepSuggestion = {
    mapping: { a: 'x', b: null, c: 'y' },
    changes: [{ node: 'a' } as SweepSuggestion['changes'][number]],
    deltaScore: 0.02,
    deltaCost: -0.1,
    baseCost: 1,
    estCost: 0.9,
    budget: null,
    overBudget: false,
  };
  assert.equal(mappingSummary(s), '2 other node(s) unchanged. Expected vs the recorded runtime: Δscore +2.0pp, Δcost -$0.10 per run (cost: $1.00 → $0.90)');
  assert.equal(mappingSummary({ ...s, baseCost: null }), '2 other node(s) unchanged. Expected vs the recorded runtime: Δscore +2.0pp, Δcost -$0.10 per run');
  assert.equal(writeDisabledReason(s), null);
  assert.equal(writeDisabledReason({ ...s, overBudget: true }), "This mapping doesn't fit the budget.");
  assert.equal(writeDisabledReason({ ...s, changes: [] }), 'Nothing would change.');
});

test('keys and seconds', () => {
  assert.notEqual(sweepReportKey({ ids: ['a'], label: 'b' }), sweepReportKey({ ids: ['a'], task: 'b' }));
  assert.equal(durationS(null), '—');
  assert.equal(durationS(65), '1m 05s');
});

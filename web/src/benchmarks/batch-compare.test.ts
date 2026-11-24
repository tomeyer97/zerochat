import { test } from 'node:test';
import assert from 'node:assert/strict';
import { verdictOf, verdictText } from './batch-compare';

const counts = (over: Partial<Parameters<typeof verdictOf>[0]>) => ({ up: 0, down: 0, within: 0, needsSeeds: 0, compared: 0, ...over });

test('the verdict rule and its words are separate', () => {
  assert.equal(verdictOf(counts({})), 'n_a');
  assert.equal(verdictText(counts({})), '—');
  assert.equal(verdictOf(counts({ up: 3, compared: 3 })), 'beyond_spread');
  assert.equal(verdictText(counts({ up: 3, compared: 3 })), 'beyond seed spread on all 3 tasks (B ahead)');
  assert.equal(verdictOf(counts({ within: 1, needsSeeds: 1, compared: 2 })), 'needs_seeds');
  assert.equal(verdictText(counts({ within: 1, needsSeeds: 1, compared: 2 })), '1 within noise, 1 needs 2 seeds');
  assert.equal(verdictText(counts({ needsSeeds: 1, compared: 1 })), 'needs 2 seeds');
  assert.equal(verdictText(counts({ up: 1, within: 2, compared: 3 })), '1 of 3 beyond spread (B ahead), 2 within noise');
});

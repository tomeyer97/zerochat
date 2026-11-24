import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SPREAD_EPSILON, hasSpread } from './scores';

test('a range shows only when it is wide enough to print differently', () => {
  assert.equal(hasSpread(0.5, 0.5 + SPREAD_EPSILON), true);
  assert.equal(hasSpread(0.5, 0.504), false);
  assert.equal(hasSpread(null, 0.9), false);
  assert.equal(hasSpread(0.1, undefined), false);
});

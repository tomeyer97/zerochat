import { test } from 'node:test';
import assert from 'node:assert/strict';
import { nextTabIndex } from './a11y';

test('tab-list keys wrap at both ends', () => {
  assert.equal(nextTabIndex('ArrowLeft', 0, 3), 2);
  assert.equal(nextTabIndex('ArrowLeft', 2, 3), 1);
  assert.equal(nextTabIndex('ArrowRight', 2, 3), 0);
  assert.equal(nextTabIndex('ArrowRight', 0, 3), 1);
  assert.equal(nextTabIndex('Home', 2, 3), 0);
  assert.equal(nextTabIndex('End', 0, 3), 2);
});

test('other keys are ignored', () => {
  assert.equal(nextTabIndex('ArrowUp', 0, 3), null);
  assert.equal(nextTabIndex('Enter', 0, 3), null);
  assert.equal(nextTabIndex('toString', 0, 3), null);
});

test('a target without data-tab-index (-1) still lands on a tab', () => {
  assert.equal(nextTabIndex('ArrowRight', -1, 3), 0);
  assert.equal(nextTabIndex('ArrowLeft', -1, 3), 1);
});

// The refresh rules of hooks.ts: a reload of the same resource keeps its data (no skeleton), a new
// key starts over, and a live trigger is taken at most once per interval.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { startLoad, throttleWait, type ResourceState } from './hooks';

const loaded: ResourceState<string[]> = { key: 'run/changes/abc', data: ['a.ts'], loading: false, reloading: false };

test('a refresh of the same key reloads in the background and keeps the data', () => {
  assert.deepEqual(startLoad(loaded, 'run/changes/abc'), { ...loaded, reloading: true });
});

test('a new key starts over: loading, no data', () => {
  assert.deepEqual(startLoad(loaded, 'run/changes/def'), { key: 'run/changes/def', loading: true, reloading: false });
});

test('a throttled value is taken at once the first time, then at most once per interval', () => {
  assert.equal(throttleWait(null, 10_000, 1500), 0);
  assert.equal(throttleWait(10_000, 10_400, 1500), 1100);
  assert.equal(throttleWait(10_000, 11_500, 1500), 0);
  assert.equal(throttleWait(10_000, 20_000, 1500), 0);
});

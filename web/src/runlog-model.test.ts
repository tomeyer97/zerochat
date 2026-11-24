import { test } from 'node:test';
import assert from 'node:assert/strict';
import { agentFailed, agentLabel, lineAgent, lineShown, logFilters } from './runlog-model';
import type { BackgroundAgent } from './types';

test('agent names read as the filter chips say them', () => {
  assert.equal(agentLabel('active_memory_graph_maintenance'), 'memory graph maintenance');
  assert.equal(agentLabel('indexer'), 'indexer');
});

test("a line's agent: the server's field, an older server's prefix, or an Active Memory sentence", () => {
  assert.deepEqual(lineAgent({ agent: 'a', text: 'hi' }), { agent: 'a', text: 'hi' });
  assert.deepEqual(lineAgent({ text: '[indexer] started' }), { agent: 'indexer', text: 'started' });
  assert.deepEqual(lineAgent({ text: 'Active Memory active_memory_recall finished' }), { agent: null, text: 'memory recall finished' });
  assert.deepEqual(lineAgent({ text: 'plain' }), { agent: null, text: 'plain' });
});

test('filter chips: none without agents; All, Run, then one per agent, failed ones marked', () => {
  assert.deepEqual(logFilters([]), []);
  const agents = [
    { name: 'active_memory_recall', lines: 3, retries: 0, errors: 0, outcome: 'completed' },
    { name: 'indexer', lines: 1, retries: 0, errors: 2, outcome: null },
  ] as unknown as BackgroundAgent[];
  const chips = logFilters(agents);
  assert.deepEqual(
    chips.map((c) => c.id),
    ['all', 'run', 'active_memory_recall', 'indexer']
  );
  assert.deepEqual(
    chips.map((c) => !!c.bad),
    [false, false, false, true]
  );
  assert.equal(chips[2].label, 'memory recall');
  assert.equal(agentFailed(agents[1]), true);
});

test('which lines a filter shows', () => {
  assert.equal(lineShown({ agent: 'x' }, 'all'), true);
  assert.equal(lineShown({ agent: null }, 'run'), true);
  assert.equal(lineShown({ agent: 'x' }, 'run'), false);
  assert.equal(lineShown({ agent: 'x' }, 'x'), true);
  assert.equal(lineShown({ agent: null }, 'x'), false);
});

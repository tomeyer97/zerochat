import { test } from 'node:test';
import assert from 'node:assert/strict';
import { replaySentence } from './replay-text';
import type { ReplayContext } from '../types';

const replay = (over: Partial<ReplayContext> = {}): ReplayContext => ({
  sourceRun: 'pb-run-1',
  sourceZeroshotRunId: null,
  sourceBatch: null,
  sourceLabel: 'clone-v1',
  sourceTask: 'gron',
  sourceScore: 0.62,
  node: 'build#1',
  through: null,
  base: 'pristine task workspace',
  models: ['claude-sonnet-5'],
  recordedModel: 'claude-haiku-4-5',
  recordedScore: 0.53,
  sweepId: null,
  sweepLabel: null,
  runs: 1,
  ...over,
});

test('a replay sentence names the recorded run, the segment, the model and the baseline', () => {
  assert.deepEqual(replaySentence(replay()), {
    source: 'clone-v1 · gron',
    segment: 'build#1',
    model: 'on sonnet-5 (recorded: haiku-4-5)',
    base: 'pristine task workspace',
    sweep: null,
    end: 'build#1',
    runFinal: '(run final 0.62)',
  });
  const bare = replaySentence(replay({ sourceLabel: null, sourceTask: null, through: 'repair#1', sourceScore: 0.53, sweepId: 's1', sweepLabel: null }));
  assert.equal(bare.source, 'pb-run-1');
  assert.equal(bare.end, 'repair#1');
  assert.equal(bare.sweep, 's1');
  assert.equal(bare.runFinal, null, 'no run final when it equals the recorded score');
});

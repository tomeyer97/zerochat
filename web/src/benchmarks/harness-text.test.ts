import { test } from 'node:test';
import assert from 'node:assert/strict';
import { bareModel, harnessName, nodeSeparator, nodeTitle, versionTitle } from './harness-text';
import type { HarnessInfo, HarnessNode } from '../types';

const info = (over: Partial<HarnessInfo>): HarnessInfo => ({
  source: 'default',
  variant: null,
  runtimeFile: null,
  runtimeMatches: true,
  agentHarness: null,
  modelOverride: null,
  nodes: [],
  runs: 1,
  ...over,
});

test('a harness is named by where it came from, plus a runtime that is not its own', () => {
  assert.equal(harnessName(info({})), 'default harness (pb.toml)');
  assert.equal(harnessName(info({ source: 'variant', variant: 'clone-v1' })), 'variant clone-v1');
  assert.equal(harnessName(info({ source: 'variant', variant: 'v', runtimeFile: 'runtime-x.json', runtimeMatches: false })), 'variant v with runtime-x.json');
  assert.equal(harnessName(info({ runtimeMatches: false })), 'default harness (pb.toml) with a changed runtime');
  assert.equal(harnessName(info({ source: 'custom', runtimeMatches: false })), 'custom graph + runtime (matches no file under harness/ now)');
});

test('node chips: tooltip, and ‖ only between nodes of one parallel group', () => {
  const n = (name: string, parallel: string | null): HarnessNode => ({ name, kind: 'step', model: 'claude-sonnet-5', effort: null, loopName: null, loopMax: null, parallel });
  assert.equal(nodeTitle(n('build', null)), 'build: writes code, claude-sonnet-5');
  assert.equal(nodeTitle({ ...n('review', 'p'), kind: 'verifier', model: null, effort: 'high' }), 'review: reviews the work, no model in the runtime, high effort, in parallel group p');
  const nodes = [n('a', null), n('b', 'p'), n('c', 'p')];
  assert.deepEqual(
    nodes.map((_, i) => nodeSeparator(nodes, i)),
    [null, ' → ', ' ‖ ']
  );
});

test('model names lose only their claude- prefix; versions join with a dot', () => {
  assert.equal(bareModel('claude-haiku-4-5-20251001'), 'haiku-4-5-20251001');
  assert.equal(versionTitle({ harnessVersion: 'v3', zeroshotVersion: '16.1.0' }), 'harness version v3 · zeroshot 16.1.0');
  assert.equal(versionTitle({ harnessVersion: null, zeroshotVersion: null }), '');
});

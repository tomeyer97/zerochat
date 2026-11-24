// The run page's lane words (header line, node header, Details), the cost and background
// tooltips and the reasons in the "limited data" popover.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LIST_PRICE_NOTE, laneText, lanesLine, shortModel } from './format';
import { BACKGROUND_NOTE, runCostTitle } from './RunHeader';
import { nodeLane, withLane } from './NodePanel';
import { missingData, missingDataWhy } from './run-limits';
import type { RunView } from './types';

type Features = RunView['features'];

const claude = { harness: 'claude', provider: 'anthropic', model: 'claude-haiku-4-5', nodes: ['acceptance', 'code_review'] };
const codex = { harness: 'codex', provider: 'openai', model: 'gpt-5.1-codex', nodes: ['code'] };

test('a lane reads harness · provider · exact model id', () => {
  assert.equal(laneText(claude), 'claude · anthropic · claude-haiku-4-5');
  assert.equal(laneText({ harness: 'hermes', provider: 'nous', model: 'hermes-4-405b' }), 'hermes · nous · hermes-4-405b');
  assert.equal(laneText({ harness: null, provider: null, model: 'claude-opus-4-1-20250805' }), 'unknown · unknown · claude-opus-4-1-20250805', 'never a guess, never shortened');
  assert.equal(laneText({ harness: 'claude', provider: 'anthropic' }), 'claude · anthropic');
  // Graph cards keep their short names.
  assert.equal(shortModel('claude-haiku-4-5-20251001'), 'haiku-4-5');
});

test('the run header shows one lane, or "mixed lanes" with every lane in the tooltip', () => {
  assert.equal(lanesLine([]), null);
  assert.equal(lanesLine(undefined), null);
  assert.deepEqual(lanesLine([claude]), { text: 'claude · anthropic · claude-haiku-4-5', title: 'Agent CLI (harness) · provider · model' });
  const mixed = lanesLine([codex, claude]);
  assert.equal(mixed?.text, 'mixed lanes');
  assert.equal(mixed?.title, 'Agent CLI (harness) · provider · model:\ncode: codex · openai · gpt-5.1-codex\nacceptance, code_review: claude · anthropic · claude-haiku-4-5');
});

test("Details and the node header name the node's own lane with the exact model id", () => {
  const view = {
    graphNodes: [
      { name: 'code', harness: 'codex', provider: 'openai', model: 'gpt-5.1-codex' },
      { name: 'acceptance', harness: 'claude', provider: 'anthropic', model: 'claude-haiku-4-5' },
    ] as RunView['graphNodes'],
    lanes: [codex, claude],
  };
  assert.equal(laneText(nodeLane(view, 'code', { harness: 'codex', model: 'gpt-5.1-codex' })), 'codex · openai · gpt-5.1-codex');
  assert.equal(laneText(nodeLane(view, 'acceptance', { harness: 'claude', model: 'claude-haiku-4-5-20251001' })), 'claude · anthropic · claude-haiku-4-5-20251001');
  assert.equal(laneText(nodeLane(view, 'acceptance', undefined)), 'claude · anthropic · claude-haiku-4-5', 'before it ran: its configured lane');
  assert.equal(laneText(nodeLane({ graphNodes: [], lanes: [] }, 'x', { harness: null, model: null })), 'unknown · unknown');
  const facts = withLane(
    [
      { key: 'm', text: 'haiku-4-5 · high' },
      { key: 't', text: '4s' },
    ],
    nodeLane(view, 'acceptance', undefined),
    'high'
  );
  assert.deepEqual(
    facts.map((f) => f.text),
    ['claude · anthropic · claude-haiku-4-5 · high', '4s']
  );
});

test('the cost tooltip says once that ~$ is a list-price equivalent', () => {
  const estimated = runCostTitle({ costEstimated: true });
  assert.equal(estimated, LIST_PRICE_NOTE);
  assert.match(estimated, /list-price equivalent/);
  assert.match(estimated, /token counts/);
  assert.match(estimated, /not necessarily money spent/);
  assert.equal(runCostTitle({ costEstimated: false }), 'Reported by the provider.');
  assert.match(runCostTitle({ costEstimated: true, costSource: 'pb', zeroshotCostUsd: null }), /^pb's recorded cost/);
});

test('the background figure explains itself', () => {
  assert.equal(BACKGROUND_NOTE, 'Tokens of background agents (Active Memory); Zeroshot does not price them.');
});

test('limited data says why each item is missing', () => {
  const none: Features = { exactTimes: false, toolCalls: false, providerCost: false, workspaceSnapshots: false };
  const view = { capabilities: [] as string[], features: none, targetKind: 'direct' as RunView['targetKind'], executions: [{}] as RunView['executions'] };
  const why = missingDataWhy(view);
  assert.deepEqual(
    why.map((m) => m.what),
    missingData(view),
    'the same items as missingData'
  );
  assert.equal(why.find((m) => m.what === 'file changes per node')?.why, 'this Zeroshot version does not record them');
  for (const m of why) assert.ok(m.why.length > 0, m.what);
  assert.match(why.find((m) => m.what === 'messages to running agents')?.why ?? '', /16\.1/);
});

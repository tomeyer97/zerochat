// Tests of the pure helpers that live in page components (Timeline, Files, Chat, NodePanel,
// RunSummary), plus a few route, hooks and status cases over the shared run fixture. Look here
// for the tests of those components' exported helpers.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { errorWords, rowNote, timelineBars } from './Timeline';
import { changeRange, executionFilesChanged, retryable } from './Files';
import { pendingSent, steerScope, steerScopeNote, steerStatus, steerStatusText, steerTag, steerTagLabel } from './Chat';
import { nodeExecutions } from './NodePanel';
import { anyFilesChanged, costTitle, effectiveAction, runElapsed } from './RunSummary';
import { resolveVisit } from './route';
import { backoffDelay } from './hooks';
import { stepExecution } from './status';
import type { RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };

const view = reviewFailed as unknown as RunView;

test('timeline bars are fractions of the run span; running bars end now', () => {
  const { bars } = timelineBars(
    [
      { id: 1, startedAt: 0, endedAt: 50, state: 'succeeded', durationMs: 50 },
      { id: 2, startedAt: 50, endedAt: null, state: 'running', durationMs: null },
      { id: 3, startedAt: null, endedAt: null, state: 'voided', durationMs: null },
    ],
    100
  );
  assert.deepEqual(bars.get(1), { left: 0, width: 0.5 });
  assert.deepEqual(bars.get(2), { left: 0.5, width: 0.5 });
  assert.equal(bars.has(3), false);
});

test('per-execution changes use its snapshots, else the whole run', () => {
  assert.deepEqual(changeRange({ workspaceStart: { commit: 'a' }, workspaceEnd: { commit: 'b' }, state: 'succeeded' }), { from: 'a', to: 'b', perExecution: true });
  assert.deepEqual(changeRange({ workspaceStart: { commit: 'a' }, workspaceEnd: null, state: 'running' }), { from: 'a', to: null, perExecution: true });
  assert.deepEqual(changeRange({ workspaceStart: null, workspaceEnd: null, state: 'succeeded' }), { from: null, to: null, perExecution: false });
  assert.deepEqual(changeRange(null), { from: null, to: null, perExecution: false });
  assert.deepEqual(changeRange({ workspaceStart: { commit: 'a' }, workspaceEnd: { commit: 'c', base: 'b' }, state: 'succeeded' }), { from: 'b', to: 'c', perExecution: true });
});

test('a read-only node hides its Files tab instead of opening to an empty drawer', () => {
  // A reviewer that never touched the workspace: its end snapshot is its own base.
  assert.equal(executionFilesChanged({ workspaceStart: { commit: 'a' }, workspaceEnd: { commit: 'a', base: 'a' }, state: 'succeeded' }), false);
  assert.equal(executionFilesChanged({ workspaceStart: { commit: 'a' }, workspaceEnd: { commit: 'b', base: 'a' }, state: 'succeeded' }), true);
  // Still running, no end snapshot yet: unknown, so the tab stays.
  assert.equal(executionFilesChanged({ workspaceStart: { commit: 'a' }, workspaceEnd: null, state: 'running' }), true);
  // No per-node snapshots at all: nothing cheap to check, so the tab stays.
  assert.equal(executionFilesChanged({ workspaceStart: null, workspaceEnd: null, state: 'succeeded' }), true);
});

test('the Files button only needs showing when a snapshot actually moved', () => {
  const exec = (workspaceStart: unknown, workspaceEnd: unknown) => ({ workspaceStart, workspaceEnd }) as RunView['executions'][number];
  const withSnapshots = { features: { workspaceSnapshots: true } } as Pick<RunView, 'executions' | 'features'>;
  // A run that never got past authentication: no base/commit recorded at all.
  assert.equal(anyFilesChanged({ ...withSnapshots, executions: [exec(null, null)] }), false);
  // A node that ran but changed nothing: its end commit is its own base.
  assert.equal(anyFilesChanged({ ...withSnapshots, executions: [exec({ commit: 'a' }, { commit: 'a', base: 'a' })] }), false);
  assert.equal(anyFilesChanged({ ...withSnapshots, executions: [exec({ commit: 'a' }, { commit: 'b', base: 'a' })] }), true);
  // This Zeroshot doesn't snapshot per node: nothing cheap to check, so don't hide the button.
  assert.equal(anyFilesChanged({ executions: [], features: { exactTimes: false, toolCalls: false, providerCost: false, workspaceSnapshots: false } }), true);
});

test('only transport failures offer a retry', () => {
  assert.equal(retryable(400), false);
  assert.equal(retryable(503), false);
  assert.equal(retryable(502), true);
  assert.equal(retryable(0), true);
});

test('steer delivery statuses read as sentences', () => {
  // Delivered and queued messages show in the chat itself; only a problem is said under the composer.
  assert.equal(steerStatusText({ status: 'delivered' }), null);
  assert.equal(steerStatusText({ status: 'queued', reason: 'codex turn' }), null);
  assert.equal(steerStatusText({ status: 'rejected', reason: 'turn ended' })?.text, 'Rejected: turn ended.');
});

test('node routes resolve by round and attempt, and say when a visit does not exist', () => {
  const code = view.executions.filter((e) => e.node === 'code');
  assert.equal(resolveVisit(view, { node: 'code', round: 2 }).execution?.id, code.find((e) => e.round === 2)?.id);
  assert.equal(resolveVisit(view, { node: 'code' }).execution?.id, 26);
  const missing = resolveVisit(view, { node: 'code', round: 99 });
  assert.equal(missing.missing, true);
  assert.equal(resolveVisit(view, { node: 'code', execution: 1 }).missing, true, 'execution 1 is not a visit of code');
});

test('node executions and the default (latest) one', () => {
  const { list, current } = nodeExecutions(view, 'code');
  assert.deepEqual(
    list.map((e) => e.id),
    [3, 9, 15, 21, 26]
  );
  assert.equal(current?.id, 26);
  assert.equal(nodeExecutions(view, 'code', 9).current?.id, 9);
  assert.equal(nodeExecutions(view, 'code', 999).current?.id, 26);
  assert.equal(nodeExecutions(view, 'nope').current, undefined);
});

test('j/k step through executions in order and stop at the ends', () => {
  assert.equal(stepExecution(view, undefined, 1)?.id, 1);
  assert.equal(stepExecution(view, undefined, -1)?.id, 26);
  assert.equal(stepExecution(view, 3, 1)?.id, 4);
  assert.equal(stepExecution(view, 1, -1)?.id, 1);
  assert.equal(stepExecution(view, 26, 1)?.id, 26);
});

test('run elapsed ticks for live runs and stops at the end', () => {
  assert.equal(runElapsed({ startedAt: 100, endedAt: null, lastActivityAt: 150, status: 'running' }, 1000), 900);
  assert.equal(runElapsed({ startedAt: 100, endedAt: 400, lastActivityAt: 150, status: 'failed' }, 1000), 300);
  assert.equal(runElapsed({ startedAt: 100, endedAt: null, lastActivityAt: 150, status: 'stalled' }, 1000), 50);
  assert.equal(runElapsed({ startedAt: null, endedAt: null, lastActivityAt: null, status: 'running' }, 1000), null);
});

test('event stream reconnect backoff', () => {
  assert.deepEqual([0, 1, 2, 3, 10].map(backoffDelay), [1000, 2000, 4000, 8000, 30000]);
});

test('execution notes: plain-word errors, the failure cause on the culprit, first line of rejections', () => {
  assert.equal(errorWords({ code: 'timeout', reason: 'declared_failure' } as never), 'timed out');
  assert.equal(errorWords({ code: 'crash', reason: 'exit_status' } as never), 'crashed: exit status');
  const culprit = view.executions.find((e) => e.id === 26)!;
  assert.equal(rowNote(culprit, view).text, 'usage limit');
  const other = view.executions.find((e) => e.id === 20)!;
  assert.equal(rowNote(other, view).text, 'crashed');
  const rejected = view.executions.find((e) => e.state === 'rejected')!;
  assert.ok(rowNote(rejected, view).text.length > 0);
  assert.ok(!rowNote(rejected, view).text.includes('\n'));
  const accepted = view.executions.find((e) => e.state === 'accepted')!;
  assert.equal(rowNote(accepted, view).text, '', 'the verdict is the state; no second copy');
});

test('a sent message shows until the transcript records it', () => {
  const sent = [{ text: 'Also check logging', at: 1, status: 'delivered' as const }];
  assert.equal(pendingSent(sent, []).length, 1);
  assert.equal(pendingSent(sent, [{ kind: 'steer', at: 2, text: 'Also check logging', subagent: false }] as never).length, 0);
  assert.equal(pendingSent(sent, [{ kind: 'steer', at: 2, text: 'Also check logging\n\n[queued]', subagent: false }] as never).length, 0);
});

test('a recorded steer item says its delivery status', () => {
  assert.equal(steerTag('Also check logging'), null);
  assert.equal(steerTag('Also check logging\n\n[queued]'), 'queued');
  assert.equal(steerTagLabel(null).text, 'sent');
  assert.equal(steerTagLabel('queued').text, 'queued for its next turn');
  assert.equal(steerTagLabel('queued', 'copilot').text, 'queued for its next tool call');
  const rejected = steerTagLabel('rejected');
  assert.equal(rejected.text, 'not delivered: rejected');
  assert.equal(rejected.bad, true);
});

// pb is the ProgramBench runner: a benchmark run's cost is the one pb recorded.
test("a pb run's cost is pb's own; Zeroshot's estimate is named in the tooltip", () => {
  const title = costTitle({ costEstimated: false, costSource: 'pb', zeroshotCostUsd: 1.256, zeroshotCostEstimated: true });
  assert.match(title, /^pb's recorded cost/);
  assert.match(title, /Zeroshot's own estimate: ~\$1\.26/);
  assert.match(costTitle({ costEstimated: true }), /^Estimated from tokens/);
});

test('Zeroshot 16.1 message statuses: sent, received, not received, run-wide or to one agent', () => {
  assert.equal(steerTag('Use tabs.\n\n[received, to all agents]'), 'received, to all agents');
  assert.deepEqual(steerTagLabel('received, to all agents'), { text: 'received · sent to all agents' });
  assert.deepEqual(steerTagLabel('received'), { text: 'received' });
  assert.deepEqual(steerTagLabel('sent'), { text: 'sent, not received yet' });
  assert.deepEqual(steerTagLabel('not received'), { text: 'not received', bad: true });
  // The receipt is a success: nothing to say under the composer, the message shows in the chat.
  assert.equal(steerStatusText({ status: 'sent', scope: 'run', steerId: 's1', receivedExecutions: 0 }), null);
  // One mapping for the chip and the composer line.
  assert.deepEqual(steerStatus('rejected', 'turn ended'), { tone: 'bad', chip: 'not delivered: rejected', line: 'Rejected: turn ended.' });
  assert.equal(pendingSent([{ text: 'Use tabs.', at: 1, status: 'sent, to all agents' }], [{ kind: 'steer', at: 2, text: 'Use tabs.\n\n[sent, to all agents]', subagent: false }] as never).length, 0);
});

test('the composer says who a message reaches: this agent with --node, else every agent of the run', () => {
  assert.equal(steerScope({ capabilities: ['steer', 'steer_node'] }), 'node');
  assert.equal(steerScope({ capabilities: ['steer'] }), 'run');
  assert.equal(steerScopeNote('node', 'worker'), 'To this agent (worker) only');
  assert.equal(steerScopeNote('run', 'worker'), 'To all agents in this run, not only this one');
});

test('a model or timeout fix is a plain replay when Zeroshot cannot replay with changes (16.1)', () => {
  assert.equal(effectiveAction('replay_timeout', ['steer']), 'replay');
  assert.equal(effectiveAction('replay_model', []), 'replay');
  assert.equal(effectiveAction('replay_timeout', ['replay']), 'replay_timeout');
  assert.equal(effectiveAction('resume', []), 'resume');
});

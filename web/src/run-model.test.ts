import { test } from 'node:test';
import assert from 'node:assert/strict';
import { canReplay, copilotCreditsUsd, failureModel, harnessLabel, idTime, nodeFacts, nodeLoop, primaryLabel, replayFocus, resumeLabel, runHeaderModel, runLink, secondaryActions } from './run-model';
import { isStalled, isStopped } from './status';
import type { Execution, RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };

const view = reviewFailed as unknown as RunView;

test('replay is offered on a finished run of a Zeroshot with replay, never on a slice run or a failed one', () => {
  const done = { ...view, status: 'succeeded', failure: null, local: null, capabilities: ['replay'], context: null } as unknown as RunView;
  assert.equal(canReplay(done), true);
  assert.equal(canReplay({ ...done, capabilities: [] }), false, 'stock 16.1 only resumes');
  assert.equal(canReplay({ ...done, context: { kind: 'slice' } } as unknown as RunView), false);
  assert.equal(canReplay(view), false, 'a failed run has its own card');
});

test('run ids carry their start time when they are UUIDv7', () => {
  assert.equal(idTime('01900000-0000-7000-8000-000000000000'), 0x019000000000);
  assert.equal(idTime('00000000-0000-7000-8000-000000000000'), null, 'before 2017: not a timestamp');
  assert.equal(idTime('not-a-uuid'), null);
  assert.deepEqual(runLink(undefined, 't', 'not-a-uuid', 0), { label: 'run', title: 'Run\nnot-a-uuid' });
});

test('header model: a stop in progress shows as stopping; no usage hides the token figure', () => {
  const running = { ...view, status: 'running' } as RunView;
  assert.equal(runHeaderModel(running, true).shownStatus, 'stopping');
  assert.equal(runHeaderModel(running, false).shownStatus, 'running');
  const empty = runHeaderModel({ ...view, usage: undefined, nodeUsage: undefined, costUsd: 0, costEstimated: true } as unknown as RunView, false);
  assert.equal(empty.noTokens, true);
  assert.equal(empty.showCost, false, 'an estimated zero says nothing');
});

test('failure vocabulary: stopped and stalled', () => {
  assert.equal(isStopped({ class: 'stopped' } as never), true);
  assert.equal(isStopped({ class: 'agent' } as never), false);
  assert.equal(isStalled({ reason: 'controller_not_running', cause: '' } as never), true);
  assert.equal(isStalled({ reason: 'x', cause: 'controller lost' } as never), true);
  assert.equal(isStalled({ reason: 'review_failed', cause: 'usage limit' } as never), false);
});

test('failure model over the review-failed fixture', () => {
  const failure = view.failure!;
  const before = failureModel(failure, view, failure.resetsAt! - 1);
  assert.equal(before.waiting, true, 'the usage limit has not reset yet');
  assert.equal(before.verb, 'Failed');
  assert.equal(before.culprit?.id, 26);
  assert.deepEqual(before.secondary, ['replay'], 'resume is the primary');
  assert.match(before.titleTip, /^Agent: /);
  assert.ok(before.latestRound.every((f) => f.round === before.latestRound[0].round));
  assert.equal(failureModel(failure, view, failure.resetsAt! + 1).waiting, false);
});

test('failure actions: labels, secondary actions and which settings a replay opens', () => {
  assert.equal(resumeLabel(false), 'Start over');
  assert.equal(primaryLabel('replay', { node: 'code' }, true), 'Start again from code…');
  assert.equal(primaryLabel('replay', { node: 'code' }, true, true), 'Replay with changes from code…');
  assert.equal(primaryLabel('resume', { node: 'code' }, false), 'Start over');
  assert.deepEqual(secondaryActions({ actions: ['replay', 'resume'], primary: 'replay_model' }), ['resume']);
  assert.deepEqual(replayFocus('replay_timeout', 'code'), { node: 'code', field: 'timeout' });
  assert.equal(replayFocus('replay', 'code'), null, 'a plain replay starts collapsed');
  assert.equal(replayFocus('replay_model', null), null);
});

test('node facts: model first; a reused execution links its original instead of duration and cost', () => {
  const current = view.executions.find((e) => e.id === 26)!;
  const info = view.graphNodes.find((n) => n.name === 'code');
  const facts = nodeFacts({ current, info, visits: 5, elapsed: 1000, originalHref: null });
  assert.equal(facts[0].key, 'm');
  assert.ok(facts.some((f) => f.key === 't'));
  assert.ok(!facts.some((f) => f.key === 'o'), 'the round picker names the round');
  const reused = nodeFacts({ current: { ...current, reused: true } as Execution, info, visits: 1, elapsed: null, originalHref: '/x' });
  assert.deepEqual(
    reused.map((f) => f.key),
    ['m', 'r', ...(current.round > 1 ? ['o'] : []), ...(current.attempt > 1 ? ['a'] : [])]
  );
  assert.deepEqual(reused[1].link, { href: '/x', label: 'open original ›' });
  const timedOut = nodeFacts({ current: { ...current, state: 'failed', error: { code: 'timeout' } } as Execution, info, visits: 5, elapsed: 1000, originalHref: null });
  const t = timedOut.find((f) => f.key === 't')!;
  assert.equal(t.bad, true);
  assert.equal(t.suffix, ' (limit)');
});

test('node loop and harness names', () => {
  assert.equal(nodeLoop(view, undefined), null);
  const inLoop = view.graphNodes.find((n) => n.loops.length > 0);
  if (inLoop) assert.equal(nodeLoop(view, inLoop)?.name, inLoop.loops[inLoop.loops.length - 1]);
  assert.equal(harnessLabel('copilot'), 'Copilot');
  assert.equal(harnessLabel('claude'), 'Claude');
  assert.equal(harnessLabel(null), 'Claude');
  assert.equal(copilotCreditsUsd(250), 2.5);
});

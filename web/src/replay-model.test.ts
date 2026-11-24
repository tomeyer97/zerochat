import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  applyEdits,
  checkpointRound,
  editableNodes,
  failedHereNote,
  findNode,
  nodeEditChanged,
  originalValues,
  parseJsonField,
  preselectCheckpoint,
  replayRequest,
  replayVerbs,
  resumeText,
  rollbackText,
  startAgainStep,
  validationLines,
  withLastRoundBump,
} from './replay-model';
import type { Checkpoint, GraphNodeInfo, JsonObject, RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };

const view = reviewFailed as unknown as RunView;
const cp = (sequence: number, node: string, loopIterations: number[] = [], createdAt = sequence): Checkpoint => ({
  checkpointId: `c${sequence}`,
  sequence,
  node,
  mapIndices: [],
  loopIterations,
  createdAt,
});

test('preselects the failed node, else its enclosing group, for the failed round', () => {
  // failure.node = code (round 4), inside parallel_reviews, inside change_loop.
  const list = [cp(1, 'worker'), cp(2, 'parallel_reviews', [1]), cp(3, 'parallel_reviews', [4]), cp(4, 'review_repair_heavy', [3])];
  assert.equal(preselectCheckpoint(list, view)?.checkpointId, 'c3');
  assert.equal(preselectCheckpoint([cp(1, 'worker'), cp(2, 'code', [3]), cp(5, 'code', [4])], view)?.checkpointId, 'c5');
  assert.equal(preselectCheckpoint([], view), undefined);
  const unrelated = [cp(1, 'x', [], 1), cp(2, 'y', [], view.failure!.at! + 1)];
  assert.equal(preselectCheckpoint(unrelated, view)?.checkpointId, 'c1');
});

test('editable nodes are the agents from the boundary on', () => {
  const names = editableNodes('parallel_reviews', view.graphNodes).map((n) => n.name);
  assert.ok(names.includes('code'));
  assert.ok(names.includes('bugfinder'));
  assert.ok(!names.includes('worker'));
});

test('edits only produce the parts that changed', () => {
  const original = {
    instructions: (n: string) => view.graphNodes.find((g) => g.name === n)?.instructions ?? null,
    model: (n: string) => view.graphNodes.find((g) => g.name === n)?.model ?? null,
  };
  assert.deepEqual(applyEdits(view.graph, view.runtime, { code: { model: original.model('code')! } }, original), {
    graph: undefined,
    runtime: undefined,
  });
  const out = applyEdits(view.graph, view.runtime, { code: { model: 'claude-sonnet-5', instructions: 'Be strict.' } }, original);
  const modelOf = (runtime: JsonObject | null | undefined, name: string) => ((runtime!.nodes as JsonObject)[name] as JsonObject).model;
  assert.equal(modelOf(out.runtime, 'code'), 'claude-sonnet-5');
  assert.equal(modelOf(view.runtime, 'code'), 'claude-haiku-4-5-20251001', 'original untouched');
  assert.equal(findNode(out.graph, 'code')?.instructions, 'Be strict.');
});

test('advanced JSON fields', () => {
  assert.deepEqual(parseJsonField('', {}, 'Graph'), {});
  assert.deepEqual(parseJsonField('{"a":1}', { a: 1 }, 'Graph'), {});
  assert.deepEqual(parseJsonField('{"a":2}', { a: 1 }, 'Graph'), { value: { a: 2 } });
  assert.match(parseJsonField('{', {}, 'Graph').error!, /Graph is not valid JSON/);
});

test('validation lines from varied responses', () => {
  assert.deepEqual(validationLines({ valid: false, errors: ['a', { path: 'root.x', message: 'bad' }] }), ['a', 'root.x: bad']);
  assert.deepEqual(validationLines({ valid: true }), []);
  assert.deepEqual(validationLines({ valid: false, conflict: { code: 'completed_node_changed', node: 'reviewer', field: 'attempts', message: 'cannot change attempts' } }), [
    'reviewer attempts: cannot change attempts',
  ]);
});

test('findNode finds a graph node by name, depth first', () => {
  const graph: JsonObject = {
    root: {
      kind: 'seq',
      children: [
        { kind: 'step', name: 'a', x: 1 },
        { kind: 'loop', name: 'l', body: { kind: 'step', name: 'a', x: 2 } },
      ],
    },
  };
  assert.equal(findNode(graph, 'a')?.x, 1);
  assert.equal(findNode(graph, 'l')?.kind, 'loop');
  assert.equal(findNode({ name: 'a' }, 'a'), undefined, 'a name without a kind is not a node');
  assert.equal(findNode(graph, 'missing'), undefined);
});

test('a checkpoint in the last round of its loop starts the loop one round higher', () => {
  // code is inside change_loop (max 10).
  assert.deepEqual(checkpointRound(cp(1, 'code', [10]), view.graphNodes), { loop: 'change_loop', round: 10, max: 10 });
  assert.equal(checkpointRound(cp(1, 'code', [9]), view.graphNodes), null);
  assert.equal(checkpointRound(cp(1, 'worker', [10]), view.graphNodes), null, 'not in a loop');
  assert.deepEqual(checkpointRound(cp(1, 'change_loop', [10]), view.graphNodes)?.loop, 'change_loop', 'the loop itself');
  const last = { loop: 'change_loop', max: 10 };
  assert.deepEqual(withLastRoundBump({}, last), { change_loop: { maxRounds: '11' } });
  assert.deepEqual(withLastRoundBump({ change_loop: { maxRounds: '4' } }, last), { change_loop: { maxRounds: '4' } }, 'the person chose');
  assert.deepEqual(withLastRoundBump({}, null), {});
});

test('a node edit is a change only where it differs from the graph', () => {
  const node = { name: 'n', model: 'm', instructions: 'i', timeoutMs: 30000 } as GraphNodeInfo;
  assert.equal(nodeEditChanged({ model: 'm', instructions: 'i', timeout: '30' }, node), false);
  assert.equal(nodeEditChanged({ model: 'other' }, node), true);
  assert.equal(nodeEditChanged({ timeout: '31' }, node), true);
  assert.equal(nodeEditChanged({ timeout: '' }, node), true, 'cleared');
});

test('the replay request carries only what changed, or the problem that stops it', () => {
  const original = originalValues(view.graphNodes);
  const base = { view, checkpointId: 'c1', advanced: null, validateOnly: true };
  assert.deepEqual(replayRequest({ ...base, checkpointId: undefined, edits: null }), { tone: 'bad', lines: ['Choose a checkpoint first.'] });
  assert.deepEqual(replayRequest({ ...base, edits: null }), { checkpointId: 'c1', validateOnly: true });
  assert.deepEqual(replayRequest({ ...base, edits: { nodes: {}, original, title: ` ${view.title ?? ''} ` } }), { checkpointId: 'c1', validateOnly: true }, 'same title');
  const retitled = replayRequest({ ...base, edits: { nodes: { code: { instructions: 'Be strict.' } }, original, title: 'Again' } });
  assert.ok('checkpointId' in retitled);
  assert.equal(retitled.title, 'Again');
  assert.equal(findNode(retitled.graph as JsonObject, 'code')?.instructions, 'Be strict.');
  assert.equal(retitled.runtime, undefined);
  const advanced = { graph: '', runtime: '{', input: '' };
  assert.deepEqual(replayRequest({ ...base, edits: null, advanced }), {
    tone: 'bad',
    lines: [parseJsonField('{', null, 'The runtime JSON').error!],
    field: 'runtime',
  });
  assert.deepEqual(replayRequest({ ...base, edits: null, advanced: { graph: '', runtime: '', input: '{"x":1}' } }), { checkpointId: 'c1', initialInput: { x: 1 }, validateOnly: true });
});

// ---------- F01/F02/F13: one verb per operation; starting again asks first ----------

test('starting again from a checkpoint cannot send without the confirm step', async () => {
  let sent = 0;
  const send = async () => {
    sent += 1;
  };
  // A confirm with no ask before it (the dialog's list stage) sends nothing.
  assert.equal(await startAgainStep('choose', 'confirm', send), 'choose');
  assert.equal(sent, 0);
  // The dialog's button only asks.
  const asking = await startAgainStep('choose', 'ask', send);
  assert.equal(asking, 'confirm');
  assert.equal(sent, 0, 'asking sends nothing');
  // Cancel goes back to the list and sends nothing.
  assert.equal(await startAgainStep(asking, 'cancel', send), 'choose');
  assert.equal(sent, 0, 'cancelling sends nothing');
  // Only the confirm after an ask sends, once.
  assert.equal(await startAgainStep(asking, 'confirm', send), 'choose');
  assert.equal(sent, 1);
});

test('a refused start keeps the confirm step and reports the error', async () => {
  await assert.rejects(
    startAgainStep('confirm', 'confirm', async () => {
      throw new Error('refused');
    }),
    /refused/
  );
});

test('one verb in button, title, confirm, notice and lineage', () => {
  const stock = replayVerbs(false, 'acceptance', 'In parallel (round 2)');
  assert.equal(stock.button, 'Start again from acceptance…');
  assert.equal(stock.title, 'Start again from a checkpoint');
  assert.equal(stock.next, 'Start again…');
  assert.equal(stock.confirmTitle, 'Start again from In parallel (round 2)?');
  assert.equal(stock.confirm, 'Start again');
  assert.equal(stock.started, 'Started again from the In parallel (round 2) checkpoint.');
  for (const text of [stock.button, stock.title, stock.next, stock.confirmTitle, stock.confirm]) {
    assert.match(text, /^Start again/, text);
    assert.doesNotMatch(text, /Replay|Resume/, text);
  }
  const edit = replayVerbs(true, 'acceptance', 'In parallel (round 2)');
  for (const text of [edit.button, edit.title, edit.next, edit.confirmTitle, edit.confirm, edit.started]) assert.match(text, /^Replay with changes/, text);
  for (const text of [edit.lineageFrom, edit.lineageAs]) assert.match(text, /^Replayed with changes/, text);
  assert.equal(replayVerbs(false).button, 'Start again from…');
  assert.equal(replayVerbs(true).button, 'Replay with changes…');
});

test('the confirm step names what is rolled back', () => {
  const text = rollbackText('In parallel (round 2)');
  assert.match(text, /In parallel \(round 2\) checkpoint/);
  assert.match(text, /discarded/);
});

test('the Resume dialog says what Resume does and offers Replay with changes only when it exists', () => {
  assert.doesNotMatch(resumeText(false), /Replay/);
  assert.match(resumeText(false), /where this one stopped/);
  assert.match(resumeText(true), /use Replay with changes\.$/);
});

test('the failed checkpoint names the node the button names', () => {
  assert.equal(failedHereNote({ status: 'failed', failure: { node: 'acceptance' } as RunView['failure'] }), 'acceptance failed here');
  assert.equal(failedHereNote({ status: 'stopped', failure: { node: 'worker' } as RunView['failure'] }), 'worker interrupted here');
  assert.equal(failedHereNote({ status: 'failed', failure: null }), 'failed here');
});

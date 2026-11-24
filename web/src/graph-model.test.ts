import { test } from 'node:test';
import assert from 'node:assert/strict';
import ELK from 'elkjs/lib/elk.bundled.js';
import { buildElkGraph, buildFlowModel, conditionLabel, guardNames, humanId, readElkLayout, showEveryModel, type FlowItem } from './graph-model';
import type { RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };
import localSucceeded from './fixtures/local-succeeded.json' with { type: 'json' };

// review-failed.json: a run that failed in round 4 of its loop, on `code`'s verdict. Its graph:
//
//   worker ─┬─ errored ───▶ fail worker_failed
//           └─ otherwise ─▶ loop change_loop (max 10) ── when it runs out ─▶ fail change_attempts_exhausted
//
//   inside change_loop:
//     par parallel_reviews: acceptance, code, futureproof, mockability
//       ├─ any errored ──────▶ fail review_failed   (an exit is drawn outside the loop it is written in)
//       ├─ otherwise ────────▶ review_repair        (dead end: the next round)
//       └─ all 3 accepted ───▶ bugfinder            (a par with one branch, heavy_reviews)
//            ├─ errored ─────▶ fail review_failed_heavy (reason review_failed too)
//            ├─ otherwise ───▶ review_repair_heavy  (dead end: the next round; it ran)
//            └─ accepted ────▶ deliver
//                 ├─ delivery: ci failed or conflict ─▶ delivery_repair (dead end)
//                 ├─ errored ──▶ fail delivery_failed
//                 └─ otherwise ▶ succeed done
//
// The choices between them (worker_result, light_result, final_result, delivery_result) and the
// seqs (run, changes, change_iteration, bugfinder_stage, merge_delivery) are not drawn.
// local-succeeded.json: replay ─ repair ─┬─ fail replay_failed / succeed replay_done (reached).
const failed = reviewFailed as unknown as RunView;
const succeeded = localSucceeded as unknown as RunView;

test('agents are cards, loops and parallel groups containers; choices fold into edges', () => {
  const model = buildFlowModel(failed);
  const kind = (id: string) => model.items.find((i) => i.id === id)?.kind;
  assert.equal(kind('worker'), 'agent');
  assert.equal(kind('bugfinder'), 'agent');
  assert.equal(kind('change_loop'), 'group');
  assert.equal(kind('parallel_reviews'), 'group');
  // Choices are edges now, seq nodes are transparent, a one-branch par is just its branch.
  assert.equal(kind('worker_result'), undefined);
  assert.equal(kind('light_result'), undefined);
  assert.equal(kind('run'), undefined);
  assert.equal(kind('change_iteration'), undefined);
  assert.equal(kind('heavy_reviews'), undefined);
  assert.ok(!model.items.some((i) => i.kind === 'decision'));
  const parent = (id: string) => model.items.find((i) => i.id === id)?.parent;
  assert.equal(parent('acceptance'), 'parallel_reviews');
  assert.equal(parent('parallel_reviews'), 'change_loop');
  assert.equal(parent('bugfinder'), 'change_loop');
  assert.equal(parent('review_repair'), 'change_loop');
  assert.equal(parent('worker'), undefined);
  // A succeed/fail always ends the whole run, never just the loop/parallel group it is written
  // inside of, so it is never drawn as that group's child.
  assert.equal(parent('review_failed'), undefined);
});

test('a loop that went around draws an edge back into it from its dead end', () => {
  const model = buildFlowModel(failed);
  const loop = model.items.find((i) => i.id === 'change_loop')!;
  // Every branch in the body with nothing further in the static graph: the loop's real "continue".
  assert.deepEqual(new Set(loop.loopBackFrom), new Set(['review_repair', 'review_repair_heavy', 'delivery_repair']));
  const back = model.edges.filter((e) => e.loopBack);
  // Only the one that actually ran gets drawn; the others never executed in this run.
  assert.deepEqual(
    back.map((e) => [e.source, e.target]),
    [['review_repair_heavy', 'change_loop']]
  );
  assert.ok(back[0].taken);
});

test('only the reached exit is drawn; the others are listed on their source', () => {
  const model = buildFlowModel(failed);
  const exits = model.items.filter((i) => i.kind === 'fail' || i.kind === 'succeed').map((i) => i.id);
  assert.deepEqual(exits, ['review_failed']);
  const item = (id: string) => model.items.find((i) => i.id === id)!;
  assert.deepEqual(item('worker').otherExits, [{ id: 'worker_failed', kind: 'fail', condition: 'errored' }]);
  assert.deepEqual(
    item('change_loop').otherExits?.map((x) => x.id),
    ['change_attempts_exhausted']
  );
  assert.deepEqual(
    item('deliver').otherExits?.map((x) => x.id),
    ['delivery_failed', 'done']
  );
  for (const e of model.edges)
    assert.ok(
      model.items.some((i) => i.id === e.target),
      `${e.target} drawn`
    );
});

test('edges carry humanised conditions; a branch label is worth drawing', () => {
  const model = buildFlowModel(failed);
  const edge = (s: string, t: string) => model.edges.find((e) => e.source === s && e.target === t);
  const w = edge('worker', 'change_loop')!;
  assert.equal(w.label, 'otherwise');
  assert.equal(w.branch, false, 'worker has one way out once its error exit is folded');
  assert.equal(edge('parallel_reviews', 'review_failed')?.label, 'any errored');
  assert.ok(edge('parallel_reviews', 'review_failed')?.branch);
  assert.match(edge('parallel_reviews', 'bugfinder')?.label ?? '', /^all \d accepted$/);
  assert.equal(edge('bugfinder', 'deliver')?.label, 'accepted');
  assert.match(edge('parallel_reviews', 'bugfinder')?.title ?? '', /acceptance accepted/);
});

test('state, rounds and the reached fail exit come from the run', () => {
  const model = buildFlowModel(failed);
  const item = (id: string) => model.items.find((i) => i.id === id)!;
  assert.equal(item('bugfinder').state, 'rejected');
  assert.equal(item('code').state, 'failed');
  assert.equal(item('code').loop?.name, 'change_loop');
  assert.equal(item('code').loop?.max, 10);
  assert.equal(item('change_loop').iteration, 4);
  assert.equal(item('delivery_repair').state, 'idle');
  // Two exits share `review_failed`; the one whose condition reads the failing reviewers wins.
  assert.equal(item('review_failed').reached, true);
  assert.ok(model.edges.find((e) => e.source === 'worker' && e.target === 'change_loop')?.taken);
  assert.ok(model.edges.find((e) => e.source === 'parallel_reviews' && e.target === 'review_failed')?.taken);
  assert.ok(!model.edges.find((e) => e.target === 'delivery_repair')?.taken);
});

test('a succeeded run reaches its succeed exit', () => {
  const model = buildFlowModel(succeeded);
  assert.equal(model.items.find((i) => i.id === 'replay_done')?.reached, true);
  assert.equal(model.items.find((i) => i.id === 'repair')?.state, 'succeeded');
  assert.equal(
    model.items.find((i) => i.id === 'replay_failed'),
    undefined
  );
});

test('a live run draws no exit', () => {
  const model = buildFlowModel({ ...failed, status: 'running', terminal: null, failure: null } as unknown as RunView);
  assert.ok(!model.items.some((i) => i.kind === 'fail' || i.kind === 'succeed'));
});

test('condition labels', () => {
  assert.equal(conditionLabel({ kind: 'in', labels: ['ci_failed', 'conflict'], value: { field: 'delivery', name: 'deliver', source: 'signal' } }).label, 'deliver delivery: ci failed or conflict');
  const accepted = (name: string) => ({ kind: 'in', labels: ['accepted'], value: { field: 'verdict', name, source: 'signal' } });
  const all = { kind: 'all', guards: [accepted('a'), accepted('b')] };
  assert.equal(conditionLabel(all).label, 'a and b accepted');
  // Two reviewers reads as "both", not machine-phrased "all 2"; three or more keeps the count.
  assert.equal(conditionLabel(all, new Set(['a', 'b'])).label, 'both accepted');
  assert.equal(conditionLabel({ kind: 'all', guards: [accepted('a'), accepted('b'), accepted('c')] }).label, 'all 3 accepted');
  assert.equal(conditionLabel({ kind: 'in', labels: ['timeout'], value: { field: null, name: 'w', source: 'error' } }, new Set(['w'])).label, 'timed out');
  assert.deepEqual(guardNames(all), ['a', 'b']);
  assert.equal(conditionLabel(undefined).label, 'if');
  assert.equal(humanId('change_attempts_exhausted'), 'change attempts exhausted');
});

test('ELK lays out the compound graph with every item placed', async () => {
  const model = buildFlowModel(failed);
  for (const direction of ['RIGHT', 'DOWN'] as const) {
    const layout = await new ELK().layout(buildElkGraph(model, direction));
    const placed = readElkLayout(layout);
    for (const item of model.items) assert.ok(placed.has(item.id), `${item.id} placed`);
    const group = placed.get('parallel_reviews')!;
    const child = placed.get('acceptance')!;
    assert.ok(child.x >= group.x && child.x + child.width <= group.x + group.width + 0.5, 'children inside their group');
  }
});

test('an empty or missing graph is an empty model', () => {
  const model = buildFlowModel({ ...failed, graph: null });
  assert.deepEqual(model, { items: [], edges: [] });
});

test('a group left with nothing inside it is dropped, not drawn as an empty frame', () => {
  const graph = {
    root: {
      kind: 'seq',
      name: 'run',
      children: [
        { kind: 'step', name: 'worker', worker: 'builtin.agent.worker@1' },
        {
          kind: 'choice',
          name: 'worker_result',
          branches: [{ when: { kind: 'in', labels: ['ok'], value: { name: 'worker' } }, node: { kind: 'succeed', name: 'done' } }],
          // The map's only child is a fail: on a non-failed run it is always unreached, so the
          // map would otherwise be drawn as an empty dashed "For each item" box.
          otherwise: { kind: 'map', name: 'retry', body: { kind: 'fail', name: 'retry_invalid', reason: 'retry_invalid' } },
        },
      ],
    },
  };
  const view = { graph, graphNodes: [], nodes: {}, executions: [], status: 'succeeded', terminal: null, failure: null } as never;
  const model = buildFlowModel(view);
  assert.ok(!model.items.some((i) => i.id === 'retry'), 'the empty map group is not drawn');
  assert.ok(!model.edges.some((e) => e.target === 'retry' || e.source === 'retry'));
});

test('every card names its model when the agents use more than one, or on a benchmark run', () => {
  const agent = (id: string, model: string | null): FlowItem => ({ id, kind: 'agent', graphKind: 'step', state: 'succeeded', visited: true, info: { model } as FlowItem['info'] });
  const same = [agent('a', 'claude-sonnet-5'), agent('b', 'claude-sonnet-5')];
  const mixed = [agent('a', 'claude-sonnet-5'), agent('b', 'claude-sonnet-5'), agent('c', 'claude-haiku-4-5')];
  assert.equal(showEveryModel(same, false), false, 'one model for every agent: the run header already names it');
  assert.equal(showEveryModel(mixed, false), true);
  assert.equal(showEveryModel(same, true), true, 'a benchmark run always shows per-node models');
  assert.equal(showEveryModel([agent('a', null)], false), false);
});

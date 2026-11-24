import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildFlowModel, type FlowItem } from './graph-model';
import { READABLE_ZOOM, agentCard, autoViewport, centreClamp, cx, exitsText, focusNode, graphView, groupHeader, toFlowElements, type CardData, type Layout } from './graph-view-model';
import type { Execution, NodeStats, Pressure, RunView } from './types';
import reviewFailed from './fixtures/review-failed.json' with { type: 'json' };

// review-failed.json: the graph is sketched at the top of graph-model.test.ts.
const failed = reviewFailed as unknown as RunView;
const model = buildFlowModel(failed);
const item = (id: string) => model.items.find((i) => i.id === id)!;

const card = (over: Partial<CardData> & { item: FlowItem }): CardData => ({
  direction: 'RIGHT',
  selected: false,
  pressures: [],
  costEstimated: false,
  everyModel: false,
  live: false,
  ...over,
});

test('cx keeps the class names whose condition holds', () => {
  assert.equal(cx('a', false, null, undefined, 'b', ''), 'a b');
});

test('an agent card: state, culprit, selection and visit as classes', () => {
  const code = item('code');
  const c = agentCard(card({ item: code, selected: true, failureExecution: code.latest!.id }), 0);
  assert.equal(c.className, 'flow-card agent state-failed culprit selected');
  assert.equal(agentCard(card({ item: item('delivery_repair') }), 0).className, 'flow-card agent state-idle unvisited');
});

test("an agent card's second line, in order: state word, time and cost, model, runs, attempt", () => {
  const base: FlowItem = { id: 'n', kind: 'agent', graphKind: 'step', state: 'failed', visited: true };
  const stats = { rounds: 3, elapsedMs: 65000, costUsd: 0.5, executions: [1] } as unknown as NodeStats;
  const latest = { id: 1, attempt: 2, model: 'claude-sonnet-5' } as Execution;
  const c = agentCard(card({ item: { ...base, stats, latest, info: { attempts: 3 } as FlowItem['info'] }, everyModel: true }), 0);
  assert.deepEqual(
    c.meta.map((p) => p.key),
    ['state', 'dc', 'm', 'r', 'a']
  );
  assert.equal(c.meta.find((p) => p.key === 'm')?.text, 'sonnet-5');
  assert.equal(c.meta.find((p) => p.key === 'r')?.text, '3 runs');
  assert.equal(c.meta.find((p) => p.key === 'a')?.text, 'attempt 2 of 3');
  assert.match(c.meta.find((p) => p.key === 'dc')!.text, / · /, 'time and cost');
  // The visible card text may shorten the model; its tooltip carries the exact id.
  assert.match(c.title, /^n · Failed\nclaude-sonnet-5 · .* · .*\n/);
  assert.match(c.title, /\nClick to open its chat\.$/);
  // Reused replaces the state word; a running card shows its clock, tinted by the worst limit.
  const reused = agentCard(card({ item: { ...base, latest: { ...latest, reused: true } as Execution } }), 0);
  assert.deepEqual(reused.meta[0], { key: 'reused', className: 'muted', text: 'reused' });
  assert.match(reused.title, /^n · reused from the original run/);
  const pressures = [
    { level: 'warning', text: 'w' },
    { level: 'critical', text: 'Nearly out' },
  ] as Pressure[];
  const running = agentCard(card({ item: { ...base, state: 'running', latest: { ...latest, startedAt: 1000 } as Execution }, pressures }), 4000);
  assert.deepEqual(running.meta[0], { key: 't', className: 'tabular card-live level-critical', text: '3s' });
  assert.match(running.title, /\nNearly out\n/);
});

test('folded exits are listed in the tooltip with their condition', () => {
  assert.equal(exitsText(undefined), '');
  assert.equal(
    exitsText([
      { id: 'worker_failed', kind: 'fail', condition: 'errored' },
      { id: 'done', kind: 'succeed', condition: 'otherwise' },
    ]),
    '\nOther exits: worker failed (if errored), done'
  );
});

test('a group header: caption, and the loop round once it earns a badge', () => {
  const loop = groupHeader(item('change_loop'), false);
  assert.equal(loop.caption, 'Loop');
  assert.deepEqual(loop.round, { text: '· round 4 of 10', pressing: false });
  assert.match(loop.title, /^change_loop: ran 4 of at most 10 rounds\nOther exits: change attempts exhausted$/);
  assert.equal(groupHeader(item('parallel_reviews'), false).caption, 'In parallel');
  const once = { ...item('change_loop'), iteration: 1 };
  assert.equal(groupHeader(once, false).round, null, 'round 1 of a finished loop');
  assert.deepEqual(groupHeader(once, true).round, { text: '· round 1 of 10', pressing: false }, 'round 1 while live');
  assert.equal(groupHeader({ ...once, iteration: 9 }, true).round?.pressing, true, 'one round from the limit');
});

test('flow elements: one node per placed item, typed by kind; edges described for screen readers', () => {
  const placed = new Map(model.items.map((i, n) => [i.id, { x: n * 10, y: 0, width: 300, height: 200 }]));
  const layout: Layout = { placed, width: 1000, height: 400 };
  const context = { direction: 'RIGHT' as const, selectedNode: 'code', pressuresByNode: new Map(), costEstimated: false, everyModel: false, live: false };
  assert.deepEqual(toFlowElements(model, undefined, context), { nodes: [], edges: [] });
  const { nodes, edges } = toFlowElements(model, layout, context);
  assert.equal(nodes.length, model.items.length);
  const node = (id: string) => nodes.find((n) => n.id === id)!;
  assert.equal(node('code').type, 'agent');
  assert.equal(node('code').width, 208, 'a card keeps its own size');
  assert.equal(node('code').data.selected, true);
  assert.equal(node('code').ariaLabel, 'code: Failed. Open its chat.');
  assert.equal(node('change_loop').type, 'group');
  assert.equal(node('change_loop').width, 300, 'a group takes the laid-out size');
  assert.equal(node('review_failed').type, 'exit');
  const edge = edges.find((e) => e.source === 'parallel_reviews' && e.target === 'review_failed')!;
  assert.equal(edge.ariaLabel, 'parallel reviews to review failed (any errored)');
  assert.equal(edge.className, 'taken');
  assert.equal(edges.find((e) => e.data?.loopBack)?.className, 'taken loop-back');
});

test('the node to centre: the open one, else a running one, the failed one, the first agent', () => {
  assert.equal(focusNode(model, 'worker', 'code'), 'worker');
  assert.equal(focusNode(model, undefined, 'code'), 'code');
  assert.equal(focusNode(model, undefined, null), 'worker');
});

test('viewport: the whole graph when readable, else the focus centred at a readable zoom', () => {
  const layout: Layout = { placed: new Map([['a', { x: 1800, y: 0, width: 200, height: 50 }]]), width: 2000, height: 100 };
  assert.equal(graphView(undefined, 1, 500).canvasHeight, 160, 'nothing laid out yet');
  const small: Layout = { placed: new Map(), width: 400, height: 100 };
  const fits = graphView(small, 1, 500);
  assert.deepEqual(fits, { zoom: 1, readable: true, canvasHeight: 116 });
  assert.deepEqual(autoViewport(small, 800, fits, undefined), { x: 200, y: 8, zoom: 1 });
  const wide = graphView(layout, (800 - 16) / 2000, 500);
  assert.equal(wide.readable, false);
  const v = autoViewport(layout, 800, wide, 'a');
  assert.equal(v.zoom, READABLE_ZOOM);
  // `a` sits at the far right: the view stops where the graph's right end meets the canvas edge.
  assert.equal(v.x, 800 - 8 - 2000 * READABLE_ZOOM);
});

test('centreClamp centres what fits and never shows empty canvas beside a larger graph', () => {
  assert.equal(centreClamp(-50, 100, 200), (200 - 100 * READABLE_ZOOM) / 2, 'fits: centred whatever was asked');
  assert.equal(centreClamp(100, 1000, 200), 8, 'not past the start');
  assert.equal(centreClamp(-5000, 1000, 200), 200 - 8 - 1000 * READABLE_ZOOM, 'not past the end');
  assert.equal(centreClamp(-100, 1000, 200), -100);
});

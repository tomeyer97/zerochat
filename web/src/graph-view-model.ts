/**
 * What the run graph (Graph.tsx) draws, as plain data: each card's class, lines and tooltip, the
 * React Flow nodes and edges for a laid-out flow model, and the viewport rules (fit the whole
 * graph, or centre the node that matters at a readable zoom). Graph.tsx only renders these.
 */
import { MarkerType, type Edge, type Node } from '@xyflow/react';
import type { Pressure } from './types';
import { GROUP_KINDS, humanId, itemSize, type Direction, type FlowItem, type FlowItemKind, type FlowModel, type HiddenExit, type Placed } from './graph-model';
import { execStateInfo } from './status';
import { compact, costText, duration, elapsedMs, shortModel, totalTokens } from './format';

/** Class names from the ones whose condition holds. */
export function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ');
}

// ---------- cards ----------

/** What every card is drawn from. */
export type CardData = {
  item: FlowItem;
  direction: Direction;
  selected: boolean;
  pressures: Pressure[];
  costEstimated: boolean;
  /** The model most agents use: a card names its model only when it differs... */
  model?: string;
  /** ...unless every card names its own (mixed models, or a benchmark run: `showEveryModel`). */
  everyModel: boolean;
  /** The execution the run's failure is pinned on. */
  failureExecution?: number | null;
  live: boolean;
};

/** One span of a card's second line. */
export type CardPart = { key: string; text: string; className?: string };

export type AgentCard = { className: string; title: string; meta: CardPart[] };

/** Words only for states that need a second look; success and idle are said by the icon alone. */
const STATE_WORDS = new Set(['rejected', 'failed', 'interrupted', 'voided']);

/** The worst limit pressing on a node. */
export function worstPressure(pressures: Pressure[]): Pressure | undefined {
  return pressures.find((p) => p.level === 'critical') ?? pressures.find((p) => p.level === 'warning');
}

/** An agent card at `now` (a running card's clock ticks). */
export function agentCard(data: CardData, now: number): AgentCard {
  const { item } = data;
  const { latest, stats } = item;
  const running = item.state === 'running';
  const facts: CardFacts = {
    running,
    elapsed: elapsedMs({ live: running, startedAt: latest?.startedAt, recorded: stats?.elapsedMs }, now),
    cost: stats?.costUsd != null ? costText(stats.costUsd, stats.costEstimated ?? data.costEstimated) : null,
    model: latest?.model ?? item.info?.model,
    warn: worstPressure(data.pressures),
    rounds: stats?.rounds ?? 0,
    reused: !!latest?.reused,
  };
  const culprit = !!latest && data.failureExecution === latest.id;
  return {
    className: cx('flow-card agent', `state-${item.state}`, culprit && 'culprit', data.selected && 'selected', !item.visited && 'unvisited'),
    title: `${agentTitle(item, facts)}${exitsText(item.otherExits)}\nClick to open its chat.`,
    meta: agentMeta(data, facts),
  };
}

type CardFacts = {
  running: boolean;
  elapsed: number | null;
  /** The cost as shown, when the node has one. */
  cost: string | null;
  model: string | null | undefined;
  warn: Pressure | undefined;
  rounds: number;
  /** Reused from the original run: it did not actually run here. */
  reused: boolean;
};

/** The card's second line, in the order it shows. */
function agentMeta(data: CardData, f: CardFacts): CardPart[] {
  const { item } = data;
  const parts: CardPart[] = [];
  // Reused replaces the state word: it didn't actually run here, so "Succeeded" would be
  // misleading about what this execution did.
  if (f.reused) parts.push({ key: 'reused', className: 'muted', text: 'reused' });
  else if (STATE_WORDS.has(item.state)) parts.push({ key: 'state', className: 'card-state', text: execStateInfo[item.state].label });
  if (f.running && f.elapsed != null) {
    parts.push({ key: 't', className: cx('tabular card-live', f.warn && `level-${f.warn.level}`), text: duration(f.elapsed) });
  } else if (!f.reused && item.visited && f.elapsed != null) {
    // A finished card said nothing beyond its name: one muted line of what it cost to run
    // (tokens stay in the tooltip).
    const cost = f.cost && f.cost !== '—' ? ` · ${f.cost}` : '';
    parts.push({ key: 'dc', className: 'tabular muted', text: `${duration(f.elapsed)}${cost}` });
  }
  if (f.model && (data.everyModel || (data.model && f.model !== data.model))) parts.push({ key: 'm', text: shortModel(f.model) });
  if (!item.loop && f.rounds > 1) parts.push({ key: 'r', text: `${f.rounds} runs` });
  const latest = item.latest;
  if (latest && latest.attempt > 1) {
    const attempts = item.info?.attempts ?? null;
    parts.push({ key: 'a', text: `attempt ${latest.attempt}${attempts ? ` of ${attempts}` : ''}` });
  }
  return parts;
}

/** The tooltip: what ran, what it took, the loop round and any limit pressing on it. */
function agentTitle(item: FlowItem, f: CardFacts): string {
  const tokens = totalTokens(item.stats?.usage);
  const spent = [
    // The exact id: someone copying it from here gets a model that exists.
    f.model,
    f.elapsed != null && f.elapsed > 0 ? duration(f.elapsed) : null,
    f.cost,
    tokens ? `${compact(tokens)} tokens` : null,
  ];
  return [
    `${item.id} · ${f.reused ? 'reused from the original run' : execStateInfo[item.state].label}`,
    spent.filter(Boolean).join(' · '),
    item.loop && f.rounds > 0 ? `round ${f.rounds} of ${item.loop.max ?? '?'}` : '',
    f.warn ? f.warn.text : '',
  ]
    .filter(Boolean)
    .join('\n');
}

/** The exits folded off the canvas, for the tooltip of the item they leave from. */
export function exitsText(exits: HiddenExit[] | undefined): string {
  if (!exits?.length) return '';
  const exit = (x: HiddenExit) => `${humanId(x.id)}${x.condition && x.condition !== 'otherwise' ? ` (if ${x.condition})` : ''}`;
  return `\nOther exits: ${exits.map(exit).join(', ')}`;
}

export function exitTitle(item: FlowItem): string {
  const fail = item.kind === 'fail';
  const reason = item.failReason && item.failReason !== item.id ? ` (reason ${item.failReason})` : '';
  return `${fail ? 'Fail' : 'Succeed'} exit ${item.id}${reason}: the run ended here`;
}

export type GroupHeader = {
  caption: string;
  title: string;
  /** The loop round badge, when it earns its place. */
  round: { text: string; pressing: boolean } | null;
};

/** A group frame's header: its caption, and for a loop the round it is in. */
export function groupHeader(item: FlowItem, live: boolean): GroupHeader {
  const loop = item.groupKind === 'loop';
  const max = item.maxIterations;
  const iteration = item.iteration ?? 0;
  // The round count is a limit only while the run can still hit it.
  const pressing = live && loop && max != null && iteration >= max - 1 && iteration > 0;
  // Round 1 is the normal case for a finished loop, so a badge there only earns its place from
  // round 2. While live, the current round is exactly what a watcher wants, from round 1:
  // "Loop · round 1 of 3", not just "Loop" until a second round happens.
  const showRound = loop && (iteration > 1 || (live && iteration >= 1));
  return {
    caption: item.groupKind ? GROUP_KINDS[item.groupKind].caption : humanId(item.graphKind),
    title: `${item.id}${loop ? `: ran ${iteration} of at most ${max ?? '?'} rounds` : ''}${exitsText(item.otherExits)}`,
    round: showRound ? { text: `· round ${iteration}${max != null ? ` of ${max}` : ''}`, pressing } : null,
  };
}

// ---------- React Flow elements ----------

export type Layout = { placed: Map<string, Placed>; width: number; height: number };

/** The custom React Flow node type drawing each item kind. */
const NODE_TYPE: Record<FlowItemKind, 'agent' | 'decision' | 'exit' | 'group'> = {
  agent: 'agent',
  decision: 'decision',
  succeed: 'exit',
  fail: 'exit',
  group: 'group',
};

export type EdgeData = { label?: string; title?: string; taken: boolean; branch?: boolean; loopBack?: boolean };

/** What every card shares; each card adds its own item, pressures and selection. */
export type CardContext = Omit<CardData, 'item' | 'selected' | 'pressures'> & {
  selectedNode?: string;
  pressuresByNode: Map<string, Pressure[]>;
};

/** The React Flow nodes and edges of a laid-out model (none until it is laid out). */
export function toFlowElements(model: FlowModel, layout: Layout | undefined, context: CardContext): { nodes: Node<CardData>[]; edges: Edge<EdgeData>[] } {
  if (!layout) return { nodes: [], edges: [] };
  const { selectedNode, pressuresByNode, ...shared } = context;
  const nodes: Node<CardData>[] = [];
  for (const item of model.items) {
    const p = layout.placed.get(item.id);
    if (!p) continue;
    const size = item.kind === 'group' ? { width: p.width, height: p.height } : itemSize(item);
    const agent = item.kind === 'agent';
    nodes.push({
      id: item.id,
      type: NODE_TYPE[item.kind],
      position: { x: p.x, y: p.y },
      width: size.width,
      height: size.height,
      style: { width: size.width, height: size.height },
      zIndex: item.kind === 'group' ? 0 : 2,
      selectable: agent,
      draggable: false,
      connectable: false,
      focusable: agent,
      ariaLabel: agent ? `${humanId(item.id)}: ${execStateInfo[item.state].label}. Open its chat.` : undefined,
      data: {
        item,
        direction: shared.direction,
        selected: item.id === selectedNode,
        pressures: pressuresByNode.get(item.id) ?? [],
        costEstimated: shared.costEstimated,
        model: shared.model,
        everyModel: shared.everyModel,
        failureExecution: shared.failureExecution,
        live: shared.live,
      },
    });
  }
  const edges: Edge<EdgeData>[] = model.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    type: 'labeled',
    zIndex: 1,
    data: { label: e.label, title: e.title, taken: e.taken, branch: e.branch, loopBack: e.loopBack },
    className: `${e.taken ? 'taken' : 'untaken'}${e.loopBack ? ' loop-back' : ''}`,
    markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14, color: e.taken ? 'var(--edge-taken)' : 'var(--edge)' },
    // Screen readers get the drawn picture, in words, not the engine's internal ids.
    ariaLabel: e.loopBack ? e.title : `${humanId(e.source)} to ${humanId(e.target)}${e.label && e.label !== 'otherwise' ? ` (${e.label})` : ''}`,
  }));
  return { nodes, edges };
}

/** The node worth centring when the whole graph is too small to read: open, running, failed, first agent. */
export function focusNode(model: FlowModel, selectedNode: string | undefined, failedNode: string | null | undefined): string | undefined {
  return selectedNode ?? model.items.find((i) => i.state === 'running')?.id ?? failedNode ?? model.items.find((i) => i.kind === 'agent')?.id;
}

// ---------- viewport ----------

/**
 * Below this zoom the cards are unreadable; then the node that matters is centred at
 * `READABLE_ZOOM` instead. At 0.55 a graph could open fitted at ~64%, its 13.5px node names
 * drawn at ~9px; at 0.8 a name is at least ~11px.
 */
export const MIN_READABLE_ZOOM = 0.8;
export const READABLE_ZOOM = 0.85;

/** The tallest the canvas gets: 60% of the window, between 240 and 560px. */
export function maxCanvasHeight() {
  return typeof window === 'undefined' ? 520 : Math.max(240, Math.min(560, Math.round(window.innerHeight * 0.6)));
}

/**
 * Margin (px) kept between the graph and the canvas edge: `CANVAS_PAD` of
 * components/graph-layout.ts, which cannot be imported here (it starts the ELK worker).
 */
const PAD = 8;

/** How the graph is shown: at `zoom` (the zoom that fits it whole) when readable, and the canvas height. */
export type GraphView = { zoom: number; readable: boolean; canvasHeight: number };

/** `zoom`: the zoom that fits the whole layout in the canvas (`fitZoom`), 1 until it is laid out and measured. */
export function graphView(layout: Layout | undefined, zoom: number, maxHeight: number): GraphView {
  const readable = zoom >= MIN_READABLE_ZOOM;
  // Too small to read whole: show it at a readable zoom, but never taller than the graph needs.
  const canvasHeight = layout ? Math.min(maxHeight, Math.ceil(layout.height * (readable ? zoom : READABLE_ZOOM) + 2 * PAD)) : 160;
  return { zoom, readable, canvasHeight };
}

export type Viewport = { x: number; y: number; zoom: number };

/**
 * One axis of a centred view at `READABLE_ZOOM`: the graph `size` long in a `canvas` that long.
 * A graph that fits is centred; a larger one starts at `centred`, but never so far that empty
 * canvas shows beside either of its ends.
 */
export function centreClamp(centred: number, size: number, canvas: number): number {
  const drawn = size * READABLE_ZOOM;
  if (drawn + 2 * PAD <= canvas) return (canvas - drawn) / 2;
  return Math.min(PAD, Math.max(canvas - PAD - drawn, centred));
}

/** The view to show: the whole graph centred across when readable, else the focus centred. */
export function autoViewport(layout: Layout, width: number, view: GraphView, focusId: string | undefined): Viewport {
  if (view.readable) return { x: Math.max(PAD, (width - layout.width * view.zoom) / 2), y: PAD, zoom: view.zoom };
  const placed = focusId ? layout.placed.get(focusId) : undefined;
  const cx = placed ? placed.x + placed.width / 2 : layout.width / 2;
  const cy = placed ? placed.y + placed.height / 2 : layout.height / 2;
  return {
    x: centreClamp(width / 2 - cx * READABLE_ZOOM, layout.width, width),
    y: centreClamp(view.canvasHeight / 2 - cy * READABLE_ZOOM, layout.height, view.canvasHeight),
    zoom: READABLE_ZOOM,
  };
}

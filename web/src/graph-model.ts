import type { ElkNode } from 'elkjs/lib/elk-api';
import type { Execution, GraphNodeInfo, NodeStats, RunView } from './types';
import { nodeState, type NodeState } from './status';
import { conditionLabel, guardNames, humanId, shortLabel } from './condition-label';

export { conditionLabel, guardNames, humanId, shortLabel } from './condition-label';

/**
 * A run's graph as a flow chart of what does the work:
 * - agent nodes (`step`, `verifier`, any leaf that executes) are cards;
 * - `loop`, `par` and `map` are containers (groups) that edges enter and leave as a whole
 *   (a `par` with a single branch is just that branch);
 * - `seq` is transparent: its children are chained;
 * - `choice` nodes are folded into edges: each branch becomes an edge from what ran before
 *   the choice, carrying the branch condition;
 * - of the `succeed` / `fail` exits only the one the run reached is drawn; the others are
 *   listed on the item they leave from (`otherExits`, shown in its tooltip).
 */
export type FlowItemKind = 'agent' | 'decision' | 'succeed' | 'fail' | 'group';

/** The graph node kinds drawn as a group (a frame around their items), with their caption. */
export const GROUP_KINDS = {
  loop: { caption: 'Loop' },
  par: { caption: 'In parallel' },
  map: { caption: 'For each item' },
} as const satisfies Record<string, { caption: string }>;

export type GroupKind = keyof typeof GROUP_KINDS;

export function isGroupKind(kind: string | undefined): kind is GroupKind {
  return kind != null && Object.hasOwn(GROUP_KINDS, kind);
}

export type HiddenExit = { id: string; kind: 'succeed' | 'fail'; condition?: string };

export type FlowItem = {
  id: string;
  kind: FlowItemKind;
  /** The graph node kind (`step`, `verifier`, `choice`, `loop`, …). */
  graphKind: string;
  /** Groups only. */
  groupKind?: GroupKind;
  parent?: string;
  info?: GraphNodeInfo;
  state: NodeState;
  /** Something in or behind this item ran (or this exit was reached). */
  visited: boolean;
  /** Exit reached by this run. */
  reached?: boolean;
  stats?: NodeStats;
  latest?: Execution;
  /** Innermost enclosing loop and its maximum. */
  loop?: LoopRef;
  /** Loops: the highest round any node inside reached. */
  iteration?: number;
  maxIterations?: number | null;
  /** Fail exits: the reason recorded when reached. */
  failReason?: string | null;
  /** Exits leaving from here that this run did not reach (folded off the canvas). */
  otherExits?: HiddenExit[];
  /**
   * Loop groups only: the ids inside the body whose own branch has nothing further in the
   * static graph — this is where an iteration ends and, short of the loop's own exit, the next
   * round starts. Used to draw the loop-back edge.
   */
  loopBackFrom?: string[];
};

export type FlowEdge = {
  id: string;
  source: string;
  target: string;
  /** The humanised condition, e.g. `all 4 accepted`, `errored`, `otherwise`. */
  label?: string;
  /** The full condition, for the tooltip. */
  title?: string;
  taken: boolean;
  /** The source has more than one way out, so the condition is worth showing when taken. */
  branch?: boolean;
  /** Drawn back into the loop it belongs to, not forward through the graph. */
  loopBack?: boolean;
};

export type FlowModel = { items: FlowItem[]; edges: FlowEdge[] };

export type LoopRef = { name: string; max: number | null };

/** A node's innermost enclosing loop and that loop's maximum rounds. */
export function innermostLoop(name: string, graphNodes: readonly GraphNodeInfo[]): LoopRef | undefined {
  const loops = graphNodes.find((n) => n.name === name)?.loops ?? [];
  const loopName = loops[loops.length - 1];
  if (!loopName) return undefined;
  return { name: loopName, max: graphNodes.find((n) => n.name === loopName)?.maxIterations ?? null };
}

/** Whether two items have the same layout: only ids, kinds and nesting move the boxes. */
export function structureKey(items: FlowItem[]) {
  return items.map((i) => `${i.id}:${i.kind}:${i.parent ?? ''}`).join(',');
}

// ---------- model ----------

export type GraphSource = Pick<RunView, 'graph' | 'graphNodes' | 'nodes' | 'executions' | 'status' | 'terminal' | 'failure'>;

type RawNode = Record<string, unknown>;
/** An edge of the static graph before it is labelled; `when`/`otherwise` are its branch guard. */
type RawEdge = { source: string; target: string; when?: unknown; otherwise?: boolean };
/** The model between passes. The passes return new lists but update the items they keep in place. */
type Draft = { items: FlowItem[]; raw: RawEdge[] };

/**
 * The graph's flow model, built in passes: the items and raw edges from the graph tree, then
 * the exits the run reached, the one-branch parallel groups collapsed, group state from the
 * items inside, the choices and the unreached exits folded into edges, emptied groups dropped,
 * and finally the labelled edges plus a loop-back edge for each loop that went around.
 */
export function buildFlowModel(view: GraphSource): FlowModel {
  const built = buildItems(view);
  markReachedExits(built.items, view, built.branchGuards);
  let draft = collapseSingleBranchPars(built);
  const descendants = descendantsOf(draft.items);
  setGroupState(draft.items, view, descendants);
  draft = foldChoices(draft);
  const describe = describeEdges(descendants);
  draft = foldUnreachedExits(draft, describe);
  draft = dropEmptyGroups(draft);
  const edges = [...labelledEdges(draft, describe), ...loopBackEdges(draft.items)];
  return { items: draft.items, edges };
}

// ---------- pass 1: items and raw edges from the graph tree ----------

/** The entry and exit item ids of a built subtree, for chaining what comes before and after it. */
type Frag = { entries: string[]; exits: string[] };

type BuildState = {
  view: GraphSource;
  info: Map<string, GraphNodeInfo>;
  executions: Map<number, Execution>;
  items: FlowItem[];
  raw: RawEdge[];
  /** For each branch's first item: the node names its choice's guard reads. */
  branchGuards: Map<string, string[]>;
  /** Ids given out so far, and the counter that makes nameless or repeated ones unique. */
  seen: Set<string>;
  anonymous: number;
};

function buildItems(view: GraphSource): Draft & { branchGuards: Map<string, string[]> } {
  const s: BuildState = {
    view,
    info: new Map(view.graphNodes.map((n) => [n.name, n])),
    executions: new Map(view.executions.map((e) => [e.id, e])),
    items: [],
    raw: [],
    branchGuards: new Map(),
    seen: new Set(),
    anonymous: 0,
  };
  const root = view.graph?.root;
  if (isNode(root)) buildNode(s, root, undefined);
  return { items: s.items, raw: s.raw, branchGuards: s.branchGuards };
}

function isNode(value: unknown): value is RawNode {
  return !!value && typeof value === 'object' && typeof (value as RawNode).kind === 'string';
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

function idOf(s: BuildState, node: RawNode): string {
  let id = str(node.name) || `${node.kind}#${++s.anonymous}`;
  while (s.seen.has(id)) id = `${id}#${++s.anonymous}`;
  s.seen.add(id);
  return id;
}

function connect(s: BuildState, sources: string[], targets: string[], guard: { when?: unknown; otherwise?: boolean } = {}) {
  for (const source of sources) for (const target of targets) s.raw.push({ source, target, ...guard });
}

function buildNode(s: BuildState, node: RawNode, parent: string | undefined): Frag {
  const kind = String(node.kind);
  if (kind === 'seq') return buildSeq(s, node, parent);
  const id = idOf(s, node);
  if (isGroupKind(kind)) return buildGroup(s, node, id, kind, parent);
  switch (kind) {
    case 'choice':
      return buildChoice(s, node, id, parent);
    case 'succeed':
    case 'fail':
      return buildExit(s, node, id, kind);
    default:
      // An executing node (step, verifier or a future leaf kind).
      return buildAgent(s, id, kind, parent);
  }
}

/** A `seq` draws nothing itself: its children are chained one after the other. */
function buildSeq(s: BuildState, node: RawNode, parent: string | undefined): Frag {
  let entries: string[] | null = null;
  let prev: string[] | null = null;
  for (const child of (node.children as unknown[]) ?? []) {
    if (!isNode(child)) continue;
    const f = buildNode(s, child, parent);
    if (entries === null) entries = f.entries;
    if (prev) connect(s, prev, f.entries);
    prev = f.exits;
  }
  return { entries: entries ?? [], exits: prev ?? [] };
}

/** A loop, par or map: a frame that edges enter and leave as a whole. */
function buildGroup(s: BuildState, node: RawNode, id: string, kind: GroupKind, parent: string | undefined): Frag {
  const info = s.info.get(id);
  const item: FlowItem = {
    id,
    kind: 'group',
    graphKind: kind,
    groupKind: kind,
    parent,
    info,
    state: 'idle',
    visited: false,
    maxIterations: info?.maxIterations ?? (typeof node.maxIterations === 'number' ? node.maxIterations : null),
  };
  s.items.push(item);
  const inner: unknown[] = [];
  if (isNode(node.body)) inner.push(node.body);
  for (const c of ((node.children ?? node.branches) as unknown[]) ?? []) {
    inner.push(isNode(c) ? c : (c as RawNode)?.node);
  }
  // A loop's body has nothing after it in the static graph: whatever it exits to is really
  // "iterate again" (or the loop's own limit gives up), not a dead end.
  const bodyExits: string[] = [];
  for (const child of inner) if (isNode(child)) bodyExits.push(...buildNode(s, child, id).exits);
  if (item.groupKind === 'loop') item.loopBackFrom = bodyExits;
  return { entries: [id], exits: [id] };
}

/** A choice: an edge to each branch carrying its guard (folded into the edges before it later). */
function buildChoice(s: BuildState, node: RawNode, id: string, parent: string | undefined): Frag {
  s.items.push({ id, kind: 'decision', graphKind: 'choice', parent, info: s.info.get(id), state: 'idle', visited: false });
  const exits: string[] = [];
  for (const branch of (node.branches as unknown[]) ?? []) {
    const b = branch as RawNode;
    const child = isNode(b?.node) ? b.node : isNode(b) ? b : undefined;
    if (!isNode(child)) continue;
    const f = buildNode(s, child, parent);
    connect(s, [id], f.entries, { when: b.when });
    for (const target of f.entries) s.branchGuards.set(target, guardNames(b.when));
    exits.push(...f.exits);
  }
  if (isNode(node.otherwise)) {
    const f = buildNode(s, node.otherwise, parent);
    connect(s, [id], f.entries, { otherwise: true });
    exits.push(...f.exits);
  } else {
    exits.push(id);
  }
  return { entries: [id], exits };
}

function buildExit(s: BuildState, node: RawNode, id: string, kind: 'succeed' | 'fail'): Frag {
  const info = s.info.get(id);
  s.items.push({
    id,
    kind,
    graphKind: kind,
    // A succeed/fail always ends the whole run, never just the loop/parallel group it is
    // written inside of; drawing it there would suggest the group keeps going.
    parent: undefined,
    info,
    state: 'idle',
    visited: false,
    failReason: info?.failReason ?? str(node.reason) ?? null,
  });
  return { entries: [id], exits: [] };
}

function buildAgent(s: BuildState, id: string, kind: string, parent: string | undefined): Frag {
  const stats = s.view.nodes[id];
  const latest = stats?.executions.length ? s.executions.get(stats.executions[stats.executions.length - 1]) : undefined;
  s.items.push({
    id,
    kind: 'agent',
    graphKind: kind,
    parent,
    info: s.info.get(id),
    stats,
    latest,
    state: nodeState(latest?.state ?? stats?.state),
    visited: !!stats?.executions.length,
    loop: innermostLoop(id, s.view.graphNodes),
  });
  return { entries: [id], exits: [id] };
}

// ---------- pass 2: the exits the run reached ----------

function markReachedExits(items: FlowItem[], view: GraphSource, branchGuards: Map<string, string[]>) {
  const fails = items.filter((i) => i.kind === 'fail');
  const reachedFail = view.status === 'failed' ? pickReachedFail(fails, view, branchGuards) : undefined;
  if (reachedFail) reachedFail.reached = true;
  if (view.status === 'succeeded') {
    const succeeds = items.filter((i) => i.kind === 'succeed');
    const last = lastExecutedNode(view.executions);
    const chosen = succeeds.length === 1 ? succeeds[0] : (succeeds.find((s) => (branchGuards.get(s.id) ?? []).includes(last ?? '')) ?? succeeds[0]);
    if (chosen) chosen.reached = true;
  }
  for (const item of items) {
    if (item.reached) {
      item.visited = true;
      item.state = item.kind === 'fail' ? 'failed' : 'succeeded';
    }
  }
}

function lastExecutedNode(executions: Execution[]): string | undefined {
  return executions.length ? executions[executions.length - 1].node : undefined;
}

/** Of the fail exits with the run's reason, the one whose condition reads the failing node. */
export function pickReachedFail(fails: FlowItem[], view: GraphSource, branchGuards: Map<string, string[]>): FlowItem | undefined {
  const reason = view.terminal?.reason;
  const candidates = fails.filter((f) => f.failReason && f.failReason === reason);
  if (candidates.length <= 1) return candidates[0];
  const culprit = view.failure?.node ?? undefined;
  const recent = [...view.executions].reverse().map((e) => e.node);
  for (const name of [culprit, ...recent]) {
    if (!name) continue;
    const match = candidates.find((c) => (branchGuards.get(c.id) ?? []).includes(name));
    if (match) return match;
  }
  return candidates[0];
}

// ---------- pass 3: a parallel group with a single branch is just that branch ----------

function collapseSingleBranchPars({ items, raw }: Draft): Draft {
  for (const group of items.filter((i) => i.groupKind === 'par')) {
    const kids = items.filter((i) => i.parent === group.id);
    if (kids.length !== 1) continue;
    const only = kids[0];
    only.parent = group.parent;
    const rename = (id: string) => (id === group.id ? only.id : id);
    raw = raw.map((e) => ({ ...e, source: rename(e.source), target: rename(e.target) }));
    items = items.filter((i) => i !== group);
  }
  return { items, raw };
}

// ---------- pass 4: group state from the items inside ----------

/** Everything nested inside a group, at any depth. */
type Descendants = (id: string) => FlowItem[];

function descendantsOf(items: FlowItem[]): Descendants {
  const children = new Map<string, FlowItem[]>();
  for (const item of items) if (item.parent) children.set(item.parent, [...(children.get(item.parent) ?? []), item]);
  const descendants = (id: string): FlowItem[] => (children.get(id) ?? []).flatMap((c) => [c, ...descendants(c.id)]);
  return descendants;
}

/** A group is visited or running when something inside is; a loop's round is the highest inside. */
function setGroupState(items: FlowItem[], view: GraphSource, descendants: Descendants) {
  for (const group of items.filter((i) => i.kind === 'group')) {
    const inner = descendants(group.id);
    group.visited = inner.some((i) => i.visited);
    if (inner.some((i) => i.state === 'running')) group.state = 'running';
    if (group.groupKind === 'loop') {
      const rounds = view.graphNodes.filter((n) => n.loops.includes(group.id)).map((n) => view.nodes[n.name]?.rounds ?? 0);
      group.iteration = Math.max(0, ...rounds);
    }
  }
}

// ---------- passes 5-7: fold choices and unreached exits into edges, drop emptied groups ----------

/**
 * The edges without `id`: whatever led into it now leads straight to whatever it led to, each
 * new edge keeping the outgoing guard (or else the incoming one).
 */
function bypass(raw: RawEdge[], id: string): RawEdge[] {
  const incoming = raw.filter((e) => e.target === id);
  const outgoing = raw.filter((e) => e.source === id);
  const rest = raw.filter((e) => e.source !== id && e.target !== id);
  for (const a of incoming) {
    for (const b of outgoing) rest.push({ source: a.source, target: b.target, when: b.when ?? a.when, otherwise: b.otherwise ?? a.otherwise });
  }
  return rest;
}

/** What ran before a choice connects to each of its branches. A choice nothing leads into stays. */
function foldChoices({ items, raw }: Draft): Draft {
  for (const item of items) {
    if (item.kind === 'decision' && raw.some((e) => e.target === item.id)) raw = bypass(raw, item.id);
  }
  return { items: items.filter((i) => i.kind !== 'decision' || raw.some((e) => e.source === i.id)), raw };
}

type Describe = (e: RawEdge) => { label: string; title: string } | undefined;

/** An edge's condition in words; names inside the group it leaves are implied. */
function describeEdges(descendants: Descendants): Describe {
  const localNames = (id: string) => new Set([id, ...descendants(id).map((i) => i.id)]);
  return (e) => {
    if (e.otherwise) return { label: 'otherwise', title: 'when no other condition matched' };
    if (e.when !== undefined) return conditionLabel(e.when, localNames(e.source));
    return undefined;
  };
}

/** Exits the run did not reach fold away; the item they leave from lists them. */
function foldUnreachedExits({ items, raw }: Draft, describe: Describe): Draft {
  const itemById = new Map(items.map((i) => [i.id, i]));
  const unreached = new Set(items.filter((i) => (i.kind === 'succeed' || i.kind === 'fail') && !i.reached).map((i) => i.id));
  for (const e of raw) {
    if (!unreached.has(e.target)) continue;
    const exit = itemById.get(e.target)!;
    const source = itemById.get(e.source);
    if (!source) continue;
    source.otherExits ??= [];
    source.otherExits.push({ id: exit.id, kind: exit.kind as 'succeed' | 'fail', condition: describe(e)?.label });
  }
  return {
    items: items.filter((i) => !unreached.has(i.id)),
    raw: raw.filter((e) => !unreached.has(e.target) && itemById.has(e.source) && itemById.has(e.target)),
  };
}

/**
 * A group left with nothing inside it (its only child was an exit that just folded away, e.g.
 * a map whose single branch is an unreached fail) is not worth a frame on the canvas: drop it
 * and bypass it, as a choice is. Repeats until no group is empty, since dropping one can empty
 * its parent.
 */
function dropEmptyGroups({ items, raw }: Draft): Draft {
  let emptied = true;
  while (emptied) {
    emptied = false;
    for (const group of items.filter((i) => i.kind === 'group')) {
      if (items.some((i) => i.parent === group.id)) continue;
      emptied = true;
      raw = bypass(raw, group.id);
      items = items.filter((i) => i !== group);
    }
  }
  return { items, raw };
}

// ---------- pass 8: edges ----------

function labelledEdges({ items, raw }: Draft, describe: Describe): FlowEdge[] {
  const itemById = new Map(items.map((i) => [i.id, i]));
  const outCount = new Map<string, number>();
  for (const e of raw) outCount.set(e.source, (outCount.get(e.source) ?? 0) + 1);
  return raw.map((e, i) => {
    const label = describe(e);
    return {
      id: `${e.source}->${e.target}#${i}`,
      source: e.source,
      target: e.target,
      label: label ? shortLabel(label.label) : undefined,
      title: label?.title,
      taken: !!itemById.get(e.source)?.visited && !!itemById.get(e.target)?.visited,
      branch: !!label && (outCount.get(e.source) ?? 0) > 1,
    };
  });
}

/**
 * A loop that actually went around at least once: an explicit edge from each visited dead end
 * in its body back to the loop frame, instead of the picture reading as a straight pipeline
 * that happened to stop.
 */
function loopBackEdges(items: FlowItem[]): FlowEdge[] {
  const itemById = new Map(items.map((i) => [i.id, i]));
  const edges: FlowEdge[] = [];
  for (const group of items.filter((i) => i.groupKind === 'loop')) {
    if ((group.iteration ?? 0) <= 1) continue;
    for (const id of group.loopBackFrom ?? []) {
      const from = itemById.get(id);
      if (from?.kind !== 'agent' || !from.visited) continue;
      edges.push({
        id: `${id}->${group.id}#loopback`,
        source: id,
        target: group.id,
        label: `round ${group.iteration}`,
        title: `${humanId(id)} sends the run back into ${humanId(group.id)} for another round`,
        taken: true,
        branch: false,
        loopBack: true,
      });
    }
  }
  return edges;
}

// ---------- the run's models ----------

/**
 * Whether every agent card names its own model: when the run's agents do not all use one model
 * (a card that only named a model when it differed from the most common one left the reader to
 * guess that every unlabelled card ran on that one), and always for a benchmark run, where the
 * per-node model is the thing being measured.
 */
export function showEveryModel(items: FlowItem[], benchmark: boolean): boolean {
  if (benchmark) return true;
  const models = new Set<string>();
  for (const i of items) {
    if (i.kind !== 'agent') continue;
    const m = i.latest?.model ?? i.info?.model;
    if (m) models.add(m);
  }
  return models.size > 1;
}

/** The model the run's agents mostly use; cards show a model only when it differs. */
export function commonModel(items: FlowItem[]): string | undefined {
  const counts = new Map<string, number>();
  for (const i of items) {
    if (i.kind !== 'agent') continue;
    const m = i.latest?.model ?? i.info?.model;
    if (m) counts.set(m, (counts.get(m) ?? 0) + 1);
  }
  let best: string | undefined;
  for (const [m, n] of counts) if (!best || n > counts.get(best)!) best = m;
  return best;
}

// ---------- layout ----------

export type Direction = 'RIGHT' | 'DOWN';
export type Size = { width: number; height: number };

/**
 * The boxes ELK lays out. React Flow then draws each card at exactly its box (graph.css sizes
 * `.flow-card` to 100% of it), so these numbers are the card sizes. The text widths estimate the
 * fonts in graph.css: exit names at 12px (`.flow-card.exit`), edge labels at 11px and at most
 * 170px wide (`.edge-label`).
 */
const CARD_SIZES = {
  agent: { width: 208, height: 52 },
  decision: { width: 150, height: 32 },
  exit: { height: 32, minWidth: 96, maxWidth: 220, perChar: 7, padding: 44 },
  /** A group with nothing inside; ELK sizes the others around their items. */
  emptyGroup: { width: 200, height: 60 },
  edgeLabel: { height: 18, maxWidth: 170, perChar: 6.2, padding: 14 },
};

export function itemSize(item: FlowItem): Size {
  switch (item.kind) {
    case 'agent':
      return { ...CARD_SIZES.agent };
    case 'decision':
      return { ...CARD_SIZES.decision };
    case 'succeed':
    case 'fail': {
      const { minWidth, maxWidth, perChar, padding, height } = CARD_SIZES.exit;
      return { width: Math.min(maxWidth, Math.max(minWidth, humanId(item.id).length * perChar + padding)), height };
    }
    default:
      return { ...CARD_SIZES.emptyGroup };
  }
}

/** Edge label box for the layout: only a branch's condition is drawn on the canvas. */
export function labelSize(label: string): Size {
  const { maxWidth, perChar, padding, height } = CARD_SIZES.edgeLabel;
  return { width: Math.min(maxWidth, label.length * perChar + padding), height };
}

/** The compound ELK graph: groups contain their items; edges are not split by hierarchy. */
export function buildElkGraph(model: FlowModel, direction: Direction = 'RIGHT'): ElkNode {
  const options = (top: number, side: number) => ({
    'elk.algorithm': 'layered',
    'elk.direction': direction,
    'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
    'elk.spacing.nodeNode': direction === 'RIGHT' ? '22' : '28',
    'elk.layered.spacing.nodeNodeBetweenLayers': direction === 'RIGHT' ? '40' : '30',
    'elk.spacing.edgeNode': '14',
    'elk.spacing.edgeLabel': '4',
    'elk.edgeLabels.inline': 'true',
    'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
    'elk.padding': `[top=${top},left=${side},bottom=${side},right=${side}]`,
  });
  const nodes = new Map<string, ElkNode>();
  for (const item of model.items) {
    const node: ElkNode = item.kind === 'group' ? { id: item.id, children: [], layoutOptions: options(32, 14) } : { id: item.id, ...itemSize(item) };
    nodes.set(item.id, node);
  }
  const roots: ElkNode[] = [];
  for (const item of model.items) {
    const parent = item.parent ? nodes.get(item.parent) : undefined;
    (parent ? parent.children! : roots).push(nodes.get(item.id)!);
  }
  for (const item of model.items) {
    const node = nodes.get(item.id)!;
    if (item.kind === 'group' && !node.children!.length) Object.assign(node, { ...CARD_SIZES.emptyGroup });
  }
  return {
    id: 'root',
    layoutOptions: options(8, 8),
    children: roots,
    edges: model.edges.map((e) => ({
      id: e.id,
      sources: [e.source],
      targets: [e.target],
      labels: e.branch && e.label ? [{ text: e.label, ...labelSize(e.label) }] : undefined,
    })),
  };
}

export type Placed = { x: number; y: number; width: number; height: number };

/** Absolute positions of every item (ELK child coordinates are relative to their parent). */
export function readElkLayout(layout: ElkNode): Map<string, Placed> {
  const out = new Map<string, Placed>();
  const visit = (node: ElkNode, ox: number, oy: number) => {
    const x = ox + (node.x ?? 0);
    const y = oy + (node.y ?? 0);
    out.set(node.id, { x, y, width: node.width ?? 0, height: node.height ?? 0 });
    for (const child of node.children ?? []) visit(child, x, y);
  };
  for (const child of layout.children ?? []) visit(child, 0, 0);
  return out;
}

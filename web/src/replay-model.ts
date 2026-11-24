import type { Checkpoint, GraphNodeInfo, Json, JsonObject, ReplayBody, RunView } from './types';
import { innermostLoop } from './graph-model';

export const AGENT_KINDS = new Set(['step', 'verifier']);

export function ancestors(name: string, nodes: GraphNodeInfo[]): string[] {
  const byName = new Map(nodes.map((n) => [n.name, n]));
  const out: string[] = [];
  let current = byName.get(name)?.parent ?? null;
  while (current) {
    out.push(current);
    current = byName.get(current)?.parent ?? null;
  }
  return out;
}

// ---------- checkpoints ----------

/** The loop iteration a checkpoint belongs to (innermost loop), if any. */
export function checkpointIteration(c: Pick<Checkpoint, 'loopIterations'>): number | null {
  return c.loopIterations?.length ? c.loopIterations[c.loopIterations.length - 1] : null;
}

/**
 * The checkpoint to start from: the failed node's own (for its round when the iteration
 * matches), else its enclosing group's (parallel groups share one), else the last one before
 * the failure, else the latest.
 */
export function preselectCheckpoint(checkpoints: Checkpoint[], view: Pick<RunView, 'failure' | 'graphNodes' | 'executions'>): Checkpoint | undefined {
  if (!checkpoints.length) return undefined;
  const sorted = [...checkpoints].sort((a, b) => a.sequence - b.sequence);
  const failure = view.failure;
  const round = failure?.round;
  /** The node's latest checkpoint, preferring the failed round (or the round before it). */
  const latestFor = (name: string): Checkpoint | undefined => {
    const mine = sorted.filter((c) => c.node === name);
    const sameRound = mine.filter((c) => {
      const iteration = checkpointIteration(c);
      return round != null && (iteration === round || iteration === round - 1);
    });
    return (sameRound.length ? sameRound : mine).at(-1);
  };
  const node = failure?.node ?? view.executions.at(-1)?.node;
  if (node) {
    for (const name of [node, ...ancestors(node, view.graphNodes)]) {
      const found = latestFor(name);
      if (found) return found;
    }
  }
  if (failure?.at != null) {
    const before = sorted.filter((c) => c.createdAt <= failure.at!);
    if (before.length) return before.at(-1);
  }
  return sorted.at(-1);
}

/** When a checkpoint sits in the last allowed round of its innermost loop (or of the loop it is). */
export function checkpointRound(c: Checkpoint, nodes: GraphNodeInfo[]): { loop: string; round: number; max: number } | null {
  const round = checkpointIteration(c);
  const info = nodes.find((n) => n.name === c.node);
  const loop = info?.kind === 'loop' ? { name: info.name, max: info.maxIterations } : info && innermostLoop(info.name, nodes);
  const max = loop?.max;
  return round != null && loop && max != null && round >= max ? { loop: loop.name, round, max } : null;
}

/** Agent nodes that run again from the checkpoint on (graph order), whose prompt/model may change. */
export function editableNodes(checkpointNode: string | undefined, nodes: GraphNodeInfo[]): GraphNodeInfo[] {
  if (!checkpointNode) return [];
  const index = nodes.findIndex((n) => n.name === checkpointNode);
  if (index < 0) return nodes.filter((n) => AGENT_KINDS.has(n.kind));
  return nodes.slice(index).filter((n) => AGENT_KINDS.has(n.kind));
}

/** The models offered for a node: every model the runtime binds or an execution used. */
export function replayModels(runtime: JsonObject | null, executions: RunView['executions']): string[] {
  const set = new Set<string>();
  const runtimeNodes = (runtime?.nodes ?? {}) as Record<string, { model?: Json }>;
  for (const n of Object.values(runtimeNodes)) if (typeof n?.model === 'string') set.add(n.model);
  for (const e of executions) if (e.model) set.add(e.model);
  return [...set].sort();
}

// ---------- edits ----------

/** What the person typed for one node; a field left alone is absent. */
export type NodeEdit = {
  model?: string;
  instructions?: string;
  /** The timeout in seconds, as typed (the graph's `timeoutMs` is in milliseconds). */
  timeout?: string;
  /** A loop's maximum rounds, as typed (the graph's `maxIterations`). */
  maxRounds?: string;
};

/** A node's values before any edit. */
export type OriginalValues = {
  instructions: (name: string) => string | null;
  model: (name: string) => string | null;
  timeoutMs?: (name: string) => number | null;
  maxIterations?: (name: string) => number | null;
};

export function originalValues(graphNodes: GraphNodeInfo[]): Required<OriginalValues> {
  const byName = (name: string) => graphNodes.find((n) => n.name === name);
  return {
    instructions: (name) => byName(name)?.instructions ?? null,
    model: (name) => byName(name)?.model ?? null,
    timeoutMs: (name) => byName(name)?.timeoutMs ?? null,
    maxIterations: (name) => byName(name)?.maxIterations ?? null,
  };
}

/** Whether a node's edit changes its model, instructions or timeout (for the "changed" badge). */
export function nodeEditChanged(e: NodeEdit, n: GraphNodeInfo): boolean {
  return (
    (e.model != null && e.model !== (n.model ?? '')) ||
    (e.instructions != null && e.instructions !== (n.instructions ?? '')) ||
    (e.timeout != null && positiveInt(e.timeout) !== (n.timeoutMs != null ? n.timeoutMs / 1000 : undefined))
  );
}

/**
 * The edits when the checkpoint is in its loop's last round: starting there would run out
 * again, so the loop's max rounds start one higher (unless the person set them).
 */
export function withLastRoundBump(edits: Record<string, NodeEdit>, lastRound: { loop: string; max: number } | null): Record<string, NodeEdit> {
  if (!lastRound || edits[lastRound.loop]?.maxRounds != null) return edits;
  return { ...edits, [lastRound.loop]: { ...edits[lastRound.loop], maxRounds: String(lastRound.max + 1) } };
}

/** The node with that name (a graph node: it has a `kind`) anywhere in the graph tree, depth first. */
export function findNode(tree: Json | undefined, name: string): JsonObject | undefined {
  if (!tree || typeof tree !== 'object') return undefined;
  if (Array.isArray(tree)) {
    for (const child of tree) {
      const found = findNode(child, name);
      if (found) return found;
    }
    return undefined;
  }
  if (tree.name === name && typeof tree.kind === 'string') return tree;
  return findNode(Object.values(tree) as Json[], name);
}

/** A positive whole number from a text field, or undefined. */
export function positiveInt(text: string | undefined): number | undefined {
  if (text == null || !text.trim()) return undefined;
  const n = Number(text.trim());
  return Number.isInteger(n) && n > 0 ? n : undefined;
}

/** Keys `zeroshot run --graph` accepts; the admitted graph also has keys admission computed (`bounds`). */
const GRAPH_KEYS = ['profile', 'initialInput', 'policy', 'root'];

export function submissionGraph(graph: JsonObject | null): JsonObject | null {
  if (!graph) return graph;
  return Object.fromEntries(Object.entries(graph).filter(([k]) => GRAPH_KEYS.includes(k)));
}

/**
 * The edits that go into the graph: the graph key each sets, the value typed (undefined when
 * empty or invalid) and the value it replaces. An edit equal to the original is no change.
 */
const GRAPH_FIELDS: { key: string; edited: (e: NodeEdit) => Json | undefined; original: (o: OriginalValues, name: string) => Json }[] = [
  { key: 'instructions', edited: (e) => e.instructions ?? undefined, original: (o, name) => o.instructions(name) ?? '' },
  { key: 'timeoutMs', edited: (e) => secondsToMs(positiveInt(e.timeout)), original: (o, name) => o.timeoutMs?.(name) ?? null },
  { key: 'maxIterations', edited: (e) => positiveInt(e.maxRounds), original: (o, name) => o.maxIterations?.(name) ?? null },
];

function secondsToMs(seconds: number | undefined): number | undefined {
  return seconds != null ? seconds * 1000 : undefined;
}

/**
 * The graph and runtime with the dialog's edits applied. Returns `undefined` for a part that
 * did not change, so the request only carries what the person changed. The model is a runtime
 * binding (`runtime.nodes[name].model`); the other fields are on the graph node.
 */
export function applyEdits(graph: JsonObject | null, runtime: JsonObject | null, edits: Record<string, NodeEdit>, original: OriginalValues): { graph?: JsonObject; runtime?: JsonObject } {
  let nextGraph: JsonObject | undefined;
  let nextRuntime: JsonObject | undefined;
  for (const [name, edit] of Object.entries(edits)) {
    for (const field of GRAPH_FIELDS) {
      const value = field.edited(edit);
      if (value === undefined || value === field.original(original, name)) continue;
      nextGraph ??= clone(submissionGraph(graph) ?? {});
      const node = findNode(nextGraph.root, name);
      if (node) node[field.key] = value;
    }
    const model = edit.model?.trim();
    if (model && model !== (original.model(name) ?? '')) {
      nextRuntime ??= clone(runtime ?? {});
      nextRuntime.nodes ??= {};
      const nodes = nextRuntime.nodes as JsonObject;
      nodes[name] ??= {};
      const binding = nodes[name] as JsonObject;
      binding.model = model;
    }
  }
  return { graph: nextGraph, runtime: nextRuntime };
}

function clone<T>(value: T): T {
  return value == null ? value : structuredClone(value);
}

// ---------- the request ----------

/** The JSON fields under Advanced, which replace the graph, runtime and input wholesale. */
export const ADVANCED_FIELDS = [
  { key: 'graph', label: 'Graph JSON', errorName: 'The graph JSON' },
  { key: 'runtime', label: 'Runtime JSON', errorName: 'The runtime JSON' },
  { key: 'input', label: 'Initial input JSON', errorName: 'The input JSON' },
] as const;

export type AdvancedKey = (typeof ADVANCED_FIELDS)[number]['key'];
export type AdvancedJson = Record<AdvancedKey, string>;

/** Why a request could not be made, or what Zeroshot answered; `field` marks an Advanced field. */
export type ReplayNotice = { tone: 'ok' | 'bad'; lines: string[]; field?: AdvancedKey };

/** The Advanced fields' starting text: the edited (or else original) graph and runtime, and the input. */
export function advancedJson(view: Pick<RunView, 'graph' | 'runtime' | 'initialInput'>, edited: { graph?: JsonObject; runtime?: JsonObject }, pretty: (value: unknown) => string): AdvancedJson {
  return {
    graph: pretty(edited.graph ?? submissionGraph(view.graph)),
    runtime: pretty(edited.runtime ?? view.runtime),
    input: pretty(view.initialInput),
  };
}

/** The values each Advanced field is compared with: unchanged text is not sent. */
function advancedOriginals(view: Pick<RunView, 'graph' | 'runtime' | 'initialInput'>): Record<AdvancedKey, unknown> {
  return { graph: submissionGraph(view.graph), runtime: view.runtime, input: view.initialInput };
}

/**
 * The replay request, or the problem that stops it. With `edits` (replay with changes allowed)
 * the edited graph and runtime go along; the Advanced JSON, when open, replaces them; a title is
 * sent only when it changed.
 */
export function replayRequest(opts: {
  view: Pick<RunView, 'graph' | 'runtime' | 'initialInput' | 'title'>;
  checkpointId: string | undefined;
  /** Only when the target can replay with changes. */
  edits: { nodes: Record<string, NodeEdit>; original: OriginalValues; title: string } | null;
  advanced: AdvancedJson | null;
  validateOnly: boolean;
}): ReplayBody | ReplayNotice {
  const { view, checkpointId, edits, advanced, validateOnly } = opts;
  if (!checkpointId) return { tone: 'bad', lines: ['Choose a checkpoint first.'] };
  const edited = edits ? applyEdits(view.graph, view.runtime, edits.nodes, edits.original) : {};
  let parts: { graph?: unknown; runtime?: unknown; initialInput?: unknown } = { graph: edited.graph, runtime: edited.runtime };
  if (advanced) {
    const originals = advancedOriginals(view);
    const parsed: Partial<Record<AdvancedKey, unknown>> = {};
    for (const { key, errorName } of ADVANCED_FIELDS) {
      const field = parseJsonField(advanced[key], originals[key], errorName);
      if (field.error) return { tone: 'bad', lines: [field.error], field: key };
      parsed[key] = field.value;
    }
    parts = { graph: parsed.graph, runtime: parsed.runtime, initialInput: parsed.input };
  }
  const title = edits?.title.trim();
  return {
    checkpointId,
    ...(parts.graph !== undefined ? { graph: parts.graph } : {}),
    ...(parts.runtime !== undefined ? { runtime: parts.runtime } : {}),
    ...(parts.initialInput !== undefined ? { initialInput: parts.initialInput } : {}),
    ...(title && title !== (view.title || '') ? { title } : {}),
    validateOnly,
  };
}

/** Parses an advanced JSON field: `undefined` when empty/unchanged, an error message when invalid. */
export function parseJsonField(text: string, original: unknown, label: string): { value?: unknown; error?: string } {
  if (!text.trim()) return {};
  try {
    const value = JSON.parse(text);
    if (JSON.stringify(value) === JSON.stringify(original)) return {};
    return { value };
  } catch (cause) {
    return { error: `${label} is not valid JSON: ${cause instanceof Error ? cause.message : String(cause)}` };
  }
}

// ---------- the answer ----------
//
// The server answers a validation with `{valid: true, mode: 'resume'}` and a started replay with
// Zeroshot's own `resume` output; it refuses a replay with an error response (see `ReplayResult`
// in types.ts). A Zeroshot that replays with changes (`zeroshot replay --validate-only`) answered
// with `replayOf` and `completedNodes` on success and its problems under `errors`, `issues`,
// `problems` or a single `conflict`; these readers still understand that output, so they take
// `unknown`.

type Loose = Record<string, unknown>;

function plural(n: number, one: string, many: string) {
  return n === 1 ? one : many;
}

/** What a successful validation said will happen, in plain lines. */
export function validationSummary(result: unknown): string[] {
  if (!result || typeof result !== 'object') return [];
  const r = result as Loose;
  const of = (r.replayOf ?? {}) as Loose;
  const lines: string[] = [];
  const reused = typeof of.reusedExecutions === 'number' ? of.reusedExecutions : null;
  const completed = Array.isArray(r.completedNodes) ? (r.completedNodes as string[]) : [];
  // The checkpoint the person picked is not repeated back; only what Zeroshot adds.
  if (reused != null) {
    const ofNodes = completed.length ? ` of ${completed.join(', ')}` : '';
    lines.push(`Reuses ${reused} ${plural(reused, 'execution', 'executions')}${ofNodes}.`);
  }
  const changed = Array.isArray(of.changedNodes) ? (of.changedNodes as string[]) : [];
  if (changed.length) lines.push(`Changed: ${changed.join(', ')}.`);
  return lines;
}

/** The problems a replay answer lists, under whichever name that Zeroshot used. */
function problemList(r: Loose): unknown {
  return r.errors ?? r.issues ?? r.problems ?? (r.conflict ? [r.conflict] : undefined);
}

/** One problem: a string, or `{path?, node?, field?, message}` read as `where: message`. */
function problemLine(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object') {
    const o = e as Loose;
    const where = [o.path, o.node, o.field].filter((x) => typeof x === 'string' && x).join(' ');
    const what = typeof o.message === 'string' ? o.message : JSON.stringify(e);
    return where ? `${where}: ${what}` : what;
  }
  return String(e);
}

/** Validation errors from a replay response in plain lines. */
export function validationLines(result: unknown): string[] {
  if (!result || typeof result !== 'object') return [];
  const r = result as Loose;
  const problems = problemList(r);
  if (!Array.isArray(problems)) return typeof r.message === 'string' && r.valid === false ? [r.message] : [];
  return problems.map(problemLine);
}

// ---------- verbs ----------

/**
 * One name per operation, the same in the button, the dialog title, the confirm button, the
 * notice and the lineage line. With stock Zeroshot a new run can only start again from a
 * checkpoint unchanged (`zeroshot resume --from-checkpoint`, rolling the workspace back);
 * "Replay with changes" is offered only by a Zeroshot that can edit a replay (`replay` capability).
 * Resume (continue from where it stopped) is the run page's other verb (RunDialogs.tsx).
 */
export type ReplayVerbs = {
  /** The button that opens the dialog; with a node, the failure card's button for that node. */
  button: string;
  /** The button's tooltip. */
  hint: string;
  /** The dialog title. */
  title: string;
  /** The dialog's button that leads to the confirm step. */
  next: string;
  /** The confirm step's title and its button. */
  confirmTitle: string;
  confirm: string;
  /** The notice once it started. */
  started: string;
  /** The lineage line of the new run and of the run it came from. */
  lineageFrom: string;
  lineageAs: string;
};

export function replayVerbs(canEdit: boolean, node?: string | null, checkpoint?: string | null): ReplayVerbs {
  if (canEdit) {
    return {
      button: node ? `Replay with changes from ${node}…` : 'Replay with changes…',
      hint: 'Start a new run from a checkpoint, changing prompts or models',
      title: 'Replay with changes',
      next: 'Replay with changes…',
      confirmTitle: checkpoint ? `Replay with changes from ${checkpoint}?` : 'Replay with changes?',
      confirm: 'Replay with changes',
      started: 'Replay with changes started.',
      lineageFrom: 'Replayed with changes from',
      lineageAs: 'Replayed with changes as',
    };
  }
  return {
    button: node ? `Start again from ${node}…` : 'Start again from…',
    hint: 'Start a new run from a checkpoint you pick, with the same settings',
    title: 'Start again from a checkpoint',
    next: 'Start again…',
    confirmTitle: checkpoint ? `Start again from ${checkpoint}?` : 'Start again?',
    confirm: 'Start again',
    started: checkpoint ? `Started again from the ${checkpoint} checkpoint.` : 'Started again.',
    // Zeroshot 16.1 records a run started again from a checkpoint as `resumedFrom`, the same as a
    // Resume, and not which checkpoint it took, so the lineage line cannot tell the two apart.
    lineageFrom: 'Resumed from',
    lineageAs: 'Resumed as',
  };
}

/**
 * What the confirm step says is rolled back. A local checkpoint restore discards the workspace's
 * changes made after it (`--discard-workspace-changes`); Zeroshot's checkpoint list does not say
 * which files those are, so it says so in one sentence.
 */
export function rollbackText(checkpoint: string): string {
  return `The workspace goes back to the ${checkpoint} checkpoint: changes made in it after that point are discarded.`;
}

/** The Resume dialog's sentence: what Resume does, and where changes go when they are possible. */
export function resumeText(canEdit: boolean): string {
  const base = 'Continues in a new run from where this one stopped (its latest checkpoint), with the same graph, prompts and models. The workspace is kept as it is.';
  return canEdit ? `${base} To change settings, use Replay with changes.` : base;
}

/**
 * Starting a new run from a checkpoint rolls the workspace back, so it takes two steps: the
 * dialog's button asks (`ask`), and only the confirm step's button sends (`confirm`). Cancel
 * returns to the checkpoint list and sends nothing. `send` runs only on a confirm that follows
 * an ask; the answer is the next stage.
 */
export type StartStage = 'choose' | 'confirm';
export type StartEvent = 'ask' | 'cancel' | 'confirm';

export async function startAgainStep(stage: StartStage, event: StartEvent, send: () => Promise<void>): Promise<StartStage> {
  if (event === 'ask') return 'confirm';
  if (event === 'cancel' || stage !== 'confirm') return 'choose';
  await send();
  return 'choose';
}

/** The note on the checkpoint the run failed (or was interrupted) at, naming the node the
 * failure card's button names: `acceptance failed here`. */
export function failedHereNote(view: Pick<RunView, 'status' | 'failure'>): string {
  const what = view.status === 'failed' ? 'failed here' : 'interrupted here';
  return view.failure?.node ? `${view.failure.node} ${what}` : what;
}

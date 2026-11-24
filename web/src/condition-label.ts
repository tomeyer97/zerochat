/**
 * Graph conditions in words: a branch's guard (`when`) as the short label drawn on its edge and
 * the full condition for its tooltip, plus the node names a condition reads. graph-model.ts
 * re-exports all of it.
 */
import { ERROR_WORDS } from './generated/projection';

/** `change_attempts_exhausted` → `change attempts exhausted`. */
export function humanId(id: string): string {
  return id.replace(/#\d+$/, '').replace(/[_-]+/g, ' ').trim();
}

export function shortLabel(label: string, max = 26) {
  return label.length > max ? `${label.slice(0, max - 1)}…` : label;
}

/** Zeroshot's error labels: a guard on all of them reads `errored`. */
const ERROR_LABELS = Object.keys(ERROR_WORDS);

/**
 * A guard as the graph spells it: `in` (the node `value.name` produced one of `labels`), `any` /
 * `all` over `guards`, or `not` of one `guard`.
 */
type Guard = {
  kind?: string;
  labels?: string[];
  value?: { field?: string | null; name?: string; source?: string };
  guards?: Guard[];
  guard?: Guard;
};

/** What an `in` guard asks of its node: `accepted`, `errored`, `risk level: high or very high`. */
function predicate(g: Guard): string {
  const labels = g.labels ?? [];
  if (g.value?.source === 'error') {
    const all = ERROR_LABELS.every((l) => labels.includes(l)) && labels.length === ERROR_LABELS.length;
    return all ? 'errored' : labels.map((l) => ERROR_WORDS[l] ?? humanId(l)).join(' or ');
  }
  const words = labels.map(humanId).join(' or ');
  const field = g.value?.field;
  return field && field !== 'verdict' ? `${humanId(field)}: ${words}` : words;
}

/** Names of the nodes a condition reads. */
export function guardNames(when: unknown): string[] {
  const g = when as Guard | undefined;
  if (!g || typeof g !== 'object') return [];
  if (Array.isArray(g.guards)) return [...new Set(g.guards.flatMap(guardNames))];
  if (g.guard) return guardNames(g.guard);
  return g.value?.name ? [g.value.name] : [];
}

function isCombined(g: Guard): g is Guard & { kind: 'any' | 'all'; guards: Guard[] } {
  return (g.kind === 'any' || g.kind === 'all') && Array.isArray(g.guards);
}

/** The whole condition, every node named: `all of: a accepted; b accepted`. */
function fullCondition(g: Guard): string {
  if (g.kind === 'in') return `${g.value?.name ?? '?'} ${predicate(g)}`;
  if (isCombined(g)) return `${g.kind === 'all' ? 'all of' : 'any of'}: ${g.guards.map(fullCondition).join('; ')}`;
  if (g.kind === 'not' && g.guard) return `not (${fullCondition(g.guard)})`;
  return JSON.stringify(g);
}

/** The node a guard reads, unless it is one of the `local` names the edge already implies. */
function subjectOf(g: Guard, local: ReadonlySet<string>): string | undefined {
  return g.value?.name && !local.has(g.value.name) ? g.value.name : undefined;
}

/**
 * `any` / `all` of `in` guards that all ask the same predicate `p`:
 * - every node implied: `any accepted`, `both accepted` (not the machine-phrased "all 2
 *   accepted"), `all 3 accepted`;
 * - two named nodes: `a and b accepted`, `a or b accepted`;
 * - more named nodes: `all 3 accepted`, `any of 3 accepted`.
 */
function samePredicateLabel(kind: 'any' | 'all', guards: Guard[], p: string, local: ReadonlySet<string>): string {
  const n = guards.length;
  if (guards.every((x) => !subjectOf(x, local))) {
    if (kind === 'any') return `any ${p}`;
    return n === 2 ? `both ${p}` : `all ${n} ${p}`;
  }
  if (n === 2) return `${guards.map((x) => x.value?.name ?? '?').join(kind === 'all' ? ' and ' : ' or ')} ${p}`;
  return kind === 'all' ? `all ${n} ${p}` : `any of ${n} ${p}`;
}

/** The short label of `any` / `all`. */
function combinedLabel(g: Guard & { kind: 'any' | 'all'; guards: Guard[] }, local: ReadonlySet<string>): string {
  const guards = g.guards;
  if (guards.length === 1) return conditionLabel(guards[0], local).label;
  const predicates = new Set(guards.map((x) => (x.kind === 'in' ? predicate(x) : '')));
  if (guards.every((x) => x.kind === 'in') && predicates.size === 1) return samePredicateLabel(g.kind, guards, [...predicates][0], local);
  return `${g.kind} of ${guards.length} conditions`;
}

/**
 * A short label and the full condition. Names in `local` (the node the edge leaves, or the
 * members of the group it leaves) are implied and left out: `all 4 accepted`, `errored`,
 * `bugfinder accepted`.
 */
export function conditionLabel(when: unknown, local: ReadonlySet<string> = new Set()): { label: string; title: string } {
  const g = when as Guard | undefined;
  if (!g || typeof g !== 'object') return { label: 'if', title: JSON.stringify(when) };
  const title = `when ${fullCondition(g)}`;
  if (g.kind === 'in' && g.value) {
    const subject = subjectOf(g, local);
    return { label: `${subject ? `${subject} ` : ''}${predicate(g)}`, title };
  }
  if (isCombined(g)) return { label: combinedLabel(g, local), title };
  if (g.kind === 'not' && g.guard) return { label: `not ${conditionLabel(g.guard, local).label}`, title };
  return { label: g.kind ?? 'if', title };
}

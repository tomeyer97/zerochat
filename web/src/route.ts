/**
 * Hash routes:
 *   `#/run/<target>/<runId>`
 *   `#/run/<target>/<runId>/files`
 *   `#/run/<target>/<runId>/node/<name>[/round/<k>[/attempt/<a>]]`
 *   `#/run/<target>/<runId>/node/<name>/exec/<id>` (a visit of a map item, by execution id)
 *   `#/benchmarks`
 *   `#/benchmarks/batch/<id>`
 *   `#/benchmarks/compare?a=<batch>&b=<batch>`
 *   `#/benchmarks/start`
 *   `#/benchmarks/sweeps`
 *   `#/benchmarks/sweep-report?ids=<comma>&label=&task=&budget=`
 *   `#/features`
 *   `#/features/<name>[/slice/<id>]`
 * A bare number after the node name (`/node/<name>/5`, older links) is an execution id.
 * Query strings (`?utm_…`) are ignored, except on the `benchmarks/compare` and
 * `benchmarks/sweep-report` routes, which carry their own state in the query (`a`/`b` batch ids;
 * `ids`/`label`/`task`/`budget` sweep filters) rather than the path, so the compare/report can be
 * linked to directly.
 *
 * `parseRoute` reads a hash section by section (one function per first segment); `formatRoute`
 * writes one back, one `case` per route kind.
 */
import { queryString } from './url';

export type Route =
  | { kind: 'home' }
  | { kind: 'unknown'; path: string }
  | {
      kind: 'run';
      target: string;
      run: string;
      node?: string;
      round?: number;
      attempt?: number;
      execution?: number;
      files?: boolean;
    }
  | { kind: 'benchmarks' }
  | { kind: 'benchmarksBatch'; id: string }
  | { kind: 'features' }
  | { kind: 'feature'; name: string; slice?: string }
  | { kind: 'benchmarksCompare'; a?: string; b?: string }
  | { kind: 'benchmarksStart' }
  | { kind: 'benchmarksSweeps' }
  | { kind: 'benchmarksSweepReport'; ids: string[]; label?: string; task?: string; budget?: string };

export type Visit = { round?: number; attempt?: number; execution?: number };

type RunRoute = Extract<Route, { kind: 'run' }>;

// ---------- parsing ----------

/** The route a hash names. A path no section recognises (a typo, a stale link, a route from a
 * future version) is `unknown`, so the page can say so instead of silently showing the run list. */
export function parseRoute(hash: string): Route {
  const [path, rawQuery] = hash.replace(/^#\/?/, '').split('?');
  const [section, ...parts] = path.split('/').filter(Boolean).map(safeDecode);
  const query = new URLSearchParams(rawQuery ?? '');
  // A bare `#/run` or `#/runs` names the "Runs" tab, which is the home page.
  if (section === undefined || ((section === 'run' || section === 'runs') && parts.length === 0)) return { kind: 'home' };
  const route = section === 'benchmarks' ? parseBenchmarks(parts, query) : section === 'features' ? parseFeatures(parts) : section === 'run' ? parseRun(parts) : null;
  return route ?? { kind: 'unknown', path };
}

/** `benchmarks/…`; `null` when the rest of the path names no page. */
function parseBenchmarks([page, id]: string[], query: URLSearchParams): Route | null {
  if (page === undefined) return { kind: 'benchmarks' };
  // Anything after the batch id is ignored.
  if (page === 'batch') return id ? { kind: 'benchmarksBatch', id } : null;
  if (id !== undefined) return null;
  const param = (name: string) => query.get(name) ?? undefined;
  switch (page) {
    case 'compare':
      return { kind: 'benchmarksCompare', a: param('a'), b: param('b') };
    case 'start':
      return { kind: 'benchmarksStart' };
    case 'sweeps':
      return { kind: 'benchmarksSweeps' };
    case 'sweep-report': {
      const ids = (param('ids') ?? '')
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean);
      return { kind: 'benchmarksSweepReport', ids, label: param('label'), task: param('task'), budget: param('budget') };
    }
  }
  return null;
}

/** `features[/<name>[/slice/<id>]]`. */
function parseFeatures(parts: string[]): Route | null {
  const [name, sliceWord, slice] = parts;
  if (name === undefined) return { kind: 'features' };
  if (parts.length === 1) return { kind: 'feature', name };
  if (parts.length === 3 && sliceWord === 'slice') return { kind: 'feature', name, slice };
  return null;
}

/** `run/<target>/<run>[/files | /node/<name>[/…]]`; a node path it cannot read keeps just the node. */
function parseRun(parts: string[]): Route | null {
  if (parts.length < 2) return null;
  const [target, run, section, name, ...rest] = parts;
  const route: RunRoute = { kind: 'run', target, run };
  if (section === 'node' && name) {
    route.node = name;
    Object.assign(route, parseVisit(rest));
  } else if (section === 'files') {
    route.files = true;
  }
  return route;
}

/** After `node/<name>`: `round/<k>[/attempt/<a>]`, `exec/<id>`, or a bare execution id. */
function parseVisit(rest: string[]): Visit {
  if (rest[0] === 'round') {
    const round = count(rest[1]);
    if (round == null) return {};
    const attempt = rest[2] === 'attempt' ? count(rest[3]) : undefined;
    return attempt == null ? { round } : { round, attempt };
  }
  const execution = count(rest[0] === 'exec' ? rest[1] : rest[0]);
  return execution == null ? {} : { execution };
}

/** A whole, non-negative number, or `undefined`. */
function count(text: string | undefined) {
  const n = Number(text);
  return text && Number.isInteger(n) && n >= 0 ? n : undefined;
}

function safeDecode(part: string) {
  try {
    return decodeURIComponent(part);
  } catch {
    return part;
  }
}

// ---------- formatting ----------

export function formatRoute(route: Route): string {
  const e = encodeURIComponent;
  switch (route.kind) {
    case 'home':
      return '#/';
    case 'unknown':
      return `#/${route.path}`;
    case 'run':
      return formatRunRoute(route);
    case 'benchmarks':
      return '#/benchmarks';
    case 'benchmarksBatch':
      return `#/benchmarks/batch/${e(route.id)}`;
    case 'benchmarksCompare':
      return `#/benchmarks/compare${queryString({ a: route.a, b: route.b })}`;
    case 'benchmarksStart':
      return '#/benchmarks/start';
    case 'benchmarksSweeps':
      return '#/benchmarks/sweeps';
    case 'benchmarksSweepReport': {
      const { ids, label, task, budget } = route;
      return `#/benchmarks/sweep-report${queryString({ ids: ids.join(','), label, task, budget })}`;
    }
    case 'features':
      return '#/features';
    case 'feature':
      return `#/features/${e(route.name)}${route.slice ? `/slice/${e(route.slice)}` : ''}`;
  }
}

function formatRunRoute(route: RunRoute): string {
  const e = encodeURIComponent;
  let hash = `#/run/${e(route.target)}/${e(route.run)}`;
  if (route.node) {
    hash += `/node/${e(route.node)}`;
    if (route.round != null) {
      hash += `/round/${route.round}`;
      if (route.attempt != null && route.attempt > 1) hash += `/attempt/${route.attempt}`;
    } else if (route.execution != null) {
      hash += `/exec/${route.execution}`;
    }
  } else if (route.files) {
    hash += '/files';
  }
  return hash;
}

// ---------- links ----------

// Shorthands for `formatRoute`, one per page the other pages link to.
export const runHref = (target: string, run: string) => formatRoute({ kind: 'run', target, run });
export const benchmarksHref = () => formatRoute({ kind: 'benchmarks' });
export const benchmarksBatchHref = (id: string) => formatRoute({ kind: 'benchmarksBatch', id });
export const benchmarksCompareHref = (a?: string, b?: string) => formatRoute({ kind: 'benchmarksCompare', a, b });
export const benchmarksStartHref = () => formatRoute({ kind: 'benchmarksStart' });
export const benchmarksSweepsHref = () => formatRoute({ kind: 'benchmarksSweeps' });
export const featuresHref = () => formatRoute({ kind: 'features' });
export const featureHref = (name: string, slice?: string) => formatRoute({ kind: 'feature', name, slice });

/** The sweep report for `ids`, optionally narrowed to one label, task or budget. */
export function benchmarksSweepReportHref(ids: string[], extra: { label?: string; task?: string; budget?: string } = {}) {
  return formatRoute({ kind: 'benchmarksSweepReport', ids, ...extra });
}

/** One node of a run, at one visit (`visitOf`) or its latest when `visit` is empty. */
export function nodeHref(target: string, run: string, node: string, visit: Visit = {}) {
  return formatRoute({ kind: 'run', target, run, node, ...visit });
}

/** How a link names one execution: by round (and attempt), or by id for map items. */
export function visitOf(e: { id: number; round: number; attempt: number; mapIndices?: unknown }): Visit {
  if (Array.isArray(e.mapIndices) && e.mapIndices.length > 0) return { execution: e.id };
  return { round: e.round, attempt: e.attempt > 1 ? e.attempt : undefined };
}

/**
 * A link written as a path (`/runs/local/<id>`, `/run/…`, `/features/calc`, `/benchmarks/batch/7`)
 * as the hash route it means (`#/run/local/<id>`), so a pasted or hand-typed link opens what it
 * names; the server answers every such path with the app shell. `runs/` is read as `run/`, and a
 * bare `/runs` or `/run` (the "Runs" tab) is the run list, `#/`. `null`
 * when the path is the app's root (nothing to map) or the page already has a hash route.
 */
export function hashFromPath(pathname: string, search: string, hash: string): string | null {
  if (hash.replace(/^#\/?/, '')) return null;
  const path = pathname.replace(/^\/+/, '').replace(/\/+$/, '');
  if (!path || path === 'index.html') return null;
  const [first, ...rest] = path.split('/');
  const section = first === 'runs' ? 'run' : first;
  // The run list is the home page (`#/`); `/runs` is how the "Runs" tab reads as a path.
  if (section === 'run' && rest.length === 0) return '#/';
  return `#/${[section, ...rest].join('/')}${search}`;
}

/** Goes to `route`; `replace` swaps the current history entry instead of adding one. */
export function navigate(route: Route, replace = false) {
  const hash = formatRoute(route);
  if (replace) history.replaceState(null, '', hash);
  else if (location.hash !== hash) location.hash = hash;
  if (replace) window.dispatchEvent(new HashChangeEvent('hashchange'));
}

// ---------- tab titles ----------

/** A tab title: its parts, most specific first, then `zerochat` (`Compare · Benchmarks · zerochat`). */
export function documentTitle(...parts: (string | null | undefined)[]): string {
  return [...parts.filter(Boolean), 'zerochat'].join(' · ');
}

/**
 * The tab title of each page; `null` for the pages that name the tab themselves from what they
 * load (a run, a feature). The app shell prefixes `(n) ` while features need a person.
 */
export function pageTitle(route: Route): string | null {
  switch (route.kind) {
    case 'home':
      return documentTitle();
    case 'unknown':
      return documentTitle('Page not found');
    case 'run':
    case 'feature':
      return null;
    case 'features':
      return documentTitle('Features');
    case 'benchmarks':
      return documentTitle('Benchmarks');
    case 'benchmarksBatch':
      return documentTitle(route.id, 'Benchmarks');
    case 'benchmarksCompare':
      return documentTitle('Compare', 'Benchmarks');
    case 'benchmarksStart':
      return documentTitle('Start a batch', 'Benchmarks');
    case 'benchmarksSweeps':
      return documentTitle('Sweeps', 'Benchmarks');
    case 'benchmarksSweepReport':
      return documentTitle('Sweep report', 'Benchmarks');
  }
}

// The execution a node route names is run-data logic; it lives with the other execution helpers.
export { resolveVisit, visitFallback } from './exec-state';

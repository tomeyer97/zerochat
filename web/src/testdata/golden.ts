/**
 * Golden (snapshot) files for the web models, the same mechanism as the Rust side
 * (`crates/zerochat/tests/goldens/golden.rs`): a value is rendered as JSON with recursively sorted
 * object keys, a two-space indent and a trailing newline, and compared byte for byte with
 * `src/testdata/golden/<name>.json`. On a mismatch the test fails with the first differing lines
 * and writes `<name>.actual.json` next to the golden (gitignored). `UPDATE_GOLDENS=1` writes the
 * goldens instead. A missing golden is a failure that says how to create it.
 *
 * Importing this module also pins the environment the models format with, so the goldens do not
 * depend on the machine: the time zone is UTC, `Date.now()` is a fixed instant (`FIXED_NOW`, the
 * `now` of the API snapshots), and locale-sensitive calls made without a locale use `en-US`.
 * `node --test` runs every test file in its own process, so this never leaks into other tests.
 *
 * Inputs are the Rust side's snapshots of the server's JSON API
 * (`crates/zerochat/testdata/golden/api/**`, each file `{status, body}`), so the server's output
 * and what the UI makes of it are pinned together.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// ---------- a fixed environment ----------

/** The `now` every API snapshot was taken at. */
export const FIXED_NOW = 1795003600000;

process.env.TZ = 'UTC';
Date.now = () => FIXED_NOW;

const LOCALE = 'en-US';
type LocaleFn = (this: unknown, locales?: Intl.LocalesArgument, ...rest: unknown[]) => unknown;
function pinLocale(proto: object, method: string, localeArg: number) {
  const target = proto as Record<string, LocaleFn>;
  const original = target[method];
  target[method] = function (this: unknown, ...args: unknown[]) {
    if (args[localeArg] === undefined) args[localeArg] = LOCALE;
    return (original as (...a: unknown[]) => unknown).apply(this, args);
  } as LocaleFn;
}
pinLocale(Date.prototype, 'toLocaleString', 0);
pinLocale(Date.prototype, 'toLocaleDateString', 0);
pinLocale(Date.prototype, 'toLocaleTimeString', 0);
pinLocale(Number.prototype, 'toLocaleString', 0);
pinLocale(String.prototype, 'localeCompare', 1);

// ---------- paths ----------

const here = path.dirname(fileURLToPath(import.meta.url));
export const GOLDEN_DIR = path.join(here, 'golden');
/** The server's API responses, as the Rust goldens snapshot them. */
export const API_DIR = path.resolve(here, '../../../crates/zerochat/testdata/golden/api');

function updating(): boolean {
  const v = process.env.UPDATE_GOLDENS;
  return !!v && v !== '0';
}

// ---------- rendering ----------

/**
 * A value as plain JSON data: object keys sorted; `undefined`, `NaN` and the infinities spelled
 * out (JSON would drop or null them); a `Map` as `{"<Map>": [[key, value], …]}` and a `Set` as
 * `{"<Set>": […]}` in insertion order; a function as `"<function>"`. An object that is the very
 * same object as one in the inputs given in `refs` is written as `"<ref:path>"`: what a model
 * passes through untouched is pinned by identity instead of being copied into the golden again.
 */
export function toData(value: unknown, refs: Map<object, string> = new Map()): unknown {
  const seen = new Set<object>();
  const walk = (v: unknown): unknown => {
    if (v === undefined) return '<undefined>';
    if (typeof v === 'number') {
      if (Number.isNaN(v)) return '<NaN>';
      if (v === Infinity) return '<Infinity>';
      if (v === -Infinity) return '<-Infinity>';
      if (Object.is(v, -0)) return '<-0>';
      return v;
    }
    if (typeof v === 'function') return '<function>';
    if (typeof v === 'bigint') return `<bigint:${v}>`;
    if (v === null || typeof v !== 'object') return v;
    const ref = refs.get(v);
    if (ref) return `<ref:${ref}>`;
    if (seen.has(v)) return '<cycle>';
    seen.add(v);
    let out: unknown;
    if (v instanceof Map) out = { '<Map>': [...v.entries()].map(([k, x]) => [walk(k), walk(x)]) };
    else if (v instanceof Set) out = { '<Set>': [...v].map(walk) };
    else if (Array.isArray(v)) out = v.map(walk);
    else {
      const obj = v as Record<string, unknown>;
      const sorted: Record<string, unknown> = {};
      for (const key of Object.keys(obj).sort()) sorted[key] = walk(obj[key]);
      out = sorted;
    }
    seen.delete(v);
    return out;
  };
  return walk(value);
}

/** Every object inside the named inputs, by path (`view.executions[3]`), for `toData`'s refs. */
export function refsOf(inputs: Record<string, unknown>): Map<object, string> {
  const refs = new Map<object, string>();
  const walk = (v: unknown, at: string) => {
    if (v === null || typeof v !== 'object' || refs.has(v)) return;
    refs.set(v, at);
    if (Array.isArray(v)) v.forEach((x, i) => walk(x, `${at}[${i}]`));
    else for (const [k, x] of Object.entries(v)) walk(x, /^[A-Za-z_$][\w$]*$/.test(k) ? `${at}.${k}` : `${at}[${JSON.stringify(k)}]`);
  };
  for (const [name, v] of Object.entries(inputs)) walk(v, name);
  return refs;
}

export function render(value: unknown, refs?: Map<object, string>): string {
  return `${JSON.stringify(toData(value, refs), null, 2)}\n`;
}

// ---------- comparing ----------

function firstDifferences(expected: string, actual: string, max = 20): string {
  const e = expected.split('\n');
  const a = actual.split('\n');
  const out: string[] = [];
  for (let i = 0; i < Math.max(e.length, a.length) && out.length < max; i++) {
    if (e[i] === a[i]) continue;
    if (e[i] !== undefined) out.push(`${i + 1}- ${e[i]}`);
    if (a[i] !== undefined) out.push(`${i + 1}+ ${a[i]}`);
  }
  return out.join('\n');
}

function actualPath(file: string): string {
  const ext = path.extname(file);
  return `${file.slice(0, file.length - ext.length)}.actual${ext || '.txt'}`;
}

/** Compares `actual` with the golden file `name` (a path under `golden/`, with its extension). */
export function checkGoldenFile(name: string, actual: string): void {
  const file = path.join(GOLDEN_DIR, name);
  const actualFile = actualPath(file);
  if (updating()) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, actual);
    fs.rmSync(actualFile, { force: true });
    return;
  }
  let expected: string;
  try {
    expected = fs.readFileSync(file, 'utf8');
  } catch {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(actualFile, actual);
    assert.fail(`golden ${name}: no golden file yet. Review ${path.relative(process.cwd(), actualFile)} and run with UPDATE_GOLDENS=1 to accept it.`);
  }
  if (expected === actual) {
    fs.rmSync(actualFile, { force: true });
    return;
  }
  fs.writeFileSync(actualFile, actual);
  assert.fail(
    `golden ${name} changed:\n${firstDifferences(expected, actual)}\nactual output: ${path.relative(process.cwd(), actualFile)}\n` +
      '(if the change is intended: UPDATE_GOLDENS=1 npm --prefix web test, then review the diff)'
  );
}

/** Compares `value`, rendered as sorted JSON, with `golden/<name>.json`. */
export function checkGolden(name: string, value: unknown, refs?: Map<object, string>): void {
  checkGoldenFile(`${name}.json`, render(value, refs));
}

/** Compares a string output with `golden/<name>.txt` (a trailing newline is added). */
export function checkGoldenText(name: string, text: string): void {
  checkGoldenFile(`${name}.txt`, text.endsWith('\n') ? text : `${text}\n`);
}

// ---------- the API snapshots ----------

export type ApiResponse<T = unknown> = { status: number; body: T };

export function apiFile<T = unknown>(rel: string): ApiResponse<T> {
  return JSON.parse(fs.readFileSync(path.join(API_DIR, rel), 'utf8')) as ApiResponse<T>;
}

/** Snapshot files under `dir` (recursively) whose path matches `pattern`, sorted, relative to the API root. */
export function apiFiles(dir: string, pattern: RegExp = /\.json$/): string[] {
  const out: string[] = [];
  const walk = (rel: string) => {
    for (const entry of fs.readdirSync(path.join(API_DIR, rel), { withFileTypes: true })) {
      const child = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(child);
      else if (pattern.test(child)) out.push(child);
    }
  };
  walk(dir);
  return out.sort();
}

/** `{ "<name>": body }` of every 200 snapshot in `dir` matching `pattern`; `name` drops `dir/` and `.json`. */
export function apiBodies<T>(dir: string, pattern: RegExp = /\.json$/): [string, T][] {
  const out: [string, T][] = [];
  for (const rel of apiFiles(dir, pattern)) {
    const response = apiFile<T>(rel);
    if (response.status === 200) out.push([rel.slice(dir.length + 1).replace(/\.json$/, ''), response.body]);
  }
  return out;
}

/** Calls `f` on each named input, collecting `{name: result}`; a throw is recorded as `{"<throws>": message}`. */
export function each<T>(inputs: [string, T][], f: (input: T, name: string) => unknown): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [name, input] of inputs) out[name] = attempt(() => f(input, name));
  return out;
}

/** The result of `f`, or `{"<throws>": message}` when it throws (a throw is behaviour too). */
export function attempt(f: () => unknown): unknown {
  try {
    return f();
  } catch (cause) {
    return { '<throws>': cause instanceof Error ? `${cause.name}: ${cause.message}` : String(cause) };
  }
}

/**
 * How `value` differs from `base`, as `{path: new value}` (`"<removed>"` for a key or item that is
 * gone): the whole of `value` follows from `base` and this list, so a golden can pin a large output
 * that is mostly a copy of its input by its changes alone.
 */
export function jsonChanges(base: unknown, value: unknown, at = '$'): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const isObj = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object';
  if (isObj(base) && isObj(value) && Array.isArray(base) === Array.isArray(value)) {
    const keys = new Set([...Object.keys(base), ...Object.keys(value)]);
    for (const key of [...keys].sort()) {
      const child = Array.isArray(value) ? `${at}[${key}]` : /^[A-Za-z_$][\w$]*$/.test(key) ? `${at}.${key}` : `${at}[${JSON.stringify(key)}]`;
      if (!(key in value)) out[child] = '<removed>';
      else if (!(key in base)) out[child] = (value as Record<string, unknown>)[key];
      else Object.assign(out, jsonChanges((base as Record<string, unknown>)[key], (value as Record<string, unknown>)[key], child));
    }
    return out;
  }
  if (!Object.is(base, value)) out[at] = value;
  return out;
}

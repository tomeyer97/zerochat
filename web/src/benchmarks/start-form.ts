// The Start-a-batch form: its state, which tasks a selection runs, what it could spend, its
// defaults, its validation and the request it sends.
import type { BatchSummary, Harness, HarnessOptions, RunSlots, StartBatchRequest, TaskSet } from '../types';
import { plural, usd } from '../format';
import { spendCeiling } from './batches';
import { toggleIn } from './batch-list';

/** The task ids a selection of task-set names and individual task ids resolves to (sets expand
 * to their members; a plain id is used as-is), in first-seen order with duplicates dropped:
 * the same rule pb's own `Config.resolve_tasks` applies, so the spend preview's task count
 * matches what `pb run --tasks` will actually run. */
export function resolveTasks(setNames: string[], taskIds: string[], taskSets: TaskSet[]): string[] {
  const byName = new Map(taskSets.map((s) => [s.name, s.tasks]));
  const out: string[] = [];
  const seen = new Set<string>();
  for (const part of [...setNames, ...taskIds]) {
    for (const t of byName.get(part) ?? [part]) {
      if (!seen.has(t)) {
        seen.add(t);
        out.push(t);
      }
    }
  }
  return out;
}

/** What goes in the `tasks` field of the start request: pb's own `--tasks` syntax (set names
 * and/or task ids, comma separated); pb resolves it exactly like `resolveTasks` above. */
export function tasksArg(setNames: string[], taskIds: string[]): string {
  return [...setNames, ...taskIds].join(',');
}

/** `runs × cap`, shown before Start so a typo in seeds or cap is caught before any money is
 * spent. `null` when there is nothing to run yet (no task picked) or no cost cap applies. */
export function maxSpend(taskCount: number, seeds: number, maxCostUsd: number | null | undefined): number | null {
  if (taskCount <= 0 || seeds <= 0 || maxCostUsd == null || maxCostUsd <= 0) return null;
  return spendCeiling(taskCount * seeds, maxCostUsd);
}

export type StartField = 'tasks' | 'label' | 'seeds' | 'parallel' | 'harness' | 'maxCost' | 'maxMinutes' | 'maxSteps';

export type StartFormValues = {
  taskCount: number;
  label: string;
  seeds: number;
  parallel: string;
  harnessKind: HarnessKind;
  variant: string;
  customGraph: string;
  customRuntime: string;
  maxCost: string;
  maxMinutes: string;
  maxSteps: string;
  /** Errors the harness preview found (files missing, not a graph, ...): Start stays blocked. */
  harnessErrors?: string[];
};

const positive = (raw: string) => raw.trim() === '' || (Number.isFinite(Number(raw)) && Number(raw) > 0);
const wholePositive = (raw: string) => raw.trim() === '' || (Number.isInteger(Number(raw)) && Number(raw) > 0);

/** Every problem with the start form at once, keyed by field, so each shows next to its field
 * before Start rather than one at a time after it. The same rules the server applies
 * (`pb_exec::start_batch_args`), so a form that passes here is not refused there for a field
 * the person could have fixed. In field order, so the summary next to Start reads top-down. */
export function validateStart(v: StartFormValues): Partial<Record<StartField, string>> {
  const errors: Partial<Record<StartField, string>> = {};
  if (v.taskCount === 0) errors.tasks = 'Pick at least one task set or task.';
  if (!v.label.trim()) errors.label = 'A label is required: it names the batch in the list and in compares.';
  if (!Number.isInteger(v.seeds) || v.seeds < 1) errors.seeds = 'At least 1 seed.';
  if (!wholePositive(v.parallel)) errors.parallel = 'A whole number, at least 1.';
  if (v.harnessKind === 'variant' && !v.variant) errors.harness = 'Choose a harness variant.';
  else if (v.harnessKind === 'custom' && (!v.customGraph.trim() || !v.customRuntime.trim())) errors.harness = 'Choose both a graph and a runtime file.';
  else if (v.harnessErrors && v.harnessErrors.length > 0) errors.harness = v.harnessErrors.join(' ');
  if (!positive(v.maxCost)) errors.maxCost = 'A positive dollar amount.';
  if (!positive(v.maxMinutes)) errors.maxMinutes = 'A positive number of minutes.';
  if (!wholePositive(v.maxSteps)) errors.maxSteps = 'A whole number, at least 1.';
  return errors;
}

export const START_FIELD_LABEL: Record<StartField, string> = {
  tasks: 'Tasks',
  label: 'Label',
  seeds: 'Seeds',
  parallel: 'Parallel',
  harness: 'Harness',
  maxCost: 'Max cost',
  maxMinutes: 'Max minutes',
  maxSteps: 'Max steps',
};

/** Which ticked task sets include a task, so the Individual tasks list shows a set's members
 * as included. */
export function setsIncluding(task: string, setNames: string[], taskSets: TaskSet[]): string[] {
  return taskSets.filter((s) => setNames.includes(s.name) && s.tasks.includes(task)).map((s) => s.name);
}

/** pb's own default for `[limits] max_concurrent_runs`. */
export const DEFAULT_MAX_CONCURRENT_RUNS = 4;

/** The Start form's Parallel default: pb's default concurrency, capped by the host's run-slot
 * ceiling (more would only queue). */
export function defaultParallel(maxConcurrentRuns?: number | null): number {
  const max = maxConcurrentRuns && maxConcurrentRuns >= 1 ? maxConcurrentRuns : DEFAULT_MAX_CONCURRENT_RUNS;
  return Math.min(DEFAULT_MAX_CONCURRENT_RUNS, max);
}

/** What the Start form says while other batches run. They do not block Start: pb runs batches
 * concurrently, and every run (any batch) waits for one of the host's `max_concurrent_runs` slots.
 * `slots` is `pb slots` (null while unknown, e.g. an older pb without it). */
export function concurrencyNote(running: Pick<BatchSummary, 'id' | 'label'>[], slots: RunSlots | null, maxConcurrentRuns?: number | null): string | null {
  const max = slots?.maxConcurrentRuns ?? maxConcurrentRuns ?? DEFAULT_MAX_CONCURRENT_RUNS;
  const parts: string[] = [];
  if (running.length) parts.push(`${running.length} batch${running.length === 1 ? ' is' : 'es are'} already running`);
  if (slots && (slots.running || slots.queued)) {
    const queued = slots.queued ? `, ${slots.queued} run${slots.queued === 1 ? '' : 's'} queued` : '';
    parts.push(`${slots.running} of ${max} run slots on this host are busy${queued}`);
  }
  if (!parts.length) return null;
  return `${parts.join('; ')}. A new batch starts now; its runs queue for a free slot (at most ${max} runs at once across all batches).`;
}

/** What the form shows until `benchmarks/start-options` has loaded: no lists yet, and pb.toml's
 * usual limits as placeholders. The server's own values replace all of it once it answers. */
export function defaultHarnessOptions(): HarnessOptions {
  return {
    taskSets: [],
    tasks: [],
    repoTasks: [],
    variants: [],
    defaultMaxCostUsd: 5,
    defaultMaxWallMinutes: 45,
    defaultMaxSteps: 12,
  };
}

/** The Seeds default. 3, as the lab's baselines use, not 1: with one seed every compare against
 * the new batch reads "needs 2 seeds", so the experiment could never be decided. */
export const DEFAULT_SEEDS = 3;

export type HarnessKind = 'default' | 'variant' | 'custom';

/** Everything the person has entered. Numbers they type stay strings (empty = pb's default), so
 * a half-typed value is kept as typed; Seeds is always a whole number. */
export type StartForm = {
  setNames: string[];
  taskIds: string[];
  seeds: number;
  label: string;
  harnessKind: HarnessKind;
  variant: string;
  customGraph: string;
  customRuntime: string;
  model: string;
  maxCost: string;
  maxMinutes: string;
  maxSteps: string;
  parallel: string;
};

export function emptyStartForm(): StartForm {
  return {
    setNames: [],
    taskIds: [],
    seeds: DEFAULT_SEEDS,
    label: '',
    harnessKind: 'default',
    variant: '',
    customGraph: '',
    customRuntime: '',
    model: '',
    maxCost: '',
    maxMinutes: '',
    maxSteps: '',
    parallel: '',
  };
}

export type StartFormAction =
  | { kind: 'edit'; patch: Partial<StartForm> }
  | { kind: 'toggleSet'; name: string }
  | { kind: 'toggleTask'; id: string }
  | { kind: 'applyDefaults'; options: HarnessOptions };

export function startFormReducer(form: StartForm, action: StartFormAction): StartForm {
  switch (action.kind) {
    case 'edit':
      return { ...form, ...action.patch };
    case 'toggleSet':
      return { ...form, setNames: toggleIn(form.setNames, action.name) };
    case 'toggleTask':
      return { ...form, taskIds: toggleIn(form.taskIds, action.id) };
    case 'applyDefaults':
      return applyDefaults(form, action.options);
  }
}

/** Fills the limits and Parallel with pb.toml's defaults once they load, as ordinary editable
 * values: an empty field with a placeholder reads as already entered, not as "will default to
 * this on Start". A field the person has already filled in is kept. */
export function applyDefaults(form: StartForm, options: HarnessOptions): StartForm {
  const orDefault = (value: string, fallback: number) => (value === '' ? String(fallback) : value);
  return {
    ...form,
    maxCost: orDefault(form.maxCost, options.defaultMaxCostUsd),
    maxMinutes: orDefault(form.maxMinutes, options.defaultMaxWallMinutes),
    maxSteps: orDefault(form.maxSteps, options.defaultMaxSteps),
    parallel: orDefault(form.parallel, defaultParallel(options.maxConcurrentRuns)),
  };
}

/** Where pb keeps a harness variant (relative to pb_root): `harness/variants/<name>/`. */
export const HARNESS_VARIANTS_DIR = 'harness/variants';

export type HarnessPaths = { graph: string; runtime: string };

/** The graph/runtime pair a harness choice resolves to, as `pb run` would get it; `null` while
 * the choice is incomplete (no variant picked, a custom file missing, no default configured). */
export function harnessPaths(form: Pick<StartForm, 'harnessKind' | 'variant' | 'customGraph' | 'customRuntime'>, options: HarnessOptions): HarnessPaths | null {
  switch (form.harnessKind) {
    case 'default':
      return options.defaultGraph && options.defaultRuntime ? { graph: options.defaultGraph, runtime: options.defaultRuntime } : null;
    case 'variant': {
      if (!form.variant) return null;
      const dir = `${HARNESS_VARIANTS_DIR}/${form.variant}`;
      return { graph: `${dir}/graph.json`, runtime: `${dir}/runtime.json` };
    }
    case 'custom': {
      const graph = form.customGraph.trim();
      const runtime = form.customRuntime.trim();
      return graph && runtime ? { graph, runtime } : null;
    }
  }
}

/** The form's values as `validateStart` checks them. */
export function startFormValues(form: StartForm, taskCount: number, harnessErrors: string[]): StartFormValues {
  return { ...form, taskCount, harnessErrors };
}

/** A typed number, or `undefined` (pb's default) when the field is empty. */
export function optionalNumber(raw: string): number | undefined {
  return raw.trim() ? Number(raw) : undefined;
}

function harnessRequest(form: StartForm): Harness {
  switch (form.harnessKind) {
    case 'default':
      return { kind: 'default' };
    case 'variant':
      return { kind: 'variant', name: form.variant };
    case 'custom':
      return { kind: 'custom', graph: form.customGraph.trim(), runtime: form.customRuntime.trim() };
  }
}

/** What Start sends to `POST /api/benchmarks/batches`. */
export function toStartRequest(form: StartForm): StartBatchRequest {
  return {
    tasks: tasksArg(form.setNames, form.taskIds),
    seeds: form.seeds,
    label: form.label.trim(),
    harness: harnessRequest(form),
    model: form.model.trim() || undefined,
    maxCostUsd: optionalNumber(form.maxCost),
    maxMinutes: optionalNumber(form.maxMinutes),
    maxSteps: optionalNumber(form.maxSteps),
    parallel: optionalNumber(form.parallel),
  };
}

/** The line next to Start: `3 tasks × 2 seeds`, then either the maximum spend with the cap it
 * comes from, or why there is none yet. */
export type SpendLine = {
  runs: string;
  spend: { max: string; cap: string } | null;
  hint: string | null;
};

export function spendLine(taskCount: number, seeds: number, costCapUsd: number, defaultCapUsd: number): SpendLine {
  const runs = `${plural(taskCount, 'task')} × ${plural(seeds, 'seed')}`;
  const spend = maxSpend(taskCount, seeds, costCapUsd);
  if (spend == null) {
    const hint = taskCount === 0 ? '(pick a task to see the maximum spend)' : '(set a positive cost cap to see the maximum spend)';
    return { runs, spend: null, hint };
  }
  const cap = costCapUsd === defaultCapUsd ? `${usd(costCapUsd)} cap from pb.toml` : `${usd(costCapUsd)} cap`;
  return { runs, spend: { max: usd(spend), cap }, hint: null };
}

// The Start-a-batch form at `#/benchmarks/start`: tasks, label, harness (default, a variant or a
// custom graph + runtime, each checked by the server), model, seeds and parallelism, limits per
// run, and the maximum spend next to Start, which stays in view. Other running batches do not
// block Start: pb runs batches concurrently within the host's `max_concurrent_runs` run slots,
// and the form says how busy those are.
import { useEffect, useMemo, useReducer, useState, type ReactNode } from 'react';
import { Info, LoaderCircle, Play } from 'lucide-react';
import { errorMessage } from '../api';
import { api } from './pb-setup';
import type { BatchSummary, HarnessOptions, RunSlots } from '../types';
import { benchmarksBatchHref, navigate } from '../route';
import { useResource } from '../hooks';
import { ErrorBox, Skeleton } from '../common';
import { BenchmarksNav } from './shared';
import {
  DEFAULT_MAX_CONCURRENT_RUNS,
  START_FIELD_LABEL,
  concurrencyNote,
  defaultHarnessOptions,
  emptyStartForm,
  harnessPaths,
  optionalNumber,
  resolveTasks,
  setsIncluding,
  spendLine,
  startFormReducer,
  startFormValues,
  toStartRequest,
  validateStart,
  type HarnessKind,
  type StartField,
  type StartForm as StartFormFields,
} from './start-form';
import { noteStarted } from './started-batches';
import { HarnessPreviewBox, previewErrors, useHarnessPreview, type PreviewResult } from './HarnessPreview';

type FieldErrors = Partial<Record<StartField, string>>;
type Edit = (patch: Partial<StartFormFields>) => void;

export function StartForm() {
  const options = useResource<HarnessOptions>('benchmarks/start-options', (signal) => api.benchmarkStartOptions(signal));
  const batches = useResource<{ batches: BatchSummary[] }>('benchmarks', (signal) => api.benchmarkBatches(signal));
  const slots = useResource<RunSlots>('benchmarks/slots', (signal) => api.benchmarkSlots(signal));
  const opts = options.data ?? defaultHarnessOptions();
  const running = batches.data?.batches.filter((b) => b.status === 'running') ?? [];
  const busyNote = concurrencyNote(running, slots.data ?? null, opts.maxConcurrentRuns);
  const start = useStartForm(options.data);
  const { form, dispatch, shown } = start;
  const edit: Edit = (patch) => dispatch({ kind: 'edit', patch });

  return (
    <div className="run-page">
      <div className="run-scroll bm-start-scroll">
        {/* No second "Start a batch" button on the page that already is the form. */}
        <BenchmarksNav active="batches" showStart={false} />
        <h1>Start a batch</h1>
        {options.loading && <Skeleton lines={6} />}
        {options.error && <ErrorBox message={options.error} onRetry={options.reload} />}
        {busyNote && (
          <p className="muted small bm-running-note">
            <Info size={13} aria-hidden /> {busyNote}
            {running.length > 0 && (
              <>
                {' '}
                Watch{' '}
                {running.map((b, i) => (
                  <span key={b.id}>
                    {i > 0 && ', '}
                    <a href={benchmarksBatchHref(b.id)}>{b.label || b.id}</a>
                  </span>
                ))}
                .
              </>
            )}
          </p>
        )}
        {options.data && (
          <form
            className="bm-form"
            noValidate
            onSubmit={(e) => {
              e.preventDefault();
              void start.submit();
            }}
          >
            <p className="muted small">
              <span className="bm-required" aria-hidden>
                *
              </span>{' '}
              required
            </p>
            <LabelField value={form.label} onChange={(label) => edit({ label })} error={shown.label} />
            <TaskPicker
              options={opts}
              setNames={form.setNames}
              taskIds={form.taskIds}
              onToggleSet={(name) => dispatch({ kind: 'toggleSet', name })}
              onToggleTask={(id) => dispatch({ kind: 'toggleTask', id })}
              error={shown.tasks}
            />
            <HarnessFieldset form={form} options={opts} edit={edit} error={shown.harness} preview={start.preview} previewLoading={start.previewLoading} />
            <ModelField options={opts} value={form.model} onChange={(model) => edit({ model })} />
            <RunsFieldset form={form} options={opts} edit={edit} shown={shown} />
            <LimitsFieldset form={form} options={opts} edit={edit} shown={shown} />
            {/* Start, what it would spend and what still needs fixing stay in view at the bottom
                of the window, however long the task list is. */}
            <div className="bm-form-footer">
              <SpendSummary taskCount={start.taskCount} seeds={form.seeds} costCapUsd={optionalNumber(form.maxCost) ?? opts.defaultMaxCostUsd} defaultCapUsd={opts.defaultMaxCostUsd} />
              <ErrorSummary fields={Object.keys(shown) as StartField[]} />
              {start.serverError && <ErrorBox compact message={start.serverError} />}
              <button type="submit" className="button primary" disabled={start.busy}>
                {start.busy ? <LoaderCircle size={14} className="spin" aria-hidden /> : <Play size={14} aria-hidden />} Start
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}

/** The form's state and what follows from it: the tasks it resolves to, the harness check, the
 * field errors on show, and Start itself. */
function useStartForm(loaded: HarnessOptions | undefined) {
  const opts = loaded ?? defaultHarnessOptions();
  const [form, dispatch] = useReducer(startFormReducer, undefined, emptyStartForm);
  const [busy, setBusy] = useState(false);
  const [serverError, setServerError] = useState<string | null>(null);
  // Field errors show once Start was pressed (then all of them, live, until fixed), not while the
  // person is still filling the form in for the first time.
  const [attempted, setAttempted] = useState(false);

  useEffect(() => {
    if (loaded) dispatch({ kind: 'applyDefaults', options: loaded });
  }, [loaded]);

  const resolvedTasks = useMemo(() => resolveTasks(form.setNames, form.taskIds, opts.taskSets), [form.setNames, form.taskIds, opts.taskSets]);
  const paths = harnessPaths(form, opts);
  const preview = useHarnessPreview(loaded ? paths : null, form.model);
  const previewLoading = !!paths && !!loaded && !preview;
  const fieldErrors = validateStart(startFormValues(form, resolvedTasks.length, previewErrors(preview)));
  const shown: FieldErrors = attempted ? fieldErrors : {};

  // A server refusal is about the form as it was sent: any change clears it. The task picks
  // count by the tasks they resolve to, so ticking a set whose tasks are already in keeps it.
  const formKey = JSON.stringify({ ...form, setNames: null, taskIds: null, taskCount: resolvedTasks.length });
  useEffect(() => setServerError(null), [formKey]);

  const submit = async () => {
    setServerError(null);
    setAttempted(true);
    if (Object.keys(fieldErrors).length > 0 || previewLoading) return;
    setBusy(true);
    try {
      const res = await api.benchmarkStartBatch(toStartRequest(form));
      noteStarted(res.id);
      navigate({ kind: 'benchmarksBatch', id: res.id });
    } catch (cause) {
      setServerError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return { form, dispatch, taskCount: resolvedTasks.length, preview, previewLoading, shown, serverError, busy, submit };
}

/** A field's own error, right under it, so every problem shows next to its field at once. */
function FieldError({ id, message }: { id: string; message: string | undefined }) {
  if (!message) return null;
  return (
    <p className="field-error" id={id}>
      {message}
    </p>
  );
}

/** The ARIA a field's input carries while its error shows. */
function invalidProps(field: StartField, error: string | undefined) {
  return error ? { 'aria-invalid': true, 'aria-describedby': `bm-err-${field}` } : {};
}

function RequiredMark() {
  return (
    <>
      <span className="bm-required" aria-hidden>
        *
      </span>
      <span className="visually-hidden"> (required)</span>
    </>
  );
}

function LabelField({ value, onChange, error }: { value: string; onChange: (label: string) => void; error?: string }) {
  return (
    <label className={`field ${error ? 'invalid' : ''}`} id="bm-field-label">
      <span>
        Label <RequiredMark />
      </span>
      <input value={value} onChange={(e) => onChange(e.target.value)} placeholder="try-x" required {...invalidProps('label', error)} />
      <span className="field-note">Names this batch in the list and in compares.</span>
      <FieldError id="bm-err-label" message={error} />
    </label>
  );
}

/** The task selection: task sets (each listing its own tasks) and individual tasks, where a task
 * a ticked set already includes reads as included ("in dev") instead of staying unticked. */
function TaskPicker({
  options,
  setNames,
  taskIds,
  onToggleSet,
  onToggleTask,
  error,
}: {
  options: HarnessOptions;
  setNames: string[];
  taskIds: string[];
  onToggleSet: (name: string) => void;
  onToggleTask: (id: string) => void;
  error?: string;
}) {
  const individual = useMemo(() => [...new Set([...options.tasks, ...options.repoTasks])].sort(), [options.tasks, options.repoTasks]);
  return (
    <fieldset className={`field bm-tasks ${error ? 'invalid' : ''}`} id="bm-field-tasks" aria-describedby={error ? 'bm-err-tasks' : undefined}>
      <legend>
        Tasks{' '}
        <span className="bm-required" aria-hidden>
          *
        </span>
        <span className="visually-hidden"> (required)</span>
      </legend>
      <FieldError id="bm-err-tasks" message={error} />
      <div className="bm-subhead">Task sets</div>
      <div className="bm-sets">
        {options.taskSets.map((s) => (
          <label key={s.name} className="bm-set">
            <input type="checkbox" checked={setNames.includes(s.name)} onChange={() => onToggleSet(s.name)} />
            <span>
              <span className="bm-set-name">{s.name}</span> <span className="muted small">({s.tasks.length})</span>
              <span className="bm-set-members muted small">{s.tasks.join(', ')}</span>
            </span>
          </label>
        ))}
        {options.taskSets.length === 0 && <span className="muted small">No task sets in pb.toml.</span>}
      </div>
      <div className="bm-subhead">Individual tasks</div>
      <div className="bm-task-grid">
        {individual.map((t) => {
          const via = setsIncluding(t, setNames, options.taskSets);
          const explicit = taskIds.includes(t);
          const viaSetOnly = via.length > 0 && !explicit;
          return (
            <label key={t} className={`bm-check bm-task-check ${viaSetOnly ? 'via-set' : ''}`} title={via.length > 0 ? `included by ${via.join(', ')}` : t}>
              <input type="checkbox" checked={explicit || via.length > 0} disabled={viaSetOnly} onChange={() => onToggleTask(t)} />
              <code>{t}</code>
              {via.length > 0 && <span className="muted small">in {via.join(', ')}</span>}
            </label>
          );
        })}
        {individual.length === 0 && <span className="muted small">No tasks found.</span>}
      </div>
    </fieldset>
  );
}

const HARNESS_CHOICES: { kind: HarnessKind; title: string; hint: string }[] = [
  { kind: 'default', title: 'Default', hint: "pb.toml's graph and runtime" },
  { kind: 'variant', title: 'A variant', hint: 'harness/variants/<name>/' },
  { kind: 'custom', title: 'Custom', hint: 'any graph + runtime file under harness/' },
];

function HarnessFieldset({
  form,
  options,
  edit,
  error,
  preview,
  previewLoading,
}: {
  form: StartFormFields;
  options: HarnessOptions;
  edit: Edit;
  error?: string;
  preview: PreviewResult | null;
  previewLoading: boolean;
}) {
  return (
    <fieldset className={`field ${error ? 'invalid' : ''}`} id="bm-field-harness" aria-describedby={error ? 'bm-err-harness' : undefined}>
      <legend>Harness</legend>
      <div className="bm-harness-choice" role="radiogroup" aria-label="Harness">
        {HARNESS_CHOICES.map((c) => (
          <label key={c.kind} className="bm-radio">
            <input type="radio" name="harness" checked={form.harnessKind === c.kind} onChange={() => edit({ harnessKind: c.kind })} disabled={c.kind === 'variant' && options.variants.length === 0} />
            <span>
              {c.title} <span className="muted small">— {c.hint}</span>
            </span>
          </label>
        ))}
      </div>
      {form.harnessKind === 'variant' && (
        <label className="field">
          <span>Variant</span>
          <select value={form.variant} onChange={(e) => edit({ variant: e.target.value })}>
            <option value="">Choose a variant…</option>
            {options.variants.map((v) => (
              <option key={v} value={v}>
                {v}
              </option>
            ))}
          </select>
        </label>
      )}
      {form.harnessKind === 'custom' && <CustomHarnessFields form={form} options={options} edit={edit} />}
      <FieldError id="bm-err-harness" message={error} />
      <HarnessPreviewBox preview={preview} loading={previewLoading} />
    </fieldset>
  );
}

/** Graph and runtime file, offered from the files pb_root/harness actually has (a datalist: a
 * path typed by hand still works), checked before Start. */
function CustomHarnessFields({ form, options, edit }: { form: StartFormFields; options: HarnessOptions; edit: Edit }) {
  const files = options.harnessFiles ?? { graphs: [], runtimes: [] };
  return (
    <div className="bm-fields-row">
      <PathField
        label="Graph file"
        listId="bm-graph-files"
        files={files.graphs}
        value={form.customGraph}
        onChange={(customGraph) => edit({ customGraph })}
        placeholder="harness/variants/…/graph.json"
      />
      <PathField
        label="Runtime file"
        listId="bm-runtime-files"
        files={files.runtimes}
        value={form.customRuntime}
        onChange={(customRuntime) => edit({ customRuntime })}
        placeholder="harness/variants/…/runtime.json"
      />
    </div>
  );
}

function PathField({ label, listId, files, value, onChange, placeholder }: { label: string; listId: string; files: string[]; value: string; onChange: (path: string) => void; placeholder: string }) {
  return (
    <label className="field">
      <span>{label}</span>
      <input value={value} onChange={(e) => onChange(e.target.value)} list={listId} placeholder={placeholder} spellCheck={false} />
      <datalist id={listId}>
        {files.map((f) => (
          <option key={f} value={f} />
        ))}
      </datalist>
    </label>
  );
}

/** The models pb.toml prices are offered; anything else can still be typed. */
function ModelField({ options, value, onChange }: { options: HarnessOptions; value: string; onChange: (model: string) => void }) {
  return (
    <label className="field">
      <span>Model override</span>
      <input value={value} onChange={(e) => onChange(e.target.value)} list="bm-models" placeholder="each node keeps its runtime's model" spellCheck={false} />
      <datalist id="bm-models">
        {(options.models ?? []).map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      <span className="field-note">Every agent node runs on this model instead. A model pb.toml has no price for counts as $0, so the cost cap never stops it.</span>
    </label>
  );
}

/** A number field of the form, with its notes (`children`) and its error under it.
 *
 * Focusing the field selects its text: a number input with a value otherwise inserts at the
 * cursor rather than replacing, so typing "5" over a shown "1" would become "15". */
function NumberField({
  field,
  label,
  value,
  onChange,
  error,
  min,
  step,
  numeric,
  placeholder,
  children,
}: {
  field: StartField;
  label: string;
  value: string | number;
  onChange: (raw: string) => void;
  error?: string;
  min: number;
  step?: string;
  /** Whole numbers only: phones show the digits keyboard. */
  numeric?: boolean;
  placeholder?: string;
  children?: ReactNode;
}) {
  return (
    <label className={`field ${error ? 'invalid' : ''}`} id={`bm-field-${field}`}>
      <span>{label}</span>
      <input
        type="number"
        min={min}
        step={step}
        inputMode={numeric ? 'numeric' : undefined}
        value={value}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        {...invalidProps(field, error)}
      />
      {children}
      <FieldError id={`bm-err-${field}`} message={error} />
    </label>
  );
}

type FieldsetProps = { form: StartFormFields; options: HarnessOptions; edit: Edit; shown: FieldErrors };

function RunsFieldset({ form, options, edit, shown }: FieldsetProps) {
  return (
    <fieldset className="field">
      <legend>Runs</legend>
      <div className="bm-fields-row">
        <NumberField field="seeds" label="Seeds" min={1} numeric value={form.seeds} onChange={(raw) => edit({ seeds: Math.max(1, Number(raw) || 1) })} error={shown.seeds}>
          <span className="field-note">Independent runs of every task. 2 or more lets a compare tell a change from noise.</span>
          {form.seeds < 2 && <span className="field-note">With 1 seed, every compare against this batch will read "needs 2 seeds".</span>}
        </NumberField>
        <NumberField field="parallel" label="Parallel" min={1} numeric value={form.parallel} onChange={(parallel) => edit({ parallel })} error={shown.parallel}>
          <span className="field-note">
            How many runs go at once, across the whole batch (the host runs at most {options.maxConcurrentRuns ?? DEFAULT_MAX_CONCURRENT_RUNS} at once across all batches; the rest queue).
          </span>
        </NumberField>
      </div>
    </fieldset>
  );
}

function LimitsFieldset({ form, options, edit, shown }: FieldsetProps) {
  return (
    <fieldset className="field">
      <legend>Limits per run</legend>
      <p className="field-note">Each run stops at whichever it reaches first; pb still scores the code it had.</p>
      <div className="bm-fields-row">
        <NumberField
          field="maxCost"
          label="Max cost (USD)"
          min={0}
          step="0.5"
          value={form.maxCost}
          onChange={(maxCost) => edit({ maxCost })}
          placeholder={String(options.defaultMaxCostUsd)}
          error={shown.maxCost}
        />
        <NumberField
          field="maxMinutes"
          label="Max minutes"
          min={0}
          value={form.maxMinutes}
          onChange={(maxMinutes) => edit({ maxMinutes })}
          placeholder={String(options.defaultMaxWallMinutes)}
          error={shown.maxMinutes}
        >
          <span className="field-note">Wall time.</span>
        </NumberField>
        <NumberField field="maxSteps" label="Max steps" min={0} value={form.maxSteps} onChange={(maxSteps) => edit({ maxSteps })} placeholder={String(options.defaultMaxSteps)} error={shown.maxSteps}>
          <span className="field-note">Node executions (worker, reviewers, repairs, retries).</span>
        </NumberField>
      </div>
    </fieldset>
  );
}

function SpendSummary({ taskCount, seeds, costCapUsd, defaultCapUsd }: { taskCount: number; seeds: number; costCapUsd: number; defaultCapUsd: number }) {
  const line = spendLine(taskCount, seeds, costCapUsd, defaultCapUsd);
  return (
    <p className="bm-spend">
      {line.runs}
      {line.spend ? (
        <>
          {' '}
          — maximum spend <strong>{line.spend.max}</strong> ({line.spend.cap} per run)
        </>
      ) : (
        <span className="muted"> {line.hint}</span>
      )}
    </p>
  );
}

/** Scrolls a field into view and puts the cursor in it (a fieldset's first enabled input). */
function focusField(field: StartField) {
  const el = document.getElementById(`bm-field-${field}`);
  el?.scrollIntoView({ block: 'center' });
  const target = el?.matches('input, select') ? el : el?.querySelector<HTMLElement>('input:not([disabled]), select');
  target?.focus();
}

/** "Fix these 2 fields: Label, Tasks", each name a button to that field. */
function ErrorSummary({ fields }: { fields: StartField[] }) {
  if (fields.length === 0) return null;
  return (
    <p className="field-error bm-error-summary" role="alert">
      Fix {fields.length === 1 ? 'this field' : `these ${fields.length} fields`}:{' '}
      {fields.map((f, i) => (
        <span key={f}>
          {i > 0 && ', '}
          {/* A button, not an `#id` link: the app routes on the hash. */}
          <button type="button" className="text-button" onClick={() => focusField(f)}>
            {START_FIELD_LABEL[f]}
          </button>
        </span>
      ))}
    </p>
  );
}

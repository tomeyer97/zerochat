import { useEffect, useRef, useState } from 'react';
import { AlertTriangle, ChevronRight, LoaderCircle } from 'lucide-react';
import { api, ApiError } from '../api';
import type { FeatureAction, FeatureDetail } from '../types';
import { featureState, progressText } from '../features-model';
import { costText, duration, fullTime, timeOfDay } from '../format';
import { featuresHref } from '../route';
import { CopyButton, ErrorBox } from '../common';
import { ActionButton } from './ActionButton';
import { Attention } from './Attention';
import { useBusyAction } from './page-hooks';

export type OnDone = (message: string, tone?: 'info' | 'bad') => void;

/** The header's buttons besides Resume (unpause and start in one), in order. */
const BUTTONS: { action: Exclude<FeatureAction, 'max_parallel'>; label: string; title?: string; primary: boolean }[] = [
  { action: 'unpause', label: 'Unpause', primary: true },
  { action: 'start', label: 'Start', primary: true },
  { action: 'stop', label: 'Stop launching', title: 'Running slices keep running; nothing new starts or merges until Start', primary: false },
];

/** The feature's name and state, its actions, branch line, what needs you and what runs. */
export function FeatureHeader({ detail, now, onDone, onOpen, stale }: { detail: FeatureDetail; now: number; onDone: OnDone; onOpen: (slice?: string) => void; stale: string | null }) {
  const { busy, error, run } = useBusyAction<FeatureAction | 'resume'>();
  const act = (action: FeatureAction, extra: { maxParallel?: number } = {}) =>
    run(
      action,
      async () => onDone((await api.featureAction(detail.name, { action, ...extra })).message),
      // Refused because the state moved on: reload to show it.
      (cause) => cause instanceof ApiError && cause.status === 409 && onDone('', 'info')
    );
  const resume = () =>
    run('resume', async () => {
      await api.featureAction(detail.name, { action: 'unpause' });
      await api.featureAction(detail.name, { action: 'start' });
      onDone('Resumed');
    });
  const can = (action: FeatureAction) => detail.actions.includes(action);
  const state = featureState(detail);
  const finished = detail.finishedAt != null;
  // A paused feature that may start again gets one button for both steps.
  const canResume = can('unpause') && can('start');
  return (
    <header className="run-header feature-header">
      <p className="muted small crumbs">
        <a href={featuresHref()}>Features</a> <ChevronRight size={12} aria-hidden /> {detail.name}
      </p>
      <div className="run-title-row">
        <h1 className="run-title-text">{detail.name}</h1>
        <span className={`pill small tone-${state.tone}`}>{state.label}</span>
        <div className="run-actions">
          {canResume && (
            <ActionButton name="resume" busy={busy} primary title="Unpause and start launching again" onClick={() => void resume()}>
              Resume
            </ActionButton>
          )}
          {BUTTONS.filter((b) => can(b.action) && !(canResume && b.action !== 'stop')).map((b) => (
            <ActionButton key={b.action} name={b.action} busy={busy} primary={b.primary} title={b.title} onClick={() => void act(b.action)}>
              {b.label}
            </ActionButton>
          ))}
          {can('max_parallel') && <ParallelField value={detail.maxParallel} busy={!!busy} onSave={(n) => act('max_parallel', { maxParallel: n })} />}
        </div>
      </div>
      <div className="run-meta-line">
        <span className="branch-line" title={detail.repo}>
          <code>{detail.branch}</code> ← {detail.base}
          {detail.branchCommit && (
            <>
              {' '}
              @ <code>{detail.branchCommit.slice(0, 7)}</code>
              <CopyButton text={detail.branch} label="Copy branch name" />
            </>
          )}
        </span>
        <span>{progressText(detail)}</span>
        {detail.costUsd != null && <span>{costText(detail.costUsd, detail.costEstimated)}</span>}
        <TimeSpan detail={detail} now={now} />
      </div>
      <Attention detail={detail} now={now} onOpen={onOpen} onDone={onDone} />
      {!finished && !detail.issue && detail.running.length > 0 && (
        <p className="feature-now tone-text-running">
          <LoaderCircle size={13} className="spin" aria-hidden /> running {detail.running.join(', ')}
        </p>
      )}
      {error && <ErrorBox compact message={error} />}
      {stale && (
        <p className="feature-stale" role="status">
          <AlertTriangle size={13} aria-hidden /> Not updating ({stale.replace(/^Could not load this feature: /, '')}). Showing the state as of {timeOfDay(detail.now, Date.now())}.
        </p>
      )}
    </header>
  );
}

function TimeSpan({ detail, now }: { detail: FeatureDetail; now: number }) {
  if (detail.finishedAt && detail.startedAt) {
    return (
      <span title={`${fullTime(detail.startedAt, now)} → ${fullTime(detail.finishedAt, now)}`}>
        done {timeOfDay(detail.finishedAt, now)} · took {duration(detail.finishedAt - detail.startedAt)}
      </span>
    );
  }
  if (detail.startedAt) return <span title={fullTime(detail.startedAt, now)}>started {timeOfDay(detail.startedAt, now)}</span>;
  if (detail.createdAt) return <span title={fullTime(detail.createdAt, now)}>created {timeOfDay(detail.createdAt, now)}</span>;
  return null;
}

/** The page's cap on slices at once (zsf itself only requires at least 1). */
const MAX_PARALLEL = 64;

/** "up to [2] at once": saved on Enter or when the field loses focus; anything outside 1–MAX_PARALLEL goes back. */
function ParallelField({ value, busy, onSave }: { value: number; busy: boolean; onSave: (n: number) => Promise<void> }) {
  const [text, setText] = useState(String(value));
  const [invalid, setInvalid] = useState(false);
  // While the field has focus, a reload does not overwrite what is being typed.
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setText(String(value));
  }, [value]);
  const commit = () => {
    const n = Number(text);
    if (!Number.isInteger(n) || n < 1 || n > MAX_PARALLEL) {
      setInvalid(true);
      setText(String(value));
      return;
    }
    setInvalid(false);
    if (n !== value) void onSave(n);
  };
  return (
    <label className="parallel-form" title="How many slices may run at once (running ones are not stopped)">
      up to
      <input
        type="number"
        min={1}
        max={MAX_PARALLEL}
        value={text}
        disabled={busy}
        aria-label="Slices at once"
        aria-invalid={invalid || undefined}
        onFocus={() => (focused.current = true)}
        onChange={(e) => {
          setText(e.target.value);
          setInvalid(false);
        }}
        onBlur={() => {
          focused.current = false;
          commit();
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            commit();
          }
          if (e.key === 'Escape') setText(String(value));
        }}
      />
      at once
      {invalid && <span className="tone-text-bad"> (1–{MAX_PARALLEL})</span>}
    </label>
  );
}

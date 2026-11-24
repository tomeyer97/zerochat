/** A slice's attempts: one row per run, and the task each attempt got (with what changed since the one before). */
import { useState } from 'react';
import { api } from '../api';
import type { AttemptView, SliceView } from '../types';
import { attemptDuration, attemptKindLabel, attemptReason, attemptResult, attemptStepLabel, isActiveSlice, isStoppedSlice, shortDuration } from '../features-model';
import { costText, fullTime, shortId } from '../format';
import { runHref } from '../route';
import { useResource } from '../hooks';
import { CopyButton, ErrorBox, LongText, Skeleton } from '../common';
import { DiffView } from '../Files';

/** Lines of a task shown before "show more". */
const TASK_LINES = 24;

export function Attempts({ feature, slice, now }: { feature: string; slice: SliceView; now: number }) {
  return (
    <section>
      <h3 className="section-label">Attempts</h3>
      <table className="attempts-table">
        <thead>
          <tr>
            <th scope="col">#</th>
            <th scope="col">How</th>
            <th scope="col">Result</th>
            <th scope="col">Run</th>
            <th scope="col" className="col-num">
              Time
            </th>
            <th scope="col" className="col-num">
              Cost
            </th>
          </tr>
        </thead>
        <tbody>
          {slice.attempts.map((a, i) => (
            <AttemptRow key={a.n} slice={slice} attempt={a} index={i} now={now} />
          ))}
        </tbody>
      </table>
      <AttemptTasks feature={feature} slice={slice} />
    </section>
  );
}

function AttemptRow({ slice, attempt: a, index, now }: { slice: SliceView; attempt: AttemptView; index: number; now: number }) {
  const isLast = index === slice.attempts.length - 1;
  // The attempt a person must act on now; its backup branch is under "Why it stopped".
  const current = isLast && isStoppedSlice(slice.status);
  const running = isLast && isActiveSlice(slice.status);
  const outcome = attemptResult(a.result, running, { current, replacedBy: slice.attempts[index + 1]?.n });
  // Between its run and the merge, the last row says what the pill says (checks before merge, run finished).
  const step = isLast && !a.result ? attemptStepLabel(slice) : null;
  const result = step ? { ...outcome, label: step } : outcome;
  const reason = attemptReason(slice, index);
  return (
    <tr>
      <td className="tabular muted">{a.n}</td>
      <td>
        <span>{attemptKindLabel(a.kind, a.fresh)}</span>
        {reason && <span className="attempt-reason muted small">{reason}</span>}
        {a.backup && !current && <BackupLine branch={a.backup} />}
      </td>
      <td className={`tone-text-${result.tone}`}>{result.label}</td>
      <td>
        {a.runId ? <RunLink runId={a.runId} target={a.runTarget} /> : <span className="muted">no run</span>}
        {a.earlierRuns.map((r) => (
          <span key={r.runId} className="small muted earlier-run">
            {' '}
            (resumed after its controller was lost: <RunLink runId={r.runId} target={r.target} label="lost run ›" />)
          </span>
        ))}
      </td>
      <td className="tabular muted col-num" title={`Started ${fullTime(a.startedAt, now)}`}>
        {shortDuration(attemptDuration(a, now))}
      </td>
      <td className="tabular muted col-num">{a.costUsd != null ? costText(a.costUsd, a.costEstimated) : running ? '…' : ''}</td>
    </tr>
  );
}

/** Where an attempt's failed work was kept (a branch), to copy. */
export function BackupLine({ branch }: { branch: string }) {
  return (
    <p className="small path-line">
      <span className="muted">work kept on</span> <code>{branch}</code>
      <CopyButton text={branch} label="Copy branch name" />
    </p>
  );
}

function RunLink({ runId, target, label = 'run ›' }: { runId: string; target: string | null; label?: string }) {
  return target ? (
    <a href={runHref(target, runId)} title={runId}>
      {label}
    </a>
  ) : (
    <span className="muted" title={`${runId} (not readable here)`}>
      {shortId(runId, 8)}
    </span>
  );
}

/** The task of one attempt, with a tab per attempt that has one; it follows the newest until you pick one. */
function AttemptTasks({ feature, slice }: { feature: string; slice: SliceView }) {
  const withTask = slice.attempts.filter((a) => a.hasTask);
  const newest = withTask.length ? withTask[withTask.length - 1].n : null;
  const [picked, setPicked] = useState<number | null>(null);
  const shown = picked ?? newest;
  if (shown == null) return null;
  return (
    <>
      <div className="section-head task-head">
        <h3 className="section-label">Task</h3>
        {withTask.length > 1 && (
          <div className="round-picker" role="tablist" aria-label="Task of attempt">
            {withTask.map((a) => (
              <button key={a.n} type="button" role="tab" aria-selected={a.n === shown} className="round-tab" onClick={() => setPicked(a.n)}>
                #{a.n}
              </button>
            ))}
          </div>
        )}
      </div>
      <TaskView key={shown} feature={feature} slice={slice.id} attempt={shown} />
    </>
  );
}

function TaskView({ feature, slice, attempt }: { feature: string; slice: string; attempt: number }) {
  const resource = useResource(`task/${feature}/${slice}/${attempt}`, (signal) => api.sliceTask(feature, slice, attempt, signal));
  const [full, setFull] = useState(false);
  const task = resource.data;
  if (resource.loading) return <Skeleton lines={2} />;
  if (!task) return <ErrorBox compact message={resource.error ?? 'No task.'} />;
  if (task.previousAttempt == null) {
    // The first task is the spec below plus where the reference documents are: not shown twice.
    return (
      <div className="task-view">
        <button type="button" className="text-button" aria-expanded={full} onClick={() => setFull(!full)}>
          {full ? 'Hide the exact task' : 'The spec below, as sent; show the exact task'}
        </button>
        {full && <LongText text={task.text} maxLines={TASK_LINES} />}
      </div>
    );
  }
  return (
    <div className="task-view">
      <div className="chips" role="group" aria-label="Task view">
        <button type="button" className={`chip ${!full ? 'on' : ''}`} aria-pressed={!full} onClick={() => setFull(false)}>
          Changes vs #{task.previousAttempt}
        </button>
        <button type="button" className={`chip ${full ? 'on' : ''}`} aria-pressed={full} onClick={() => setFull(true)}>
          Full task
        </button>
      </div>
      {full ? <LongText text={task.text} maxLines={TASK_LINES} /> : task.diff ? <DiffView text={task.diff} /> : <p className="muted small">Same task as #{task.previousAttempt}.</p>}
    </div>
  );
}

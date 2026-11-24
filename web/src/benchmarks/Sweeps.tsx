// `pb sweeps`' list at `#/benchmarks/sweeps`: pick one or more sweeps for a report, or report on
// every sweep.
import { useState } from 'react';
import { SlidersHorizontal } from 'lucide-react';
import { api } from './pb-setup';
import type { SweepSummary } from '../types';
import { costText, fullTime, timeOfDay, truncate } from '../format';
import { navigate, runHref } from '../route';
import { useNow, useResource } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { BenchmarksNav } from './shared';
import { toggleIn } from './batch-list';

export function Sweeps() {
  const resource = useResource<{ sweeps: SweepSummary[] }>('benchmarks/sweeps', (signal) => api.benchmarkSweeps(signal));
  const sweeps = resource.data?.sweeps ?? [];
  const now = useNow(true, 30_000);
  const [selected, setSelected] = useState<string[]>([]);
  const toggle = (id: string) => setSelected((s) => toggleIn(s, id));

  return (
    <div className="home">
      <BenchmarksNav active="sweeps" />
      <h1 className="visually-hidden">Sweeps</h1>
      {resource.loading && <Skeleton lines={6} />}
      {resource.error && <ErrorBox compact message={resource.error} onRetry={resource.reload} />}
      {resource.data && sweeps.length === 0 && (
        <EmptyState title="No sweeps yet" icon={<SlidersHorizontal size={22} aria-hidden />}>
          <p>
            Run one with <code>pb sweep</code>.
          </p>
        </EmptyState>
      )}
      {sweeps.length > 0 && (
        <>
          <div className="bm-matrix-scroll">
            <table className="run-table bm-table">
              <thead className="quiet-head">
                <tr>
                  <th scope="col" className="col-status">
                    <span className="visually-hidden">Pick</span>
                  </th>
                  <th scope="col">Sweep</th>
                  <th scope="col">Source run</th>
                  <th scope="col" className="bm-task">
                    Task
                  </th>
                  <th scope="col">Items</th>
                  <th scope="col">Models</th>
                  <th scope="col" className="col-num">
                    Runs
                  </th>
                  <th scope="col" className="col-num col-cost">
                    Spend
                  </th>
                  <th scope="col" className="col-time">
                    Created
                  </th>
                </tr>
              </thead>
              <tbody>
                {sweeps.map((s) => (
                  <SweepRow key={s.sweepId} sweep={s} now={now} picked={selected.includes(s.sweepId)} onToggle={() => toggle(s.sweepId)} />
                ))}
              </tbody>
            </table>
          </div>
          <div className="bm-compare-bar">
            <button type="button" className="button primary small" disabled={selected.length === 0} onClick={() => navigate({ kind: 'benchmarksSweepReport', ids: selected })}>
              Report{selected.length > 0 && ` (${selected.length})`}
            </button>
            <button type="button" className="button small" onClick={() => navigate({ kind: 'benchmarksSweepReport', ids: [] })}>
              Report on every sweep
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function SweepRow({ sweep: s, now, picked, onToggle }: { sweep: SweepSummary; now: number; picked: boolean; onToggle: () => void }) {
  return (
    <tr>
      <td className="col-status">
        <input type="checkbox" aria-label={`Pick sweep ${s.sweepId}`} checked={picked} onChange={onToggle} />
      </td>
      <td className="col-run">
        <code>{s.sweepId}</code>
        {s.label && <span className="muted small"> ({s.label})</span>}
      </td>
      <td>
        <a href={runHref('pb', s.sourceRun)}>{s.sourceRun}</a>
      </td>
      <td className="muted" title={s.taskId ?? undefined}>
        {truncate(s.taskId ?? '—', 26)}
      </td>
      <td className="muted small">{s.items.join(', ')}</td>
      <td className="muted small">{s.models.join(', ')}</td>
      <td className="col-num tabular">
        {s.runs}
        {s.running ? ` (${s.running} running)` : ''}
      </td>
      <td className="col-num col-cost tabular">{costText(s.costUsd, false)}</td>
      <td className="col-time tabular" title={fullTime(s.createdAt, now)}>
        {timeOfDay(s.createdAt, now)}
      </td>
    </tr>
  );
}

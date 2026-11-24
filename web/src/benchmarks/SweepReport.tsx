// `pb sweep-report`'s page at `#/benchmarks/sweep-report?ids=...`: per node and model, the score
// Δ against the recorded model, a noise verdict and a recommendation; the suggested (and
// budgeted) runtime mapping; and "Write runtime". All of it is pb's own math, fetched as JSON,
// not re-derived in the browser.
import { useState } from 'react';
import { ChevronRight, SlidersHorizontal } from 'lucide-react';
import { api } from './pb-setup';
import type { SweepAllocation, SweepReport as SweepReportData, SweepSuggestion } from '../types';
import { usd } from '../format';
import { benchmarksSweepsHref } from '../route';
import { useResource } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { BenchmarksNav } from './shared';
import { signedPp, signedUsd } from './batch-compare';
import { DEFAULT_SUGGESTED_RUNTIME_PATH, mappingSummary, recommendationTone, sweepReportKey, sweepRowCells, writeDisabledReason, type SweepReportFilter } from './sweep-report';

export function SweepReport({ route }: { route: SweepReportFilter }) {
  const resource = useResource<SweepReportData>(sweepReportKey(route), (signal) => api.benchmarkSweepReport(route, signal));
  const rep = resource.data;
  const [writeOpen, setWriteOpen] = useState(false);
  const [writePath, setWritePath] = useState(DEFAULT_SUGGESTED_RUNTIME_PATH);
  const [writtenFile, setWrittenFile] = useState<string | null>(null);

  return (
    <div className="run-page">
      <div className="run-scroll">
        <BenchmarksNav active="sweeps" />
        <header className="run-header">
          <p className="muted small">
            <a href={benchmarksSweepsHref()}>Sweeps</a> <ChevronRight size={12} aria-hidden style={{ verticalAlign: 'middle' }} /> Report
          </p>
          {/* The same name as the tab and the breadcrumb. */}
          <h1>Sweep report{route.label ? ` · ${route.label}` : ''}</h1>
        </header>
        {resource.loading && <Skeleton lines={6} />}
        {resource.error && <ErrorBox message={resource.error} onRetry={resource.reload} />}
        {rep && rep.allocation.length === 0 && (
          <EmptyState title="No sweep replays match" icon={<SlidersHorizontal size={22} aria-hidden />}>
            <p>Still running, or nothing matches these filters.</p>
          </EmptyState>
        )}
        {rep && rep.allocation.length > 0 && (
          <>
            <p className="muted small">
              {rep.rows} replay run(s) of {rep.source.runs.length} recorded run(s): {rep.source.runs.join(', ')}
            </p>
            {rep.allocation.map((a) => (
              <AllocationTable key={a.key} allocation={a} />
            ))}
            <MappingTable title="Suggested runtime" suggestion={rep.suggestion} recorded={rep.recorded} />
            {rep.budgeted && (
              <MappingTable
                title={`Budgeted allocation (${route.budget ? `within ${usd(Number(route.budget))}/run` : route.budget})`}
                suggestion={rep.budgeted}
                recorded={rep.recorded}
                overBudget={rep.budgeted.overBudget}
              />
            )}
            <p className="muted small">{rep.caveat}</p>
            <WriteRuntimeButton toWrite={rep.budgeted ?? rep.suggestion} onOpen={() => setWriteOpen(true)} />
            {writtenFile && <p className="muted small">Wrote {writtenFile}.</p>}
          </>
        )}
      </div>
      {writeOpen && (
        <ConfirmDialog
          title="Write runtime file?"
          confirmLabel="Write"
          onClose={() => setWriteOpen(false)}
          onConfirm={async () => {
            const res = await api.benchmarkWriteRuntime({ ids: route.ids, label: route.label, task: route.task, budget: route.budget ? Number(route.budget) : undefined, path: writePath });
            setWrittenFile(res.writtenRuntimeFile ?? writePath);
            setWriteOpen(false);
          }}
        >
          <label className="field">
            <span>Path (relative to pb_root)</span>
            <input value={writePath} onChange={(e) => setWritePath(e.target.value)} />
          </label>
          <p className="muted small">Writes the {rep?.budgeted ? 'budgeted' : 'suggested'} mapping as a runtime.json, with every other connection kept.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}

/** One measured node (or segment): every model tried there, and pb's recommendation. */
function AllocationTable({ allocation: a }: { allocation: SweepAllocation }) {
  return (
    <section className="bm-sweep-item">
      <h2>{a.key}</h2>
      <div className="bm-matrix-scroll">
        <table className="bm-matrix">
          <thead>
            <tr>
              <th scope="col">Model</th>
              <th scope="col" className="col-num">
                n
              </th>
              <th scope="col" className="col-num">
                Score
              </th>
              <th scope="col" className="col-num">
                Δ score
              </th>
              <th scope="col">Verdict</th>
              <th scope="col" className="col-num">
                Node cost
              </th>
              <th scope="col" className="col-num">
                Δ node
              </th>
              <th scope="col" className="col-num">
                Seg cost
              </th>
              <th scope="col" className="col-num">
                Δ seg
              </th>
              <th scope="col" className="col-num">
                Δscore/$
              </th>
            </tr>
          </thead>
          <tbody>
            {a.models.map((m) => {
              const cells = sweepRowCells(m);
              return (
                <tr key={m.model}>
                  <td>
                    {m.model}
                    {m.isBaseline && <span className="muted"> (baseline)</span>}
                  </td>
                  <td className="col-num tabular">{cells.n}</td>
                  <td className="col-num tabular">{cells.score}</td>
                  <td className="col-num tabular">{cells.delta}</td>
                  <td className={cells.noiseClass}>{m.noise}</td>
                  <td className="col-num tabular">{usd(m.nodeCost)}</td>
                  <td className="col-num tabular">{cells.dnode}</td>
                  <td className="col-num tabular">{usd(m.segCost)}</td>
                  <td className="col-num tabular">{cells.dcost}</td>
                  <td className="col-num tabular">{cells.perUsd}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <p className={`bm-recommendation ${recommendationTone(a.recommendation.action)}`}>{a.recommendation.text}</p>
    </section>
  );
}

/** One `suggestion`/`budgeted` mapping of `pb sweep-report`: node → recorded vs suggested model,
 * why, and the expected Δscore/Δcost. With no changes it is one sentence, not a table. */
function MappingTable({ title, suggestion, recorded, overBudget }: { title: string; suggestion: SweepSuggestion; recorded: Record<string, string | null>; overBudget?: boolean }) {
  const changed = new Map(suggestion.changes.map((c) => [c.node, c]));
  if (suggestion.changes.length === 0) {
    return (
      <section className="bm-sweep-item">
        <h2>{title}</h2>
        <p className="muted small">No change: keep the recorded runtime{suggestion.baseCost != null ? ` (all nodes as recorded, ${usd(suggestion.baseCost)}/run)` : ''}.</p>
      </section>
    );
  }
  return (
    <section className="bm-sweep-item">
      <h2>{title}</h2>
      {overBudget && <p className="bm-signal-neg small">No mapping fits this budget: the cheapest estimate is {usd(suggestion.estCost)}/run.</p>}
      <div className="bm-matrix-scroll">
        <table className="bm-matrix">
          <thead>
            <tr>
              <th scope="col">Node</th>
              <th scope="col">Recorded</th>
              <th scope="col">Suggested</th>
              <th scope="col">Why</th>
              <th scope="col" className="col-num">
                Δ score
              </th>
              <th scope="col" className="col-num">
                Δ cost / run
              </th>
            </tr>
          </thead>
          <tbody>
            {Object.entries(suggestion.mapping)
              .filter(([node]) => changed.has(node))
              .map(([node, model]) => {
                const c = changed.get(node)!;
                return (
                  <tr key={node}>
                    <th scope="row">{node}</th>
                    <td className="muted">{recorded[node] ?? '—'}</td>
                    <td>{model ?? '—'}</td>
                    <td className="muted small">{`${c.why} (${c.key}, ×${c.execs.toFixed(2)}/run)`}</td>
                    <td className="col-num tabular">{signedPp(c.delta)}</td>
                    <td className="col-num tabular">{signedUsd(c.dcostRun)}</td>
                  </tr>
                );
              })}
          </tbody>
        </table>
      </div>
      <p className="muted small">{mappingSummary(suggestion)}</p>
    </section>
  );
}

/** "Write runtime…", off (with the reason beside it) when the mapping would change nothing or
 * does not fit the budget. */
function WriteRuntimeButton({ toWrite, onOpen }: { toWrite: SweepSuggestion; onOpen: () => void }) {
  const disabledReason = writeDisabledReason(toWrite);
  return (
    <div className="modal-buttons">
      <button type="button" className="button" disabled={!!disabledReason} title={disabledReason ?? undefined} onClick={onOpen}>
        Write runtime…
      </button>
      {disabledReason && <span className="muted small">{disabledReason}</span>}
    </div>
  );
}

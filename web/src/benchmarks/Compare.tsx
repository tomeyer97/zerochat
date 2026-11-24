// `#/benchmarks/compare`, with or without `?a=&b=` yet: a picker while a batch is missing, then
// the comparison: per task (score Δ with seed spread, pb's noise verdict, Δ cost, Δ time), a
// totals row, and per node (Δ mean cost/time). The server computes all of it, not this page.
import { useState } from 'react';
import { ChevronRight, FlaskConical } from 'lucide-react';
import { api } from './pb-setup';
import type { BatchCompare, BatchSummary } from '../types';
import { truncate, usd } from '../format';
import { benchmarksBatchHref, benchmarksCompareHref, benchmarksHref, navigate } from '../route';
import { useResource } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { BenchmarksNav } from './shared';
import { deltaTone, durationS, noiseText, signalClass, signedDuration, signedPp, signedUsd, totalsVerdict } from './batch-compare';
import { taskAggText } from './scores';

export function Compare({ a, b }: { a?: string; b?: string }) {
  return a && b ? <CompareView a={a} b={b} /> : <ComparePicker a={a} b={b} />;
}

/** Choose A and B when the link did not name both. */
function ComparePicker({ a, b }: { a?: string; b?: string }) {
  const listResource = useResource<{ batches: BatchSummary[] }>('benchmarks', (signal) => api.benchmarkBatches(signal));
  const [pickA, setPickA] = useState(a ?? '');
  const [pickB, setPickB] = useState(b ?? '');
  const batches = listResource.data?.batches ?? [];
  return (
    <div className="run-page">
      <div className="run-scroll">
        <BenchmarksNav active="batches" />
        <h1>Compare two batches</h1>
        {listResource.loading && <Skeleton lines={4} />}
        {listResource.error && <ErrorBox message={listResource.error} onRetry={listResource.reload} />}
        {listResource.data && batches.length < 2 && (
          <EmptyState title="Not enough batches yet" icon={<FlaskConical size={22} aria-hidden />}>
            <p>Run at least two batches, then compare them here.</p>
          </EmptyState>
        )}
        {listResource.data && batches.length >= 2 && (
          <div className="bm-compare-pick">
            <BatchSelect label="A (baseline)" value={pickA} onChange={setPickA} batches={batches} />
            <BatchSelect label="B (candidate)" value={pickB} onChange={setPickB} batches={batches} />
            <button type="button" className="button primary" disabled={!pickA || !pickB} onClick={() => navigate({ kind: 'benchmarksCompare', a: pickA, b: pickB })}>
              Compare
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function BatchSelect({ label, value, onChange, batches }: { label: string; value: string; onChange: (id: string) => void; batches: BatchSummary[] }) {
  return (
    <label className="field">
      <span>{label}</span>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">Choose…</option>
        {batches.map((bt) => (
          <option key={bt.id} value={bt.id}>
            {bt.label || bt.id}
          </option>
        ))}
      </select>
    </label>
  );
}

function CompareView({ a, b }: { a: string; b: string }) {
  const compareResource = useResource<BatchCompare>(`benchmarks/compare/${a}/${b}`, (signal) => api.benchmarkCompare(a, b, signal));
  const cmp = compareResource.data;
  return (
    <div className="run-page">
      <div className="run-scroll">
        <BenchmarksNav active="batches" />
        <header className="run-header">
          <p className="muted small">
            <a href={benchmarksHref()}>Benchmarks</a> <ChevronRight size={12} aria-hidden style={{ verticalAlign: 'middle' }} /> Compare
          </p>
          <h1>{cmp ? `${cmp.aLabel || cmp.aId} vs ${cmp.bLabel || cmp.bId}` : 'Compare'}</h1>
          {/* Both sides link to their own matrix, and Swap is a real control, not a hand-edited URL. */}
          {cmp && (
            <p className="muted small">
              A: <a href={benchmarksBatchHref(cmp.aId)}>{cmp.aLabel || cmp.aId}</a> ({cmp.aRunCount} runs) · B: <a href={benchmarksBatchHref(cmp.bId)}>{cmp.bLabel || cmp.bId}</a> ({cmp.bRunCount}{' '}
              runs) · <a href={benchmarksCompareHref(cmp.bId, cmp.aId)}>⇄ swap</a>
            </p>
          )}
        </header>
        {compareResource.loading && <Skeleton lines={6} />}
        {compareResource.error && <ErrorBox message={compareResource.error} onRetry={compareResource.reload} />}
        {cmp && (
          <>
            <TaskTable cmp={cmp} />
            {cmp.nodes.length > 0 && <NodeTable nodes={cmp.nodes} />}
          </>
        )}
      </div>
    </div>
  );
}

function TaskTable({ cmp }: { cmp: BatchCompare }) {
  return (
    <div className="bm-matrix-scroll">
      <table className="bm-matrix bm-compare-table">
        <thead>
          <tr>
            <th scope="col" className="bm-task">
              Task
            </th>
            <th scope="col" className="col-num">
              A ({cmp.aRunCount} runs)
            </th>
            <th scope="col" className="col-num">
              B ({cmp.bRunCount} runs)
            </th>
            <th scope="col" className="col-num">
              Δ score
            </th>
            <th scope="col" className="col-num">
              Δ cost
            </th>
            <th scope="col" className="col-num">
              Δ time
            </th>
            <th scope="col">Verdict</th>
          </tr>
        </thead>
        <tbody>
          {cmp.tasks.map((t) => {
            // Colour only where attention is needed: a within-noise or needs-seeds Δ stays
            // neutral however large its number; only a beyond-spread change is green (B ahead)
            // or red (B behind).
            const tone = deltaTone(t.noise, t.deltaScore);
            return (
              <tr key={t.task}>
                <th scope="row" className="bm-task" title={t.task}>
                  {truncate(t.task, 32)}
                </th>
                <td className="col-num tabular">{taskAggText(t.a)}</td>
                <td className="col-num tabular">{taskAggText(t.b)}</td>
                <td className={`col-num tabular ${signalClass(tone, '')}`}>{signedPp(t.deltaScore)}</td>
                <td className="col-num tabular">{signedUsd(t.deltaCostUsd)}</td>
                <td className="col-num tabular">{signedDuration(t.deltaTimeS)}</td>
                <td className={signalClass(tone)}>{noiseText(t.noise)}</td>
              </tr>
            );
          })}
        </tbody>
        {cmp.totals && <CompareTotals tasks={cmp.tasks} totals={cmp.totals} />}
      </table>
    </div>
  );
}

/** The "Mean over N task(s)" row, with its own verdict counted per task: a change counts only
 * if every task moved beyond its seed spread the same way. */
function CompareTotals({ tasks, totals }: { tasks: BatchCompare['tasks']; totals: NonNullable<BatchCompare['totals']> }) {
  const overall = totalsVerdict(tasks);
  const tone = overall.verdict === 'beyond_spread' ? deltaTone('beyond_spread', totals.deltaScore) : null;
  return (
    <tfoot>
      <tr>
        <th scope="row">Mean over {totals.tasks} task(s)</th>
        <td></td>
        <td></td>
        <td className={`col-num tabular ${signalClass(tone, '')}`}>{signedPp(totals.deltaScore)}</td>
        <td className="col-num tabular">{signedUsd(totals.deltaCostUsd)}</td>
        <td className="col-num tabular">{signedDuration(totals.deltaTimeS)}</td>
        <td className={signalClass(tone)}>{overall.text}</td>
      </tr>
    </tfoot>
  );
}

function NodeTable({ nodes }: { nodes: BatchCompare['nodes'] }) {
  return (
    <>
      <h2>Per node</h2>
      <div className="bm-matrix-scroll">
        <table className="bm-matrix bm-compare-table">
          <thead>
            <tr>
              <th scope="col">Node</th>
              <th scope="col" className="col-num">
                A mean cost
              </th>
              <th scope="col" className="col-num">
                B mean cost
              </th>
              <th scope="col" className="col-num">
                Δ cost
              </th>
              <th scope="col" className="col-num">
                A mean time
              </th>
              <th scope="col" className="col-num">
                B mean time
              </th>
              <th scope="col" className="col-num">
                Δ time
              </th>
            </tr>
          </thead>
          <tbody>
            {nodes.map((n) => (
              <tr key={n.node}>
                <th scope="row">{n.node}</th>
                <td className="col-num tabular">{usd(n.a?.meanCostUsd ?? null)}</td>
                <td className="col-num tabular">{usd(n.b?.meanCostUsd ?? null)}</td>
                <td className="col-num tabular">{signedUsd(n.deltaMeanCostUsd)}</td>
                <td className="col-num tabular">{durationS(n.a?.meanTimeS)}</td>
                <td className="col-num tabular">{durationS(n.b?.meanTimeS)}</td>
                <td className="col-num tabular">{signedDuration(n.deltaMeanTimeS)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}

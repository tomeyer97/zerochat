// The Batches list (`#/benchmarks`): one table of pb's batches, and a bar to compare two of them.
import { useState } from 'react';
import { FlaskConical } from 'lucide-react';
import { api } from './pb-setup';
import type { BatchSummary } from '../types';
import { costText, fullTime, timeOfDay } from '../format';
import { benchmarksBatchHref, benchmarksCompareHref } from '../route';
import { useInterval, useNow, useResource } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { batchIsLive, retriedText } from './batches';
import { batchCompactText, batchDoneTitle, batchIdentity, batchNoCostReason, batchNoScoreReason, batchScoreText, comparePair, toggleCompare } from './batch-list';
import { BatchStatusPill, BenchmarksNav } from './shared';

export function BatchList() {
  const resource = useResource<{ batches: BatchSummary[] }>('benchmarks', (signal) => api.benchmarkBatches(signal));
  const batches = resource.data?.batches ?? [];
  const live = batches.some(batchIsLive);
  const now = useNow(live, 1000);
  useInterval(resource.reload, live ? 3000 : 15_000, true);
  const compare = useCompareSelection(batches);

  return (
    <div className="home">
      <BenchmarksNav active="batches" />
      <h1 className="visually-hidden">Benchmarks</h1>
      {resource.loading && <Skeleton lines={10} />}
      {resource.error && <ErrorBox compact message={resource.error} onRetry={resource.reload} />}
      {resource.data && batches.length === 0 && (
        <EmptyState title="No batches yet" icon={<FlaskConical size={22} aria-hidden />}>
          <p>Run a benchmark batch with pb, then reload.</p>
        </EmptyState>
      )}
      {batches.length > 0 && (
        <table className="run-table bm-table">
          <BatchListHead />
          <tbody>
            {batches.map((b) => (
              <BatchRow key={b.id} batch={b} now={now} picked={compare.picked.includes(b.id)} onToggle={() => compare.toggle(b.id)} />
            ))}
          </tbody>
        </table>
      )}
      {compare.picked.length > 0 && <CompareBar picked={compare.picked} pair={compare.pair} labelOf={compare.labelOf} onClear={compare.clear} />}
    </div>
  );
}

/** Up to two batches ticked to compare, and the pair in A/B order (`comparePair`). */
function useCompareSelection(batches: BatchSummary[]) {
  const [picked, setPicked] = useState<string[]>([]);
  return {
    picked,
    pair: comparePair(picked, batches),
    toggle: (id: string) => setPicked((sel) => toggleCompare(sel, id)),
    clear: () => setPicked([]),
    labelOf: (id: string) => batches.find((b) => b.id === id)?.label || id,
  };
}

/** A visible header, unlike the main Runs list's: "1×2", "2/2" and a score mean nothing without
 * a header naming them (at phone width, where these columns hide, the row's own line labels
 * each number instead). */
function BatchListHead() {
  return (
    <thead>
      <tr>
        <th scope="col" className="col-status">
          <span className="visually-hidden">Compare</span>
        </th>
        <th scope="col" className="col-status bm-status-col">
          Status
        </th>
        <th scope="col">Batch</th>
        <th scope="col" className="col-num">
          Tasks × seeds
        </th>
        <th scope="col" className="col-num" title="cells (task × seed) done / planned; +N = retried attempts">
          Runs done
        </th>
        <th scope="col" className="col-num col-score" title="mean score of the scored runs (lowest–highest), 0 to 1">
          Score (range)
        </th>
        <th scope="col" className="col-num col-cost">
          Cost
        </th>
        <th scope="col" className="col-time">
          Started
        </th>
      </tr>
    </thead>
  );
}

function BatchRow({ batch: b, now, picked, onToggle }: { batch: BatchSummary; now: number; picked: boolean; onToggle: () => void }) {
  const identity = batchIdentity(b);
  const retried = retriedText(b);
  const noScore = batchNoScoreReason(b);
  const noCost = batchNoCostReason(b);
  return (
    <tr>
      <td className="col-status">
        <input type="checkbox" aria-label={`Compare ${b.label || b.id}`} checked={picked} onChange={onToggle} />
      </td>
      <td className="col-status bm-status-col">
        <BatchStatusPill status={b.status} />
      </td>
      <td className="col-run">
        <a href={benchmarksBatchHref(b.id)} className="run-link" title={b.id}>
          {b.label || <em>(no label)</em>}
        </a>
        {identity && (
          <span className="run-cause">
            <span className="muted" title={b.zeroshotVersion ? `zeroshot ${b.zeroshotVersion}` : undefined}>
              {identity}
            </span>
          </span>
        )}
        {/* Shown only at phone width, where the numeric columns are hidden (benchmarks.css). */}
        <span className="run-cause bm-row-compact muted small">{batchCompactText(b, now)}</span>
      </td>
      <td className="col-num tabular">
        {b.taskCount}×{b.seedCount}
      </td>
      <td className="col-num tabular" title={batchDoneTitle(b)}>
        {b.runsDone}/{b.runsTotal}
        {b.retriedAttempts > 0 && <span className="muted small bm-retried"> +{b.retriedAttempts}</span>}
      </td>
      <td className="col-num col-score tabular">
        {batchScoreText(b)}
        {noScore && <span className="bm-why muted small">{noScore}</span>}
      </td>
      <td className="col-num col-cost tabular" title={noCost ?? (retried ? `every attempt, including ${retried}` : undefined)}>
        {costText(b.totalCostUsd, false)}
      </td>
      <td className="col-time tabular" title={fullTime(b.startedAt, now)}>
        {timeOfDay(b.startedAt, now)}
      </td>
    </tr>
  );
}

/** Sticky under the table: a bar placed after a long table would sit off-screen until the list
 * is scrolled to its end. */
function CompareBar({ picked, pair, labelOf, onClear }: { picked: string[]; pair: [string, string] | null; labelOf: (id: string) => string; onClear: () => void }) {
  return (
    <div className="bm-compare-bar">
      {pair ? (
        <span>
          Compare <strong>A: {labelOf(pair[0])}</strong> vs <strong>B: {labelOf(pair[1])}</strong>
        </span>
      ) : (
        <span>
          <strong>{labelOf(picked[0])}</strong> picked — pick one more batch to compare
        </span>
      )}
      {pair && (
        <a className="button primary small" href={benchmarksCompareHref(pair[0], pair[1])}>
          Compare
        </a>
      )}
      <button type="button" className="button small" onClick={onClear}>
        Clear
      </button>
    </div>
  );
}

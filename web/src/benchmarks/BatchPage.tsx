// The batch page (`#/benchmarks/batch/<id>`): header, what ran, the summary line and the task ×
// seed matrix; before pb has recorded a new batch, a "Starting…" page instead.
import { useState, type ReactNode } from 'react';
import { ChevronRight, FlaskConical, LoaderCircle, Square } from 'lucide-react';
import { api } from './pb-setup';
import type { BatchDetail } from '../types';
import { duration } from '../format';
import { benchmarksCompareHref, benchmarksHref } from '../route';
import { useInterval, useNow, useResource } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { batchCostText, batchCostTitle, batchCounts, batchMaxSpend, batchStartPhase, detailIsLive, isReplayBatch, limitsText, retriedText, startedAtText, type BatchCounts } from './batches';
import { summaryText } from './cell-states';
import { hasSpread, scoreText } from './scores';
import { startedRecently } from './started-batches';
import { BatchMatrix } from './BatchMatrix';
import { BatchStatusPill, ScoreValue } from './shared';
import { HarnessSection, ReplaySection } from './WhatRan';

export function BatchPage({ id }: { id: string }) {
  const resource = useResource<BatchDetail>(`batch/${id}`, (signal) => api.benchmarkBatch(id, signal));
  const detail = resource.data;
  // Right after Start the batch has no row in pb.sqlite yet: keep asking (every 2 s) instead of
  // settling on "Batch not found".
  const startPhase = batchStartPhase(detail, resource.errorStatus, id);
  const live = (!!detail && detailIsLive(detail)) || startPhase === 'starting';
  const now = useNow(live, 1000);
  useInterval(resource.reload, startPhase === 'starting' || detail?.starting ? 2000 : live ? 3000 : 15_000, true);
  const [stopOpen, setStopOpen] = useState(false);

  if (resource.loading && !detail) {
    return (
      <PageShell>
        <Skeleton lines={2} />
        <Skeleton height={280} />
      </PageShell>
    );
  }
  if (startPhase) {
    return <BatchStarting id={id} phase={startPhase} detail={detail} now={now} />;
  }
  if (!detail) {
    return (
      <PageShell>
        {resource.errorStatus === 404 ? (
          <EmptyState title="Batch not found" icon={<FlaskConical size={22} aria-hidden />}>
            <p>
              <a href={benchmarksHref()}>All batches</a>
            </p>
          </EmptyState>
        ) : (
          <ErrorBox message={resource.error ?? 'Could not load this batch.'} onRetry={resource.reload} />
        )}
      </PageShell>
    );
  }

  const counts = batchCounts(detail);
  const replays = detail.replays ?? [];
  return (
    <PageShell>
      <BatchHeader detail={detail} plannedCells={counts.planned} now={now} onStop={() => setStopOpen(true)} />
      {replays.length > 0 && <ReplaySection replays={replays} />}
      <HarnessSection detail={detail} />
      {stopOpen && (
        <ConfirmDialog
          title="Stop this batch?"
          confirmLabel="Stop"
          danger
          onClose={() => setStopOpen(false)}
          onConfirm={async () => {
            await api.benchmarkStopBatch(detail.id);
            setStopOpen(false);
            resource.reload();
          }}
        >
          <p>pb force-stops every Zeroshot run of this batch and marks them interrupted; results already scored are kept.</p>
        </ConfirmDialog>
      )}
      {detail.starting && (
        <p className="bm-starting" role="status">
          <LoaderCircle size={14} className="spin" aria-hidden /> Starting… <span className="muted">pb is preparing the first run's sandbox.</span>
        </p>
      )}
      <BatchSummaryLine detail={detail} counts={counts} />
      <BatchMatrix detail={detail} isReplay={isReplayBatch(detail)} now={now} />
    </PageShell>
  );
}

function PageShell({ children }: { children: ReactNode }) {
  return (
    <div className="run-page">
      <div className="run-scroll">{children}</div>
    </div>
  );
}

function Breadcrumb({ id }: { id: string }) {
  return (
    <p className="muted small">
      <a href={benchmarksHref()}>Benchmarks</a> <ChevronRight size={12} aria-hidden style={{ verticalAlign: 'middle' }} /> {id}
    </p>
  );
}

/** Title, status, Stop and Compare, then the meta line: limits, parallelism, start time and total
 * spend, the facts an A/B benchmark exists to vary. What the harness is has its own section. */
function BatchHeader({ detail, plannedCells, now, onStop }: { detail: BatchDetail; plannedCells: number; now: number; onStop: () => void }) {
  // A `label:` id is a pseudo-batch (runs grouped by label, from before pb's batches table
  // existed): there is no pb process behind it to stop.
  const canStop = detail.status === 'running' && !detail.id.startsWith('label:');
  const retriedAttempts = detail.retriedAttempts ?? 0;
  const retried = retriedText({ retriedAttempts, retriedCostUsd: detail.retriedCostUsd ?? null });
  const maxSpendTotal = batchMaxSpend(detail.limits?.maxCostUsd, plannedCells, retriedAttempts);
  const limits = limitsText(detail.limits);
  const started = startedAtText(detail.startedAt, now);
  return (
    <header className="run-header">
      <Breadcrumb id={detail.id} />
      <div className="run-title-row">
        <h1 className="run-title-text">{detail.label || <em>(no label)</em>}</h1>
        <BatchStatusPill status={detail.status} />
        <span className="run-actions">
          {canStop && (
            <button type="button" className="button danger small" onClick={onStop}>
              <Square size={13} aria-hidden /> Stop
            </button>
          )}
          {/* Compare from here, not only by ticking two rows in the list. */}
          <a className="button small" href={benchmarksCompareHref(detail.id)}>
            Compare with…
          </a>
        </span>
      </div>
      <div className="run-meta-line">
        {limits && <span title="limits per run">{limits} per run</span>}
        {detail.parallel != null && <span>×{detail.parallel} parallel</span>}
        <span title={started.title}>{started.text}</span>
        {detail.totalCostUsd != null && <span title={batchCostTitle(plannedCells, retriedAttempts)}>{batchCostText(detail.totalCostUsd, maxSpendTotal)}</span>}
        {retried && <span className="muted">{retried}</span>}
        {detail.totalWallS != null && <span title="summed over every run">{duration(detail.totalWallS * 1000)} total</span>}
      </div>
    </header>
  );
}

/** The batch mean, said once and plainly above the table, then how many cells are in each state. */
function BatchSummaryLine({ detail, counts }: { detail: BatchDetail; counts: BatchCounts }) {
  return (
    <div className="bm-summary">
      <span className="bm-summary-mean" title="mean score over every scored cell (each the share of its task's hidden tests that pass)">
        mean score <ScoreValue score={detail.meanScore} />
        {hasSpread(detail.minScore, detail.maxScore) && (
          <span className="muted small tabular">
            {' '}
            ({scoreText(detail.minScore)}–{scoreText(detail.maxScore)})
          </span>
        )}
      </span>
      {counts.cutShort > 0 && (
        <span className="small" title="the mean over cells whose harness finished on its own">
          <ScoreValue score={detail.meanScoreFinished ?? null} /> <span className="muted">without the {counts.cutShort} cut-short</span>
        </span>
      )}
      <span className="muted small">{summaryText(counts.byGroup, counts.planned, counts.cutShort)}</span>
    </div>
  );
}

/** The batch page before pb has written the batch (or after its runner died before it could).
 * The page polls every 2 s and shows the matrix as soon as pb has the batch. */
function BatchStarting({ id, phase, detail, now }: { id: string; phase: 'starting' | 'failed_to_start'; detail: BatchDetail | undefined; now: number }) {
  const since = detail?.startedAt ?? startedRecently(id, now);
  const starting = phase === 'starting';
  return (
    <PageShell>
      <header className="run-header">
        <Breadcrumb id={id} />
        <div className="run-title-row">
          <h1 className="run-title-text">{starting ? 'Starting…' : 'Batch did not start'}</h1>
          <BatchStatusPill status={starting ? 'starting' : 'failed to start'} />
        </div>
      </header>
      {starting ? (
        <p className="bm-starting" role="status">
          <LoaderCircle size={14} className="spin" aria-hidden /> pb is starting batch <code>{id}</code>
          {since != null && <span className="muted tabular"> · {duration(Math.max(0, now - since))}</span>}
          <span className="muted"> — this page fills in as soon as pb records it.</span>
        </p>
      ) : (
        <ErrorBox message={`pb's runner for batch ${id} exited before it recorded the batch, so nothing ran and nothing was spent.`} />
      )}
      {detail?.startLog && (
        <details className="bm-start-log" open={!starting}>
          <summary className="small">pb's log (last lines)</summary>
          <pre>{detail.startLog}</pre>
        </details>
      )}
    </PageShell>
  );
}

// The batch page's task × seed matrix: one cell per run, a per-task mean on the right, and the
// legend that explains its colours and marks.
import { Fragment, useMemo } from 'react';
import type { BatchDetail, BenchmarkAggregate, BenchmarkCell } from '../types';
import { duration, truncate, usd } from '../format';
import { runHref } from '../route';
import { aggregateByKey, cellKey, cellMap, cellTitle, cutShortBadge, liveNodeText, seedHeader } from './batch-cells';
import { cellStateTone, STATE_LEGEND, stateLabel } from './cell-states';
import { hasSpread, scoreColor, scoreText } from './scores';

/** More columns than this and the headers become bare seed numbers. */
const MANY_SEEDS = 8;

export function BatchMatrix({ detail, isReplay, now }: { detail: BatchDetail; isReplay: boolean; now: number }) {
  const cells = useMemo(() => cellMap(detail.cells), [detail]);
  // Only per-task means are shown: a per-seed mean (`colAggregates`) would pool unrelated tasks.
  const rowAgg = useMemo(() => aggregateByKey(detail.rowAggregates), [detail]);
  const manySeeds = detail.seeds.length > MANY_SEEDS;
  return (
    <>
      <div className="bm-matrix-scroll">
        <table className={`bm-matrix bm-batch-matrix ${manySeeds ? 'bm-many-seeds' : ''}`}>
          <thead>
            <tr>
              <th scope="col" className="bm-task">
                {manySeeds ? (isReplay ? 'Task \\ replay' : 'Task \\ seed') : 'Task'}
              </th>
              {detail.seeds.map((seed) => {
                const h = seedHeader(seed, isReplay, manySeeds);
                return (
                  <th scope="col" key={seed} className="col-num bm-seed-col" title={h.title}>
                    {h.text}
                  </th>
                );
              })}
              <th scope="col" className="col-num bm-mean-col">
                Mean
              </th>
            </tr>
          </thead>
          <tbody>
            {detail.tasks.map((task) => (
              <tr key={task}>
                <th scope="row" title={task} className="bm-task">
                  {truncate(task, 40)}
                </th>
                {detail.seeds.map((seed) => (
                  <td key={seed} className="col-num bm-cell">
                    <MatrixCell cell={cells.get(cellKey(task, seed))} now={now} />
                  </td>
                ))}
                <AggCell agg={rowAgg.get(task)} className="bm-mean-col" />
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <BatchLegend manySeeds={manySeeds} />
    </>
  );
}

function MatrixCell({ cell, now }: { cell: BenchmarkCell | undefined; now: number }) {
  if (!cell) return <span className="muted">—</span>;
  if (cell.state === 'queued') {
    return (
      <span className="bm-cell-queued" title={cellTitle(cell)}>
        queued
      </span>
    );
  }
  if (cell.state === 'not_started') {
    return (
      <span className="muted small" title={cellTitle(cell)}>
        not started
      </span>
    );
  }
  const tone = cellStateTone(cell.state);
  const badge = cutShortBadge(cell);
  return (
    <a
      href={cell.zeroshotRunId ? runHref('pb', cell.zeroshotRunId) : undefined}
      className={`bm-cell-link ${tone ? `tone-text-${tone}` : ''}`}
      title={cellTitle(cell, now)}
      aria-disabled={!cell.zeroshotRunId}
    >
      {cell.score != null ? (
        <span style={{ color: scoreColor(cell.score), fontWeight: 600 }}>{scoreText(cell.score)}</span>
      ) : cell.state === 'running_agent' ? (
        <RunningCell cell={cell} now={now} />
      ) : (
        stateLabel(cell.state, cell.exitReason)
      )}
      {badge && <span className="bm-cut">{badge.short}</span>}
      {cell.attempts > 1 && <span className="bm-attempts">×{cell.attempts}</span>}
    </a>
  );
}

/** A running cell links to its live run (pb records the Zeroshot run id as soon as its ledger
 * exists) and says how long it has run, which node it is in, and what it has spent so far: a
 * live estimate (`~`), since pb records a run's spend only when its agent finishes. */
function RunningCell({ cell, now }: { cell: BenchmarkCell; now: number }) {
  const elapsed = cell.startedAt != null ? duration(Math.max(0, now - cell.startedAt)) : null;
  return (
    <span className="bm-live">
      <span>running{elapsed && <span className="tabular"> · {elapsed}</span>}</span>
      {(cell.liveNodes || cell.liveCostUsd != null) && (
        <span className="bm-live-detail small">
          {liveNodeText(cell.liveNodes) ?? (cell.liveNodes ? 'finishing' : null)}
          {cell.liveNodes && cell.liveCostUsd != null && ' · '}
          {cell.liveCostUsd != null && <span className="tabular">~{usd(cell.liveCostUsd)}</span>}
        </span>
      )}
    </span>
  );
}

function AggCell({ agg, className = '' }: { agg: BenchmarkAggregate | undefined; className?: string }) {
  if (!agg || agg.mean == null) return <td className={`col-num tabular bm-agg muted ${className}`}>—</td>;
  const range = agg.min != null && agg.max != null ? `, range ${scoreText(agg.min)}–${scoreText(agg.max)}` : '';
  return (
    <td className={`col-num tabular bm-agg ${className}`} style={{ color: scoreColor(agg.mean) }} title={`mean of ${agg.n} scored seed${agg.n === 1 ? '' : 's'}${range}`}>
      {scoreText(agg.mean)}
      {/* Its own span: at phone width the range goes under the mean instead of widening the
          pinned Mean column. */}
      {hasSpread(agg.min, agg.max) && (
        <span className="bm-agg-range">
          {' '}
          ({scoreText(agg.min)}–{scoreText(agg.max)})
        </span>
      )}
    </td>
  );
}

/** The page's key to its own numbers, colours and marks: one line of short items, not a paragraph. */
function BatchLegend({ manySeeds }: { manySeeds: boolean }) {
  return (
    <ul className="bm-legend small" aria-label="How to read this table">
      <li>
        <strong>Score</strong> = share of the task's hidden tests that pass, 0–1 (0.85 = 85%)
      </li>
      <li>
        <strong>Mean</strong> = per task, over its scored seeds (range in brackets)
      </li>
      {manySeeds && <li>column headers are seed numbers</li>}
      <li>
        {STATE_LEGEND.map((item, i) => (
          <Fragment key={item.text}>
            {i > 0 && ' · '}
            <span className={item.className}>{item.text}</span>
          </Fragment>
        ))}
      </li>
      <li>
        <span className="bm-cut">$6 cap</span> <span className="bm-cut">⏱ 45m</span> <span className="bm-cut">24 steps</span> <span className="bm-cut">stopped</span> = cut short by that limit, a Stop
        or a crash; scored on the code it had, counted in the mean
      </li>
      <li>
        <span className="bm-attempts">×3</span> = attempts at this cell; the latest scored one is shown
      </li>
    </ul>
  );
}

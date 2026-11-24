import { Fragment, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type { Execution, RunView } from './types';
import { execStateInfo, nodeState } from './status';
import { compact, costText, duration, elapsedMs, totalTokens, usageBreakdown, clockTime } from './format';
import { nodeHref, visitOf } from './route';
import { useNow } from './hooks';
import { gridTemplate, rowMatches, rowNote, stateTone, timelineBars, timelineLayout, visibleColumns, type TimelineColumn, type TimelineColumnId } from './timeline-model';

// Kept here for existing imports; the rules live in timeline-model.ts.
export { errorWords, rowNote, timelineBars } from './timeline-model';

/** The table's width, followed as it resizes (the columns depend on it). */
function useWidth(initial: number) {
  const [width, setWidth] = useState(initial);
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return { width, ref };
}

export function Timeline({ view, selectedExecution }: { view: RunView; selectedExecution?: number }) {
  const running = view.executions.some((e) => e.state === 'running');
  const now = useNow(running);
  const [filter, setFilter] = useState('');
  const { width, ref: tableRef } = useWidth(1000);
  const { bars, start } = useMemo(() => timelineBars(view.executions, now), [view.executions, now]);
  const needle = filter.trim().toLowerCase();
  const rows = view.executions.filter((e) => rowMatches(e, view, needle));
  if (!view.executions.length) {
    return (
      <section className="timeline" aria-labelledby="timeline-title">
        <h2 id="timeline-title" className="section-title">
          Executions
        </h2>
        <p className="muted">No node has started yet.</p>
      </section>
    );
  }
  const columns = visibleColumns(timelineLayout(view, width));
  const grid = { gridTemplateColumns: gridTemplate(columns) };
  return (
    <section className="timeline" aria-labelledby="timeline-title">
      <div className="section-head">
        <h2 id="timeline-title" className="section-title">
          Executions <span className="muted small">({needle ? `${rows.length} of ${view.executions.length}` : view.executions.length})</span>
        </h2>
        {view.executions.length > 8 && (
          <input
            className="timeline-filter"
            type="search"
            placeholder="Filter by node, state, round or note"
            aria-label="Filter executions"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        )}
      </div>
      {needle && rows.length === 0 ? (
        <p className="muted">
          No execution matches “{filter.trim()}”.{' '}
          <button type="button" className="text-button" onClick={() => setFilter('')}>
            Clear filter
          </button>
        </p>
      ) : (
        <div className="timeline-table" role="table" aria-label="Executions" ref={tableRef}>
          <div className="tl-row tl-head" role="row" style={grid}>
            {columns.map((c) => (
              <span key={c.id} role="columnheader" className={c.headerClass}>
                {c.header}
              </span>
            ))}
          </div>
          {rows.map((e) => (
            <TimelineRow key={e.id} e={e} view={view} now={now} start={start} bar={bars.get(e.id)} columns={columns} grid={grid} selected={selectedExecution === e.id} />
          ))}
        </div>
      )}
    </section>
  );
}

function TimelineRow({
  e,
  view,
  now,
  start,
  bar,
  columns,
  grid,
  selected,
}: {
  e: Execution;
  view: RunView;
  now: number;
  /** When the first execution started: offsets are from here. */
  start: number;
  bar: { left: number; width: number } | undefined;
  columns: TimelineColumn[];
  grid: CSSProperties;
  selected: boolean;
}) {
  const state = nodeState(e.state);
  const culprit = view.failure?.execution === e.id;
  const offset = e.startedAt != null ? `${e.timesApproximate ? '≈' : ''}+${duration(e.startedAt - start)} · ${clockTime(e.startedAt, true)}` : '';
  const cell = (id: TimelineColumnId) => {
    switch (id) {
      case 'node':
        return (
          <span role="cell" className="tl-node">
            {e.node}
          </span>
        );
      case 'round':
        return (
          <span role="cell" className="tabular muted">
            {e.round}
            {e.attempt > 1 && <span title={`Attempt ${e.attempt}`}> · try {e.attempt}</span>}
          </span>
        );
      case 'state':
        return (
          <span role="cell" className={`tl-state tone-text-${stateTone(state, culprit)}`} title={execStateInfo[state].explain}>
            {execStateInfo[state].label}
          </span>
        );
      case 'duration':
      case 'cost':
      case 'tokens':
        // A reused execution ran in the original run: nothing of its own to show here.
        return e.reused ? <span role="cell" /> : <UsageCell id={id} e={e} now={now} />;
      case 'note': {
        const note = rowNote(e, view);
        return (
          <span role="cell" className={`tl-verdict ${culprit ? `tone-text-${stateTone(state, culprit)}` : ''}`} title={note.title}>
            {note.text}
          </span>
        );
      }
      case 'bar':
        return (
          <span role="cell" className="tl-bar-cell" aria-hidden title={offset}>
            {bar && <span className={`tl-bar state-${state} ${culprit ? 'culprit' : ''}`} style={{ left: `${bar.left * 100}%`, width: `${bar.width * 100}%` }} />}
          </span>
        );
    }
  };
  return (
    <a
      role="row"
      style={grid}
      href={nodeHref(view.target, view.runId, e.node, visitOf(e))}
      className={`tl-row state-${state} ${selected ? 'selected' : ''}`}
      aria-current={selected ? 'true' : undefined}
      title={`Execution #${e.id}${offset ? ` · started ${offset}` : ''}`}
    >
      {columns.map((c) => (
        <Fragment key={c.id}>{cell(c.id)}</Fragment>
      ))}
    </a>
  );
}

/** Duration, cost or tokens of an execution that ran in this run. */
function UsageCell({ id, e, now }: { id: 'duration' | 'cost' | 'tokens'; e: Execution; now: number }) {
  if (id === 'duration') {
    const elapsed = elapsedMs({ live: e.state === 'running', startedAt: e.startedAt, recorded: e.durationMs }, now);
    return (
      <span role="cell" className="tabular tl-duration">
        {e.timesApproximate && elapsed != null ? '≈' : ''}
        {duration(elapsed)}
      </span>
    );
  }
  if (id === 'cost') {
    const title = e.costSource === 'estimate' ? 'Estimated from tokens' : e.costSource === 'provider' ? 'Reported by the provider' : '';
    return (
      <span role="cell" className="tabular tl-cost" title={title}>
        {costText(e.costUsd, e.costSource === 'estimate')}
      </span>
    );
  }
  const pending = e.usageSource === 'pending';
  const total = totalTokens(e.usage);
  const live = e.usageSource === 'session_log' && e.state === 'running' ? ' live' : '';
  return (
    <span role="cell" className="tabular tl-tokens" title={pending ? 'Counted when the node finishes' : usageBreakdown(e.usage)}>
      {pending ? '…' : total ? `${compact(total)}${live}` : '—'}
    </span>
  );
}

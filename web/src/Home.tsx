// Home: every target's runs in one table, with status chips and a text filter; with no runs, the
// first-run card (onboarding/FirstRunCard).
import { useMemo, useState } from 'react';
import { Search } from 'lucide-react';
import { homeRows, runRowFacts, type HomeGroup, type HomeRow } from './status';
import { costText, duration, fullTime, timeOfDay } from './format';
import { runHref } from './route';
import type { OverviewState } from './overview-poll';
import { cx, ErrorBox, Skeleton, StatusDot } from './common';
import type { Setup } from './generated/discovery';
import { FirstRunCard } from './onboarding/FirstRunCard';
import { FirstVisitHint } from './onboarding/FirstVisitHint';
import { StatusLegend } from './onboarding/StatusLegend';
import { repeatedTitles, rowSubtitle } from './onboarding/run-rows';

/**
 * The chips: the runs that need you (stopped and succeeded runs are found with the filter box).
 * The set is fixed and a chip stays at 0: one that hid at 0 and came back at 1 would shift every
 * chip after it under the cursor of someone about to click one.
 */
const groups: { id: HomeGroup; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'running', label: 'Running' },
  { id: 'failed', label: 'Failed' },
];

export function Home({ overview, now, setup = null }: { overview: OverviewState; now: number; setup?: Setup | null }) {
  const [filter, setFilter] = useState('');
  const [group, setGroup] = useState<HomeGroup>('all');
  const targets = overview.data?.targets;
  const { all, rows, counts, manyTargets } = useMemo(() => homeRows(targets ?? [], filter, group), [targets, filter, group]);
  const problems = (targets ?? []).filter((t) => t.status !== 'ok');
  const repeated = useMemo(() => repeatedTitles(all), [all]);
  return (
    <div className="home">
      {/* Every page has an h1 so screen readers land on a document outline. Visually hidden: the
          page itself already is the run list. */}
      <h1 className="visually-hidden">All runs</h1>
      {all.length > 0 && <FirstVisitHint />}
      {all.length > 0 && (
        <div className="home-bar">
          <div className="chips" role="group" aria-label="Filter by status">
            {groups.map((g) => (
              <button key={g.id} type="button" aria-pressed={group === g.id} className={cx('chip', group === g.id && 'on')} onClick={() => setGroup(g.id)}>
                {g.label}
                {g.id !== 'all' && <span className="chip-count"> {counts[g.id] ?? 0}</span>}
              </button>
            ))}
          </div>
          <label className="home-search">
            <Search size={14} aria-hidden />
            <input type="search" placeholder="Filter by title, status, cause, repo" aria-label="Filter runs" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </label>
        </div>
      )}
      {overview.loading && <Skeleton lines={10} />}
      {overview.error && <ErrorBox compact message={overview.error} onRetry={overview.reload} />}
      {/* A config problem the run list still works around (e.g. one bad field): a muted note,
          not an alarm — red is for when no run can be listed at all. The message already names
          the file, so it needs no separate heading. */}
      {overview.data?.issue && <p className="muted small">{overview.data.issue}</p>}
      {overview.data && !overview.error && all.length === 0 && <FirstRunCard setup={setup} stateRoot={overview.data.stateRoot} />}
      {rows.length > 0 && (
        <table className="run-table">
          <thead className="run-table-head">
            <tr>
              <th scope="col" className="col-status">
                <span className="visually-hidden">Status</span>
              </th>
              <th scope="col">Run</th>
              <th scope="col" className="col-time">
                Started
              </th>
              <th scope="col" className="col-num">
                Duration
              </th>
              <th scope="col" className="col-num col-cost">
                Cost
              </th>
              {manyTargets && (
                <th scope="col" className="col-target">
                  Target
                </th>
              )}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <RunRow key={`${r.target}/${r.runId}`} row={r} now={now} manyTargets={manyTargets} repeated={repeated} />
            ))}
          </tbody>
        </table>
      )}
      {rows.length > 0 && <StatusLegend />}
      {all.length > 0 && rows.length === 0 && (
        <p className="muted">
          No match.{' '}
          <button
            type="button"
            className="text-button"
            onClick={() => {
              setFilter('');
              setGroup('all');
            }}
          >
            Clear filter
          </button>
        </p>
      )}
      {problems.length > 0 && (
        <ul className="target-problems">
          {problems.map((t) => (
            <li key={t.id} className="muted small">
              <strong>{t.name}</strong>: {t.error ?? (t.status === 'unreachable' ? 'unreachable' : 'not supported yet')}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * One run in Home's table; `manyTargets` adds the target column. A run whose title `repeated`
 * shows its context line (slice and attempt, task and seed) when it has no failure cause.
 */
export function RunRow({ row, now, manyTargets, repeated }: { row: HomeRow; now: number; manyTargets: boolean; repeated: Set<string> }) {
  const facts = runRowFacts(row, now);
  const { status, live, started, ms } = facts;
  const cause = rowSubtitle(row, facts.cause, repeated);
  const summary = row.summary;
  // A live run's cost is still being counted: blank rather than a partial figure.
  const cost = live && summary?.costUsd == null ? '' : costText(summary?.costUsd, summary?.costEstimated ?? true);
  return (
    <tr className={`status-${status}`}>
      <td className="col-status">
        <StatusDot status={status} />
      </td>
      <td className="col-run">
        <a href={runHref(row.target, row.runId)} className="run-link" title={row.title || row.runId}>
          {row.title || <em>Untitled</em>}
        </a>
        {/* Under 900px the time columns fold into this line, so equal titles still differ by start time. */}
        <span className="run-cause">
          {cause && <span>{cause}</span>}
          <span className="run-row-when">
            {cause && ' · '}
            {timeOfDay(started, now)}
            {ms != null && ` · ${duration(ms)}`}
          </span>
        </span>
      </td>
      <td className="col-time tabular" title={started ? fullTime(started, now) : ''}>
        {timeOfDay(started, now)}
      </td>
      <td className="col-num tabular">{ms != null ? duration(ms) : '—'}</td>
      <td className="col-num col-cost tabular">{cost}</td>
      {manyTargets && <td className="col-target muted">{row.targetName}</td>}
    </tr>
  );
}

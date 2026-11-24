// The run page's sidebar: every target's runs, newest first, with a filter box.
import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, Cloud, Container, Eye, Laptop, Search, X, type LucideIcon } from 'lucide-react';
import type { RunListing, TargetKind, TargetOverview } from './types';
import { matchesFilter, orderTargets, rowLive, runRowFacts, sidebarRunLabel, sortRuns, statusGroup } from './status';
import { duration, fullTime, relativeAge, timeOfDay } from './format';
import { runHref } from './route';
import { cx, ErrorBox, Skeleton, StatusDot } from './common';
import type { OverviewState } from './overview-poll';

/** Each kind of target: its icon and the hover text of its heading. */
const TARGET_KINDS: Record<TargetKind, { icon: LucideIcon; label: (target: TargetOverview) => string }> = {
  local: { icon: Laptop, label: () => 'This machine' },
  direct: { icon: Container, label: (t) => `Docker target · ${t.origin ?? ''}` },
  hosted: { icon: Cloud, label: () => 'Platform target' },
  observe: { icon: Eye, label: () => 'Configured source · observe only' },
};
/** A kind this build does not know yet (a newer server). */
const UNKNOWN_KIND = { icon: Container, label: () => 'Platform target' };

type Selected = { target: string; run: string } | null;

export function Sidebar({ overview, selected, now }: { overview: OverviewState; selected: Selected; now: number }) {
  const [filter, setFilter] = useState('');
  const targets = useMemo(() => orderTargets(overview.data?.targets ?? []), [overview.data]);
  return (
    <nav className="sidebar" aria-label="Runs">
      <div className="sidebar-search">
        <Search size={14} aria-hidden className="search-icon" />
        <input
          type="search"
          placeholder="Filter runs"
          aria-label="Filter runs"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape' && filter) {
              e.stopPropagation();
              setFilter('');
            }
          }}
        />
        {filter && (
          <button type="button" className="icon-button tiny" aria-label="Clear filter" onClick={() => setFilter('')}>
            <X size={13} />
          </button>
        )}
      </div>
      <div className="sidebar-scroll">
        {overview.loading && <Skeleton lines={8} />}
        {overview.error &&
          // The list below is still the last good one and polling already retries itself: a
          // stale list only needs a muted word, not a heading and a Retry button.
          (targets.length > 0 ? (
            <p className="muted small offline-note" title={overview.error}>
              Offline · retrying
            </p>
          ) : (
            <ErrorBox compact message={overview.error} onRetry={overview.reload} />
          ))}
        {targets.map((target) => (
          <TargetSection key={target.id} target={target} filter={filter} selected={selected} now={now} single={targets.length === 1} />
        ))}
      </div>
    </nav>
  );
}

/** One target's runs; with a single target there is no heading (it would only repeat the list). */
function TargetSection({ target, filter, selected, now, single }: { target: TargetOverview; filter: string; selected: Selected; now: number; single: boolean }) {
  const [open, setOpen] = useState(target.status === 'ok' && (target.runs.length > 0 || target.kind === 'local'));
  const [limit, setLimit] = useState(60);
  const runs = useMemo(() => sortRuns(target.runs).filter((r) => matchesFilter(r, filter)), [target.runs, filter]);
  const kind = TARGET_KINDS[target.kind] ?? UNKNOWN_KIND;
  const Icon = kind.icon;
  const expanded = single || open || (!!filter && runs.length > 0);
  return (
    <section className={`target target-${target.status}`}>
      {!single && (
        <button type="button" className="target-heading" aria-expanded={expanded} onClick={() => setOpen(!open)} title={kind.label(target)}>
          {expanded ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
          <Icon size={14} aria-hidden />
          <span className="target-name">{target.name}</span>
          <span className="target-count">{targetCount(target, runs, filter)}</span>
        </button>
      )}
      {expanded && (
        <div className="target-body">
          <TargetEmpty target={target} shown={runs.length} filter={filter} />
          <ul className="run-list">
            {runs.slice(0, limit).map((run) => (
              <li key={run.runId}>
                <RunItem run={run} href={runHref(target.id, run.runId)} active={selected?.target === target.id && selected.run === run.runId} now={now} />
              </li>
            ))}
          </ul>
          {runs.length > limit && (
            <button type="button" className="text-button show-more" onClick={() => setLimit(limit + 100)}>
              Show {Math.min(100, runs.length - limit)} more of {runs.length - limit}
            </button>
          )}
        </div>
      )}
    </section>
  );
}

/** The heading's count: `3 runs · 1 running`, `2 of 9` while filtering, or why there are none. */
function targetCount(target: TargetOverview, shown: RunListing[], filter: string): string {
  if (target.status === 'unreachable') return 'unreachable';
  if (target.status !== 'ok') return 'not supported';
  const total = target.runs.length;
  if (filter) return `${shown.length} of ${total}`;
  const running = target.runs.filter(rowLive).length;
  return `${total} ${total === 1 ? 'run' : 'runs'}${running ? ` · ${running} running` : ''}`;
}

/** What an expanded target shows when it has no run to list: one of four messages, or nothing. */
function TargetEmpty({ target, shown, filter }: { target: TargetOverview; shown: number; filter: string }) {
  if (target.status !== 'ok') {
    const fallback = target.status === 'unreachable' ? 'zerochat cannot reach this target.' : 'zerochat cannot read this kind of target yet.';
    return <p className="target-error">{target.error ?? fallback}</p>;
  }
  if (target.runs.length > 0) {
    return shown === 0 ? <p className="sidebar-empty">No run matches “{filter}”.</p> : null;
  }
  // An observed source is filled by another tool; only a target zerochat can run on gets the hint.
  if (target.kind === 'observe') return <p className="sidebar-empty">No runs yet.</p>;
  return (
    <p className="sidebar-empty">
      No runs yet. Start one with <code>zeroshot run …</code>
      {target.kind === 'direct' && <> --target {target.name}</>}.
    </p>
  );
}

/** One run in the list: its status dot, label, and cause · start · duration. */
function RunItem({ run, href, active, now }: { run: RunListing; href: string; active: boolean; now: number }) {
  const { status, started, ms, cause } = runRowFacts(run, now);
  // A failed row's cause already needs the width; a relative age fits where a clock time and a
  // duration (which the run page has anyway) together would not.
  const failed = statusGroup(status) === 'failed';
  const age = failed ? relativeAge(started, now) : timeOfDay(started, now);
  const shownMs = failed ? null : ms;
  const label = sidebarRunLabel(run);
  // The visible " · " separators are aria-hidden (decorative), which would fuse the fields with
  // no space for screen readers ("rejected ×328m ago"); an explicit label keeps them apart.
  const itemLabel = [label || 'Untitled run', cause, age, shownMs != null ? duration(shownMs) : null].filter(Boolean).join(', ');
  return (
    <a href={href} className={cx('run-item', active && 'active')} aria-current={active ? 'page' : undefined} title={label || 'Untitled run'} aria-label={itemLabel}>
      <StatusDot status={status} />
      <span className="run-item-text">
        <span className="run-title">{label || <em>Untitled</em>}</span>
        <span className="run-meta">
          {cause && (
            <>
              <span>{cause}</span>
              <span aria-hidden> · </span>
            </>
          )}
          <span title={started ? fullTime(started, now) : undefined}>{age}</span>
          {shownMs != null && (
            <>
              <span aria-hidden> · </span>
              <span>{duration(shownMs)}</span>
            </>
          )}
        </span>
      </span>
    </a>
  );
}

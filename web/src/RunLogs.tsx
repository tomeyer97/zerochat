import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import type { RunView } from './types';
import { fullTime, offsetText, plural, tokenParts, totalTokens } from './format';
import { agentFailed, agentLabel, lineAgent, lineShown, logFilters } from './runlog-model';

/**
 * One collapsed Log: the run's own lines and those of the agents Zeroshot runs outside the graph
 * (Active Memory after the run), filterable by agent.
 */
export function RunLogs({ view }: { view: RunView }) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<string>('all');
  const agents = view.backgroundAgents ?? [];
  const lines = useMemo(() => view.runLog.map((l) => ({ ...l, ...lineAgent(l) })), [view.runLog]);
  if (!lines.length && !agents.length) return null;
  const failed = agents.filter(agentFailed).length;
  const tokens = view.backgroundUsage && totalTokens(view.backgroundUsage) > 0 ? tokenParts(view.backgroundUsage).total : null;
  const shown = lines.filter((l) => lineShown(l, filter));
  const selected = agents.find((a) => a.name === filter);
  const filters = logFilters(agents);
  return (
    <section className="run-logs">
      <button
        type="button"
        className="section-toggle"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        title="The run's own lines, and the agents Zeroshot runs outside the graph (such as Active Memory after the run)"
      >
        {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
        Log
        {agents.length > 0 && (
          <span className="muted small">
            {' '}
            · {plural(agents.length, 'background agent')}
            {failed > 0 && <span className="tone-text-bad"> · {failed} failed</span>}
            {tokens && ` · ${tokens}`}
          </span>
        )}
      </button>
      {open && (
        <div className="log-body">
          {filters.length > 0 && (
            <div className="log-filters" role="group" aria-label="Show lines of">
              {filters.map((f) => (
                <button key={f.id} type="button" className={`chip ${filter === f.id ? 'on' : ''}`} aria-pressed={filter === f.id} title={f.title} onClick={() => setFilter(f.id)}>
                  {f.label}
                  {f.bad && <span className="tone-text-bad"> ✗</span>}
                </button>
              ))}
            </div>
          )}
          {selected?.note && <p className="muted small">{selected.note}</p>}
          <ol className="log-lines">
            {shown.length === 0 && <li className="muted small">No lines in the recent log.</li>}
            {shown.map((l) => (
              <li key={l.seq} className={`log-line stream-${l.stream}`}>
                <time title={fullTime(l.at)}>{offsetText(l.at, view.startedAt) || '—'}</time>
                {filter === 'all' && l.agent && <span className="log-agent">{agentLabel(l.agent)}</span>}
                <span className="log-text">{l.text}</span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </section>
  );
}

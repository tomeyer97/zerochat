import { useEffect, useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { api } from '../api';
import type { FeatureEvent } from '../types';
import { eventLinksRun, eventSlices, eventText, eventTone, isNoiseEvent } from '../features-model';
import { fullTime, timeOfDay } from '../format';
import { featureHref, runHref } from '../route';
import { useResource } from '../hooks';
import { ErrorBox, Skeleton } from '../common';
import { useLastDefined } from './page-hooks';

// ---------- timeline ----------

/** An event's time: to the second today (events come seconds apart), else as `timeOfDay`. */
function preciseTime(at: number, now: number): string {
  const date = new Date(at);
  const today = date.toDateString() === new Date(now).toDateString();
  if (!today) return timeOfDay(at, now);
  return date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit', second: '2-digit' });
}

export function FeatureTimeline({ name, stamp, slices, selected }: { name: string; stamp: string; slices: string[]; selected?: string }) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState<string | null>(selected ?? null);
  const [all, setAll] = useState(false);
  // The open slice is the filter (one selection, not two).
  useEffect(() => setFilter(selected ?? null), [selected]);
  const resource = useResource<{ events: FeatureEvent[] }>(open ? `feature-events/${name}/${stamp}` : null, (signal) => api.featureEvents(name, null, signal));
  // Keep the last answer while the next loads (the key changes with every state write).
  const events = useLastDefined(resource.data?.events) ?? [];
  const chips = eventSlices(events).filter((s) => slices.includes(s));
  const relevant = events.filter((e) => !filter || e.slice === filter);
  const shown = all ? relevant : relevant.filter((e) => !isNoiseEvent(e));
  const hidden = relevant.length - shown.length;
  const now = Date.now();
  return (
    <section className="run-logs feature-timeline">
      <button type="button" className="section-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
        Timeline
      </button>
      {open && (
        <div className="log-body">
          {resource.error && <ErrorBox compact message={resource.error} onRetry={resource.reload} />}
          {chips.length > 0 && (
            <div className="log-filters" role="group" aria-label="Show events of">
              <button type="button" className={`chip ${filter == null ? 'on' : ''}`} aria-pressed={filter == null} onClick={() => setFilter(null)}>
                All
              </button>
              {chips.map((s) => (
                <button key={s} type="button" className={`chip ${filter === s ? 'on' : ''}`} aria-pressed={filter === s} onClick={() => setFilter(s)}>
                  {s}
                </button>
              ))}
            </div>
          )}
          {resource.loading && !events.length ? (
            <Skeleton lines={4} />
          ) : (
            <ol className="log-lines event-lines">
              {shown.length === 0 && <li className="muted small">No events.</li>}
              {[...shown].reverse().map((e, i) => {
                const text = eventText(e);
                return (
                  <li key={`${e.ts}-${i}`} className="log-line event-line">
                    <time title={fullTime(e.ts, now)}>{preciseTime(e.ts, now)}</time>
                    <span className="event-slice tabular">
                      {e.slice ? (
                        <a href={featureHref(name, e.slice)}>
                          {e.slice}
                          {e.attempt ? ` #${e.attempt}` : ''}
                        </a>
                      ) : (
                        ''
                      )}
                    </span>
                    <span className="log-text">
                      <span className={`event-kind tone-text-${eventTone(e.kind)}`}>{text.what}</span>
                      {text.detail && <span className="event-detail"> {text.detail}</span>}
                      {e.run_id && e.runTarget && eventLinksRun(e.kind) && (
                        <>
                          {' '}
                          <a href={runHref(e.runTarget, e.run_id)} title={e.run_id}>
                            run ›
                          </a>
                        </>
                      )}
                    </span>
                  </li>
                );
              })}
            </ol>
          )}
          {hidden > 0 && (
            <button type="button" className="text-button" onClick={() => setAll(true)}>
              Show all events ({hidden} more)
            </button>
          )}
          {all && (
            <button type="button" className="text-button" onClick={() => setAll(false)}>
              Hide repeated events
            </button>
          )}
        </div>
      )}
    </section>
  );
}

import { useMemo } from 'react';
import { api } from '../api';
import type { FeatureRow } from '../types';
import { isLiveRow, nowParts, progressText, sortFeatures } from '../features-model';
import { costText, fullTime, relativeAge, timeOfDay } from '../format';
import { featureHref } from '../route';
import { useResource } from '../hooks';
import { ErrorBox, Skeleton } from '../common';
import { useSetup } from '../onboarding/setup-api';
import { useDocumentTitle, useFeaturePolling } from './page-hooks';
import { FeaturesEmpty } from './FeaturesEmpty';

/** The feature's status chips; a chip about one slice opens it when `onOpen` is given. */
function NowText({ row, onOpen }: { row: FeatureRow; onOpen?: (slice: string) => void }) {
  const parts = nowParts(row);
  return (
    <span className="feature-now-parts">
      {parts.map((p, i) => (
        <span key={i} className={`tone-text-${p.tone}`}>
          {i > 0 && (
            <span className="muted" aria-hidden>
              {' '}
              ·{' '}
            </span>
          )}
          {p.slice && onOpen ? (
            <button type="button" className="text-button inline-link" onClick={() => onOpen(p.slice!)}>
              {p.text}
            </button>
          ) : (
            p.text
          )}
        </span>
      ))}
    </span>
  );
}

/**
 * The Status and Merged columns folded into the name cell. Only narrow screens show it
 * (`.feature-now-narrow`); they hide those two columns instead.
 */
function NarrowStatus({ row }: { row: FeatureRow }) {
  return (
    <span className="feature-now-narrow">
      <NowText row={row} />
      <span aria-hidden> · </span>
      <span>
        {row.merged}/{row.total}
      </span>
    </span>
  );
}

/** `12 min ago` while it runs, the finish time when done. */
function whenText(row: FeatureRow, now: number): { text: string; title: string } {
  if (row.finishedAt) return { text: timeOfDay(row.finishedAt, now), title: `Finished ${fullTime(row.finishedAt, now)}` };
  if (row.updatedAt) return { text: relativeAge(row.updatedAt, now), title: `Last progress ${fullTime(row.updatedAt, now)}` };
  return { text: '—', title: '' };
}

function FeatureListRow({ row: r, now }: { row: FeatureRow; now: number }) {
  const when = whenText(r, now);
  return (
    <tr className={r.needsYou ? 'needs-you' : ''}>
      <td className="col-run">
        <a href={featureHref(r.name)} className="run-link" title={r.name}>
          {r.name}
        </a>
        <span className="run-cause">
          {r.branch !== r.name && (
            <span title={r.repo}>
              {r.branch}
              <span aria-hidden> · </span>
            </span>
          )}
          <NarrowStatus row={r} />
        </span>
      </td>
      <td className="col-now">
        <NowText row={r} />
      </td>
      <td className="col-num tabular" title={progressText(r)}>
        {r.merged}/{r.total}
        {r.skipped > 0 && <span className="muted"> +{r.skipped} skipped</span>}
      </td>
      <td className="col-time tabular" title={when.title}>
        {when.text}
      </td>
      <td className="col-num col-cost tabular">{costText(r.costUsd, r.costEstimated)}</td>
    </tr>
  );
}

/** The empty state, with the setup check (read only when there is no feature: it says whether zsf is installed). */
function EmptyFeatures() {
  return <FeaturesEmpty setup={useSetup()} />;
}

/** The Features list: one table, what needs you first. `#/features`. */
export function FeaturesList() {
  const resource = useResource<{ features: FeatureRow[]; now: number }>('features', (signal) => api.features(signal));
  const rows = useMemo(() => sortFeatures(resource.data?.features ?? []), [resource.data]);
  const now = useFeaturePolling(resource, rows.some(isLiveRow));
  const needYou = rows.filter((r) => r.needsYou).length;
  useDocumentTitle('Features · zerochat');
  return (
    <div className="home">
      <h1 className="visually-hidden">Features</h1>
      {resource.loading && <Skeleton lines={6} />}
      {resource.error && <ErrorBox compact message={resource.error} onRetry={resource.reload} />}
      {resource.data && rows.length === 0 && <EmptyFeatures />}
      {rows.length > 0 && (
        <table className="run-table feature-table">
          <caption className="visually-hidden">{needYou ? `${needYou} need you` : 'Features'}</caption>
          <thead className="quiet-head">
            <tr>
              <th scope="col">Feature</th>
              <th scope="col" className="col-now">
                Status
              </th>
              <th scope="col" className="col-num">
                Merged
              </th>
              <th scope="col" className="col-time">
                Finished or last progress
              </th>
              <th scope="col" className="col-num col-cost">
                Cost
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <FeatureListRow key={r.name} row={r} now={now} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

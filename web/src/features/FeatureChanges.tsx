import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { api } from '../api';
import type { FeatureDetail } from '../types';
import { splitDiff, statText } from '../features-model';
import { featureHref } from '../route';
import { useResource } from '../hooks';
import { CopyButton, ErrorBox, Skeleton } from '../common';
import { DiffView } from '../Files';
import { useLastDefined } from './page-hooks';

// ---------- whole-feature changes ----------

export function FeatureChangesSection({ detail }: { detail: FeatureDetail }) {
  const [open, setOpen] = useState(false);
  const [showDiff, setShowDiff] = useState(false);
  const anyMerged = detail.merged > 0;
  // A new commit on the branch or the diff toggled on loads again; the last answer stays meanwhile.
  const key = anyMerged ? `feature-changes/${detail.name}/${detail.branchCommit}/${showDiff}` : null;
  const resource = useResource(key, (signal) => api.featureChanges(detail.name, showDiff, signal));
  const changes = useLastDefined(resource.data);
  if (!anyMerged) return null;
  return (
    <section className="run-logs feature-changes">
      <button type="button" className="section-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDown size={14} aria-hidden /> : <ChevronRight size={14} aria-hidden />}
        Changes
        {changes && (
          <span className="muted small section-toggle-meta">
            {changes.commits.length} commit{changes.commits.length === 1 ? '' : 's'} · {statText(changes.stat)}
          </span>
        )}
      </button>
      {open && (
        <div className="log-body">
          {resource.error && !changes && <ErrorBox compact message={resource.error} onRetry={resource.reload} />}
          {changes && (
            <>
              <p className="small path-line">
                <code>
                  git diff {changes.base}...{changes.branch}
                </code>
                <CopyButton text={`git diff ${changes.base}...${changes.branch}`} label="Copy command" />
              </p>
              <ol className="commit-list">
                {changes.commits.map((c) => (
                  <li key={c.sha}>
                    <code title={c.sha}>{c.sha.slice(0, 7)}</code> {c.slice ? <a href={featureHref(detail.name, c.slice)}>{c.subject}</a> : <span>{c.subject}</span>}
                  </li>
                ))}
              </ol>
              <ul className="file-stat-list small">
                {changes.stat.files.map((f) => (
                  <li key={f.path}>
                    <span className="mono">{f.path}</span>{' '}
                    <span className="muted tabular">
                      +{f.additions} −{f.deletions}
                    </span>
                  </li>
                ))}
              </ul>
              <button type="button" className="text-button" aria-expanded={showDiff} onClick={() => setShowDiff(!showDiff)}>
                {showDiff ? 'Hide diff' : 'Show diff'}
              </button>
              {showDiff && (resource.loading && !changes.diff ? <Skeleton lines={4} /> : changes.diff != null ? <FileDiffs text={changes.diff} /> : null)}
            </>
          )}
          {!changes && resource.loading && <Skeleton lines={3} />}
        </div>
      )}
    </section>
  );
}

/** A patch per file, each collapsible. */
export function FileDiffs({ text }: { text: string }) {
  const files = splitDiff(text);
  if (!files.length) return <p className="muted small">No changes.</p>;
  return (
    <div className="diff-files">
      {files.map((f, i) => (
        <details key={i} className="diff-file" open={files.length <= 3}>
          <summary className="diff-file-name mono small">{f.path}</summary>
          <DiffView text={f.text} />
        </details>
      ))}
    </div>
  );
}

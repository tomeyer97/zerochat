/**
 * The slice panel's sections, top to bottom: why it stopped, its worktree (the changes to review,
 * with the pull request, while it waits for review), its commit, the notes
 * every later attempt gets, its attempts, its artifacts and its spec.
 */
import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { api } from '../api';
import type { AttemptView, FeatureDetail, SliceView } from '../types';
import { artifactKind, cleanOutput, diffStat, isStoppedSlice, reasonSummary, specBody, statText, verdictLine } from '../features-model';
import { useResource, type Resource } from '../hooks';
import { CopyButton, ErrorBox, LongText, Skeleton } from '../common';
import { Markdown } from '../Markdown';
import { FileDiffs } from './FeatureChanges';
import { Attempts, BackupLine } from './Attempts';

// How much of each text shows before "show more". Each is its own choice; they need not agree.
/** Lines of the spec. */
const SPEC_PREVIEW_LINES = 30;
/** Lines from the end of a log. */
const LOG_TAIL_LINES = 30;
/** Characters of a log short enough to show whole. */
const LOG_PREVIEW_CHARS = 1600;
/** Lines of any other text artifact. */
const ARTIFACT_TEXT_LINES = 30;
/** Up to this many artifacts are listed at once; more wait behind "Show all". */
const ARTIFACT_LIST_OPEN_UP_TO = 8;
/** Lines of the gate's output under "Why it stopped". */
const GATE_OUTPUT_LINES = 12;
/** Lines of a stop reason's full message. */
const REASON_LINES = 8;
/** LongText's limits out of reach: a whole log, once asked for. */
const WHOLE = { maxLines: 100_000, maxChars: 10_000_000 };

/** Every section of the panel body; the spec hides while the edit form shows it. */
export function SliceSections({ feature, slice, now, showSpec }: { feature: FeatureDetail; slice: SliceView; now: number; showSpec: boolean }) {
  const stopped = isStoppedSlice(slice.status);
  const review = slice.status === 'waiting_for_review';
  return (
    <>
      {slice.reason && stopped && <WhyStopped slice={slice} />}
      {(stopped || review) && slice.worktree && <WorktreeSection feature={feature} slice={slice} review={review} />}
      {slice.commit && <CommitSection feature={feature.name} slice={slice} />}
      {(slice.notes.length > 0 || slice.autoNotes.length > 0) && <SliceNotes slice={slice} />}
      {slice.attempts.length > 0 && <Attempts feature={feature.name} slice={slice} now={now} />}
      {slice.artifacts.length > 0 && <Artifacts feature={feature.name} slice={slice} />}
      {showSpec && (
        <section>
          <h3 className="section-label">Spec</h3>
          {slice.spec != null ? <SpecText text={specBody(slice.spec, slice.id)} /> : <p className="muted small">Cannot read {slice.specPath}.</p>}
        </section>
      )}
    </>
  );
}

function WhyStopped({ slice }: { slice: SliceView }) {
  const last = slice.attempts[slice.attempts.length - 1];
  const gate = last?.result === 'gate_failed' && last.reason;
  const summary = reasonSummary(slice.reason);
  const same = slice.attempts.length > 1 ? trailingSame(slice.attempts) : 1;
  return (
    <section>
      <h3 className="section-label">Why it stopped</h3>
      <p className={`why-headline tone-text-${slice.status === 'failed' ? 'bad' : 'warn'}`}>
        {gate ? `Gate failed: ${verdictLine(last.reason)}` : summary.headline}
        {same > 1 && <span className="muted"> ({same} attempts in a row)</span>}
      </p>
      {gate ? (
        <LongText text={cleanOutput(last.reason!)} maxLines={GATE_OUTPUT_LINES} className="gate-output" />
      ) : (
        summary.full !== summary.headline && (
          <details className="why-details">
            <summary className="small muted">Full message</summary>
            <LongText text={summary.full} maxLines={REASON_LINES} />
          </details>
        )
      )}
      {last?.backup && <BackupLine branch={last.backup} />}
    </section>
  );
}

/** How many attempts in a row, counting back from the last, ended the same way. */
function trailingSame(attempts: AttemptView[]): number {
  const last = attempts[attempts.length - 1];
  let n = 0;
  for (let i = attempts.length - 1; i >= 0 && attempts[i].result === last.result; i--) n++;
  return n;
}

/** `Show diff` / `Hide diff`, for a diff that loads when opened. */
function DiffButton({ open, onToggle }: { open: boolean; onToggle: () => void }) {
  return (
    <button type="button" className="text-button" aria-expanded={open} onClick={onToggle}>
      {open ? 'Hide diff' : 'Show diff'}
    </button>
  );
}

/** A diff loaded on demand: a skeleton while it loads, then its files, or why it could not load. */
function LoadedDiff({ resource }: { resource: Resource<{ diff: string }> }) {
  if (resource.loading) return <Skeleton lines={4} />;
  if (resource.data) return <FileDiffs text={resource.data.diff} />;
  return <ErrorBox compact message={resource.error ?? 'No diff.'} />;
}

/**
 * A stopped slice's worktree: what it holds (what Merge worktree would merge), and where it is.
 * `review`: the slice waits for review: its changes against the feature branch (what Approve
 * checks and merges), open from the start, and the pull request its run opened.
 */
function WorktreeSection({ feature, slice, review }: { feature: FeatureDetail; slice: SliceView; review: boolean }) {
  const [open, setOpen] = useState(review);
  const key = open ? `worktree/${feature.name}/${slice.id}/${slice.attempts.length}` : null;
  const resource = useResource(key, (signal) => api.sliceWorktree(feature.name, slice.id, signal));
  // What it holds against the feature branch; nothing to show when it holds no changes.
  const changes = slice.worktreeChanges?.files.length ? slice.worktreeChanges : null;
  return (
    <section>
      <h3 className="section-label">{review ? 'Changes to review' : 'Worktree'}</h3>
      {slice.worktreeExists ? (
        <>
          <p className="small path-line">
            {changes ? <span>{statText(changes)} not merged</span> : <span className="muted">no changes against the feature branch</span>}
            {changes && <DiffButton open={open} onToggle={() => setOpen(!open)} />}
            {slice.prUrl && (
              <a className="slice-pr-link" href={slice.prUrl} target="_blank" rel="noreferrer noopener" title={slice.prUrl}>
                Pull request <ExternalLink size={12} aria-hidden />
              </a>
            )}
          </p>
          {open && <LoadedDiff resource={resource} />}
          <p className="small path-line">
            <code>{slice.worktree}</code>
            <CopyButton text={slice.worktree!} label="Copy path" />
          </p>
          {slice.branch && (
            <p className="small path-line muted">
              branch <code>{slice.branch}</code>
            </p>
          )}
        </>
      ) : (
        <p className="small muted">The worktree was removed.</p>
      )}
    </section>
  );
}

/** The merged commit (open from the start while it waits for approval: that is what you review). */
function CommitSection({ feature, slice }: { feature: string; slice: SliceView }) {
  const [open, setOpen] = useState(slice.status === 'waiting_for_approval');
  const resource = useResource(`commit/${feature}/${slice.id}/${slice.commit}`, (signal) => api.sliceCommit(feature, slice.id, signal));
  const commit = slice.commit!;
  return (
    <section>
      <h3 className="section-label">Commit</h3>
      <p className="small path-line">
        <code title={commit}>{commit.slice(0, 7)}</code>
        <CopyButton text={commit} label="Copy commit" />
        {resource.data && <span className="muted">{statText(diffStat(resource.data.diff))}</span>}
        <DiffButton open={open} onToggle={() => setOpen(!open)} />
      </p>
      {open && <LoadedDiff resource={resource} />}
    </section>
  );
}

/** Notes every later attempt gets: zsf's own after a conflict or a failed gate (zsf: Zeroshot features, the scheduler), then yours. */
function SliceNotes({ slice }: { slice: SliceView }) {
  return (
    <section>
      <h3 className="section-label">Notes for every later attempt</h3>
      <ul className="slice-notes">
        {slice.autoNotes.map((n, i) => (
          <li key={`z${i}`} className="note-auto">
            <span className="muted small">{n.cause === 'gate_failed' ? 'zsf, after the gate failed: ' : 'zsf, after a conflict: '}</span>
            <NoteText text={n.text} />
          </li>
        ))}
        {slice.notes.map((n, i) => (
          <li key={`p${i}`}>
            <span className="muted small">You: </span>
            <NoteText text={n} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** A note as Markdown (zsf's gate notes carry the gate output as a code block); one line stays beside its label. */
function NoteText({ text }: { text: string }) {
  return text.includes('\n') ? <Markdown text={text} /> : <Markdown inline text={text} />;
}

function SpecText({ text }: { text: string }) {
  const [all, setAll] = useState(false);
  const lines = text.split('\n');
  const clamped = lines.length > SPEC_PREVIEW_LINES && !all;
  return (
    <div className={`spec-text ${clamped ? 'clamped' : ''}`}>
      <Markdown text={clamped ? lines.slice(0, SPEC_PREVIEW_LINES).join('\n') : text} />
      {lines.length > SPEC_PREVIEW_LINES && (
        <button type="button" className="text-button" onClick={() => setAll(!all)}>
          {all ? 'Show less' : 'Show all'}
        </button>
      )}
    </div>
  );
}

function Artifacts({ feature, slice }: { feature: string; slice: SliceView }) {
  const [file, setFile] = useState<string | null>(null);
  const [listOpen, setListOpen] = useState(slice.artifacts.length <= ARTIFACT_LIST_OPEN_UP_TO);
  return (
    <section>
      <h3 className="section-label">
        Artifacts <span className="muted">({slice.artifacts.length})</span>
      </h3>
      {!listOpen ? (
        <button type="button" className="text-button" onClick={() => setListOpen(true)}>
          Show all {slice.artifacts.length} files
        </button>
      ) : (
        <ul className="artifact-list">
          {slice.artifacts.map((a) => (
            <li key={a}>
              <button type="button" className={`text-button mono ${file === a ? 'on' : ''}`} aria-pressed={file === a} onClick={() => setFile(file === a ? null : a)} title={a}>
                {a}
              </button>
            </li>
          ))}
        </ul>
      )}
      {file && <ArtifactView feature={feature} slice={slice.id} path={file} />}
    </section>
  );
}

/** One artifact's content, shown by its kind (see `artifactKind`). */
function ArtifactView({ feature, slice, path }: { feature: string; slice: string; path: string }) {
  const resource = useResource(`artifact/${feature}/${slice}/${path}`, (signal) => api.sliceArtifact(feature, slice, path, signal));
  if (resource.loading) return <Skeleton lines={3} />;
  if (!resource.data) return <ErrorBox compact message={resource.error ?? 'Cannot read it.'} />;
  const content = resource.data.content ?? '';
  switch (artifactKind(path)) {
    case 'markdown':
      return (
        <div className="spec-text">
          <Markdown text={content} />
        </div>
      );
    case 'log':
      return <LogTail text={content} />;
    case 'text':
      return <LongText text={content} maxLines={ARTIFACT_TEXT_LINES} />;
  }
}

/** A log shows its end (where test runners put the failure), with the rest one click away. */
function LogTail({ text }: { text: string }) {
  const [all, setAll] = useState(false);
  const lines = text.replace(/\n$/, '').split('\n');
  if (all) return <LongText text={text} {...WHOLE} />;
  if (lines.length <= LOG_TAIL_LINES) return <LongText text={text} maxLines={LOG_TAIL_LINES} maxChars={LOG_PREVIEW_CHARS} />;
  return (
    <div className="long-text">
      <button type="button" className="text-button" onClick={() => setAll(true)}>
        Show the {lines.length - LOG_TAIL_LINES} lines before
      </button>
      <pre>{lines.slice(-LOG_TAIL_LINES).join('\n')}</pre>
    </div>
  );
}

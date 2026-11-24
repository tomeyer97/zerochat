import { useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, Info } from 'lucide-react';
import { api, retryable } from './api';
import type { ChangedFile, ChangesResponse, Execution, RunView } from './types';
import { changeRange, fileStatusBadge, fileStatusLabel, parseDiff, rangeTitle, type ChangeRange, type DiffFile } from './diff';
import { buildTree, isArtefact, type TreeEntry } from './file-tree';
import { shortId } from './format';
import { type Resource, useDelayedFlag, useResource, useThrottled } from './hooks';
import { fileRows, listChanges, NO_CHANGES, type FileRow, type ListChanges } from './file-changes';
import { ErrorBox, Skeleton, tabListKeyDown } from './common';
import { SidePanel } from './components/SidePanel';

// The execution rules this panel uses live with the files model in diff.ts (`changeRange`,
// `executionFilesChanged`) and the HTTP retry rule in api.ts; re-exported for existing imports.
export { changeRange, executionFilesChanged } from './diff';
export { retryable } from './api';

/** The run-level Files drawer (whole run compared with its base). */
export function RunFilesPanel({ view, onClose }: { view: RunView; onClose: () => void }) {
  return (
    <SidePanel label="Files changed by the run" title="Files" onClose={onClose}>
      <div className="panel-body">
        <div className="panel-pad">
          <FilesView view={view} execution={null} />
        </div>
      </div>
    </SidePanel>
  );
}

/** A live list refreshes at most this often (the run's cursor moves on every event). */
const LIVE_REFRESH_MS = 1500;
/** A refresh slower than this shows "updating…" in the Files bar. */
const SLOW_REFRESH_MS = 600;
/** How long a file no longer listed stays drawn, fading, before it goes. */
const REMOVED_FADE_MS = 1200;

export function FilesView({ view, execution }: { view: RunView; execution: Execution | null }) {
  const [mode, setMode] = useState<'changes' | 'browse'>('changes');
  const [updating, setUpdating] = useState(false);
  const range = changeRange(execution);
  const modes: { id: 'changes' | 'browse'; label: string }[] = [
    { id: 'changes', label: 'Changed' },
    { id: 'browse', label: 'All files' },
  ];
  return (
    <div className="files">
      <div className="files-bar">
        <div className="segmented" role="tablist" aria-label="Files view" onKeyDown={(e) => tabListKeyDown(e, modes.length, (i) => setMode(modes[i].id))}>
          {modes.map(({ id, label }, i) => (
            <button
              key={id}
              id={`files-tab-${id}`}
              type="button"
              role="tab"
              aria-selected={mode === id}
              aria-controls={`files-tabpanel-${id}`}
              tabIndex={mode === id ? 0 : -1}
              data-tab-index={i}
              onClick={() => setMode(id)}
            >
              {label}
            </button>
          ))}
        </div>
        {execution && !range.perExecution && (
          <span className="muted small" title="This Zeroshot does not snapshot the workspace per node, so a node shows the whole run's changes.">
            whole run <Info size={11} aria-hidden />
          </span>
        )}
        {updating && mode === 'changes' && <span className="muted small files-updating">updating…</span>}
      </div>
      <div id={`files-tabpanel-${mode}`} role="tabpanel" aria-labelledby={`files-tab-${mode}`}>
        {mode === 'changes' ? <Changes view={view} range={range} onSlowRefresh={setUpdating} /> : <Browser view={view} commit={range.to ?? range.from} />}
      </div>
    </div>
  );
}

/** What a changes list shows (run and range start); the event cursor is not part of it. */
export function changesKey(view: Pick<RunView, 'target' | 'runId'>, range: Pick<ChangeRange, 'from'>): string {
  return `${view.target}/${view.runId}/changes/${range.from}`;
}

function Changes({ view, range, onSlowRefresh }: { view: RunView; range: ChangeRange; onSlowRefresh: (slow: boolean) => void }) {
  // The key names what is shown; a range ending "now" (live) refreshes on the run's cursor, throttled.
  const live = useThrottled(view.cursor, LIVE_REFRESH_MS);
  const refresh = range.to ?? live;
  const changes = useResource<ChangesResponse>(changesKey(view, range), (signal) => api.changes(view.target, view.runId, { from: range.from, to: range.to }, signal), refresh);
  const slow = useDelayedFlag(changes.reloading, SLOW_REFRESH_MS);
  useEffect(() => onSlowRefresh(slow), [slow, onSlowRefresh]);
  return <ChangesBody view={view} range={range} changes={changes} refresh={refresh} />;
}

/** The changed files as loaded: a skeleton only before the first answer, never on a refresh. */
export function ChangesBody({ view, range, changes, refresh }: { view: RunView; range: ChangeRange; changes: Resource<ChangesResponse>; refresh: Refresh }) {
  const marks = useChangeMarks(changes.data?.files);
  const [selected, setSelected] = useState<string | null>(null);
  const [showHidden, setShowHidden] = useState(false);
  // A file the refresh removed closes its diff.
  if (selected && marks.changes.removed.some((f) => f.path === selected)) setSelected(null);
  if (changes.loading && !changes.data) return <Skeleton lines={6} />;
  if (changes.error && !changes.data) return <ErrorBox title="Changed files are not available" message={changes.error} onRetry={retryable(changes.errorStatus) ? changes.reload : undefined} />;
  return (
    <ChangesList view={view} data={changes.data!} range={range} rows={marks.rows} refresh={refresh} selected={selected} onSelect={setSelected} showHidden={showHidden} onShowHidden={setShowHidden} />
  );
}

/** What refreshes a loaded list or diff: its end snapshot, or the run's (throttled) cursor. */
type Refresh = string | number | null | undefined;

/** The rows of the latest list with its change marks; removed rows stay `REMOVED_FADE_MS`. */
function useChangeMarks(files: ChangedFile[] | undefined): { rows: FileRow[]; changes: ListChanges } {
  const [state, setState] = useState<{ files: ChangedFile[] | undefined; rows: FileRow[]; changes: ListChanges }>(() => ({
    files,
    rows: fileRows(undefined, files ?? [], NO_CHANGES),
    changes: NO_CHANGES,
  }));
  if (files !== state.files) {
    // Set while rendering, not in an effect, so a new list never shows a frame without its marks.
    const changes = files && state.files ? listChanges(state.files, files) : NO_CHANGES;
    setState({ files, rows: fileRows(state.files, files ?? [], changes), changes });
  }
  const loaded = state.changes;
  useEffect(() => {
    if (!loaded.removed.length) return;
    const id = window.setTimeout(() => setState((s) => ({ ...s, rows: s.rows.filter((r) => r.mark !== 'removed') })), REMOVED_FADE_MS);
    return () => window.clearTimeout(id);
  }, [loaded]);
  return state;
}

type ChangesListProps = {
  view: RunView;
  data: ChangesResponse;
  range: ChangeRange;
  rows: FileRow[];
  /** What refreshes an open diff (the list's own refresh trigger). */
  refresh: Refresh;
  selected: string | null;
  onSelect: (path: string | null) => void;
  showHidden: boolean;
  onShowHidden: (show: boolean) => void;
};

function ChangesList({ view, data, range, rows, refresh, selected, onSelect, showHidden, onShowHidden }: ChangesListProps) {
  const files = data.files ?? [];
  const from = data.from ?? range.from;
  const to = data.to ?? range.to;
  const artefacts = files.filter((f) => isArtefact(f.path));
  const shown = showHidden ? rows : rows.filter((r) => !isArtefact(r.file.path));
  const title = rangeTitle({ from, to }, data.note, data.workspace);
  return (
    <div className="changes">
      {shown.length === 0 && files.length === 0 ? (
        <p className="muted" title={title}>
          No files changed.
        </p>
      ) : (
        <ul className="changed-files" title={title}>
          {shown.map(({ file: f, mark }) => {
            const open = selected === f.path;
            const label = fileStatusLabel[f.status] ?? f.status;
            const badge = fileStatusBadge(f.status);
            if (mark === 'removed') {
              // Gone from the list: drawn in place a moment, struck through, not clickable.
              return (
                <li key={f.path} className="mark-removed" aria-hidden>
                  <span className="changed-file">
                    <Chevron binary open={false} />
                    <span className="file-path">{f.path}</span>
                    {badge && <span className="file-status">{badge}</span>}
                    <FileCounts file={f} />
                  </span>
                </li>
              );
            }
            return (
              <li key={f.path} className={mark === 'added' ? 'mark-added' : undefined}>
                <button
                  type="button"
                  className={`changed-file ${open ? 'active' : ''}`}
                  onClick={() => !f.binary && onSelect(open ? null : f.path)}
                  aria-expanded={f.binary ? undefined : open}
                  disabled={f.binary}
                  title={`${f.path} · ${label}`}
                >
                  <Chevron binary={f.binary} open={open} />
                  <span className="file-path">{f.path}</span>
                  {badge && <span className="file-status">{badge}</span>}
                  <FileCounts file={f} marked={mark === 'changed'} />
                </button>
                {open && <DiffLoader view={view} from={from} to={to} path={f.path} refresh={to ?? refresh} />}
              </li>
            );
          })}
        </ul>
      )}
      {artefacts.length > 0 && (
        <button type="button" className="text-button small" onClick={() => onShowHidden(!showHidden)} title={artefacts.map((f) => f.path).join('\n')}>
          {showHidden
            ? 'Hide generated files'
            : // Untracked in the workspace, so a `git add --all` would commit them.
              `${artefacts.length} generated ${artefacts.length === 1 ? 'file' : 'files'} in the workspace (would be committed)`}
        </button>
      )}
    </div>
  );
}

/** A binary file cannot open, so it keeps the chevron's space empty to stay aligned. */
function Chevron({ binary, open }: { binary: boolean | undefined; open: boolean }) {
  if (binary) return <span className="chevron-space" />;
  return open ? <ChevronDown size={13} aria-hidden /> : <ChevronRight size={13} aria-hidden />;
}

function FileCounts({ file, marked = false }: { file: ChangedFile; marked?: boolean }) {
  if (file.binary) return <span className="file-counts">binary</span>;
  return (
    // Keyed by the counts, so each move restarts the highlight.
    <span key={`${file.status}/${file.additions}/${file.deletions}/${file.untracked}`} className={`file-counts tabular${marked ? ' mark-changed' : ''}`}>
      {!!file.additions && <span>+{file.additions}</span>}
      {!!file.deletions && <span>−{file.deletions}</span>}
      {file.untracked && <span>untracked</span>}
    </span>
  );
}

type DiffLoaderProps = { view: RunView; from: string | null | undefined; to: string | null | undefined; path: string; refresh: Refresh };

function DiffLoader({ view, from, to, path, refresh }: DiffLoaderProps) {
  // Keyed on what it shows; a new end snapshot (or a live tick) refreshes it in place.
  const key = `${view.target}/${view.runId}/diff/${from}/${path}`;
  const diff = useResource<{ diff: string }>(key, (signal) => api.diff(view.target, view.runId, { from, to, path }, signal), refresh);
  if (diff.loading && !diff.data) return <Skeleton lines={4} />;
  if (diff.error && !diff.data) return <ErrorBox compact title="Could not load the diff" message={diff.error} onRetry={diff.reload} />;
  return <DiffView text={diff.data?.diff ?? ''} />;
}

/** Lines a diff shows before "Show the whole diff". */
const MAX_DIFF_LINES = 2500;
/** Lines a browsed file shows; the rest is cut (the message below says so). */
const MAX_FILE_LINES = 5000;

export function DiffView({ text }: { text: string }) {
  const files = useMemo(() => parseDiff(text), [text]);
  const [all, setAll] = useState(false);
  if (!text.trim()) return <p className="muted small diff-empty">No textual difference.</p>;
  let budget = all ? Infinity : MAX_DIFF_LINES;
  return (
    <div className="diff">
      {files.map((f: DiffFile, i) => {
        const lines = f.lines.slice(0, Math.max(0, budget));
        budget -= lines.length;
        return (
          <table key={i} className="diff-table">
            <tbody>
              {lines.map((l, j) => (
                <tr key={j} className={`dl dl-${l.kind}`}>
                  <td className="ln">{l.oldNo ?? ''}</td>
                  <td className="ln">{l.newNo ?? ''}</td>
                  <td className="dl-sign" aria-hidden>
                    {l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ''}
                  </td>
                  <td className="dl-text">
                    <span className="visually-hidden">{l.kind === 'add' ? 'added: ' : l.kind === 'del' ? 'removed: ' : ''}</span>
                    {l.text}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        );
      })}
      {budget <= 0 && !all && (
        <button type="button" className="text-button" onClick={() => setAll(true)}>
          Show the whole diff
        </button>
      )}
    </div>
  );
}

function Browser({ view, commit }: { view: RunView; commit: string | null }) {
  const key = `${view.target}/${view.runId}/tree/${commit}`;
  const tree = useResource<{ paths: string[] }>(key, (signal) => api.tree(view.target, view.runId, commit, signal));
  const [filter, setFilter] = useState('');
  const [path, setPath] = useState<string | null>(null);
  const paths = tree.data?.paths ?? [];
  const filtered = useMemo(() => (filter ? paths.filter((p) => p.toLowerCase().includes(filter.toLowerCase())).slice(0, 500) : []), [paths, filter]);
  const entries = useMemo(() => buildTree(paths), [paths]);
  if (tree.loading && !tree.data) return <Skeleton lines={6} />;
  if (tree.error) return <ErrorBox title="Cannot browse files" message={tree.error} onRetry={tree.reload} />;
  return (
    <div className="browser">
      <input
        type="search"
        placeholder={`Find among ${paths.length.toLocaleString()} files`}
        aria-label="Find a file"
        title={commit ? `Files at ${shortId(commit, 10)}` : 'Files in the workspace as it is now'}
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <div className="browser-split">
        <div className="browser-tree">
          {filter ? (
            <ul className="tree">
              {filtered.map((p) => (
                <li key={p}>
                  <button type="button" className={`tree-file ${p === path ? 'active' : ''}`} onClick={() => setPath(p)}>
                    <File size={12} aria-hidden /> {p}
                  </button>
                </li>
              ))}
              {!filtered.length && <li className="muted small">No file matches.</li>}
            </ul>
          ) : (
            <Tree entries={entries} selected={path} onSelect={setPath} />
          )}
        </div>
        {path && <FileContent view={view} commit={commit} path={path} />}
      </div>
    </div>
  );
}

function Tree({ entries, selected, onSelect, depth = 0 }: { entries: TreeEntry[]; selected: string | null; onSelect: (p: string) => void; depth?: number }) {
  return (
    <ul className="tree">
      {entries.map((e) =>
        e.dir ? (
          <TreeDir key={e.path} entry={e} selected={selected} onSelect={onSelect} depth={depth} />
        ) : (
          <li key={e.path}>
            <button type="button" className={`tree-file ${e.path === selected ? 'active' : ''}`} style={{ paddingLeft: 8 + depth * 14 }} onClick={() => onSelect(e.path)}>
              <File size={12} aria-hidden /> {e.name}
            </button>
          </li>
        )
      )}
    </ul>
  );
}

function TreeDir({ entry, selected, onSelect, depth }: { entry: TreeEntry; selected: string | null; onSelect: (p: string) => void; depth: number }) {
  const [open, setOpen] = useState(false);
  return (
    <li>
      <button type="button" className="tree-dir" style={{ paddingLeft: 8 + depth * 14 }} aria-expanded={open} onClick={() => setOpen(!open)}>
        {open ? <ChevronDown size={12} aria-hidden /> : <ChevronRight size={12} aria-hidden />}
        <Folder size={12} aria-hidden /> {entry.name}
      </button>
      {open && entry.children && <Tree entries={entry.children} selected={selected} onSelect={onSelect} depth={depth + 1} />}
    </li>
  );
}

function FileContent({ view, commit, path }: { view: RunView; commit: string | null; path: string }) {
  const key = `${view.target}/${view.runId}/file/${commit}/${path}`;
  const file = useResource<{ content: string }>(key, (signal) => api.file(view.target, view.runId, { commit, path }, signal));
  if (file.loading && !file.data) return <Skeleton lines={8} />;
  if (file.error) return <ErrorBox compact title={`Cannot show ${path}`} message={file.error} />;
  const lines = (file.data?.content ?? '').split('\n');
  return (
    <div className="file-content">
      <div className="file-content-head">
        <code>{path}</code>
      </div>
      <table className="diff-table">
        <tbody>
          {lines.slice(0, MAX_FILE_LINES).map((l, i) => (
            <tr key={i} className="dl dl-ctx">
              <td className="ln">{i + 1}</td>
              <td className="dl-text">{l}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {lines.length > MAX_FILE_LINES && <p className="muted small">Showing the first {MAX_FILE_LINES.toLocaleString('en-US')} lines.</p>}
    </div>
  );
}

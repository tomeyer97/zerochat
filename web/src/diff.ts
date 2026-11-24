import type { Execution } from './types';
import { shortId } from './format';

/** A parsed unified diff (git diff output) with old/new line numbers for display. */
export type DiffLine = {
  kind: 'add' | 'del' | 'ctx' | 'hunk' | 'meta' | 'note';
  text: string;
  oldNo?: number;
  newNo?: number;
};

export type DiffFile = {
  oldPath: string;
  newPath: string;
  lines: DiffLine[];
  additions: number;
  deletions: number;
  binary: boolean;
};

/** Where the parser is: the file being read, and inside a hunk the next old/new line numbers. */
type ParseState = { file: DiffFile; inHunk: boolean; oldNo: number; newNo: number };

const HUNK_HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/;

/** Appends a new file to `files` and returns the state for reading it. */
function startFile(files: DiffFile[], oldPath = '', newPath = ''): ParseState {
  const file: DiffFile = { oldPath, newPath, lines: [], additions: 0, deletions: 0, binary: false };
  files.push(file);
  return { file, inHunk: false, oldNo: 0, newNo: 0 };
}

/** `--- a/path` / `+++ b/path` before the first hunk; true when the row was one of them. */
function readFileHeader(file: DiffFile, raw: string): boolean {
  if (raw.startsWith('--- ')) {
    const p = raw.slice(4).replace(/^a\//, '');
    if (p !== '/dev/null') file.oldPath ||= p;
    return true;
  }
  if (raw.startsWith('+++ ')) {
    const p = raw.slice(4).replace(/^b\//, '');
    if (p !== '/dev/null') file.newPath ||= p;
    if (!file.oldPath && p !== '/dev/null') file.oldPath = '';
    return true;
  }
  return false;
}

/** `@@ -old,n +new,m @@`: starts a hunk and resets the line numbers; true when the row was one. */
function readHunkHeader(state: ParseState, raw: string): boolean {
  const hunk = HUNK_HEADER.exec(raw);
  if (!hunk) return false;
  state.oldNo = Number(hunk[1]);
  state.newNo = Number(hunk[2]);
  state.inHunk = true;
  state.file.lines.push({ kind: 'hunk', text: raw });
  return true;
}

/** Anything between the `diff --git` line and the first hunk (mode, rename, `Binary files …`). */
function readMetaLine(file: DiffFile, raw: string) {
  if (raw.startsWith('Binary files')) file.binary = true;
  // Blob ids and file mode (`index 5d21c42..0d25a22 100644`) say nothing a reader needs.
  if (raw.trim() && !raw.startsWith('index ')) file.lines.push({ kind: 'meta', text: raw });
}

/** A line inside a hunk, by its first character. `last`: the row after the output's final newline. */
function readBodyLine(state: ParseState, raw: string, last: boolean) {
  const { file } = state;
  if (raw.startsWith('+')) {
    file.lines.push({ kind: 'add', text: raw.slice(1), newNo: state.newNo++ });
    file.additions++;
  } else if (raw.startsWith('-')) {
    file.lines.push({ kind: 'del', text: raw.slice(1), oldNo: state.oldNo++ });
    file.deletions++;
  } else if (raw.startsWith('\\') || raw.startsWith('… [diff truncated')) {
    // `\ No newline at end of file`, or the server's truncation marker.
    file.lines.push({ kind: 'note', text: raw });
  } else if (raw === '' && last) {
    // The newline that ends the output.
  } else {
    file.lines.push({ kind: 'ctx', text: raw.slice(1), oldNo: state.oldNo++, newNo: state.newNo++ });
  }
}

export function parseDiff(text: string): DiffFile[] {
  const files: DiffFile[] = [];
  let state: ParseState | null = null;
  const rows = text.split('\n');
  for (const [index, raw] of rows.entries()) {
    if (raw.startsWith('diff --git ')) {
      const m = /^diff --git a\/(.*) b\/(.*)$/.exec(raw);
      state = startFile(files, m?.[1] ?? '', m?.[2] ?? '');
      continue;
    }
    // A diff without a `diff --git` line (the server's synthetic diff for an untracked file).
    if (!state && (raw.startsWith('--- ') || raw.startsWith('@@'))) state = startFile(files);
    if (!state) continue;
    if (!state.inHunk && readFileHeader(state.file, raw)) continue;
    if (readHunkHeader(state, raw)) continue;
    if (!state.inHunk) readMetaLine(state.file, raw);
    else readBodyLine(state, raw, index === rows.length - 1);
  }
  for (const f of files) if (!f.newPath) f.newPath = f.oldPath;
  return files;
}

export const fileStatusLabel: Record<string, string> = {
  A: 'added',
  M: 'modified',
  D: 'deleted',
  R: 'renamed',
  C: 'copied',
  T: 'type changed',
  U: 'unmerged',
};

/**
 * The short badge a changed file shows next to its path: `new`, `deleted`, `renamed`, …; none
 * for a plain modification (the common case needs no mark).
 */
export function fileStatusBadge(status: string): string | null {
  if (status === 'M') return null;
  if (status === 'A') return 'new';
  if (status === 'D') return 'deleted';
  return (fileStatusLabel[status] ?? status).toLowerCase();
}

// ---------- which commits the Files tab compares ----------

/** Two commits to compare (`null`: the run's base, or the workspace as it is now). */
export type ChangeRange = {
  from: string | null;
  to: string | null;
  /** True when the range is one execution's own; false for the whole run. */
  perExecution: boolean;
};

type ExecutionSnapshots = Pick<Execution, 'workspaceStart' | 'workspaceEnd' | 'state'>;

/**
 * The range to compare for an execution, or the whole run (`execution` null, or a Zeroshot that
 * records no snapshots). Zeroshot records a `workspace` snapshot on an execution's started and
 * completed ledger events; its own changes run from the end snapshot's `base` (or the start
 * snapshot's `commit`) to the end snapshot's `commit`.
 */
export function changeRange(execution: ExecutionSnapshots | null): ChangeRange {
  const from = execution?.workspaceEnd?.base ?? execution?.workspaceStart?.commit ?? null;
  if (execution && from) {
    return { from, to: execution.workspaceEnd?.commit ?? null, perExecution: true };
  }
  return { from: null, to: null, perExecution: false };
}

/**
 * Whether this one execution's own snapshot moved: cheap, from data already on it, no fetch. A
 * read-only reviewer that never touches the workspace hides its Files tab instead of opening to
 * an empty drawer with a same-hash range. Unknown (no per-node snapshots, or still running with
 * no end snapshot yet) keeps the tab rather than guess.
 */
export function executionFilesChanged(execution: ExecutionSnapshots | null): boolean {
  const range = changeRange(execution);
  return !range.perExecution || !range.to || range.to !== range.from;
}

/** The hover text of a changes list: `Compared: abc → def`, then any notes, one per line. */
export function rangeTitle(range: Pick<ChangeRange, 'from' | 'to'>, ...notes: (string | null | undefined)[]): string {
  const from = range.from ? shortId(range.from, 10) : 'base';
  const to = range.to ? shortId(range.to, 10) : 'now';
  return [`Compared: ${from} → ${to}`, ...notes.filter(Boolean)].join('\n');
}

// The Files tab's folder tree and artefact filter live in file-tree.ts; re-exported for existing imports.
export { buildTree, isArtefact, type TreeEntry } from './file-tree';

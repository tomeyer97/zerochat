/** The Executions table: bar geometry, row notes and tones, the filter and the column layout. */
import type { Execution, RunView } from './types';
import { ERROR_WORDS } from './generated/projection';
import { diagnosticText, execStateInfo, nodeState, type NodeState } from './status';

/** Start offset and length of each bar as fractions of the run's span. */
export function timelineBars(executions: Pick<Execution, 'id' | 'startedAt' | 'endedAt' | 'state' | 'durationMs'>[], now: number) {
  const starts = executions.map((e) => e.startedAt).filter((v): v is number => v != null);
  if (!starts.length) return { span: 0, start: 0, bars: new Map<number, { left: number; width: number }>() };
  const start = Math.min(...starts);
  const ends = executions.map((e) => barEnd(e, now, start));
  const end = Math.max(start + 1, ...ends);
  const span = end - start;
  const bars = new Map<number, { left: number; width: number }>();
  executions.forEach((e, i) => {
    if (e.startedAt == null) return;
    const left = (e.startedAt - start) / span;
    const width = Math.max(0.004, (ends[i] - e.startedAt) / span);
    bars.set(e.id, { left, width: Math.min(width, 1 - left) });
  });
  return { span, start, bars };
}

/** Where a bar ends: now while running, else its end, else its start plus its duration. */
function barEnd(e: Pick<Execution, 'startedAt' | 'endedAt' | 'state' | 'durationMs'>, now: number, runStart: number): number {
  if (e.state === 'running') return now;
  if (e.endedAt != null) return e.endedAt;
  if (e.startedAt != null && e.durationMs != null) return e.startedAt + e.durationMs;
  return e.startedAt ?? runStart;
}

/** Zeroshot's error codes in the words projection.rs uses, except a malformed output: a row note
 * says `malformed output`, where a sentence about the node says it `gave malformed output`. */
const ROW_ERROR_WORDS: Readonly<Record<string, string>> = { ...ERROR_WORDS, malformed: 'malformed output' };

/** An execution's error in plain words: `timed out`, `crashed` (Zeroshot's `declared_failure` says nothing). */
export function errorWords(error: Execution['error']): string {
  if (!error) return '';
  const code = error.code ? (ROW_ERROR_WORDS[error.code] ?? error.code.replace(/_/g, ' ')) : '';
  const reason = error.reason && error.reason !== 'declared_failure' ? error.reason.replace(/_/g, ' ') : '';
  return [code, reason].filter(Boolean).join(': ');
}

/** The one note a row needs beyond its state: the reviewer's first line of feedback, or the error. */
export function rowNote(e: Execution, view: Pick<RunView, 'failure'>): { text: string; title: string } {
  if (e.reused) return { text: 'reused', title: 'Reused from the original run' };
  if (e.state === 'rejected') {
    const feedback = diagnosticText(e.diagnostic).trim();
    return { text: feedback.split('\n')[0], title: feedback };
  }
  if (e.state === 'failed') {
    const text = view.failure?.execution === e.id && view.failure.cause ? view.failure.cause : errorWords(e.error);
    const code = e.error ? `${e.error.code ?? ''}${e.error.reason ? `: ${e.error.reason}` : ''}` : '';
    return { text, title: [text, code].filter(Boolean).join(' · ') };
  }
  if (e.state === 'voided') return { text: e.voidReason ?? '', title: e.voidReason ?? '' };
  return { text: '', title: '' };
}

/** State words take colour only when they need attention: failed, running, or the run's culprit.
 * Red stays reserved for an actual failure: a rejected culprit is amber, like every other
 * "Rejected" elsewhere on the page, and a deliberately stopped/interrupted node is never
 * repainted red just for being the deciding round. */
export function stateTone(state: NodeState, culprit: boolean): string {
  if (state === 'failed') return 'bad';
  if (state === 'rejected') return culprit ? 'warn' : 'muted';
  if (state === 'running') return 'running';
  return 'muted';
}

/** Whether a row matches every word of the filter. It matches what the row actually shows, not
 * just the node id and raw state enum: the round number, the note text and the state's own label
 * ("Rejected", not just "rejected"). */
export function rowMatches(e: Execution, view: Pick<RunView, 'failure'>, needle: string): boolean {
  if (!needle) return true;
  const haystack = [e.node, e.state, execStateInfo[nodeState(e.state)].label, `round ${e.round}`, `attempt ${e.attempt}`, rowNote(e, view).text].join(' ').toLowerCase();
  return needle.split(/\s+/).every((word) => haystack.includes(word));
}

// ---------- columns ----------

/** Below this table width the bars give way (no sideways scroll). */
export const BARS_MIN_WIDTH = 640;
/** Below this table width (a node panel is open) tokens step back to the row tooltip and the panel. */
export const TOKENS_MIN_WIDTH = 560;

/** Which optional columns the table draws: a column that would be empty is not drawn. */
export type TimelineLayout = { rounds: boolean; tokens: boolean; note: boolean; bars: boolean };

export function timelineLayout(view: Pick<RunView, 'executions' | 'failure'>, width: number): TimelineLayout {
  return {
    rounds: view.executions.some((e) => e.round > 1 || e.attempt > 1),
    tokens: width >= TOKENS_MIN_WIDTH,
    note: view.executions.some((e) => rowNote(e, view).text),
    bars: view.executions.length > 1 && width >= BARS_MIN_WIDTH,
  };
}

export type TimelineColumnId = 'node' | 'round' | 'state' | 'duration' | 'cost' | 'tokens' | 'note' | 'bar';

export type TimelineColumn = {
  id: TimelineColumnId;
  /** Its grid track. */
  width: string;
  header: string;
  headerClass?: string;
  /** Drawn only when the layout says so; always drawn without this. */
  show?: (layout: TimelineLayout) => boolean;
};

/** The table's columns in order: the header, every row and the grid template follow this list. */
export const TIMELINE_COLUMNS: readonly TimelineColumn[] = [
  { id: 'node', width: 'minmax(90px, 1.2fr)', header: 'Node' },
  { id: 'round', width: '44px', header: 'Round', show: (l) => l.rounds },
  { id: 'state', width: '80px', header: 'State' },
  { id: 'duration', width: '64px', header: 'Duration', headerClass: 'tl-duration' },
  { id: 'cost', width: '56px', header: 'Cost', headerClass: 'tl-cost' },
  { id: 'tokens', width: '58px', header: 'Tokens', headerClass: 'tl-tokens', show: (l) => l.tokens },
  { id: 'note', width: 'minmax(0, 1.6fr)', header: 'Note', show: (l) => l.note },
  { id: 'bar', width: 'minmax(80px, 1.2fr)', header: 'Timeline', headerClass: 'tl-bar-head', show: (l) => l.bars },
];

export function visibleColumns(layout: TimelineLayout): TimelineColumn[] {
  return TIMELINE_COLUMNS.filter((c) => !c.show || c.show(layout));
}

export function gridTemplate(columns: TimelineColumn[]): string {
  return columns.map((c) => c.width).join(' ');
}

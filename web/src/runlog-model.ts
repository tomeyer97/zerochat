/**
 * The run's Log section: run-level lines and the background agents Zeroshot runs outside the
 * graph (Active Memory after the run), adapted from the server's wording to the page's.
 */
import type { BackgroundAgent, LogLine } from './types';
import { plural } from './format';

/** `active_memory_graph_maintenance` → `memory graph maintenance`. */
export function agentLabel(name: string): string {
  return name.replace(/^active_memory_/, 'memory ').replace(/_/g, ' ');
}

/** Older servers put the agent in the text as a `[name] ` prefix instead of the `agent` field. */
const LEGACY_AGENT_PREFIX = /^\[([^\] ]{1,80})\] ([\s\S]*)$/;
/** A run-level line about an agent, in the engine's words: `Active Memory <name> …`. */
const ACTIVE_MEMORY_SENTENCE = /^Active Memory (\S+) /;

/** The agent of a log line: the server's field, or the prefix of older servers. A run-level
 * "Active Memory <name> …" sentence names it the engine's way; say it the way the filter chips
 * already do. */
export function lineAgent(l: Pick<LogLine, 'agent' | 'text'>): { agent: string | null; text: string } {
  if (l.agent) return { agent: l.agent, text: l.text };
  const legacy = LEGACY_AGENT_PREFIX.exec(l.text);
  if (legacy) return { agent: legacy[1], text: legacy[2] };
  const am = ACTIVE_MEMORY_SENTENCE.exec(l.text);
  return { agent: null, text: am ? l.text.replace(am[0], `${agentLabel(am[1])} `) : l.text };
}

/** An agent that failed outright or logged errors. */
export function agentFailed(a: Pick<BackgroundAgent, 'outcome' | 'errors'>): boolean {
  return a.outcome === 'failed' || a.errors > 0;
}

/** An agent chip's tooltip: its name, counts, outcome, note and last line. */
export function agentTip(a: BackgroundAgent): string {
  return [
    a.name,
    plural(a.lines, 'line'),
    a.retries ? plural(a.retries, 'retry', 'retries') : '',
    a.errors ? plural(a.errors, 'error') : '',
    a.outcome ?? '',
    a.note ?? '',
    a.lastLine ? `Last: ${a.lastLine}` : '',
  ]
    .filter(Boolean)
    .join('\n');
}

/** One filter chip: `all`, `run` (lines of no agent), or an agent's name. */
export type LogFilter = { id: string; label: string; title?: string; bad?: boolean };

/** The filter chips: none when there are no background agents (every line is the run's). */
export function logFilters(agents: BackgroundAgent[]): LogFilter[] {
  if (!agents.length) return [];
  return [{ id: 'all', label: 'All' }, { id: 'run', label: 'Run' }, ...agents.map((a) => ({ id: a.name, label: agentLabel(a.name), title: agentTip(a), bad: agentFailed(a) }))];
}

/** Whether a line passes the chosen filter. */
export function lineShown(line: { agent: string | null }, filter: string): boolean {
  return filter === 'all' || (filter === 'run' ? !line.agent : line.agent === filter);
}

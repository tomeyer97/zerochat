/**
 * The node chat's model: how a transcript is grouped and windowed for display, what each item
 * shows (tool cards, notices, prompts, returned JSON) and the summaries around it. Pure functions;
 * Chat.tsx renders what they return.
 */
import type { Json, Tool, TranscriptItem, TranscriptItemKind } from './types';
import { clockTime, prettyJson, truncate } from './format';
import { SEARCH_SEPARATOR, isResultTool, isSummarisedTool, toolSummary } from './tool-summary';
import { relativeTo } from './workspace-paths';

export { editPreview, isResultTool, isSummarisedTool, toolPaths, toolSummary } from './tool-summary';
export { commonDir, relativeTo } from './workspace-paths';

export type ChatEntry =
  | { kind: 'item'; key: string; item: TranscriptItem; index: number }
  /** Consecutive tool calls fold into one block so a run of 40 reads does not bury the messages. */
  | { kind: 'tools'; key: string; items: { item: TranscriptItem; index: number }[] };

/**
 * Groups the transcript for display: runs of 3+ consecutive tool calls become one entry. Short
 * reasoning between tool calls doesn't break the run (it renders inside the group once opened);
 * only real messages (text, errors, …) do.
 */
export function chatEntries(items: TranscriptItem[], minGroup = 3): ChatEntry[] {
  const out: ChatEntry[] = [];
  let run: { item: TranscriptItem; index: number }[] = [];
  const flush = () => {
    const toolCount = run.filter((r) => r.item.kind === 'tool').length;
    if (toolCount >= minGroup) out.push({ kind: 'tools', key: `tools-${run[0].index}`, items: run });
    else for (const r of run) out.push({ kind: 'item', key: `item-${r.index}`, item: r.item, index: r.index });
    run = [];
  };
  items.forEach((item, index) => {
    if (item.kind === 'tool' && item.tool) run.push({ item, index });
    else if (item.kind === 'thinking' && run.length > 0) run.push({ item, index });
    else {
      flush();
      out.push({ kind: 'item', key: `item-${index}`, item, index });
    }
  });
  flush();
  return out;
}

/** The window of entries to render: the last `count` (long transcripts stay fast). */
export function visibleRange(total: number, count: number): { start: number; end: number } {
  return { start: Math.max(0, total - count), end: total };
}

export const CHAT_PAGE = 150;

/** The index of the last `thinking` item (expanded by default). */
export function lastThinkingIndex(items: TranscriptItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) if (items[i].kind === 'thinking') return i;
  return -1;
}

/**
 * When the transcript has no native prompt, what the node was given: its instructions and
 * the fields of its input (long text fields shown as text, the rest as JSON).
 */
export function synthesizedPrompt(
  instructions: string | null | undefined,
  input: Json | undefined
): {
  instructions: string | null;
  textFields: { name: string; text: string }[];
  otherFields: Record<string, Json> | null;
  raw: Json | undefined;
} {
  const textFields: { name: string; text: string }[] = [];
  let otherFields: Record<string, Json> | null = null;
  if (input && typeof input === 'object' && !Array.isArray(input)) {
    for (const [name, value] of Object.entries(input)) {
      if (typeof value === 'string' && (value.length > 60 || value.includes('\n'))) textFields.push({ name, text: value });
      else {
        otherFields ??= {};
        otherFields[name] = value;
      }
    }
  }
  return { instructions: instructions?.trim() ? instructions : null, textFields, otherFields, raw: input };
}

/**
 * Transcript item kinds (generated `ItemKind`) shown as small centered notices rather than
 * bubbles, and each one's CSS class, written out so a selector such as `.notice-rate_limit` can be
 * found from the stylesheet.
 */
const NOTICE_CLASS = {
  system: 'notice-system',
  auto: 'notice-auto',
  retry: 'notice-retry',
  rate_limit: 'notice-rate_limit',
  compaction: 'notice-compaction',
} as const satisfies Partial<Record<TranscriptItemKind, string>>;

type NoticeKind = keyof typeof NOTICE_CLASS;

/** An item kind shown as a notice (`NOTICE_CLASS`). Not transcript.rs `is_notice`, which picks
 * harness lines out of an agent's log text. */
export function isNotice(kind: string): kind is NoticeKind {
  return Object.hasOwn(NOTICE_CLASS, kind);
}

/** The CSS class of a notice kind. */
export function noticeClass(kind: NoticeKind): string {
  return NOTICE_CLASS[kind];
}

/** A shell command's non-zero exit, shown beside the call when the tool itself did not fail. */
export function exitNote(result: { isError: boolean; exitCode?: number } | undefined): string | null {
  return result && !result.isError && result.exitCode != null && result.exitCode !== 0 ? `exit ${result.exitCode}` : null;
}

/** A permission answer worth flagging on the call's line (anything but an approval). */
export function deniedPermission(permission: { kind: string; result?: string } | undefined): string | null {
  return permission?.result && !permission.result.startsWith('approved') ? `${permission.kind} permission ${permission.result.replace(/-/g, ' ')}` : null;
}

export function hasNativePrompt(items: TranscriptItem[]) {
  return items.some((i) => i.kind === 'prompt');
}

/** Tool calls and how many failed, for the transcript summary line. */
export function transcriptStats(items: TranscriptItem[]) {
  let tools = 0;
  let toolErrors = 0;
  let messages = 0;
  let thinking = 0;
  for (const i of items) {
    if (i.kind === 'tool') {
      tools++;
      if (i.tool?.result?.isError) toolErrors++;
    } else if (i.kind === 'text') messages++;
    else if (i.kind === 'thinking') thinking++;
  }
  return { tools, toolErrors, messages, thinking };
}

/**
 * The fixed frame Zeroshot puts around a node's prompt: `Execute this graph node…`, then
 * `Authored instructions:`, runtime guidance, `Input JSON:` and the response contract. These are
 * Zeroshot's template headings; if Zeroshot rewords them, `splitZeroshotPrompt` finds no frame
 * and the chat shows the prompt whole.
 */
const PROMPT_FRAME = {
  /** Only a prompt that mentions this before the instructions is a node prompt. */
  intro: 'graph node',
  authored: 'Authored instructions:\n',
  runtime: 'Runtime-owned',
  input: 'Input JSON:\n',
} as const;

/**
 * Splits Zeroshot's node prompt (`PROMPT_FRAME`) so the chat can lead with what the graph's
 * author wrote and the task, and keep the frame one click away. `null` for any other prompt.
 */
export function splitZeroshotPrompt(text: string): { authored: string; input: Json | undefined; inputText: string | null } | null {
  const at = text.indexOf(PROMPT_FRAME.authored);
  if (at < 0 || !text.slice(0, at).includes(PROMPT_FRAME.intro)) return null;
  const rest = text.slice(at + PROMPT_FRAME.authored.length);
  const ends = [`\n${PROMPT_FRAME.runtime}`, `\n${PROMPT_FRAME.input}`].map((m) => rest.indexOf(m)).filter((i) => i >= 0);
  const authored = rest.slice(0, ends.length ? Math.min(...ends) : rest.length).trim();
  const inputAt = text.indexOf(PROMPT_FRAME.input);
  let input: Json | undefined;
  let inputText: string | null = null;
  if (inputAt >= 0) {
    inputText = text.slice(inputAt + PROMPT_FRAME.input.length).split('\n')[0];
    try {
      input = JSON.parse(inputText) as Json;
    } catch {
      input = undefined;
    }
  }
  return { authored, input, inputText };
}

/** The first non-empty line, for a one-line preview. */
export function firstLine(text: string | null | undefined, max = 120): string {
  const line =
    (text ?? '')
      .split('\n')
      .map((l) => l.trim())
      .find(Boolean) ?? '';
  return truncate(line.replace(/^#+\s*/, ''), max);
}

/** The latest moment the agent was seen doing something. */
export function lastSeen(items: TranscriptItem[], lastActivityAt: number | null | undefined): number | null {
  let latest = lastActivityAt ?? null;
  for (const item of items) {
    for (const t of [item.at, item.tool?.result?.at]) if (t != null && (latest == null || t > latest)) latest = t;
  }
  return latest;
}

/** Silence after which a live node says how long it has been quiet. */
export const IDLE_AFTER_MS = 60_000;

/**
 * Agent text that is only a JSON object (bare, or fenced as ```json) is its structured result:
 * the JSON without the fence markers, for a collapsed "Returned JSON". `null` for any other text.
 */
export function returnedJson(text: string): string | null {
  const t = text.trim();
  if (!t.startsWith('{') && !t.startsWith('```json')) return null;
  const json = t
    .replace(/^```json\s*/, '')
    .replace(/```$/, '')
    .trim();
  try {
    const value = JSON.parse(json);
    return value && typeof value === 'object' ? json : null;
  } catch {
    return null;
  }
}

/** The one-line head of a folded group of tool calls: how many, how many failed, and the verbs
 * counted (`Read ×3, Edit ×1`) for its tooltip. */
export function toolGroupSummary(items: { item: TranscriptItem }[]): { calls: number; errors: number; verbs: string } {
  const counts = new Map<string, number>();
  let calls = 0;
  let errors = 0;
  for (const { item } of items) {
    if (item.kind !== 'tool' || !item.tool) continue;
    calls++;
    const verb = toolSummary(item.tool).verb;
    counts.set(verb, (counts.get(verb) ?? 0) + 1);
    if (item.tool.result?.isError) errors++;
  }
  return { calls, errors, verbs: [...counts.entries()].map(([verb, n]) => `${verb} ×${n}`).join(', ') };
}

/** What a tool card shows. */
export type ToolCardView = {
  /** `none`: no result and the node is no longer running. */
  status: 'ok' | 'error' | 'pending' | 'none';
  /** The harness's result-collecting call (`isResultTool`), shown as "Returned result". */
  resultTool: boolean;
  verb: string;
  /** The summary detail, its paths relative to the workspace. */
  detail: string;
  /** The head's tooltip: the full detail (or the tool's name), its description, the time. */
  title: string;
  /** A non-zero exit of a call that did not fail (`exitNote`). */
  exit: string | null;
  /** A permission answer that was not an approval (`deniedPermission`). */
  denied: string | null;
  /** Any other permission answer, for the card's body. */
  permission: string | null;
  /** The raw input, as text. */
  input: string;
  /** The summary already says what went in: the raw input sits in a closed "Input" fold. */
  foldInput: boolean;
  /** Zeroshot cut the input in its log (`Tool.inputCut`): the note after the detail, which then
   * ends in `…`. A neutral note, not an error. */
  cut: string | null;
};

/** The note on a call whose input Zeroshot cut in its log. */
export const INPUT_CUT_NOTE = '(input cut by Zeroshot)';

export function toolCardView(item: TranscriptItem, tool: Tool, live: boolean, workspace: string | null): ToolCardView {
  const summary = toolSummary(tool);
  const result = tool.result;
  const resultTool = isResultTool(tool.name);
  const fields = tool.input && typeof tool.input === 'object' && !Array.isArray(tool.input) ? (tool.input as Record<string, Json>) : null;
  const description = typeof fields?.description === 'string' ? fields.description : '';
  const when = item.at ? clockTime(item.at, true) : '';
  const denied = deniedPermission(tool.permission);
  const cut = tool.inputCut === true && !resultTool;
  const detail = resultTool
    ? ''
    : summary.detail
        .split(SEARCH_SEPARATOR)
        .map((part) => relativeTo(part, workspace))
        .join(SEARCH_SEPARATOR);
  return {
    status: result ? (result.isError ? 'error' : 'ok') : live ? 'pending' : 'none',
    resultTool,
    verb: resultTool ? 'Returned result' : summary.verb,
    detail: cut && detail ? `${detail} …` : detail,
    title: [resultTool ? tool.name : summary.detail, description, cut ? 'Zeroshot cut this input in its log.' : '', when].filter(Boolean).join('\n'),
    exit: exitNote(result),
    denied,
    permission: tool.permission && !denied ? `${tool.permission.kind} permission ${tool.permission.result?.replace(/-/g, ' ') ?? 'pending'}` : null,
    input: typeof tool.input === 'string' ? tool.input : prettyJson(tool.input),
    foldInput: isSummarisedTool(tool.name),
    cut: cut ? INPUT_CUT_NOTE : null,
  };
}

/**
 * What the chat knows about each harness's tool calls: a one-line summary (`Bash  npm test`,
 * `Edit  src/x.ts`), whether that summary already says what went in, and how an edit-like call's
 * input reads as a diff. Every tool name zerochat recognises is one row of `TOOLS`, Claude Code's
 * and GitHub Copilot CLI's side by side; adding a tool or a harness is a change to that table.
 */
import type { Json, Tool } from './types';
import { RESULT_TOOL } from './generated/projection';
import { truncate } from './format';
import { diffLines } from './line-diff';

type ToolInput = Record<string, Json>;

/** How an edit-like call's input becomes diff text. */
type EditShape =
  /** One replacement: the old and the new text. */
  | { kind: 'replace'; oldKey: string; newKey: string }
  /** A list of replacements under `edits`, each with the old and the new text. */
  | { kind: 'edits'; oldKey: string; newKey: string }
  /** A whole file written: every line is new. */
  | { kind: 'write'; contentKey: string };

type ToolShape = {
  verb: string;
  /** The input keys for the detail (the first non-empty one wins), or how to build it. */
  detail: readonly string[] | ((input: ToolInput) => string);
  /** The detail is free text: keep its first line, at most 140 characters. */
  firstLine?: true;
  /** The summary already says what went in, so the tool card folds the raw input away. */
  foldInput?: true;
  edit?: EditShape;
};

/** Joins a search's pattern and where it looked, in the summary detail (`TODO  in  src`). */
export const SEARCH_SEPARATOR = '  in  ';

/** Input keys that name a file. */
const PATH_KEYS = ['file_path', 'path', 'notebook_path'] as const;
const FILE = ['file_path', 'path'] as const;

const search = (input: ToolInput) => [str(input.pattern), str(input.path)].filter(Boolean).join(SEARCH_SEPARATOR);
const todoCount = (input: ToolInput) => (Array.isArray(input.todos) ? `${input.todos.length} items` : '');

const CLAUDE_CODE_TOOLS: Record<string, ToolShape> = {
  Bash: { verb: 'Bash', detail: ['command', 'description'], firstLine: true, foldInput: true },
  Read: { verb: 'Read', detail: FILE, foldInput: true },
  Edit: { verb: 'Edit', detail: FILE, foldInput: true, edit: { kind: 'replace', oldKey: 'old_string', newKey: 'new_string' } },
  MultiEdit: { verb: 'Edit', detail: FILE, foldInput: true, edit: { kind: 'edits', oldKey: 'old_string', newKey: 'new_string' } },
  Write: { verb: 'Write', detail: FILE, foldInput: true, edit: { kind: 'write', contentKey: 'content' } },
  NotebookEdit: { verb: 'Edit notebook', detail: ['notebook_path'], foldInput: true },
  Glob: { verb: 'Find files', detail: ['pattern'], foldInput: true },
  Grep: { verb: 'Search', detail: search, foldInput: true },
  WebFetch: { verb: 'Fetch', detail: ['url'], foldInput: true },
  WebSearch: { verb: 'Web search', detail: ['query'], foldInput: true },
  Task: { verb: 'Sub-agent', detail: ['description', 'prompt'], firstLine: true },
  Agent: { verb: 'Sub-agent', detail: ['description', 'prompt'], firstLine: true },
  TodoWrite: { verb: 'Update todo list', detail: todoCount },
};

const COPILOT_CLI_TOOLS: Record<string, ToolShape> = {
  bash: { verb: 'Bash', detail: ['command', 'description'], firstLine: true, foldInput: true },
  read_bash: { verb: 'Read shell', detail: ['shellId'] },
  write_bash: { verb: 'Write to shell', detail: ['input', 'shellId'], firstLine: true },
  stop_bash: { verb: 'Stop shell', detail: ['shellId'] },
  view: { verb: 'Read', detail: ['path'], foldInput: true },
  edit: { verb: 'Edit', detail: ['path'], foldInput: true, edit: { kind: 'replace', oldKey: 'old_str', newKey: 'new_str' } },
  str_replace: { verb: 'Edit', detail: ['path'], edit: { kind: 'replace', oldKey: 'old_str', newKey: 'new_str' } },
  create: { verb: 'Write', detail: ['path'], foldInput: true, edit: { kind: 'write', contentKey: 'file_text' } },
  grep: { verb: 'Search', detail: search, foldInput: true },
  rg: { verb: 'Search', detail: search },
  glob: { verb: 'Find files', detail: ['pattern'], foldInput: true },
  web_fetch: { verb: 'Fetch', detail: ['url'], foldInput: true },
  task: { verb: 'Sub-agent', detail: ['description', 'prompt'], firstLine: true },
};

/** Every recognised tool by its exact name (the two harnesses' names never collide). */
const TOOLS = new Map<string, ToolShape>([...Object.entries(CLAUDE_CODE_TOOLS), ...Object.entries(COPILOT_CLI_TOOLS)]);

/** For a tool not in `TOOLS`: the first of these keys with a value is the detail. */
const FALLBACK_DETAIL_KEYS = ['command', 'file_path', 'path', 'pattern', 'query', 'url', 'description'];

function str(value: Json | undefined): string {
  return typeof value === 'string' ? value : '';
}

/** The call's input when it is an object of named fields. */
function inputFields(input: Json): ToolInput | null {
  return input && typeof input === 'object' && !Array.isArray(input) ? (input as ToolInput) : null;
}

function firstLineOf(text: string): string {
  return truncate(text.split('\n')[0].trim(), 140);
}

/** One-line description of a tool call: `Bash  npm test`, `Edit  src/x.ts`. */
export function toolSummary(tool: Pick<Tool, 'name' | 'input'>): { verb: string; detail: string } {
  const input = inputFields(tool.input) ?? {};
  const name = tool.name || 'tool';
  const shape = TOOLS.get(name);
  if (shape) {
    const detail = typeof shape.detail === 'function' ? shape.detail(input) : (shape.detail.map((key) => str(input[key])).find(Boolean) ?? '');
    return { verb: shape.verb, detail: shape.firstLine ? firstLineOf(detail) : detail };
  }
  const key = FALLBACK_DETAIL_KEYS.find((k) => str(input[k]));
  if (key) return { verb: name, detail: firstLineOf(str(input[key])) };
  if (typeof tool.input === 'string') return { verb: name, detail: firstLineOf(tool.input) };
  return { verb: name, detail: '' };
}

/** Whether the tool's one-line summary already says what went in (its card folds the input). */
export function isSummarisedTool(name: string): boolean {
  return TOOLS.get(name)?.foldInput === true;
}

/** The file paths a tool call names, for finding the workspace they share. */
export function toolPaths(tool: Pick<Tool, 'input'> | undefined): string[] {
  const input = tool ? inputFields(tool.input) : null;
  return input ? PATH_KEYS.map((key) => input[key]).filter((p): p is string => typeof p === 'string') : [];
}

/** Diff text for an edit-like call, so edits read like changes, not JSON. */
export function editPreview(tool: Pick<Tool, 'name' | 'input'>): string | null {
  const input = inputFields(tool.input);
  const edit = TOOLS.get(tool.name)?.edit;
  if (!input || !edit) return null;
  switch (edit.kind) {
    case 'replace': {
      const before = input[edit.oldKey];
      const after = input[edit.newKey];
      return typeof before === 'string' && typeof after === 'string' ? diffLines(before, after).join('\n') : null;
    }
    case 'edits':
      if (!Array.isArray(input.edits)) return null;
      return input.edits
        .map((item) => {
          const one = item as ToolInput;
          return diffLines(str(one[edit.oldKey]), str(one[edit.newKey])).join('\n');
        })
        .join('\n@@\n');
    case 'write': {
      const content = input[edit.contentKey];
      return typeof content === 'string'
        ? content
            .split('\n')
            .map((line) => `+${line}`)
            .join('\n')
        : null;
    }
  }
}

/** The tool call Zeroshot's harness uses to collect the node's result, not part of the work
 * (`RESULT_TOOL`, from projection.rs). */
export function isResultTool(name: string | undefined): boolean {
  return name === RESULT_TOOL;
}

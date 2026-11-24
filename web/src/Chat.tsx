/**
 * The node chat: one execution's transcript as a conversation (messages, reasoning, tool calls
 * folded into groups, notices), the outcome it ended with, and while it runs, the composer.
 * What each item shows is worked out in transcript-model.ts; this file is the markup.
 */
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject, type UIEvent } from 'react';
import { AlertTriangle, ArrowDown, ChevronDown, ChevronRight, CircleAlert, Info, LoaderCircle, User, X } from 'lucide-react';
import { api } from './api';
import type { Execution, Json, RunView, TranscriptItem, TranscriptResponse } from './types';
import {
  CHAT_PAGE,
  IDLE_AFTER_MS,
  chatEntries,
  commonDir,
  editPreview,
  firstLine,
  hasNativePrompt,
  isNotice,
  lastSeen,
  lastThinkingIndex,
  noticeClass,
  returnedJson,
  splitZeroshotPrompt,
  synthesizedPrompt,
  toolCardView,
  toolGroupSummary,
  toolPaths,
  visibleRange,
  type ChatEntry,
} from './transcript-model';
import { clockTime, duration, prettyJson } from './format';
import { diagnosticText, nodeState } from './status';
import { nodeHref, visitOf } from './route';
import { errorWords } from './Timeline';
import { Markdown } from './Markdown';
import { ErrorBox, LongText, Skeleton } from './common';
import { atBottom, useNow, useResource } from './hooks';
import { Composer } from './Composer';
import { pendingSent, steerBody, steerTag, steerTagLabel, type Sent } from './steer';

// The steer helpers lived here first; their tests and callers still import them from Chat.
export {
  pendingSent,
  steerBody,
  steerScope,
  steerScopeNote,
  steerStatus,
  steerStatusText,
  steerTag,
  steerTagLabel,
  type Sent,
  type SteerTone,
} from './steer';

/**
 * A message sent just before the node finished can race the ledger write that records it. Once
 * the node is done nothing refetches the transcript, so the chat would show such a message as
 * "sent" forever; instead it reloads a few times, this far apart, while one is still missing.
 */
const STEER_CONFIRM_RETRIES = 5;
const STEER_CONFIRM_DELAY_MS = 2_000;

export function Chat({ view, execution, refreshKey }: { view: RunView; execution: Execution; refreshKey: string }) {
  const key = `${view.target}/${view.runId}/${execution.id}`;
  const resource = useResource<TranscriptResponse>(key, (signal) => api.transcript(view.target, view.runId, execution.id, signal));
  const [sent, setSent] = useState<Sent[]>([]);
  const running = execution.state === 'running';
  useReloadOnChange(refreshKey, resource.reload);
  const unconfirmed = !running && !!resource.data && pendingSent(sent, resource.data.transcript.items).length > 0;
  useRetryWhile(unconfirmed, resource.data, resource.reload, STEER_CONFIRM_RETRIES, STEER_CONFIRM_DELAY_MS);
  return (
    <div className="chat">
      {resource.loading && !resource.data && (
        <div className="chat-scroll">
          <Skeleton lines={10} />
        </div>
      )}
      {resource.error && !resource.data && (
        <div className="chat-scroll">
          <ErrorBox title="Could not load this node's conversation" message={resource.error} onRetry={resource.reload} />
        </div>
      )}
      {resource.data && <ChatBody key={key} data={resource.data} execution={execution} view={view} live={running} sent={sent} />}
      {resource.error && resource.data && <p className="chat-refresh-error">Could not refresh: {resource.error}</p>}
      {running && (
        <Composer
          view={view}
          execution={execution}
          onSent={(message) => {
            if (message) setSent((list) => [...list, message]);
            resource.reload();
          }}
        />
      )}
    </div>
  );
}

/** Calls `reload` when `key` changes, not on mount (the first load is already under way). */
function useReloadOnChange(key: string, reload: () => void) {
  const mounted = useRef(false);
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true;
      return;
    }
    reload();
  }, [key, reload]);
}

/**
 * Calls `reload` after `delayMs` while `waiting`, at most `tries` times in all. `answer` is what
 * the last reload brought; each new answer that still leaves it waiting schedules the next try.
 */
function useRetryWhile(waiting: boolean, answer: unknown, reload: () => void, tries: number, delayMs: number) {
  const tried = useRef(0);
  useEffect(() => {
    if (!waiting || tried.current >= tries) return;
    tried.current += 1;
    const id = window.setTimeout(reload, delayMs);
    return () => window.clearTimeout(id);
  }, [waiting, answer, reload, tries, delayMs]);
}

/** The run a reused execution came from (a replay's or resume's original). */
export function originalRun(view: Pick<RunView, 'replayOf' | 'resumedFrom'>, execution?: Pick<Execution, 'reusedFrom'>): string | null {
  return execution?.reusedFrom ?? (typeof view.replayOf?.runId === 'string' ? view.replayOf.runId : null) ?? view.resumedFrom ?? null;
}

/**
 * Keeps a log scrolled to its newest row while the reader is at the bottom (`pinned`; a live log
 * opens there). Once they scroll up, rows added below are counted in `unseen` instead, and `jump`
 * scrolls back down.
 */
function useStickToBottom(scroller: RefObject<HTMLDivElement | null>, rows: number, live: boolean) {
  const [pinned, setPinned] = useState(live);
  const [unseen, setUnseen] = useState(0);
  const lastRows = useRef(rows);
  const opensAtBottom = useRef(live);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && opensAtBottom.current) el.scrollTop = el.scrollHeight;
  }, [scroller]);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const added = rows - lastRows.current;
    lastRows.current = rows;
    if (added <= 0) return;
    if (pinned) el.scrollTop = el.scrollHeight;
    else setUnseen((n) => n + added);
  }, [scroller, rows, pinned]);

  const onScroll = (e: UIEvent<HTMLElement>) => {
    const bottom = atBottom(e.currentTarget);
    if (bottom !== pinned) setPinned(bottom);
    if (bottom) setUnseen(0);
  };
  const jump = () => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: 'smooth' });
    setUnseen(0);
  };
  return { pinned, unseen, onScroll, jump };
}

function ChatBody({ data, execution, view, live, sent }: { data: TranscriptResponse; execution: Execution; view: RunView; live: boolean; sent: Sent[] }) {
  const items = data.transcript.items;
  const entries = useMemo(() => chatEntries(items), [items]);
  const [count, setCount] = useState(CHAT_PAGE);
  const { start } = visibleRange(entries.length, count);
  const latestThinking = useMemo(() => lastThinkingIndex(items), [items]);
  const workspace = useMemo(() => view.local?.workspace ?? commonDir(items.flatMap((i) => toolPaths(i.tool))), [items, view.local?.workspace]);
  const waiting = pendingSent(sent, items);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useStickToBottom(scroller, items.length + waiting.length, live);
  const prompt = hasNativePrompt(items) ? null : synthesizedPrompt(data.instructions, execution.input);

  return (
    <>
      <div className="chat-scroll" ref={scroller} tabIndex={0} role="log" aria-label={`Conversation of ${execution.node}`} onScroll={stick.onScroll}>
        {!live && <OutcomeCard execution={execution} view={view} />}
        {data.transcript.limitations.length > 0 && (
          <p className="chat-caveat">
            <Info size={12} aria-hidden /> {data.transcript.limitations.join(' · ')}
          </p>
        )}
        {prompt && !execution.reused && <SynthesizedPrompt prompt={prompt} />}
        {start > 0 && (
          <button type="button" className="text-button show-earlier" onClick={() => setCount(count + CHAT_PAGE)}>
            Show {Math.min(CHAT_PAGE, start)} earlier entries ({start} hidden)
          </button>
        )}
        {entries.slice(start).map((entry) => (
          <Entry key={entry.key} entry={entry} latestThinking={latestThinking} live={live} workspace={workspace} />
        ))}
        {waiting.map((s, i) => (
          <SteerMessage key={`sent-${i}`} text={s.text} tag={steerTagLabel(s.status, execution.harness)} title={clockTime(s.at, true)} chipTitle={s.reason ?? undefined} pending />
        ))}
        {items.length === 0 && !execution.reused && <p className="chat-empty">{live ? 'The agent has not written anything yet.' : 'Nothing was recorded for this execution.'}</p>}
        {live && <Typing node={execution.node} seen={lastSeen(items, execution.lastActivityAt)} />}
      </div>
      {live && !stick.pinned && (
        <button type="button" className="jump-latest" onClick={stick.jump}>
          <ArrowDown size={13} aria-hidden /> {stick.unseen > 0 ? `${stick.unseen} new` : 'Jump to latest'}
        </button>
      )}
    </>
  );
}

/** The live node's last line: working, or how long it has been quiet. */
function Typing({ node, seen }: { node: string; seen: number | null }) {
  const now = useNow();
  const quiet = seen != null ? now - seen : 0;
  const idle = quiet >= IDLE_AFTER_MS;
  return (
    <div className={`typing ${idle ? 'idle' : ''}`} aria-live="polite">
      {idle ? (
        <>No output for {duration(quiet)}</>
      ) : (
        <>
          <LoaderCircle size={13} className="spin" aria-hidden /> {node} is working…
        </>
      )}
    </div>
  );
}

/** A message to the agent: recorded in the transcript, or sent from here and not recorded yet (`pending`). */
function SteerMessage({ text, tag, title, chipTitle, pending = false }: { text: string; tag: { text: string; bad?: boolean }; title?: string; chipTitle?: string; pending?: boolean }) {
  return (
    <article className={pending ? 'msg steer pending' : 'msg steer'} title={title}>
      <header className="msg-head">
        <User size={13} aria-hidden /> You{' '}
        <span className={`sent-mark ${tag.bad ? 'tone-text-bad' : ''}`} title={chipTitle}>
          {tag.text}
        </span>
      </header>
      <p className="prewrap">{text}</p>
    </article>
  );
}

/** A disclosure that reads as one line: a label and a muted preview. */
function Disclosure({ label, preview, className = '', open, children }: { label: React.ReactNode; preview?: string; className?: string; open?: boolean; children: React.ReactNode }) {
  return (
    <details className={`fold ${className}`} open={open}>
      <summary>
        <ChevronRight size={12} className="fold-chevron" aria-hidden />
        <span className="fold-label">{label}</span>
        {preview && <span className="fold-preview">{preview}</span>}
      </summary>
      <div className="fold-body">{children}</div>
    </details>
  );
}

function InputFields({ input }: { input: Json | undefined }) {
  const prompt = synthesizedPrompt(null, input);
  const empty = (v: Json) => v === '' || v == null || (typeof v === 'object' && !Array.isArray(v) && !Object.keys(v).length);
  const other = prompt.otherFields ? Object.fromEntries(Object.entries(prompt.otherFields).filter(([, v]) => !empty(v))) : null;
  return (
    <>
      {prompt.textFields.map((f) => (
        <section key={f.name} className="prompt-part">
          <h4>{f.name}</h4>
          <Markdown text={f.text} />
        </section>
      ))}
      {other && Object.keys(other).length > 0 && <pre className="json small">{prettyJson(other)}</pre>}
    </>
  );
}

/** The prompt as Zeroshot sent it: the author's instructions and the task first, the frame folded. */
function NativePrompt({ text, subagent }: { text: string; subagent: boolean }) {
  const split = subagent ? null : splitZeroshotPrompt(text);
  const preview = split ? firstLine(split.authored) || firstLine(text) : firstLine(text);
  return (
    <Disclosure label={subagent ? 'Sub-agent prompt' : 'Prompt'} preview={preview} className="prompt-card">
      {split ? (
        <>
          <section className="prompt-part">
            <h4>Instructions</h4>
            <p className="prewrap">{split.authored || '(none)'}</p>
          </section>
          <InputFields input={split.input} />
          <Disclosure label="Full prompt" className="nested">
            <p className="prewrap small">{text}</p>
          </Disclosure>
        </>
      ) : (
        <p className="prewrap">{text}</p>
      )}
    </Disclosure>
  );
}

function SynthesizedPrompt({ prompt }: { prompt: ReturnType<typeof synthesizedPrompt> }) {
  const preview = firstLine(prompt.instructions) || firstLine(prompt.textFields[0]?.text);
  return (
    <Disclosure label="Prompt" preview={preview} className="prompt-card">
      <p className="muted small">Instructions and input; the exact text is not recorded.</p>
      {prompt.instructions && (
        <section className="prompt-part">
          <h4>Instructions</h4>
          <p className="prewrap">{prompt.instructions}</p>
        </section>
      )}
      <InputFields input={prompt.raw} />
      {!prompt.textFields.length && !prompt.otherFields && <pre className="json small">{prettyJson(prompt.raw ?? null)}</pre>}
    </Disclosure>
  );
}

const Entry = memo(function Entry({ entry, latestThinking, live, workspace }: { entry: ChatEntry; latestThinking: number; live: boolean; workspace: string | null }) {
  if (entry.kind === 'tools') return <ToolGroup items={entry.items} latestThinking={latestThinking} live={live} workspace={workspace} />;
  return <Item item={entry.item} index={entry.index} latestThinking={latestThinking} live={live} workspace={workspace} />;
});

/** Reasoning this short reads inline; longer reasoning is a one-line disclosure. */
const SHORT_THOUGHT = 220;

function Item({ item, index, latestThinking, live, workspace }: { item: TranscriptItem; index: number; latestThinking: number; live: boolean; workspace: string | null }) {
  const when = item.at ? clockTime(item.at, true) : undefined;
  const sub = item.subagent ? (
    <span className="sub-tag" title="A sub-agent started by this node's agent (Task tool)">
      sub-agent
    </span>
  ) : null;
  switch (item.kind) {
    case 'prompt':
      return <NativePrompt text={item.text ?? ''} subagent={!!item.subagent} />;
    case 'text': {
      const json = returnedJson(item.text ?? '');
      if (json != null)
        return (
          <Disclosure label="Returned JSON" className="quiet">
            <pre className="json small">{json}</pre>
          </Disclosure>
        );
      return (
        <article className={`msg assistant ${item.subagent ? 'subagent' : ''}`} title={when}>
          {sub}
          <Markdown text={item.text ?? ''} />
        </article>
      );
    }
    case 'thinking': {
      const text = item.text ?? '';
      if (text.length <= SHORT_THOUGHT && !text.includes('\n'))
        return (
          <p className={`thought ${item.subagent ? 'subagent' : ''}`} title={when}>
            {sub}
            {text}
          </p>
        );
      return (
        <Disclosure label="Reasoning" preview={firstLine(text, 90)} className={`thinking ${item.subagent ? 'subagent' : ''}`} open={index === latestThinking}>
          <div className="thinking-text">{text}</div>
        </Disclosure>
      );
    }
    case 'tool':
      return item.tool ? <ToolCard item={item} live={live} workspace={workspace} /> : null;
    case 'compaction':
      // The provider summarised the conversation to free context; the summary is what it kept.
      return item.detail ? (
        <Disclosure label={item.text ?? 'Context compacted'} className="quiet compaction">
          <div className="thinking-text">{item.detail}</div>
        </Disclosure>
      ) : (
        <p className={`notice ${noticeClass('compaction')}`} title={when}>
          <Info size={12} aria-hidden />
          <span className="notice-text">{item.text}</span>
        </p>
      );
    case 'steer': {
      const text = item.text ?? '';
      return <SteerMessage text={steerBody(text)} tag={steerTagLabel(steerTag(text))} title={when} />;
    }
    case 'auto':
      return (
        <p className={`notice ${noticeClass('auto')}`} title={`${item.text ?? ''}${when ? `\n${when}` : ''}`}>
          <span className="notice-who">Zeroshot:</span> <span className="notice-text">{item.text}</span>
        </p>
      );
    case 'error':
      return (
        <article className="msg error" title={when}>
          <header className="msg-head">
            <CircleAlert size={13} aria-hidden /> Error
          </header>
          <pre>{item.text}</pre>
        </article>
      );
    default:
      if (isNotice(item.kind))
        return (
          <p className={`notice ${noticeClass(item.kind)}`} title={when}>
            {item.kind === 'rate_limit' || item.kind === 'retry' ? <AlertTriangle size={12} aria-hidden /> : <Info size={12} aria-hidden />}
            <span className="notice-text">{item.text}</span>
          </p>
        );
      return (
        <p className="notice" title={when}>
          <span className="notice-text">
            {item.kind}: {item.text}
          </span>
        </p>
      );
  }
}

function ToolGroup({ items, latestThinking, live, workspace }: { items: { item: TranscriptItem; index: number }[]; latestThinking: number; live: boolean; workspace: string | null }) {
  const [open, setOpen] = useState(false);
  const { calls, errors, verbs } = toolGroupSummary(items);
  return (
    <div className={`tool-group ${open ? 'open' : ''}`}>
      <button type="button" className="tool-group-head" aria-expanded={open} onClick={() => setOpen(!open)} title={verbs}>
        {open ? <ChevronDown size={12} aria-hidden /> : <ChevronRight size={12} aria-hidden />}
        <span>{calls} tool calls</span>
        {errors > 0 && <span className="muted">· {errors === 1 ? '1 error' : `${errors} errors`}</span>}
      </button>
      {open && (
        <div className="tool-group-body">
          {/* Short reasoning between tool calls doesn't break the group; it's part of this body. */}
          {items.map(({ item, index }) =>
            item.kind === 'tool' && item.tool ? (
              <ToolCard key={index} item={item} live={live} workspace={workspace} />
            ) : (
              <Item key={index} item={item} index={index} latestThinking={latestThinking} live={live} workspace={workspace} />
            )
          )}
        </div>
      )}
    </div>
  );
}

function ToolCard({ item, live, workspace }: { item: TranscriptItem; live: boolean; workspace: string | null }) {
  const tool = item.tool!;
  const [open, setOpen] = useState(false);
  const card = toolCardView(item, tool, live, workspace);
  const result = tool.result;
  const preview = open ? editPreview(tool) : null;
  const input = <pre className="json small">{card.input}</pre>;
  return (
    <article className={`tool-card status-${card.status} ${card.resultTool ? 'result-tool' : ''} ${item.subagent ? 'subagent' : ''}`}>
      <button type="button" className="tool-head" aria-expanded={open} onClick={() => setOpen(!open)} title={card.title}>
        {open ? <ChevronDown size={12} aria-hidden /> : <ChevronRight size={12} aria-hidden />}
        <span className="tool-verb">{card.verb}</span>
        <code className={card.cut ? 'tool-detail cut' : 'tool-detail'}>{card.detail}</code>
        {card.cut && <span className="tool-note tool-cut">{card.cut}</span>}
        {item.subagent && <span className="sub-tag">sub-agent</span>}
        {card.exit && (
          <span className="tool-note" title="The command exited with this status">
            {card.exit}
          </span>
        )}
        {card.denied && <span className="tool-note tone-text-bad">{card.denied}</span>}
        {card.status === 'error' && (
          <span className="tool-status" title="The tool returned an error">
            <X size={12} aria-label="error" />
          </span>
        )}
        {card.status === 'pending' && (
          <span className="tool-status" title="Waiting for the result">
            <LoaderCircle size={12} className="spin" aria-hidden />
          </span>
        )}
      </button>
      {open && (
        <div className="tool-body">
          {preview ? <EditPreview diff={preview} /> : null}
          {card.foldInput ? (
            <Disclosure label="Input" className="nested">
              {input}
            </Disclosure>
          ) : (
            input
          )}
          {card.permission && <p className="muted small">{card.permission}</p>}
          {result ? (
            <LongText text={result.content || '(empty)'} className={result.isError ? 'tool-error' : ''} maxLines={30} />
          ) : (
            <p className="muted small">{live ? 'Waiting for the result…' : 'No result recorded.'}</p>
          )}
        </div>
      )}
    </article>
  );
}

/** An edit call's diff text (`editPreview`), added and removed lines marked. */
function EditPreview({ diff }: { diff: string }) {
  return (
    <pre className="edit-preview">
      {diff.split('\n').map((line, i) => (
        <span key={i} className={line.startsWith('+') ? 'add' : line.startsWith('-') ? 'del' : ''}>
          {line}
          {'\n'}
        </span>
      ))}
    </pre>
  );
}

/** What the chat leads with once a node is done (see `outcomeOf`). */
type Outcome =
  /** One line of text, with a link after it. */
  | { kind: 'line'; text: string; link?: { href: string; label: string } }
  /** A bordered card: an optional title, then the feedback. */
  | { kind: 'card'; tone: '' | 'culprit' | 'warn-culprit'; label: string; title: string | null; feedback: string }
  /** The feedback, folded to its first line. */
  | { kind: 'fold'; feedback: string };

/**
 * What the chat leads with when the node is done, only when it says more than the header's
 * state: the reviewer's feedback (an acceptance folded to its first line), the error, why it
 * stopped, or where a reused execution's conversation is.
 */
function outcomeOf(execution: Execution, view: RunView): Outcome | null {
  const state = nodeState(execution.state);
  if (state === 'running') return null;
  const feedback = diagnosticText(execution.diagnostic).trim();
  const isFailureNode = view.failure?.execution === execution.id;
  if (execution.reused) {
    const original = originalRun(view, execution);
    return original
      ? { kind: 'line', text: 'Reused from the original run. ', link: { href: nodeHref(view.target, original, execution.node, visitOf(execution)), label: 'Open its conversation ›' } }
      : { kind: 'line', text: 'Reused from the original run.' };
  }
  // Red stays reserved for an actual failure: this node decided the run's outcome, but
  // "Rejected" is amber everywhere else on the page too.
  if (state === 'rejected' && feedback) return { kind: 'card', tone: isFailureNode ? 'warn-culprit' : '', label: 'Feedback', title: null, feedback };
  if (state === 'failed') {
    // The panel header already names this node and, for a timeout, its duration; the card adds
    // only why it failed, not a repeat of either.
    const words = errorWords(execution.error);
    const why = words ? words[0].toUpperCase() + words.slice(1) : isFailureNode && view.failure ? view.failure.cause : 'Failed';
    return { kind: 'card', tone: 'culprit', label: 'Outcome', title: why, feedback };
  }
  if (state === 'interrupted') return { kind: 'line', text: view.status === 'stopped' ? 'Stopped while running.' : 'The run stopped while it was working.' };
  if (state === 'voided') return { kind: 'line', text: `Voided by Zeroshot${execution.voidReason ? `: ${execution.voidReason}` : '.'}` };
  return feedback ? { kind: 'fold', feedback } : null;
}

function OutcomeCard({ execution, view }: { execution: Execution; view: RunView }) {
  const outcome = outcomeOf(execution, view);
  switch (outcome?.kind) {
    case undefined:
      return null;
    case 'line':
      return (
        <p className="outcome-line">
          {outcome.text}
          {outcome.link && <a href={outcome.link.href}>{outcome.link.label}</a>}
        </p>
      );
    case 'card':
      return (
        <section className={`outcome ${outcome.tone}`} aria-label={outcome.label}>
          {outcome.title != null && <p className="outcome-title">{outcome.title}</p>}
          {outcome.feedback && <Markdown text={outcome.feedback} className="outcome-feedback" />}
        </section>
      );
    case 'fold':
      return (
        <Disclosure label="Feedback" preview={firstLine(outcome.feedback, 140)} className="outcome-fold">
          <Markdown text={outcome.feedback} className="outcome-feedback" />
        </Disclosure>
      );
  }
}

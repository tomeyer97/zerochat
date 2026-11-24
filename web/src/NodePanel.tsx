import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Info } from 'lucide-react';
import { api } from './api';
import type { Execution, GraphNodeInfo, RunView, TranscriptResponse } from './types';
import { execStateInfo, executionLabel, nodeState, stateMark } from './status';
import { clockTime, compact, costText, duration, elapsedMs, fullTime, laneText, plural, prettyJson, timeOfDay, tokenParts, totalTokens, usageBreakdown, type LaneParts } from './format';
import { Chat, originalRun } from './Chat';
import { FilesView } from './Files';
import { executionFilesChanged } from './diff';
import { CopyButton, StatePill, tabListKeyDown } from './common';
import { SidePanel } from './components/SidePanel';
import { useNow, useResource } from './hooks';
import { nodeHref, visitOf } from './route';
import { copilotCreditsUsd, harnessLabel, nodeExecutions, nodeFacts, nodeLoop, type PanelFact } from './run-model';

// Kept here for existing imports; the rule lives in run-model.ts.
export { nodeExecutions } from './run-model';

type Tab = 'chat' | 'files' | 'details';

// Text-only: a decorative icon next to a label that already says the same thing adds nothing.
const tabs: { id: Tab; label: string }[] = [
  { id: 'chat', label: 'Chat' },
  { id: 'files', label: 'Files' },
  { id: 'details', label: 'Details' },
];

export function NodePanel({ view, node, execution, onClose, onSelect }: { view: RunView; node: string; execution?: number; onClose: () => void; onSelect: (execution: Execution) => void }) {
  const [tab, setTab] = useState<Tab>('chat');
  const { list, current } = useMemo(() => nodeExecutions(view, node, execution), [view, node, execution]);
  // Tabs already visited for this execution stay mounted (hidden, not removed) so flipping
  // Details <-> Chat doesn't re-fetch the transcript or reset its scroll position each time.
  const [visitedTabs, setVisitedTabs] = useState<Set<Tab>>(() => new Set([tab]));
  useEffect(() => setVisitedTabs(new Set([tab])), [current?.id]);
  const selectTab = (id: Tab) => {
    setTab(id);
    setVisitedTabs((prev) => (prev.has(id) ? prev : new Set(prev).add(id)));
  };
  const info = view.graphNodes.find((n) => n.name === node);
  const running = current?.state === 'running';
  const now = useNow(running);
  const refreshKey = current ? (running ? `live-${view.cursor}` : `done-${current.endSeq}`) : '';
  const elapsed = current ? elapsedMs({ live: running, startedAt: current.startedAt, recorded: current.durationMs }, now) : null;
  const label = `Node ${node}`;
  if (!info && !list.length) {
    return <SidePanel label={label} title={node} onClose={onClose} header={<p className="muted small panel-facts">No such node in this run.</p>} />;
  }
  if (!current) {
    return (
      <SidePanel label={label} title={node} onClose={onClose} titleExtra={<span className="muted small">not started</span>}>
        <div className="panel-body">
          <div className="panel-pad">
            <Details view={view} execution={undefined} node={node} />
          </div>
        </div>
      </SidePanel>
    );
  }
  const original = current.reused ? originalRun(view, current) : null;
  const facts = withLane(
    nodeFacts({
      current,
      info,
      visits: list.length,
      elapsed,
      originalHref: original ? nodeHref(view.target, original, node, visitOf(current)) : null,
    }),
    nodeLane(view, node, current),
    info?.effort
  );
  const visibleTabs = tabs.filter((t) => t.id !== 'files' || executionFilesChanged(current));
  const panel = (id: Tab, className: string, body: ReactNode) =>
    visitedTabs.has(id) && (
      <div id={`panel-tabpanel-${id}`} role="tabpanel" aria-labelledby={`panel-tab-${id}`} className={className} hidden={tab !== id}>
        {body}
      </div>
    );
  const header = (
    <>
      <p className="panel-facts">
        {facts.map((f) => (
          <Fact key={f.key} fact={f} />
        ))}
      </p>
      {list.length > 1 && <RoundPicker view={view} node={node} list={list} current={current} onSelect={onSelect} />}
      <div className="tabs" role="tablist" aria-label="Node views" onKeyDown={(e) => tabListKeyDown(e, visibleTabs.length, (i) => selectTab(visibleTabs[i].id))}>
        {visibleTabs.map(({ id, label }, i) => (
          <button
            key={id}
            id={`panel-tab-${id}`}
            type="button"
            role="tab"
            aria-selected={tab === id}
            aria-controls={`panel-tabpanel-${id}`}
            tabIndex={tab === id ? 0 : -1}
            data-tab-index={i}
            className="tab"
            onClick={() => selectTab(id)}
          >
            {label}
          </button>
        ))}
      </div>
    </>
  );
  return (
    <SidePanel label={label} title={node} onClose={onClose} titleExtra={<StatePill state={nodeState(current.state)} />} header={header}>
      <div id="node-tabpanel" className={`panel-body tab-${tab}`}>
        {panel('chat', 'tabpanel-fill', <Chat key={current.id} view={view} execution={current} refreshKey={refreshKey} />)}
        {panel('files', 'panel-pad', <FilesView key={current.id} view={view} execution={current} />)}
        {panel('details', 'panel-pad', <Details view={view} execution={current} node={node} />)}
      </div>
    </SidePanel>
  );
}

/** The lane a node's execution ran on (else the one its node is bound to): harness and model from
 * the execution, the provider from the node's lane (the API keeps it per node). */
export function nodeLane(view: Pick<RunView, 'graphNodes' | 'lanes'>, node: string, execution: Pick<Execution, 'harness' | 'model'> | undefined): LaneParts {
  const info = view.graphNodes.find((n) => n.name === node);
  const lane = view.lanes?.find((l) => l.nodes.includes(node));
  return {
    harness: execution?.harness ?? info?.harness ?? lane?.harness,
    provider: info?.provider ?? lane?.provider,
    model: execution?.model ?? info?.model ?? lane?.model,
  };
}

/** The header's first fact is the node's lane with the exact model id (`laneText`), and its effort. */
export function withLane(facts: PanelFact[], lane: LaneParts, effort: string | null | undefined): PanelFact[] {
  const text = `${laneText(lane)}${effort ? ` · ${effort}` : ''}`;
  return facts.map((f) => (f.key === 'm' ? { ...f, text, title: 'Agent CLI (harness) · provider · model' } : f));
}

function Fact({ fact }: { fact: PanelFact }) {
  return (
    <span className={fact.bad ? 'tone-text-bad' : undefined} title={fact.title}>
      {fact.text}
      {fact.link && (
        <>
          {' · '}
          <a href={fact.link.href}>{fact.link.label}</a>
        </>
      )}
      {fact.suffix && <span className="muted">{fact.suffix}</span>}
    </span>
  );
}

/** One tab per visit of the node (round or attempt), marked with its state. */
function RoundPicker({ view, node, list, current, onSelect }: { view: RunView; node: string; list: Execution[]; current: Execution; onSelect: (execution: Execution) => void }) {
  return (
    <div className="round-picker" role="tablist" aria-label="Visits of this node" onKeyDown={(e) => tabListKeyDown(e, list.length, (i) => onSelect(list[i]))}>
      {list.map((e, i) => {
        const s = nodeState(e.state);
        const selected = e.id === current.id;
        return (
          <a
            key={e.id}
            id={`round-tab-${e.id}`}
            role="tab"
            aria-selected={selected}
            aria-controls="node-tabpanel"
            tabIndex={selected ? 0 : -1}
            data-tab-index={i}
            className={`round-tab state-${s}`}
            href={nodeHref(view.target, view.runId, node, visitOf(e))}
            title={`${executionLabel(e)} · ${execStateInfo[s].label}${e.startedAt ? ` · ${clockTime(e.startedAt, true)}` : ''}`}
            onClick={(ev) => {
              ev.preventDefault();
              onSelect(e);
            }}
          >
            {executionLabel(e)}{' '}
            <span className="round-mark" aria-hidden="true">
              {stateMark(s)}
            </span>
            <span className="visually-hidden"> {execStateInfo[s].label}</span>
          </a>
        );
      })}
    </div>
  );
}

function JsonBlock({ title, value }: { title: string; value: unknown }) {
  const text = prettyJson(value);
  return (
    <section className="io-block">
      <div className="io-head">
        <h3>{title}</h3>
        <CopyButton text={text} label={`Copy ${title.toLowerCase()}`} />
      </div>
      <pre className="json">{text}</pre>
    </section>
  );
}

/** What went in and came out, as JSON (the chat shows the same in words). */
function RawJson({ execution }: { execution: Execution }) {
  const blocks = [
    { title: 'Input', value: execution.input },
    { title: 'Output', value: execution.output },
    { title: 'Outcome', value: execution.outcome },
  ];
  return (
    <section>
      <h3>Raw</h3>
      {blocks.map(
        ({ title, value }) =>
          value != null && (
            <details key={title} className="io-raw">
              <summary>{title} JSON</summary>
              <JsonBlock title={title} value={value} />
            </details>
          )
      )}
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </>
  );
}

/** The Details tab: this execution's facts and raw JSON (once it ran), then the node's definition. */
function Details({ view, execution, node }: { view: RunView; execution: Execution | undefined; node: string }) {
  const info = view.graphNodes.find((n) => n.name === node);
  return (
    <div className="details">
      {execution && <ExecutionFacts view={view} execution={execution} info={info} />}
      {execution && <RawJson execution={execution} />}
      <NodeDefinition view={view} info={info} started={!!execution} />
    </div>
  );
}

function ExecutionFacts({ view, execution, info }: { view: RunView; execution: Execution; info: GraphNodeInfo | undefined }) {
  const transcript = useResource<TranscriptResponse>(`${view.target}/${view.runId}/${execution.id}/details`, (signal) => api.transcript(view.target, view.runId, execution.id, signal));
  const t = transcript.data;
  const mapIndices = Array.isArray(execution.mapIndices) && execution.mapIndices.length > 0 ? execution.mapIndices : null;
  const snapshots = execution.workspaceStart?.commit || execution.workspaceEnd?.commit;
  // The session id is worth its own row only when the log path does not already show it.
  const sessionId = execution.sessionId && !(t?.transcript.sourcePath ?? '').includes(execution.sessionId) ? execution.sessionId : null;
  return (
    <section>
      <h3>This execution</h3>
      <dl className="kv">
        <Row label="Execution">#{execution.id}</Row>
        {mapIndices && <Row label="Map item">{JSON.stringify(mapIndices)}</Row>}
        <Row label="Ran">{execution.reused ? 'in the original run' : <RanWhen execution={execution} />}</Row>
        <Row label="Agent">
          <span title="Agent CLI (harness) · provider · model">{laneText(nodeLane(view, execution.node, execution))}</span>
          {info?.model && execution.model && info.model !== execution.model && <span className="muted"> (configured: {info.model})</span>}
        </Row>
        <Row label="Tokens">
          <ExecutionTokens execution={execution} />
        </Row>
        <Row label="Cost">
          <span title={execCostTitle(execution.costSource)}>{costText(execution.costUsd, execution.costSource === 'estimate')}</span>
        </Row>
        {t?.transcript.copilotCredits != null && (
          <Row label="Copilot's figure">
            <span title="Sum of Copilot's per-call AI-credit charges from its session log (1 credit = $0.01). Copilot's own number, not the estimate above.">
              {t.transcript.copilotCredits.toFixed(1)} AI credits (${copilotCreditsUsd(t.transcript.copilotCredits).toFixed(2)})
            </span>
          </Row>
        )}
        <Row label="Transcript">{t ? <TranscriptSource execution={execution} data={t} /> : '…'}</Row>
        {sessionId && (
          <Row label="Session id">
            <code>{sessionId}</code>
          </Row>
        )}
        {snapshots && (
          <Row label="Workspace snapshots">
            <code>{(execution.workspaceEnd?.base ?? execution.workspaceStart?.commit ?? '').slice(0, 10)}</code> → <code>{(execution.workspaceEnd?.commit ?? 'now').slice(0, 10)}</code>
          </Row>
        )}
        {execution.error && (
          <Row label="Outcome code">
            <code>{[execution.error.code, execution.error.reason].filter(Boolean).join(': ')}</code>
          </Row>
        )}
        {execution.retries > 0 && <Row label="API retries">{plural(execution.retries, 'retry', 'retries')} survived</Row>}
      </dl>
    </section>
  );
}

function execCostTitle(source: Execution['costSource']): string {
  if (source === 'provider') return 'Reported by the provider';
  if (source === 'estimate') return 'Estimated at API list prices';
  return '';
}

/** How long it ran and when it started (≈ when the times were inferred from the log). */
function RanWhen({ execution }: { execution: Execution }) {
  const approx = execution.timesApproximate ? ' ≈' : '';
  return (
    <span title={`${fullTime(execution.startedAt)} → ${fullTime(execution.endedAt)}`}>
      {duration(execution.durationMs)}
      {approx} · {timeOfDay(execution.startedAt)}
    </span>
  );
}

function ExecutionTokens({ execution }: { execution: Execution }) {
  const cached = execution.usage?.cacheReadInputTokens ?? 0;
  let body: ReactNode;
  if (totalTokens(execution.usage)) {
    body = (
      <>
        {tokenParts(execution.usage).total}
        {cached > 0 && <span className="muted"> · {compact(cached)} cached</span>}
      </>
    );
  } else {
    body = execution.usageSource === 'pending' ? 'counted when it finishes' : '0';
  }
  return <span title={usageBreakdown(execution.usage)}>{body}</span>;
}

/** Where the chat was read from: the harness's own session log, or the run ledger (and why). */
function TranscriptSource({ execution, data }: { execution: Execution; data: TranscriptResponse }) {
  if (data.transcript.source === 'native') {
    return (
      <span className="kv-inline">
        {harnessLabel(execution.harness)} session log
        {data.transcript.sourcePath && <CopyButton text={data.transcript.sourcePath} label="Copy the session log path" />}
      </span>
    );
  }
  return (
    <span className="kv-inline">
      run ledger
      {data.sourceNote && (
        <span className="info-tip" title={data.sourceNote}>
          <Info size={12} aria-label={data.sourceNote} />
        </span>
      )}
    </span>
  );
}

/** The node as the graph defines it; its model and instructions only before it has run (the
 * execution facts and the chat show them after). */
function NodeDefinition({ view, info, started }: { view: RunView; info: GraphNodeInfo | undefined; started: boolean }) {
  const loop = nodeLoop(view, info);
  return (
    <section>
      <h3>Node definition</h3>
      <dl className="kv">
        <Row label="Kind">{info?.kind ?? '—'}</Row>
        <Row label="Worker">{info?.worker ?? '—'}</Row>
        {!started && (
          <Row label="Agent">
            <span title="Agent CLI (harness) · provider · model">{laneText(nodeLane(view, info?.name ?? '', undefined))}</span>
          </Row>
        )}
        {info?.effort && <Row label="Effort">{info.effort}</Row>}
        <Row label="Timeout">{info?.timeoutMs != null ? duration(info.timeoutMs) : 'none'}</Row>
        <Row label="Attempts">{info?.attempts ?? 1}</Row>
        <Row label="Loop">{loop ? `${loop.name}, at most ${loop.max ?? '?'} rounds` : 'not in a loop'}</Row>
      </dl>
      {!started && (
        <>
          <h4>Instructions</h4>
          {info?.instructions ? <pre className="instructions">{info.instructions}</pre> : <p className="muted small">None (the worker's built-in behaviour).</p>}
        </>
      )}
    </section>
  );
}

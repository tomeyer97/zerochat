// What a batch actually ran, above its matrix: the harness (its agent nodes, models and loops)
// and, for a replay batch, what it replayed and the recorded score to read it against.
import type { ReactNode } from 'react';
import type { BatchDetail, HarnessNode, ReplayContext } from '../types';
import { benchmarksBatchHref, runHref } from '../route';
import { harnessName, harnessSegments, nodeModelText, nodeSeparator, nodeTitle, versionTitle } from './harness-text';
import { replaySentence } from './replay-text';
import { ScoreValue } from './shared';

/** One harness's agent nodes in graph order, each with its model and effort, a loop drawn as one
 * bracket with its round limit, and nodes of one parallel group joined by ‖. */
export function HarnessNodes({ nodes }: { nodes: HarnessNode[] }) {
  return (
    <ol className="bm-hnodes">
      {harnessSegments(nodes).map((seg, i) => {
        const chips = <HarnessChips nodes={seg.nodes} />;
        if (!seg.loopName) {
          return (
            <li key={i} className="bm-hseq">
              {i > 0 && <Separator text=" → " />}
              {chips}
            </li>
          );
        }
        return (
          <li key={i} className="bm-hloop">
            <span className="bm-hloop-caption muted small">
              {i > 0 && '→ '}loop {seg.loopName}
              {seg.loopMax != null ? `, up to ${seg.loopMax} rounds` : ''}
            </span>
            <span className="bm-hloop-body">{chips}</span>
          </li>
        );
      })}
    </ol>
  );
}

function HarnessChips({ nodes }: { nodes: HarnessNode[] }) {
  return nodes.map((n, i) => {
    const sep = nodeSeparator(nodes, i);
    return (
      <span key={`${n.name}-${i}`} className="bm-hnode-wrap">
        {sep && <Separator text={sep} />}
        <span className="bm-hnode" title={nodeTitle(n)}>
          <span className="bm-hnode-name">{n.name}</span> <span className="muted">{nodeModelText(n)}</span>
        </span>
      </span>
    );
  });
}

function Separator({ text }: { text: string }) {
  return (
    <span className="bm-hsep muted" aria-hidden>
      {text}
    </span>
  );
}

/** "Harness: <what ran>": the harness's name, or the models alone when no run kept its harness. */
function HarnessLine({ children }: { children: ReactNode }) {
  return (
    <p className="small">
      <span className="muted">Harness:</span> {children}
    </p>
  );
}

export function HarnessSection({ detail }: { detail: BatchDetail }) {
  const harnesses = detail.harnesses ?? [];
  const title = versionTitle(detail) || undefined;
  if (harnesses.length === 0) {
    // No run kept a copy of its harness (deleted runs, or a pb from before it kept one): the
    // version's own model list is all there is.
    return (
      <section className="bm-harness" aria-label="Harness">
        <HarnessLine>
          <span title={title}>{detail.models.join(', ') || 'not recorded'}</span>
        </HarnessLine>
      </section>
    );
  }
  const total = harnesses.reduce((n, h) => n + h.runs, 0);
  const mixed = harnesses.length > 1;
  return (
    <section className="bm-harness" aria-label="Harness">
      {mixed && (
        <p className="small bm-harness-mixed">
          <strong>This batch's runs used {harnesses.length} different harnesses</strong> <span className="muted">— compare its cells with care.</span>
        </p>
      )}
      {harnesses.map((h, i) => (
        <div key={i} className="bm-harness-one">
          <HarnessLine>
            <strong title={title}>{harnessName(h)}</strong>
            {h.modelOverride && <span> · every node on {h.modelOverride} (--model)</span>}
            {h.agentHarness && <span className="muted"> · {h.agentHarness}</span>}
            {mixed && (
              <span className="muted">
                {' '}
                · {h.runs} of {total} runs
              </span>
            )}
          </HarnessLine>
          <HarnessNodes nodes={h.nodes} />
        </div>
      ))}
    </section>
  );
}

/** What a replay batch replayed, one sentence per replayed segment (`replaySentence`), with the
 * recorded run and its batch as links. */
export function ReplaySection({ replays }: { replays: ReplayContext[] }) {
  return (
    <section className="bm-replay" aria-label="What was replayed">
      {replays.map((r) => {
        const s = replaySentence(r);
        return (
          <p key={`${r.sourceRun}|${r.node}|${r.through ?? ''}`} className="small">
            <span className="muted">Replay of</span>{' '}
            {r.sourceZeroshotRunId ? (
              <a href={runHref('pb', r.sourceZeroshotRunId)} title={`pb run ${r.sourceRun}`}>
                {s.source}
              </a>
            ) : (
              <span title={`pb run ${r.sourceRun}`}>{s.source}</span>
            )}
            {r.sourceBatch && (
              <>
                {' '}
                <a className="muted" href={benchmarksBatchHref(r.sourceBatch)} title="the batch the recorded run belongs to">
                  (its batch)
                </a>
              </>
            )}{' '}
            <span className="muted">from</span> <strong>{s.segment}</strong>
            {s.model && <span> {s.model}</span>}
            {s.base && <span className="muted"> · started from {s.base}</span>}
            {s.sweep && (
              <span className="muted">
                {' '}
                · sweep <span title={r.sweepId ?? undefined}>{s.sweep}</span>
              </span>
            )}
            <span className="bm-replay-baseline">
              {' · '}
              <span className="muted">recorded score after {s.end}:</span> <ScoreValue score={r.recordedScore} />
              {s.runFinal && <span className="muted"> {s.runFinal}</span>}
            </span>
          </p>
        );
      })}
    </section>
  );
}

// The compact score card under a benchmark run's own header: score, tests, regression, a
// per-step curve and cost per node. Only rendered for pb-source runs; no actions.
import { useState } from 'react';
import { api } from '../api';
import type { RunScore, RunView } from '../types';
import { duration, usd } from '../format';
import { useResource } from '../hooks';
import { Skeleton } from '../common';
import { scoreColor, scoreText } from './scores';
import { ScoreValue } from './shared';

export function ScoreCard({ view }: { view: Pick<RunView, 'runId' | 'context'> }) {
  const isBenchmark = view.context?.kind === 'benchmark';
  const resource = useResource<RunScore>(isBenchmark ? `score/${view.runId}` : null, (signal) => api.benchmarkRunScore(view.runId, signal));
  const [testsOpen, setTestsOpen] = useState(false);
  if (!isBenchmark) return null;
  if (resource.loading && !resource.data)
    return (
      <div className="score-card">
        <Skeleton lines={2} />
      </div>
    );
  const score = resource.data;
  if (!score) return null;
  return (
    <section className="score-card" aria-label="Benchmark score">
      <div className="score-card-row">
        <span className="score-card-item">
          score <ScoreValue score={score.score} />
        </span>
        {score.testsTotal != null && (
          <span className="score-card-item">
            tests {score.testsPassed ?? 0}/{score.testsTotal}
            {score.failingTests.length > 0 && (
              <details className="limited" open={testsOpen} onToggle={(e) => setTestsOpen((e.target as HTMLDetailsElement).open)}>
                <summary>{score.failingTests.length} failing</summary>
                <div className="popover">
                  <ul>
                    {score.failingTests.map((t) => (
                      <li key={t}>
                        <code>{t}</code>
                      </li>
                    ))}
                  </ul>
                </div>
              </details>
            )}
          </span>
        )}
        {score.regressionTotal != null && (
          <span className="score-card-item">
            regression {score.regressionPassed ?? 0}/{score.regressionTotal}
          </span>
        )}
        {score.wallS != null && <span className="score-card-item muted">{duration(score.wallS * 1000)}</span>}
        {score.costUsd != null && (
          <span className="score-card-item muted" title={score.capUsd != null ? `of ${usd(score.capUsd)} cap` : undefined}>
            {usd(score.costUsd)}
          </span>
        )}
      </div>
      {score.steps.length > 0 && (
        <ol className="score-steps">
          {score.steps.map((s) => (
            <li key={s.seq} title={s.testsTotal != null ? `${s.node}: tests ${s.testsPassed ?? 0}/${s.testsTotal}` : s.node}>
              <span className="score-step-node muted">{s.node}</span>
              <span className="score-step-bar" aria-hidden>
                <span style={{ width: `${Math.max(0, Math.min(1, s.score ?? 0)) * 100}%`, background: s.score != null ? scoreColor(s.score) : 'transparent' }} />
              </span>
              <span className="score-step-value tabular">{scoreText(s.score)}</span>
            </li>
          ))}
        </ol>
      )}
      {score.nodeCosts.length > 0 && (
        <p className="score-card-nodecosts muted small">
          {score.nodeCosts.map((n, i) => (
            <span key={n.node}>
              {i > 0 && ' · '}
              {n.node} {usd(n.costUsd)}
            </span>
          ))}
        </p>
      )}
    </section>
  );
}

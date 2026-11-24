// Small pieces every Benchmarks page uses: the sub-nav, a batch's status pill and a score.
import { Play } from 'lucide-react';
import { benchmarksHref, benchmarksStartHref, benchmarksSweepsHref } from '../route';
import { scoreColor, scoreText } from './scores';

const BATCH_STATUS_TONE: Record<string, string> = { running: 'running', starting: 'running', done: 'ok', stopped: 'muted', crashed: 'bad', 'failed to start': 'bad' };

export function BatchStatusPill({ status }: { status: string }) {
  return <span className={`pill small tone-${BATCH_STATUS_TONE[status] ?? 'muted'}`}>{status}</span>;
}

/** The sub-nav inside every Benchmarks page: Batches | Sweeps, plus the one action that starts
 * something (blue, as every action is). */
export function BenchmarksNav({ active, showStart = true }: { active: 'batches' | 'sweeps'; showStart?: boolean }) {
  return (
    <div className="bm-subnav">
      <nav aria-label="Benchmarks sections">
        <a href={benchmarksHref()} aria-current={active === 'batches' ? 'page' : undefined} className={active === 'batches' ? 'on' : ''}>
          Batches
        </a>
        <a href={benchmarksSweepsHref()} aria-current={active === 'sweeps' ? 'page' : undefined} className={active === 'sweeps' ? 'on' : ''}>
          Sweeps
        </a>
      </nav>
      {showStart && (
        <a href={benchmarksStartHref()} className="button primary small">
          <Play size={13} aria-hidden /> Start a batch
        </a>
      )}
    </div>
  );
}

export function ScoreValue({ score }: { score: number | null }) {
  if (score == null) return <span className="muted">—</span>;
  return <span style={{ color: scoreColor(score), fontWeight: 600 }}>{scoreText(score)}</span>;
}

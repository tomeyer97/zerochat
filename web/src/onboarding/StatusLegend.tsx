// The legend under Home's run table: what each status dot means.
import { StatusDot } from '../common';
import { runStatusInfo, type RunStatus } from '../status';

export const LEGEND_STATUSES: RunStatus[] = ['running', 'failed', 'stalled', 'stopped', 'succeeded'];

export function StatusLegend() {
  return (
    <ul className="status-legend" aria-label="Status dots">
      {LEGEND_STATUSES.map((s) => (
        <li key={s}>
          <StatusDot status={s} title={runStatusInfo[s].explain} />
          {runStatusInfo[s].label}
        </li>
      ))}
    </ul>
  );
}

/** Feedback boxes: an error, a banner, a loading placeholder and an empty state. */
import type { ReactNode } from 'react';
import { AlertTriangle } from 'lucide-react';

/** An error the person can read: the server's message verbatim, and a retry when possible. */
export function ErrorBox({ title, message, onRetry, compact }: { title?: string; message: string; onRetry?: () => void; compact?: boolean }) {
  return (
    <div className={`error-box ${compact ? 'compact' : ''}`} role="alert">
      <AlertTriangle size={16} aria-hidden />
      <div>
        {title && <strong>{title}</strong>}
        <p>{message}</p>
      </div>
      {onRetry && (
        <button type="button" className="button small" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

export function InfoBanner({ children, tone = 'info', icon }: { children: ReactNode; tone?: 'info' | 'warn' | 'bad'; icon?: ReactNode }) {
  return (
    <div className={`banner banner-${tone}`} role={tone === 'bad' ? 'alert' : 'note'}>
      {icon}
      <div>{children}</div>
    </div>
  );
}

export function Skeleton({ lines = 3, height }: { lines?: number; height?: number }) {
  if (height) return <div className="skeleton block" style={{ height }} aria-hidden />;
  return (
    <div className="skeleton-stack" aria-busy="true" aria-label="Loading">
      {Array.from({ length: lines }, (_, i) => (
        <div key={i} className="skeleton line" style={{ width: `${92 - ((i * 17) % 40)}%` }} />
      ))}
    </div>
  );
}

export function EmptyState({ title, children, icon }: { title: string; children?: ReactNode; icon?: ReactNode }) {
  return (
    <div className="empty-state">
      {icon}
      <h3>{title}</h3>
      {children && <div className="empty-body">{children}</div>}
    </div>
  );
}

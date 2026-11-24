import { Component, type ReactNode } from 'react';

/**
 * Catches a render crash in whatever it wraps: a plain "something broke" plus Reload, the stack
 * behind a Details disclosure. No claim about what is or isn't affected — zerochat also
 * writes to runs (stop, resume, replay), so that would be false.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="fatal">
        <h2>Something broke on this page.</h2>
        <button type="button" className="button primary" onClick={() => location.reload()}>
          Reload
        </button>
        <details className="fatal-details">
          <summary>Details</summary>
          <pre>{this.state.error.message}</pre>
        </details>
      </div>
    );
  }
}

// The harness check under the Start form's Harness choice: the server reads the chosen graph and
// runtime before Start and lists the nodes that would run, or what would stop pb from starting.
import { useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api';
import { api } from './pb-setup';
import type { HarnessPreview } from '../types';
import { harnessName } from './harness-text';
import type { HarnessPaths } from './start-form';
import { HarnessNodes } from './WhatRan';
import { resourceKey } from './resource-key';

/** How long the form waits after the last keystroke before asking the server. */
const PREVIEW_DEBOUNCE_MS = 250;

export type PreviewResult = { preview?: HarnessPreview; error?: string };

/** The server's check of `paths` (with the model override), debounced; `null` while there is
 * nothing to check or the answer for the current choice has not arrived. */
export function useHarnessPreview(paths: HarnessPaths | null, model: string): PreviewResult | null {
  const [state, setState] = useState<(PreviewResult & { key: string }) | null>(null);
  const request = paths ? { graph: paths.graph, runtime: paths.runtime, model: model.trim() || undefined } : null;
  // `key` is everything the request depends on, so the effect re-runs exactly when the request
  // changes; it reads the request itself from a ref (the object is new on every render).
  const key = request ? resourceKey(request.graph, request.runtime, request.model) : '';
  const latest = useRef(request);
  latest.current = request;
  useEffect(() => {
    const req = latest.current;
    if (!req) {
      setState(null);
      return;
    }
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      api
        .benchmarkHarnessPreview(req, controller.signal)
        .then((preview) => setState({ key, preview }))
        .catch((cause) => {
          if (!controller.signal.aborted) setState({ key, error: errorMessage(cause) });
        });
    }, PREVIEW_DEBOUNCE_MS);
    return () => {
      controller.abort();
      window.clearTimeout(timer);
    };
  }, [key]);
  return state?.key === key ? state : null;
}

/** What blocks Start in a preview: its own errors, or the request failing. */
export function previewErrors(preview: PreviewResult | null): string[] {
  return preview?.preview?.errors ?? (preview?.error ? [preview.error] : []);
}

export function HarnessPreviewBox({ preview, loading }: { preview: PreviewResult | null; loading: boolean }) {
  if (loading) return <p className="muted small">Checking the harness…</p>;
  if (!preview) return null;
  if (preview.error) return <p className="field-error">{preview.error}</p>;
  const p = preview.preview!;
  return (
    <div className="bm-preview">
      {p.info && (
        <>
          <p className="small">
            <span className="muted">Runs:</span> <strong>{harnessName(p.info)}</strong>
            {p.info.modelOverride && <span> · every node on {p.info.modelOverride}</span>}
          </p>
          <HarnessNodes nodes={p.info.nodes} />
        </>
      )}
      {p.warnings.map((w) => (
        <p key={w} className="bm-preview-warn small">
          {w}
        </p>
      ))}
    </div>
  );
}

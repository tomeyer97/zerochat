/** The Features page before the first feature: what it is, the three steps, and zsf's docs. */
import { ExternalLink, Layers } from 'lucide-react';
import type { Setup } from '../generated/discovery';
import { CopyButton, EmptyState } from '../common';
import { FEATURES_INTRO, FEATURE_STEPS, ZSF_DOCS_URL, ZSF_INSTALL } from './features-empty';

function CommandLine({ command, label }: { command: string; label: string }) {
  return (
    <p className="path-line features-command">
      <code>{command}</code>
      <CopyButton text={command} label={label} />
    </p>
  );
}

/** `setup` is the setup check (`null` until it answers): only a "not found" zsf shows how to get it. */
export function FeaturesEmpty({ setup }: { setup: Setup | null }) {
  const missing = setup != null && !setup.zsf.found;
  return (
    <EmptyState title="No features yet" icon={<Layers size={22} aria-hidden />}>
      <div className="features-empty">
        <p>{FEATURES_INTRO.join(' ')}</p>
        {missing && (
          <div className="features-install" role="note">
            <p className="tone-text-warn">
              {ZSF_INSTALL.message} {ZSF_INSTALL.how}
            </p>
            <CommandLine command={ZSF_INSTALL.npm} label="Copy the npm command" />
            <CommandLine command={ZSF_INSTALL.source} label="Copy the cargo command" />
          </div>
        )}
        <ol className="features-steps">
          {FEATURE_STEPS.map((step) => (
            <li key={step.label}>
              <span className="features-step-label">{step.label}</span>
              <CommandLine command={step.command} label={`Copy: ${step.label}`} />
              {step.hint && <span className="muted small">{step.hint}</span>}
            </li>
          ))}
        </ol>
        <a className="features-docs" href={ZSF_DOCS_URL} target="_blank" rel="noreferrer">
          zsf README <ExternalLink size={12} aria-hidden />
        </a>
      </div>
    </EmptyState>
  );
}

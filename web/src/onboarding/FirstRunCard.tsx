// Home with no runs: where zerochat reads runs from, whether Zeroshot is usable, and how to start one.
import type { ReactNode } from 'react';
import { Check, GitBranch, X } from 'lucide-react';
import type { Setup } from '../generated/discovery';
import { CopyButton } from '../common';
import { EXAMPLE_COMMAND, zeroshotProblem } from './setup-model';

/** A command with its copy button. */
export function Command({ text, label }: { text: string; label: string }) {
  return (
    <div className="onb-command">
      <pre>
        <code>{text}</code>
      </pre>
      <CopyButton text={text} label={label} />
    </div>
  );
}

function Ok({ children }: { children: ReactNode }) {
  return (
    <span className="onb-ok">
      <Check size={14} aria-label="OK" /> {children}
    </span>
  );
}

function Bad({ children }: { children: ReactNode }) {
  return (
    <span className="onb-bad">
      <X size={14} aria-label="Problem" /> {children}
    </span>
  );
}

/**
 * `setup` is the setup check, or `null` when it could not be read: then the card names the
 * overview's `stateRoot` and leaves the Zeroshot line out.
 */
export function FirstRunCard({ setup, stateRoot }: { setup: Setup | null; stateRoot: string }) {
  const zeroshot = zeroshotProblem(setup);
  const runsProblem = setup?.problems.find((p) => p.check === 'runs');
  return (
    <section className="onb-card" aria-labelledby="onb-title">
      <h2 id="onb-title">
        <GitBranch size={18} aria-hidden /> No runs yet
      </h2>
      <dl className="onb-checks">
        <div>
          <dt>Reading</dt>
          <dd>
            <code className="onb-path">{setup?.runs.stateDir ?? stateRoot}</code>
            {runsProblem && <Bad>{runsProblem.message}</Bad>}
          </dd>
        </div>
        {setup && (
          <div>
            <dt>Zeroshot</dt>
            <dd>
              {zeroshot ? (
                <>
                  <Bad>{zeroshot.message}</Bad>
                  {zeroshot.fix && <Command text={zeroshot.fix} label="Copy the install command" />}
                </>
              ) : (
                <Ok>
                  {setup.zeroshot.version ?? 'found'} <span className="muted">{setup.zeroshot.program}</span>
                </Ok>
              )}
            </dd>
          </div>
        )}
        <div>
          <dt>Start one</dt>
          <dd>
            <Command text={EXAMPLE_COMMAND} label="Copy the example" />
          </dd>
        </div>
      </dl>
      <p className="muted small">
        Next to <code>zeroshot ui</code> (profiles), zerochat keeps every run’s history, explains failures, and lets you steer, stop, resume or replay a run.
      </p>
    </section>
  );
}

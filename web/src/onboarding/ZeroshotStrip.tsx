// One thin amber line on pages with runs when Zeroshot is missing or too old; the fix one click away.
import { AlertTriangle } from 'lucide-react';
import type { Setup } from '../generated/discovery';
import { CopyButton } from '../common';
import { stripText, zeroshotProblem } from './setup-model';

export function ZeroshotStrip({ setup }: { setup: Setup | null }) {
  const text = stripText(setup);
  const fix = zeroshotProblem(setup)?.fix;
  if (!text) return null;
  return (
    <div className="zeroshot-strip" role="status">
      <AlertTriangle size={14} aria-hidden />
      <span>{text}</span>
      {fix && (
        <details>
          <summary>Fix</summary>
          <code>{fix}</code>
          <CopyButton text={fix} label="Copy the install command" />
        </details>
      )}
    </div>
  );
}

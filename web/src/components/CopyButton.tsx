import { useEffect, useRef, useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { copyText } from './clipboard';

/** How long the button shows "Copied" after a click. */
const COPIED_FLASH_MS = 1400;

type CopyButtonProps = {
  text: string;
  label?: string;
  showLabel?: boolean;
  /** A short id worth showing in the tooltip (e.g. the run id). Most copied values (JSON, paths,
   * commands) are too long to read in a native tooltip, so they're left out. */
  preview?: string;
};

export function CopyButton({ text, label = 'Copy', showLabel = false, preview }: CopyButtonProps) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  return (
    <button
      type="button"
      className={`copy-button ${showLabel ? 'with-label' : ''}`}
      title={copied ? 'Copied' : preview ? `${label}: ${preview}` : label}
      aria-label={copied ? 'Copied' : label}
      onClick={async (event) => {
        event.stopPropagation();
        await copyText(text);
        setCopied(true);
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => setCopied(false), COPIED_FLASH_MS);
      }}
    >
      {copied ? <Check size={13} aria-hidden /> : <Copy size={13} aria-hidden />}
      {showLabel && <span>{copied ? 'Copied' : label}</span>}
    </button>
  );
}

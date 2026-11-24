// The "?" in the top bar and the glossary dialog it opens.
import { Fragment, useEffect, useState, type ReactNode } from 'react';
import { CircleHelp } from 'lucide-react';
import { Modal } from '../common';
import { GLOSSARY } from './glossary-terms';
import { setupLine } from './setup-model';
import type { Setup } from '../generated/discovery';

/** `text` with `code` spans for the backticked parts. */
export function withCode(text: string): ReactNode {
  return text.split('`').map((part, i) => (i % 2 ? <code key={i}>{part}</code> : <Fragment key={i}>{part}</Fragment>));
}

export function GlossaryList() {
  return (
    <dl className="glossary">
      {GLOSSARY.map((g) => (
        <div key={g.term}>
          <dt>{g.term}</dt>
          <dd>{withCode(g.meaning)}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Which zerochat, which Zeroshot and which runs folder: the top bar's setup line, which a phone has no room for. */
function SetupSummary({ setup }: { setup: Setup | null }) {
  const line = setupLine(setup);
  if (!setup || !line) return null;
  return (
    <p className="glossary-setup small muted" title={line.title}>
      zerochat {setup.version} · {line.text}
    </p>
  );
}

export function GlossaryButton({ setup = null }: { setup?: Setup | null }) {
  const [open, setOpen] = useState(false);
  // It lives in the top bar, above every page: going to another page closes it, so it never
  // stays over a page the user did not open it on.
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener('hashchange', close);
    return () => window.removeEventListener('hashchange', close);
  }, [open]);
  return (
    <>
      <button type="button" className="icon-button" aria-label="Glossary" title="Glossary" aria-haspopup="dialog" onClick={() => setOpen(true)}>
        <CircleHelp size={18} aria-hidden />
      </button>
      {open && (
        <Modal title="Glossary" onClose={() => setOpen(false)}>
          <SetupSummary setup={setup} />
          <GlossaryList />
        </Modal>
      )}
    </>
  );
}

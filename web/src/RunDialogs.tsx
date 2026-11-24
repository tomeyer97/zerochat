// The run page's three dialogs (replay, force stop, resume), each with the API call it confirms.
import { api } from './api';
import type { RunView } from './types';
import { navigate } from './route';
import type { ReplayFocus } from './RunSummary';
import { ReplayDialog } from './ReplayDialog';
import { ConfirmDialog } from './components/ConfirmDialog';
import { resumeText } from './replay-model';

export type RunDialog = 'replay' | 'stop' | 'resume';

type Props = {
  dialog: RunDialog | null;
  /** The run, as the page's route names it. */
  target: string;
  run: string;
  view: RunView;
  /** Where the replay dialog starts (a failure card's node), or null for the whole run. */
  replayFocus: ReplayFocus;
  onClose: () => void;
  /** Reload the run view after an action changed it. */
  reload: () => void;
  /** A message for the page's notice (a replay that did not start a new run says why). */
  onNotice: (text: string) => void;
  /** A force stop was sent: the status pill says "Stopping" until the run has stopped. */
  onStopRequested: () => void;
};

/** The open dialog, if any. An action that starts a new run goes to it; the others reload this one. */
export function RunDialogs({ dialog, target, run, view, replayFocus, onClose, reload, onNotice, onStopRequested }: Props) {
  const goToRun = (next: string) => navigate({ kind: 'run', target, run: next });
  switch (dialog) {
    case null:
      return null;
    case 'replay':
      return (
        <ReplayDialog
          view={view}
          focus={replayFocus}
          onClose={onClose}
          onStarted={(runId, message) => {
            onClose();
            if (runId) {
              goToRun(runId);
            } else {
              onNotice(message);
              reload();
            }
          }}
        />
      );
    case 'stop':
      return (
        <ConfirmDialog
          title="Force stop?"
          confirmLabel="Force stop"
          danger
          onClose={onClose}
          onConfirm={async () => {
            await api.stop(target, run);
            onClose();
            onStopRequested();
            reload();
          }}
        >
          <p>Every agent stops now. The workspace is kept; you can resume later.</p>
        </ConfirmDialog>
      );
    case 'resume':
      return <ResumeDialog target={target} run={run} view={view} onClose={onClose} reload={reload} goToRun={goToRun} />;
  }
}

/**
 * Resume from the latest checkpoint, or start over when no checkpoint was saved; warns while an
 * agent process still runs in the workspace.
 */
function ResumeDialog({ target, run, view, onClose, reload, goToRun }: Pick<Props, 'target' | 'run' | 'view' | 'onClose' | 'reload'> & { goToRun: (run: string) => void }) {
  const startsOver = !!view.local && !view.local.hasCheckpoints;
  const pids = view.local?.agentProcesses?.map((p) => p.pid) ?? [];
  return (
    <ConfirmDialog
      title={startsOver ? 'Start over?' : 'Resume?'}
      confirmLabel={startsOver ? 'Start over' : 'Resume'}
      onClose={onClose}
      onConfirm={async () => {
        const result = await api.resume(target, run);
        onClose();
        const next = typeof result.runId === 'string' && result.runId !== run ? result.runId : null;
        // Go to the new run, which is what you want to watch; its header links back here.
        if (next) goToRun(next);
        else reload();
      }}
    >
      <p>{startsOver ? START_OVER_TEXT : resumeText((view.capabilities ?? []).includes('replay'))}</p>
      {pids.length > 0 && <p className="tone-text-bad">An agent process (pid {pids.join(', ')}) is still running in this workspace. Stop it first, or two agents will edit the same files.</p>}
    </ConfirmDialog>
  );
}

const START_OVER_TEXT = 'Starts a new run with the same graph, prompts and models. No checkpoint was saved, so it starts over: finished nodes run again, in the workspace as it is now.';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ChevronLeft, ChevronRight, Mic, RotateCcw, Square, X } from 'lucide-react';
import {
  cancelVoiceEditSession,
  cancelVoiceEditInstruction,
  cancelAndCloseVoiceEditSession,
  commitVoiceEditSession,
  applyVoiceEditTextInstruction,
  finalizeVoiceEditDictation,
  finalizeVoiceEditInstruction,
  getVoiceEditState,
  startVoiceEditInstruction,
  startVoiceEditSession,
  undoVoiceEditPreview,
  redoVoiceEditPreview,
  type VoiceEditSnapshot,
  type VoiceEditTurn,
} from '../../api/voice-edit';
import { ToolWindowHeader } from '../../components/ui/ToolWindowHeader';
import { useModalActive } from '../../ui/layout/AppViewport';
import './voice-edit-panel.css';

const isTerminal = (phase: VoiceEditSnapshot['phase']) =>
  phase === 'completed' || phase === 'cancelled';
const errorDetail = (reason: unknown) => {
  const value = reason instanceof Error ? reason.message : String(reason);
  // Never render provider responses, selection text or a raw transport error.
  const key = value.split(':')[0];
  return /^voiceEdit[A-Za-z]+$/.test(key) ? key : 'voiceEditUnknownError';
};

export interface VoiceEditPanelProps {
  embedded?: boolean;
  active?: boolean;
  closeError?: string;
  onRequestClose?: () => void;
  onRequestDismiss?: () => Promise<boolean>;
}

export function VoiceEditPanel({
  embedded = false,
  active = true,
  closeError = '',
  onRequestClose,
  onRequestDismiss,
}: VoiceEditPanelProps) {
  const { t } = useTranslation();
  const modalActive = useModalActive();
  const [snapshot, setSnapshot] = useState<VoiceEditSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [closing, setClosing] = useState(false);
  const [error, setError] = useState('');
  const [textInstruction, setTextInstruction] = useState('');
  const [cancellingInstruction, setCancellingInstruction] = useState(false);
  const generation = useRef(0);
  const closingRef = useRef(false);
  const readInFlight = useRef<number>();
  const runningAction = useRef<number>();
  const instructionRef = useRef<{ sessionId: string; instructionId: string }>();
  const cancellingRef = useRef<number>();

  const refresh = useCallback(async (afterFailure = false) => {
    if (closingRef.current || (!afterFailure && runningAction.current !== undefined)) return;
    const request = generation.current;
    if (readInFlight.current === request) return;
    readInFlight.current = request;
    try {
      const next = await getVoiceEditState();
      if (request === generation.current) setSnapshot(next);
    } catch (reason) {
      if (request === generation.current) setError(errorDetail(reason));
    } finally {
      if (readInFlight.current === request) readInFlight.current = undefined;
    }
  }, []);

  useEffect(() => {
    setBusy(false);
    setCancellingInstruction(false);
    if (!active) return;
    setSnapshot(null);
    void refresh();
    const timer = window.setInterval(() => void refresh(), 500);
    return () => {
      generation.current += 1;
      runningAction.current = undefined;
      instructionRef.current = undefined;
      cancellingRef.current = undefined;
      closingRef.current = false;
      window.clearInterval(timer);
    };
  }, [active, refresh]);

  useEffect(() => {
    if (closeError) setError(closeError);
  }, [closeError]);

  const run = async (action: () => Promise<VoiceEditSnapshot | null>): Promise<boolean> => {
    if (
      runningAction.current !== undefined ||
      closingRef.current ||
      cancellingRef.current !== undefined
    ) {
      return false;
    }
    const request = ++generation.current;
    runningAction.current = request;
    setBusy(true);
    setError('');
    try {
      const next = await action();
      if (request !== generation.current) return false;
      setError('');
      if (next) setSnapshot(next);
      else await refresh(true);
      return true;
    } catch (reason) {
      if (request === generation.current) {
        setError(errorDetail(reason));
        await refresh(true);
      }
      return false;
    } finally {
      if (runningAction.current === request) runningAction.current = undefined;
      if (request === generation.current) setBusy(false);
    }
  };

  const runSession = (action: (sessionId: string) => Promise<VoiceEditSnapshot | null>) => {
    if (snapshot) return run(() => action(snapshot.sessionId));
  };

  const submitTextInstruction = async () => {
    const instruction = textInstruction.trim();
    if (!instruction || !snapshot?.context) return;
    const submitted = textInstruction;
    const owner = { sessionId: snapshot.sessionId, instructionId: crypto.randomUUID() };
    const applied = await run(async () => {
      instructionRef.current = owner;
      try {
        return await applyVoiceEditTextInstruction(owner.sessionId, instruction, {
          instructionId: owner.instructionId,
          expectedRevision: snapshot.context!.previewRevision,
        });
      } finally {
        if (instructionRef.current === owner) instructionRef.current = undefined;
      }
    });
    if (applied) setTextInstruction((current: string) => (current === submitted ? '' : current));
  };

  const recordInstruction = () => {
    if (!snapshot?.context) return;
    const owner = { sessionId: snapshot.sessionId, instructionId: crypto.randomUUID() };
    void run(async () => {
      instructionRef.current = owner;
      try {
        return await startVoiceEditInstruction(owner.sessionId, {
          instructionId: owner.instructionId,
          expectedRevision: snapshot.context!.previewRevision,
        });
      } catch (reason) {
        if (instructionRef.current === owner) instructionRef.current = undefined;
        throw reason;
      }
    });
  };

  const cancelInstruction = async () => {
    const owner = snapshot?.instructionId
      ? { sessionId: snapshot.sessionId, instructionId: snapshot.instructionId }
      : instructionRef.current?.sessionId === snapshot?.sessionId
        ? instructionRef.current
        : undefined;
    if (!owner || closingRef.current || cancellingRef.current !== undefined) return;
    const request = generation.current;
    cancellingRef.current = request;
    setCancellingInstruction(true);
    try {
      const next = await cancelVoiceEditInstruction(owner.sessionId, owner.instructionId);
      if (request !== generation.current) return;
      // Only confirmed cancellation retires the unresolved action. Failure keeps
      // its response observable; it may not have reached backend admission yet.
      generation.current += 1;
      runningAction.current = undefined;
      instructionRef.current = undefined;
      setSnapshot(next);
      setError('');
      setBusy(false);
    } catch (reason) {
      if (request === generation.current) setError(errorDetail(reason));
    } finally {
      if (cancellingRef.current === request) {
        cancellingRef.current = undefined;
        setCancellingInstruction(false);
      }
    }
  };

  const phase = snapshot?.phase;
  const context = snapshot?.context;
  const hasInstruction = Boolean(
    snapshot?.instructionId ||
      (busy && instructionRef.current?.sessionId === snapshot?.sessionId && instructionRef.current),
  );
  const canCancel = Boolean(snapshot && !isTerminal(snapshot.phase) && !closing);
  const closePanel = useCallback(async () => {
    if (closingRef.current) return false;
    closingRef.current = true;
    const request = ++generation.current;
    runningAction.current = undefined;
    setClosing(true);
    setError('');
    try {
      if (embedded && onRequestDismiss) {
        if (!(await onRequestDismiss())) return false;
      } else {
        await cancelAndCloseVoiceEditSession(
          snapshot && !isTerminal(snapshot.phase) ? snapshot.sessionId : undefined,
        );
      }
      if (request === generation.current) onRequestClose?.();
      return true;
    } catch (reason) {
      if (request === generation.current) setError(errorDetail(reason));
      return false;
    } finally {
      if (request === generation.current) {
        closingRef.current = false;
        setClosing(false);
        setBusy(false);
      }
    }
  }, [embedded, onRequestClose, onRequestDismiss, snapshot]);

  useEffect(() => {
    if (!active || modalActive || (embedded && onRequestDismiss)) return;
    const onKeydown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.defaultPrevented) return;
      event.preventDefault();
      void closePanel();
    };
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  }, [active, closePanel, embedded, modalActive, onRequestDismiss]);

  return (
    <main className="ol-tool-window ol-voice-edit-window">
      <ToolWindowHeader
        icon={<Mic />}
        title={t('voiceEdit.title')}
        description={t('voiceEdit.description')}
        onClose={() => {
          void closePanel();
        }}
        closeLabel={t('common.close')}
        closeDisabled={closing}
      />
      <section className={`ol-voice-edit-content${embedded ? ' is-embedded' : ''}`}>
        {!snapshot || isTerminal(snapshot.phase) ? (
          <div className="ol-voice-edit-empty">
            <p>{t(embedded ? 'voiceEdit.androidTargetHint' : 'voiceEdit.targetHint')}</p>
            {!embedded && (
              <button
                className="ol-tool-button is-primary"
                disabled={busy || closing}
                onClick={() => void run(() => startVoiceEditSession())}
              >
                <Mic size={18} />
                {t('voiceEdit.startDraft')}
              </button>
            )}
          </div>
        ) : (
          <>
            <div className="ol-voice-edit-status">
              <span aria-live="polite">
                {t('voiceEdit.status', { phase: t(`voiceEdit.phase.${snapshot.phase}`) })}
              </span>
              <span>{t('voiceEdit.turns', { count: context?.previewHistoryIndex ?? 0 })}</span>
            </div>
            {context?.draft ? (
              <div className="ol-voice-edit-card">
                <span className="ol-voice-edit-label">{t('voiceEdit.draft')}</span>
                <div>{context.draft}</div>
              </div>
            ) : null}
            {context?.preview && context.preview !== context.draft ? (
              <div className="ol-voice-edit-card is-preview">
                <span className="ol-voice-edit-label">{t('voiceEdit.preview')}</span>
                <div>{context.preview}</div>
              </div>
            ) : null}
            {phase === 'draft_ready' || phase === 'preview' ? (
              <div className="ol-voice-edit-text-edit">
                <label className="ol-voice-edit-label" htmlFor="voice-edit-instruction">
                  {t('voiceEdit.textInstruction')}
                </label>
                <div className="ol-voice-edit-text-row">
                  <input
                    id="voice-edit-instruction"
                    value={textInstruction}
                    disabled={busy || closing}
                    onChange={(event) => setTextInstruction(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
                        event.preventDefault();
                        void submitTextInstruction();
                      }
                    }}
                  />
                  <button
                    className="ol-tool-button is-primary"
                    disabled={busy || closing || !textInstruction.trim()}
                    onClick={submitTextInstruction}
                  >
                    {t('voiceEdit.applyInstruction')}
                  </button>
                </div>
              </div>
            ) : null}
            {context?.previewHistoryIndex ? (
              <div className="ol-voice-edit-history">
                <span className="ol-voice-edit-label">{t('voiceEdit.history')}</span>
                {context.turns
                  .slice(0, context.previewHistoryIndex)
                  .map((turn: VoiceEditTurn, index: number) => (
                    <div className="ol-voice-edit-turn" key={`${snapshot.sessionId}-${index}`}>
                      <div>
                        <b>{t('voiceEdit.turn', { count: index + 1 })}</b>: {turn.instructionRaw}
                      </div>
                      {turn.instructionPolished !== turn.instructionRaw ? (
                        <div className="ol-voice-edit-turn-polished">
                          {t('voiceEdit.polishedInstruction', { text: turn.instructionPolished })}
                        </div>
                      ) : null}
                      <div className="ol-voice-edit-turn-preview">
                        {t('voiceEdit.turnPreview', { text: turn.previewAfter })}
                      </div>
                    </div>
                  ))}
              </div>
            ) : null}
            <div className="ol-voice-edit-actions">
              {phase === 'dictating' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing}
                  onClick={() => void runSession(finalizeVoiceEditDictation)}
                >
                  <Square size={16} />
                  {t('voiceEdit.finishDraft')}
                </button>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <>
                  <button
                    className="ol-tool-button"
                    disabled={busy || closing || (context?.previewHistoryIndex ?? 0) === 0}
                    onClick={() => void runSession(undoVoiceEditPreview)}
                  >
                    <ChevronLeft size={16} />
                    {t('voiceEdit.undo')}
                  </button>
                  <button
                    className="ol-tool-button"
                    disabled={
                      busy ||
                      closing ||
                      (context?.previewHistoryIndex ?? 0) >=
                        (context?.previewHistory.length ?? 1) - 1
                    }
                    onClick={() => void runSession(redoVoiceEditPreview)}
                  >
                    <ChevronRight size={16} />
                    {t('voiceEdit.redo')}
                  </button>
                </>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <button
                  className="ol-tool-button"
                  disabled={busy || closing}
                  onClick={recordInstruction}
                >
                  <Mic size={16} />
                  {t('voiceEdit.nextInstruction')}
                </button>
              ) : null}
              {phase === 'editing' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing}
                  onClick={() =>
                    void runSession((sessionId) =>
                      finalizeVoiceEditInstruction(sessionId, snapshot.instructionId ?? undefined),
                    )
                  }
                >
                  <Square size={16} />
                  {t('voiceEdit.finishInstruction')}
                </button>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy || closing || !context}
                  onClick={() => {
                    if (context) {
                      void runSession((sessionId) =>
                        commitVoiceEditSession(sessionId, context.previewRevision),
                      );
                    }
                  }}
                >
                  <Check size={16} />
                  {t('voiceEdit.commit')}
                </button>
              ) : null}
              {canCancel ? (
                <button
                  className="ol-tool-button ol-voice-edit-cancel"
                  disabled={
                    closing ||
                    cancellingInstruction ||
                    phase === 'committing' ||
                    (busy && !hasInstruction)
                  }
                  onClick={() => {
                    if (hasInstruction) void cancelInstruction();
                    else void runSession(cancelVoiceEditSession);
                  }}
                >
                  <X size={16} />
                  {t(hasInstruction ? 'voiceEdit.cancelInstruction' : 'common.cancel')}
                </button>
              ) : null}
            </div>
          </>
        )}
        {error ? (
          <div className="ol-tool-error" role="alert">
            {t(`voiceEdit.errors.${errorDetail(error)}`, {
              defaultValue: t('voiceEdit.errors.voiceEditUnknownError'),
            })}
          </div>
        ) : null}
        {!embedded && snapshot && isTerminal(snapshot.phase) ? (
          <button
            className="ol-tool-button"
            disabled={busy || closing}
            onClick={() => void run(() => startVoiceEditSession())}
          >
            <RotateCcw size={16} />
            {t('voiceEdit.restart')}
          </button>
        ) : null}
      </section>
    </main>
  );
}

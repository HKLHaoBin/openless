import { useCallback, useEffect, useState } from 'react';
import { Check, Mic, RotateCcw, Square, X } from 'lucide-react';
import {
  cancelVoiceEditSession,
  closeVoiceEditWindow,
  commitVoiceEditSession,
  finalizeVoiceEditDictation,
  finalizeVoiceEditInstruction,
  getVoiceEditState,
  startVoiceEditInstruction,
  startVoiceEditSession,
  type VoiceEditSnapshot,
} from '../lib/ipc';
import { ToolWindowHeader } from '../components/ui/ToolWindowHeader';
import './voice-edit-panel.css';

const isTerminal = (phase: VoiceEditSnapshot['phase']) =>
  phase === 'completed' || phase === 'cancelled';

interface VoiceEditPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
}

export function VoiceEditPanel({ embedded = false, onRequestClose }: VoiceEditPanelProps) {
  const [snapshot, setSnapshot] = useState<VoiceEditSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await getVoiceEditState());
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const run = async (action: () => Promise<VoiceEditSnapshot | VoiceEditSnapshot | null>) => {
    setBusy(true);
    setError('');
    try {
      const next = await action();
      if (next) setSnapshot(next);
      else await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const phase = snapshot?.phase;
  const context = snapshot?.context;
  const canCancel = Boolean(snapshot && !isTerminal(snapshot.phase) && !busy);
  const closePanel = async () => {
    if (canCancel) {
      await run(async () => {
        const next = await cancelVoiceEditSession();
        await closeVoiceEditWindow();
        onRequestClose?.();
        return next;
      });
      return;
    }
    await closeVoiceEditWindow();
    onRequestClose?.();
  };

  return (
    <main className="ol-tool-window ol-voice-edit-window">
      <ToolWindowHeader
        icon={<Mic />}
        title="语音编辑会话"
        description="先口述草稿，再用多轮语音指令修改，确认后才写回字段。"
        onClose={() => {
          void closePanel();
        }}
        closeLabel="取消"
        closeDisabled={!canCancel}
      />
      <section className={`ol-voice-edit-content${embedded ? ' is-embedded' : ''}`}>
        {!snapshot || isTerminal(snapshot.phase) ? (
          <div className="ol-voice-edit-empty">
            <p>请先把光标放入目标输入框，或选中需要编辑的文本。</p>
            <button
              className="ol-tool-button is-primary"
              disabled={busy}
              onClick={() => void run(() => startVoiceEditSession())}
            >
              <Mic size={18} />
              开始口述草稿
            </button>
          </div>
        ) : (
          <>
            <div className="ol-voice-edit-status">
              <span aria-live="polite">状态：{phaseLabel(snapshot.phase)}</span>
              <span>回合：{context?.turns.length ?? 0}</span>
            </div>
            {context?.draft ? (
              <div className="ol-voice-edit-card">
                <span className="ol-voice-edit-label">草稿</span>
                <div>{context.draft}</div>
              </div>
            ) : null}
            {context?.preview && context.preview !== context.draft ? (
              <div className="ol-voice-edit-card is-preview">
                <span className="ol-voice-edit-label">当前预览</span>
                <div>{context.preview}</div>
              </div>
            ) : null}
            {context?.turns.length ? (
              <div className="ol-voice-edit-history">
                <span className="ol-voice-edit-label">指令历史</span>
                {context.turns.map((turn, index) => (
                  <div className="ol-voice-edit-turn" key={`${snapshot.sessionId}-${index}`}>
                    <div><b>第 {index + 1} 轮</b>：{turn.instructionRaw}</div>
                    {turn.instructionPolished !== turn.instructionRaw ? (
                      <div className="ol-voice-edit-turn-polished">润色后：{turn.instructionPolished}</div>
                    ) : null}
                    <div className="ol-voice-edit-turn-preview">预览：{turn.previewAfter}</div>
                  </div>
                ))}
              </div>
            ) : null}
            <div className="ol-voice-edit-actions">
              {phase === 'dictating' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy}
                  onClick={() => void run(finalizeVoiceEditDictation)}
                >
                  <Square size={16} />
                  完成草稿
                </button>
              ) : null}
              {phase === 'draft_ready' || phase === 'preview' ? (
                <button
                  className="ol-tool-button"
                  disabled={busy}
                  onClick={() => void run(startVoiceEditInstruction)}
                >
                  <Mic size={16} />
                  录下一条语音指令
                </button>
              ) : null}
              {phase === 'editing' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy}
                  onClick={() => void run(finalizeVoiceEditInstruction)}
                >
                  <Square size={16} />
                  完成语音指令
                </button>
              ) : null}
              {phase === 'preview' ? (
                <button
                  className="ol-tool-button is-primary"
                  disabled={busy}
                  onClick={() => void run(commitVoiceEditSession)}
                >
                  <Check size={16} />
                  确认并写回
                </button>
              ) : null}
              {canCancel ? (
                <button
                  className="ol-tool-button ol-voice-edit-cancel"
                  disabled={busy}
                  onClick={() => void run(cancelVoiceEditSession)}
                >
                  <X size={16} />
                  取消
                </button>
              ) : null}
            </div>
          </>
        )}
        {error ? <div className="ol-tool-error" role="alert">{error}</div> : null}
        {snapshot && isTerminal(snapshot.phase) ? (
          <button className="ol-tool-button" disabled={busy} onClick={() => void run(() => startVoiceEditSession())}>
            <RotateCcw size={16} />
            再来一次
          </button>
        ) : null}
      </section>
    </main>
  );
}

function phaseLabel(phase: VoiceEditSnapshot['phase']): string {
  switch (phase) {
    case 'dictating': return '口述草稿';
    case 'draft_ready': return '草稿就绪';
    case 'editing': return '录制指令';
    case 'applying': return '应用编辑计划';
    case 'preview': return '等待确认';
    case 'committing': return '写回输入框';
    case 'completed': return '已写回';
    case 'cancelled': return '已取消';
    default: return phase;
  }
}

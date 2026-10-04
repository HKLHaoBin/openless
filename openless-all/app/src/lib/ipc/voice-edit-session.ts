import { invokeOrMock } from './shared';

export type VoiceEditPhase =
  | 'dictating'
  | 'draft_ready'
  | 'editing'
  | 'applying'
  | 'preview'
  | 'committing'
  | 'completed'
  | 'cancelled';

export interface VoiceEditTurn {
  instructionRaw: string;
  instructionPolished: string;
  editPlan: unknown;
  previewAfter: string;
}

export interface VoiceEditContext {
  fieldText: string;
  selection?: { start: number; end: number } | null;
  dictatedSegment: string;
  draft: string;
  preview: string;
  turns: VoiceEditTurn[];
}

export interface VoiceEditSnapshot {
  sessionId: string;
  phase: VoiceEditPhase;
  context?: VoiceEditContext | null;
  target: { kind: 'selection'; start: number; end: number } | { kind: 'full_field' };
}

export interface VoiceEditStartArgs {
  fieldContext?: string;
  selection?: { start: number; end: number };
}

const mockSnapshot = (phase: VoiceEditPhase = 'dictating'): VoiceEditSnapshot => ({
  sessionId: 'voice-edit-demo',
  phase,
  context: {
    fieldText: '这是待编辑的草稿。',
    selection: null,
    dictatedSegment: '',
    draft: '',
    preview: '',
    turns: [],
  },
  target: { kind: 'full_field' },
});

export function startVoiceEditSession(
  args?: VoiceEditStartArgs,
): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_session', { args }, () => mockSnapshot());
}

export function openVoiceEditWindow(): Promise<void> {
  return invokeOrMock('voice_edit_window_open', undefined, () => undefined);
}

export function closeVoiceEditWindow(): Promise<void> {
  return invokeOrMock('voice_edit_window_close', undefined, () => undefined);
}

export function finalizeVoiceEditDictation(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('finalize_voice_edit_dictation', undefined, () =>
    mockSnapshot('draft_ready'),
  );
}

export function startVoiceEditInstruction(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_instruction', undefined, () => mockSnapshot('editing'));
}

export function finalizeVoiceEditInstruction(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('stop_voice_edit_instruction', undefined, () =>
    mockSnapshot('preview'),
  );
}

export function commitVoiceEditSession(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('commit_voice_edit', undefined, () => mockSnapshot('completed'));
}

export function cancelVoiceEditSession(): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('cancel_voice_edit_session', undefined, () => null);
}

export function getVoiceEditState(): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('get_voice_edit_state', undefined, () => null);
}

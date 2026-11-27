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
  previewHistory: string[];
  previewHistoryIndex: number;
  previewRevision: number;
}

export interface VoiceEditSnapshot {
  sessionId: string;
  instructionId?: string | null;
  phase: VoiceEditPhase;
  context?: VoiceEditContext | null;
  target: { kind: 'selection'; start: number; end: number } | { kind: 'full_field' };
}

const mockSnapshot = (phase: VoiceEditPhase = 'dictating'): VoiceEditSnapshot => ({
  sessionId: 'voice-edit-demo',
  phase,
  context: {
    fieldText: '',
    selection: null,
    dictatedSegment: '',
    draft: '',
    preview: '',
    turns: [],
    previewHistory: [],
    previewHistoryIndex: 0,
    previewRevision: 0,
  },
  target: { kind: 'full_field' },
});

export function startVoiceEditSession(): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_session', undefined, () => mockSnapshot());
}

export function openVoiceEditWindow(): Promise<void> {
  return invokeOrMock('voice_edit_window_open', undefined, () => undefined);
}

export function closeVoiceEditWindow(sessionId?: string): Promise<void> {
  return invokeOrMock('voice_edit_window_close', { sessionId }, () => undefined);
}

export function finalizeVoiceEditDictation(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('finalize_voice_edit_dictation', { sessionId }, () =>
    mockSnapshot('draft_ready'),
  );
}

export interface VoiceEditInstructionOptions {
  instructionId?: string;
  expectedRevision?: number;
}

export function startVoiceEditInstruction(
  sessionId: string,
  options: VoiceEditInstructionOptions = {},
): Promise<VoiceEditSnapshot> {
  return invokeOrMock('start_voice_edit_instruction', { sessionId, ...options }, () =>
    mockSnapshot('editing'),
  );
}

export function finalizeVoiceEditInstruction(
  sessionId: string,
  instructionId?: string,
): Promise<VoiceEditSnapshot> {
  return invokeOrMock('stop_voice_edit_instruction', { sessionId, instructionId }, () =>
    mockSnapshot('preview'),
  );
}

export function applyVoiceEditTextInstruction(
  sessionId: string,
  instruction: string,
  options: VoiceEditInstructionOptions = {},
): Promise<VoiceEditSnapshot> {
  return invokeOrMock(
    'apply_voice_edit_text_instruction',
    { sessionId, instruction, ...options },
    () => mockSnapshot('preview'),
  );
}

export function undoVoiceEditPreview(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('undo_voice_edit_preview', { sessionId }, () => mockSnapshot('preview'));
}

export function redoVoiceEditPreview(sessionId: string): Promise<VoiceEditSnapshot> {
  return invokeOrMock('redo_voice_edit_preview', { sessionId }, () => mockSnapshot('preview'));
}

export function commitVoiceEditSession(
  sessionId: string,
  expectedRevision?: number,
): Promise<VoiceEditSnapshot> {
  return invokeOrMock('commit_voice_edit', { sessionId, expectedRevision }, () =>
    mockSnapshot('completed'),
  );
}

export function cancelVoiceEditInstruction(
  sessionId: string,
  instructionId: string,
): Promise<VoiceEditSnapshot> {
  return invokeOrMock('cancel_voice_edit_instruction', { sessionId, instructionId }, () =>
    mockSnapshot('preview'),
  );
}

export function cancelVoiceEditSession(sessionId?: string): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('cancel_voice_edit_session', { sessionId }, () => null);
}

export async function cancelAndCloseVoiceEditSession(sessionId?: string): Promise<void> {
  const currentId = sessionId ?? (await getVoiceEditState())?.sessionId;
  await cancelVoiceEditSession(currentId);
  await closeVoiceEditWindow(currentId);
}

export function getVoiceEditState(): Promise<VoiceEditSnapshot | null> {
  return invokeOrMock('get_voice_edit_state', undefined, () => null);
}

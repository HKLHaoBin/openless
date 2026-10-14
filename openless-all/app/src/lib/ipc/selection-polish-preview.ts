import { invokeOrMock } from './shared';

export interface SelectionPolishPreviewOwner {
  sessionId?: string;
  generation?: number;
}

export interface SelectionPolishPreview extends SelectionPolishPreviewOwner {
  text: string;
  sourceText: string;
}

export interface SelectionPolishPreviewClientEvent {
  event: string;
  requestId?: string;
  sessionId?: string;
  generation?: number;
  attempt?: number;
  epoch?: number;
  textChars?: number;
  sourceChars?: number;
  elapsedMs?: number;
  reason?: string;
  outcome?: string;
}

export function createSelectionPolishPreviewRequestId(): string {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `preview-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function logSelectionPolishPreviewEvent(
  event: SelectionPolishPreviewClientEvent,
): Promise<void> {
  const args: Record<string, unknown> = { ...event };
  return invokeOrMock('log_selection_polish_preview_event', args, () => undefined).catch(() => {
    // Diagnostics must never change the preview flow when the optional logger is unavailable.
  });
}

export function getSelectionPolishPreview(
  owner?: Pick<SelectionPolishPreviewOwner, 'generation'>,
  requestId?: string,
): Promise<SelectionPolishPreview | null> {
  const args = {
    ...(owner?.generation === undefined ? {} : { generation: owner.generation }),
    ...(requestId === undefined ? {} : { requestId }),
  };
  return invokeOrMock('get_selection_polish_preview', Object.keys(args).length ? args : undefined, () => ({
    text: '这里显示润色后的文字。',
    sourceText: '这里显示原始选区。',
  }));
}

export function confirmSelectionPolishPreview(
  text: string,
  owner?: SelectionPolishPreviewOwner | null,
  requestId?: string,
): Promise<void> {
  const args = {
    text,
    ...(owner?.sessionId === undefined ? {} : { sessionId: owner.sessionId }),
    ...(owner?.generation === undefined ? {} : { generation: owner.generation }),
    ...(requestId === undefined ? {} : { requestId }),
  };
  return invokeOrMock('confirm_selection_polish_preview', args, () => undefined);
}

export function cancelSelectionPolishPreview(
  owner?: SelectionPolishPreviewOwner | null,
  requestId?: string,
): Promise<void> {
  const args = {
    ...(owner?.sessionId === undefined ? {} : { sessionId: owner.sessionId }),
    ...(owner?.generation === undefined ? {} : { generation: owner.generation }),
    ...(requestId === undefined ? {} : { requestId }),
  };
  return invokeOrMock('cancel_selection_polish_preview', args, () => undefined);
}

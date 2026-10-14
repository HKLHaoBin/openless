import { invokeOrMock } from './shared';

export interface SelectionPolishPreviewOwner {
  sessionId?: string;
  generation?: number;
}

export interface SelectionPolishPreview extends SelectionPolishPreviewOwner {
  text: string;
  sourceText: string;
}

export function getSelectionPolishPreview(
  owner?: Pick<SelectionPolishPreviewOwner, 'generation'>,
): Promise<SelectionPolishPreview | null> {
  const args = owner?.generation === undefined ? undefined : { generation: owner.generation };
  return invokeOrMock('get_selection_polish_preview', args, () => ({
    text: '这里显示润色后的文字。',
    sourceText: '这里显示原始选区。',
  }));
}

export function confirmSelectionPolishPreview(
  text: string,
  owner?: SelectionPolishPreviewOwner | null,
): Promise<void> {
  const args = {
    text,
    ...(owner?.sessionId === undefined ? {} : { sessionId: owner.sessionId }),
    ...(owner?.generation === undefined ? {} : { generation: owner.generation }),
  };
  return invokeOrMock('confirm_selection_polish_preview', args, () => undefined);
}

export function cancelSelectionPolishPreview(
  owner?: SelectionPolishPreviewOwner | null,
): Promise<void> {
  const args = {
    ...(owner?.sessionId === undefined ? {} : { sessionId: owner.sessionId }),
    ...(owner?.generation === undefined ? {} : { generation: owner.generation }),
  };
  return invokeOrMock('cancel_selection_polish_preview', args, () => undefined);
}

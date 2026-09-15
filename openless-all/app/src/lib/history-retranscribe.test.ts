import { canRetranscribeHistoryEntry } from './history-retranscribe';

function assert(condition: boolean, message: string) {
  if (!condition) throw new Error(message);
}

const archivedEntry = {
  hasAudioRecording: true,
  pipelineMode: 'traditional',
} as const;

assert(
  canRetranscribeHistoryEntry({ ...archivedEntry, errorCode: null }),
  'completed entries with an archived recording should be retranscribable',
);
assert(
  canRetranscribeHistoryEntry({ ...archivedEntry, errorCode: 'polishFailed' }),
  'entries whose polishing failed should still be retranscribable',
);
assert(
  canRetranscribeHistoryEntry({
    ...archivedEntry,
    errorCode: 'transcribeFailed',
  }),
  'entries whose transcription failed should be retranscribable',
);
assert(
  !canRetranscribeHistoryEntry({
    hasAudioRecording: false,
    pipelineMode: 'traditional',
    errorCode: null,
  }),
  'entries without an archived recording should not show retranscription',
);
assert(
  !canRetranscribeHistoryEntry({
    hasAudioRecording: null,
    pipelineMode: undefined,
    errorCode: null,
  }),
  'legacy entries without recording metadata should not show retranscription',
);
assert(
  !canRetranscribeHistoryEntry({
    ...archivedEntry,
    pipelineMode: 'multimodal',
    errorCode: null,
  }),
  'multimodal entries should not show an unsupported retranscription action',
);

console.log('history-retranscribe: all assertions passed');

import type { DictationSession } from './types';

/**
 * 重新转录需要一份仍存在的 WAV 归档。多模态历史目前没有对应的重转录
 * provider 通道，避免展示一个点击后必然失败的按钮。
 *
 * errorCode 不参与判断：成功转录、润色失败和转录失败的条目都可能有可用录音，
 * 用户都应能用同一份音频重新验证当前 ASR provider。
 */
export function canRetranscribeHistoryEntry(
  session: Pick<DictationSession, 'hasAudioRecording' | 'pipelineMode' | 'errorCode'>,
): boolean {
  return session.hasAudioRecording === true && session.pipelineMode !== 'multimodal';
}

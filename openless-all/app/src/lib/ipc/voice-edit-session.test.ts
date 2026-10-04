let calls: string[] = [];

export {};

Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: {
    __TAURI_INTERNALS__: {
      invoke: async (command: string) => {
        calls.push(command);
        if (command === 'get_startup_snapshot') {
          return { contractVersion: '2.0.0', backend: { running: true } };
        }
        if (command === 'get_voice_edit_state') return null;
        return {
          sessionId: 'session-1',
          phase: command === 'start_voice_edit_session' ? 'dictating' : 'preview',
          context: null,
          target: { kind: 'full_field' },
        };
      },
    },
  },
});

try {
  const {
    startVoiceEditSession,
    openVoiceEditWindow,
    closeVoiceEditWindow,
    finalizeVoiceEditDictation,
    startVoiceEditInstruction,
    finalizeVoiceEditInstruction,
    commitVoiceEditSession,
    cancelVoiceEditSession,
    getVoiceEditState,
  } = await import('./voice-edit-session');

  await openVoiceEditWindow();
  await startVoiceEditSession();
  await finalizeVoiceEditDictation();
  await startVoiceEditInstruction();
  await finalizeVoiceEditInstruction();
  await commitVoiceEditSession();
  await cancelVoiceEditSession();
  if ((await getVoiceEditState()) !== null) throw new Error('state query did not return null');
  await closeVoiceEditWindow();

  const expected = [
    'get_startup_snapshot',
    'voice_edit_window_open',
    'start_voice_edit_session',
    'finalize_voice_edit_dictation',
    'start_voice_edit_instruction',
    'stop_voice_edit_instruction',
    'commit_voice_edit',
    'cancel_voice_edit_session',
    'get_voice_edit_state',
    'voice_edit_window_close',
  ];
  if (JSON.stringify(calls) !== JSON.stringify(expected)) {
    throw new Error(`unexpected voice edit command sequence: ${calls.join(',')}`);
  }
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}

console.log('voice-edit-session.test.ts passed');

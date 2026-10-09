// @ts-nocheck — Node-only runtime harness; production IPC remains strictly typed.
import assert from 'node:assert/strict';

const browserWindow = new EventTarget() as EventTarget & {
  __TAURI_INTERNALS__?: { invoke: (command: string, args?: unknown) => Promise<unknown> };
};
Object.defineProperty(globalThis, 'window', {
  configurable: true,
  value: browserWindow,
});

try {
  const shared = await import('./shared.ts?platform-capability-readiness-test');
  const browserCapabilities = await shared.platformCapabilities();
  assert.equal(browserCapabilities.platform, 'desktop');
  assert.equal(shared.isTauri, false);

  const nativeCapabilities = {
    platform: 'desktop' as const,
    supportsDesktopHotkey: true,
    supportsTray: true,
    supportsOverlay: true,
    supportsImeInput: true,
    supportsLocalAsr: true,
    supportsLocalQwen3Mlx: false,
    supportsInAppDictation: false,
    supportsAutoUpdate: true,
  };
  let calls = 0;
  browserWindow.__TAURI_INTERNALS__ = {
    invoke: async (command) => {
      calls += 1;
      assert.equal(command, 'get_platform_capabilities');
      return nativeCapabilities;
    },
  };
  browserWindow.dispatchEvent(new Event(shared.TAURI_READY_EVENT));

  assert.equal(shared.isTauri, true);
  assert.deepEqual(await shared.platformCapabilities(), nativeCapabilities);
  assert.equal(calls, 1);
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}

console.log('shared platform readiness test passed');

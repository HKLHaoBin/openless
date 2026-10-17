import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

const source = readFileSync(new URL('../harmony/entry/src/main/ets/inputmethod/InputSession.ets', import.meta.url), 'utf8');
const stripImports = (text) => text.replace(/\r\n/g, '\n').replace(/^import .*;\n/gm, '');
assert.equal(stripImports(source), stripImports(source.replace(/\r?\n/g, '\r\n')));
const compiled = ts.transpileModule(stripImports(source), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function host(getEditorAttribute, clientOverrides = {}) {
  const schemas = [], inserted = [];
  let pending = '', acknowledgements = 0;
  let timer, recordings = 0;
  class Storage {
    constructor(initial) { this.values = new Map(Object.entries(initial)); }
    setOrCreate(key, value) { this.values.set(key, value); }
    get(key) { return this.values.get(key); }
  }
  class Voice {
    async start() { recordings++; }
    async cancel() {}
  }
  const exports = {};
  vm.runInNewContext(compiled, {
    exports, console, LocalStorage: Storage, VoiceClient: Voice,
    setTimeout: (callback, delay) => {
      if (delay === 800) return setTimeout(callback, 20);
      if (delay === 150 || delay === 60) return setTimeout(callback, 1);
      timer = callback; return -1;
    }, clearTimeout: (handle) => { if (handle !== -1) clearTimeout(handle); },
    initializeRime: async () => {}, testRime: async () => {},
    inputMethodEngine: {
      ENTER_KEY_TYPE_UNSPECIFIED: 0, ENTER_KEY_TYPE_SEND: 1, ENTER_KEY_TYPE_SEARCH: 2,
      ENTER_KEY_TYPE_DONE: 3, ENTER_KEY_TYPE_NEXT: 4, ENTER_KEY_TYPE_GO: 5,
      ENTER_KEY_TYPE_NEWLINE: 6,
      PATTERN_PASSWORD: 10, PATTERN_PASSWORD_NUMBER: 11,
      PATTERN_PASSWORD_SCREEN_LOCK: 12, PATTERN_NEW_PASSWORD: 13,
    },
    rimeRequest: async (operation, session, argument) => {
      if (operation === 'create') { schemas.push(argument); pending = ''; }
      if (operation === 'select') pending = '测试';
      if (operation === 'ack') { acknowledgements++; pending = ''; }
      if (operation === 'clear') pending = '';
      return { session: 'test', revision: 1, input: '', preedit: '', candidates: [], commit: pending, page: 0, hasNext: false };
    },
  });
  const client = { getEditorAttribute, insertText: async (text) => { inserted.push(text); return true; }, ...clientOverrides };
  const session = exports.inputSession, state = exports.imeStorage;
  session.attach({ filesDir: '/test', resourceManager: {} }, {}, client);
  const pump = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
  };
  const drain = async () => {
    await pump();
    await session.queue;
  };
  return { session, state, schemas, inserted, drain, pump, timeout: () => timer(), recordings: () => recordings,
    acknowledgements: () => acknowledgements };
}

const normal = host(async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: 'com.huawei.hmos.settings' }));
await normal.drain();
assert.deepEqual(normal.schemas, ['learning']);
assert.equal(normal.state.get('rimeReady'), true);
assert.equal(normal.state.get('editorVerified'), true);
assert.equal(normal.state.get('enterLabel'), '搜索');
normal.session.selectCandidate(0);
await normal.drain();
assert.equal(normal.acknowledgements(), 1);
assert.equal(normal.state.get('pendingCommit'), false);

let asyncWrites = 0, syncWrites = 0, asyncDeletes = 0;
const bridge = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.droi.tong' }), {
  insertText: async () => { asyncWrites++; return true; },
  insertTextSync: () => { syncWrites++; }, // Public sync API returns void on success.
  deleteForward: async () => { asyncDeletes++; return true; },
});
await bridge.drain();
bridge.session.selectCandidate(0);
await bridge.drain();
assert.equal(syncWrites, 0, 'never block the UI through synchronous IPC');
assert.equal(asyncWrites, 1);
assert.equal(bridge.acknowledgements(), 1);
assert.equal(bridge.state.get('pendingCommit'), false);
bridge.session.backspace();
await bridge.drain();
assert.equal(asyncDeletes, 1);

let accidentalRetries = 0, asynchronousAttempts = 0;
const failedWrite = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.huawei.hmos.settings' }), {
  insertText: async () => { asynchronousAttempts++; return false; },
  insertTextSync: () => { accidentalRetries++; },
});
await failedWrite.drain();
failedWrite.session.selectCandidate(0);
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), true);
assert.equal(failedWrite.acknowledgements(), 0);
assert.equal(accidentalRetries, 0, 'an uncertain write must never automatically replay through another transport');
failedWrite.session.acknowledgePendingCommit();
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), false);
assert.equal(failedWrite.acknowledgements(), 1);
assert.equal(asynchronousAttempts, 1, 'manual confirmation of an applied write must not insert it again');
failedWrite.session.selectCandidate(0);
await failedWrite.drain();
failedWrite.session.cancelPendingCommit();
await failedWrite.drain();
assert.equal(failedWrite.state.get('pendingCommit'), false);
assert.equal(failedWrite.acknowledgements(), 1, 'cancel does not claim another failed write succeeded');

const syncFailure = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.droi.tong' }), {
  insertText: async () => { throw { code: 12800003 }; },
  insertTextSync: () => { throw { code: 12800003 }; },
});
await syncFailure.drain();
syncFailure.session.selectCandidate(0);
await syncFailure.drain();
assert.equal(syncFailure.state.get('pendingCommit'), true);
assert.equal(syncFailure.acknowledgements(), 0);

let anonymousSyncWrites = 0;
const anonymousBridge = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  insertText: async () => true,
  insertTextSync: () => { anonymousSyncWrites++; },
});
await anonymousBridge.drain();
anonymousBridge.session.selectCandidate(0);
await anonymousBridge.drain();
assert.equal(anonymousSyncWrites, 0);
assert.equal(anonymousBridge.state.get('pendingCommit'), false);
assert.equal(anonymousBridge.acknowledgements(), 1);

let observedAttempts = 0;
const observed = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  insertText: () => { observedAttempts++; return new Promise(() => {}); },
});
await observed.drain();
observed.session.observeText('原文字');
observed.session.observeSelection(1, 2);
observed.session.selectCandidate(0);
await observed.pump();
observed.session.observeText('原错误字');
await observed.pump();
assert.equal(observed.acknowledgements(), 0, 'unrelated text must not confirm a commit');
observed.session.observeSelection(3, 3);
observed.session.observeText('原测试字');
await observed.drain();
assert.equal(observed.acknowledgements(), 1, 'exact selected-range replacement confirms an applied edit');
assert.equal(observedAttempts, 1, 'observation must not replay the insertion');
assert.equal(observed.state.get('pendingCommit'), false);

const noReceipt = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  insertText: () => new Promise(() => {}),
});
await noReceipt.drain();
noReceipt.session.selectCandidate(0);
await noReceipt.drain();
assert.equal(noReceipt.state.get('writeInFlight'), false, 'unresponsive IPC has a bounded UI wait');
assert.equal(noReceipt.state.get('pendingCommit'), true);
noReceipt.session.observeText('测试');
noReceipt.session.observeSelection(2, 2);
await noReceipt.pump();
assert.equal(noReceipt.acknowledgements(), 0, 'without a baseline, text alone is insufficient proof');
noReceipt.session.acknowledgePendingCommit();
await noReceipt.drain();
noReceipt.session.selectCandidate(0);
await noReceipt.pump();
noReceipt.session.observeText('测试测试');
noReceipt.session.observeSelection(4, 4);
await noReceipt.drain();
assert.equal(noReceipt.acknowledgements(), 2, 'manual recovery preserves the active-field baseline for the next word');

let fieldText = '原文', fieldCursor = 2, readbackWrites = 0;
const readback = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  getTextIndexAtCursor: async () => fieldCursor,
  getForward: async (length) => fieldText.slice(0, fieldCursor).slice(-length),
  insertText: (text) => {
    readbackWrites++;
    fieldText += text;
    fieldCursor += text.length;
    return new Promise(() => {});
  },
});
await readback.drain();
readback.session.selectCandidate(0);
await readback.drain();
readback.session.selectCandidate(0);
await readback.drain();
assert.equal(readbackWrites, 2);
assert.equal(readback.acknowledgements(), 2, 'readback permits repeated words without native write replies');
assert.equal(fieldText, '原文测试测试');

const unchanged = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: '' }), {
  getTextIndexAtCursor: async () => 2,
  getForward: async () => '测试',
  insertText: async () => false,
});
await unchanged.drain();
unchanged.session.selectCandidate(0);
await unchanged.drain();
assert.equal(unchanged.acknowledgements(), 0, 'an existing identical word is not proof of a new insertion');
assert.equal(unchanged.state.get('pendingCommit'), true);

let releaseOldWrite, hangingAttempts = 0;
const hanging = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => {
    hangingAttempts++;
    return hangingAttempts === 1 ? new Promise((resolve) => { releaseOldWrite = resolve; }) : Promise.resolve(true);
  },
});
await hanging.drain();
hanging.session.selectCandidate(0);
await hanging.pump();
assert.equal(hanging.state.get('pendingCommit'), true);
assert.equal(hanging.state.get('writeInFlight'), true);
// Recovery must work while the original insertion promise is still unresolved.
hanging.session.acknowledgePendingCommit();
await hanging.drain();
assert.equal(hanging.state.get('pendingCommit'), false);
hanging.session.selectCandidate(0);
await hanging.drain();
assert.equal(hangingAttempts, 2);
assert.equal(hanging.acknowledgements(), 2);
releaseOldWrite(false);
await hanging.pump();
assert.equal(hanging.state.get('status'), 'Rime 拼音', 'late failure must not replace recovered state');
assert.equal(hanging.acknowledgements(), 2);

let lateFieldWrite;
const rebound = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'native.test' }), {
  insertText: () => new Promise((resolve) => { lateFieldWrite = resolve; }),
});
await rebound.drain();
rebound.session.selectCandidate(0);
await rebound.pump();
rebound.session.attach({ filesDir: '/test', resourceManager: {} }, {}, {
  getEditorAttribute: async () => ({ inputPattern: 0, enterKeyType: 2, bundleName: 'native.other' }),
  insertText: async () => true,
});
await rebound.drain();
assert.equal(rebound.state.get('rimeReady'), true, 'new field initialization must not wait for old IPC');
assert.equal(rebound.state.get('enterLabel'), '搜索');
assert.equal(rebound.state.get('pendingCommit'), false);
lateFieldWrite(true);
await rebound.pump();
assert.equal(rebound.acknowledgements(), 0, 'late old-field success must not acknowledge the new session');

const rejected = host(async () => { throw { code: 43 }; });
await rejected.drain();
assert.deepEqual(rejected.schemas, ['private']);
assert.equal(rejected.state.get('rimeReady'), true);
assert.equal(rejected.state.get('editorVerified'), false);
assert.equal(rejected.state.get('enterLabel'), '换行');
await rejected.session.startVoice();
assert.equal(rejected.recordings(), 0);
rejected.session.toggleLanguage();
rejected.session.insert('a');
await rejected.drain();
assert.deepEqual(rejected.inserted, ['a'], 'attribute failure must not block ordinary input');

let lateResolve;
const stalled = host(() => new Promise((resolve) => { lateResolve = resolve; }));
await stalled.drain();
assert.equal(stalled.state.get('rimeReady'), false);
stalled.timeout();
await stalled.drain();
assert.deepEqual(stalled.schemas, ['private']);
lateResolve({ inputPattern: 0, enterKeyType: 1 });
await stalled.drain();
assert.deepEqual(stalled.schemas, ['private'], 'late attributes must not upgrade unknown fields to learning');

const password = host(async () => ({ inputPattern: 10, enterKeyType: 0 }));
await password.drain();
assert.equal(password.state.get('password'), true);
assert.equal(password.state.get('chinese'), false);
assert.deepEqual(password.schemas, []);
password.session.observeText('private');
password.session.observeSelection(7, 7);
assert.equal(password.session.observedText, undefined);
assert.equal(password.session.selectionBegin, -1);
await password.session.startVoice();
assert.equal(password.recordings(), 0);

const detached = host(() => new Promise(() => {}));
detached.session.detach();
detached.timeout();
await detached.drain();
assert.deepEqual(detached.schemas, []);
assert.equal(detached.state.get('connected'), false);
console.log('Harmony editor compatibility: transports, commit acknowledgements, cancellation, attributes and privacy passed');

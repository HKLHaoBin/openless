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
    setTimeout: (callback) => { timer = callback; return 1; }, clearTimeout: () => {},
    initializeRime: async () => {}, testRime: async () => {},
    inputMethodEngine: {
      ENTER_KEY_TYPE_UNSPECIFIED: 0, ENTER_KEY_TYPE_SEND: 1, ENTER_KEY_TYPE_SEARCH: 2,
      ENTER_KEY_TYPE_DONE: 3, ENTER_KEY_TYPE_NEXT: 4, ENTER_KEY_TYPE_GO: 5,
      ENTER_KEY_TYPE_NEWLINE: 6,
      PATTERN_PASSWORD: 10, PATTERN_PASSWORD_NUMBER: 11,
      PATTERN_PASSWORD_SCREEN_LOCK: 12, PATTERN_NEW_PASSWORD: 13,
    },
    rimeRequest: async (operation, session, argument) => {
      if (operation === 'create') schemas.push(argument);
      if (operation === 'select') pending = '测试';
      if (operation === 'ack') { acknowledgements++; pending = ''; }
      if (operation === 'clear') pending = '';
      return { session: 'test', revision: 1, input: '', preedit: '', candidates: [], commit: pending, page: 0, hasNext: false };
    },
  });
  const client = { getEditorAttribute, insertText: async (text) => { inserted.push(text); return true; }, ...clientOverrides };
  const session = exports.inputSession, state = exports.imeStorage;
  session.attach({ filesDir: '/test', resourceManager: {} }, {}, client);
  const drain = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
    await session.queue;
  };
  return { session, state, schemas, inserted, drain, timeout: () => timer(), recordings: () => recordings,
    acknowledgements: () => acknowledgements };
}

const normal = host(async () => ({ inputPattern: 0, enterKeyType: 2 }));
await normal.drain();
assert.deepEqual(normal.schemas, ['learning']);
assert.equal(normal.state.get('rimeReady'), true);
assert.equal(normal.state.get('editorVerified'), true);
assert.equal(normal.state.get('enterLabel'), '搜索');
normal.session.selectCandidate(0);
await normal.drain();
assert.equal(normal.acknowledgements(), 1);
assert.equal(normal.state.get('pendingCommit'), false);

let asyncWrites = 0, syncWrites = 0, syncDeletes = 0;
const bridge = host(async () => ({ inputPattern: 0, enterKeyType: 0, bundleName: 'com.droi.tong' }), {
  insertText: async () => { asyncWrites++; return false; },
  insertTextSync: () => { syncWrites++; }, // Public sync API returns void on success.
  deleteForwardSync: () => { syncDeletes++; },
});
await bridge.drain();
bridge.session.selectCandidate(0);
await bridge.drain();
assert.equal(syncWrites, 1);
assert.equal(asyncWrites, 0, 'compatibility transport must be selected before the first write');
assert.equal(bridge.acknowledgements(), 1);
assert.equal(bridge.state.get('pendingCommit'), false);
bridge.session.backspace();
await bridge.drain();
assert.equal(syncDeletes, 1);

let accidentalRetries = 0, asynchronousAttempts = 0;
const failedWrite = host(async () => ({ inputPattern: 0, enterKeyType: 0 }), {
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
  insertTextSync: () => { throw { code: 12800003 }; },
});
await syncFailure.drain();
syncFailure.session.selectCandidate(0);
await syncFailure.drain();
assert.equal(syncFailure.state.get('pendingCommit'), true);
assert.equal(syncFailure.acknowledgements(), 0);

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
await password.session.startVoice();
assert.equal(password.recordings(), 0);

const detached = host(() => new Promise(() => {}));
detached.session.detach();
detached.timeout();
await detached.drain();
assert.deepEqual(detached.schemas, []);
assert.equal(detached.state.get('connected'), false);
console.log('Harmony editor compatibility: transports, commit acknowledgements, cancellation, attributes and privacy passed');

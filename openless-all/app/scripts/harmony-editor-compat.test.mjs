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

function host(getEditorAttribute) {
  const schemas = [], inserted = [];
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
      return { session: 'test', revision: 1, input: '', preedit: '', candidates: [], commit: '', page: 0, hasNext: false };
    },
  });
  const client = { getEditorAttribute, insertText: async (text) => { inserted.push(text); return true; } };
  const session = exports.inputSession, state = exports.imeStorage;
  session.attach({ filesDir: '/test', resourceManager: {} }, {}, client);
  const drain = async () => {
    for (let i = 0; i < 20; i++) await Promise.resolve();
    await session.queue;
  };
  return { session, state, schemas, inserted, drain, timeout: () => timer(), recordings: () => recordings };
}

const normal = host(async () => ({ inputPattern: 0, enterKeyType: 2 }));
await normal.drain();
assert.deepEqual(normal.schemas, ['learning']);
assert.equal(normal.state.get('rimeReady'), true);
assert.equal(normal.state.get('editorVerified'), true);
assert.equal(normal.state.get('enterLabel'), '搜索');

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
console.log('Harmony editor compatibility: rejected/stalled attributes, private sessions, passwords and stale clients passed');

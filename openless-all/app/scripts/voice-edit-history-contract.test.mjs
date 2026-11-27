import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';

// Execute the actual component and its event handlers, replacing only React's
// renderer/hooks and the IPC transport. This is not a native/browser UI test.
const source = readFileSync(
  new URL('../src/features/voice-edit/VoiceEditPanel.tsx', import.meta.url),
  'utf8',
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.CommonJS,
    target: ts.ScriptTarget.ES2022,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
};
const tick = () => new Promise((resolve) => setImmediate(resolve));
const snapshot = (revision = 7, overrides = {}) => ({
  sessionId: '00000000-0000-4000-8000-000000000001',
  phase: 'preview',
  context: {
    fieldText: 'original',
    selection: null,
    dictatedSegment: '',
    draft: 'original',
    preview: 'English draft',
    turns: [
      { instructionRaw: 'translate', instructionPolished: 'translate', editPlan: {}, previewAfter: 'English draft' },
      { instructionRaw: 'HIDDEN_REDO_INSTRUCTION', instructionPolished: 'hidden', editPlan: {}, previewAfter: 'hidden' },
    ],
    previewHistory: ['original', 'English draft', 'hidden'],
    previewHistoryIndex: 1,
    previewRevision: revision,
  },
  target: { kind: 'full_field' },
  ...overrides,
});

function mount(initial = snapshot()) {
  const state = [initial];
  const refs = [];
  const callbacks = [];
  const effects = [];
  const calls = [];
  let stateIndex = 0;
  let refIndex = 0;
  let operation = 0;
  const implementations = { getVoiceEditState: async () => initial };
  const api = new Proxy({}, {
    get: (_, name) => (...args) => {
      calls.push({ name, args });
      return implementations[name]?.(...args) ?? Promise.resolve(initial);
    },
  });
  const jsx = (type, props) => ({ type, props: props ?? {} });
  const hooks = {
    useState(value) {
      const index = stateIndex++;
      if (!(index in state)) state[index] = value;
      return [state[index], (next) => {
        state[index] = typeof next === 'function' ? next(state[index]) : next;
      }];
    },
    useRef(value) {
      const index = refIndex++;
      return (refs[index] ??= { current: value });
    },
    useCallback(callback) { callbacks.push(callback); return callback; },
    useEffect(effect) { effects.push(effect); },
  };
  const scope = {
    exports: {},
    window: { setInterval: () => 1, clearInterval() {} },
    crypto: { randomUUID: () => `00000000-0000-4000-8000-${String(++operation).padStart(12, '0')}` },
    require(name) {
      if (name === 'react') return hooks;
      if (name === 'react/jsx-runtime') return { jsx, jsxs: jsx, Fragment: 'fragment' };
      if (name === 'react-i18next') return { useTranslation: () => ({ t: (key, options) => options?.defaultValue ?? key }) };
      if (name === 'lucide-react') return new Proxy({}, { get: (_, key) => key });
      if (name.endsWith('/api/voice-edit')) return api;
      if (name.endsWith('/ToolWindowHeader')) return { ToolWindowHeader: 'header' };
      if (name.endsWith('/AppViewport')) return { useModalActive: () => false };
      if (name.endsWith('.css')) return {};
      throw new Error(`Uninjected production dependency: ${name}`);
    },
  };
  vm.runInNewContext(compiled, scope);
  let tree;
  const walk = (node, predicate) => {
    if (!node || typeof node !== 'object') return undefined;
    if (predicate(node)) return node;
    for (const child of [node.props?.children].flat(Infinity)) {
      const found = walk(child, predicate);
      if (found) return found;
    }
  };
  const render = () => {
    stateIndex = 0;
    refIndex = 0;
    callbacks.length = 0;
    effects.length = 0;
    tree = scope.exports.VoiceEditPanel({});
  };
  render();
  return {
    calls, implementations, render,
    refresh: () => callbacks[0](),
    activate: () => effects[0](),
    input: () => walk(tree, (node) => node.type === 'input'),
    close: () => walk(tree, (node) => node.type === 'header').props.onClose(),
    button: (key) => {
      const result = walk(tree, (node) => node.type === 'button' && [node.props.children].flat(Infinity).includes(key));
      assert.ok(result, `Missing production button: ${key}`);
      return result;
    },
    text: () => JSON.stringify(tree, (_, value) => typeof value === 'function' ? undefined : value),
    current: () => state[0],
  };
}

// Failed/rejected/busy submissions retain input. IME Enter is not submit.
{
  const ui = mount();
  const pending = deferred();
  ui.implementations.applyVoiceEditTextInstruction = () => pending.promise;
  ui.input().props.onChange({ target: { value: '  调整语气  ' } });
  ui.render();
  ui.input().props.onKeyDown({ key: 'Enter', nativeEvent: { isComposing: true }, preventDefault() { throw new Error('IME consumed'); } });
  assert.equal(ui.calls.length, 0);
  ui.button('voiceEdit.applyInstruction').props.onClick();
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  assert.equal(ui.calls.filter((call) => call.name === 'applyVoiceEditTextInstruction').length, 1);
  assert.equal(ui.input().props.value, '  调整语气  ');
  const sent = ui.calls[0].args;
  assert.equal(sent[1], '调整语气');
  assert.equal(sent[2].expectedRevision, 7);
  assert.ok(sent[2].instructionId);
  pending.reject(new Error('PRIVATE_PROVIDER_RESPONSE: source selection and model output'));
  await tick();
  ui.render();
  assert.equal(ui.input().props.value, '  调整语气  ');
  assert.ok(!ui.text().includes('PRIVATE_PROVIDER_RESPONSE'));
  assert.ok(!ui.text().includes('source selection and model output'));
  assert.ok(!ui.text().includes('HIDDEN_REDO_INSTRUCTION'));
}

// Successful submit clears only its value; commit uses the displayed token.
{
  const ui = mount();
  ui.input().props.onChange({ target: { value: 'change tone' } });
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  await tick();
  ui.render();
  assert.equal(ui.input().props.value, '');
  ui.button('voiceEdit.commit').props.onClick();
  assert.equal(ui.calls.at(-1).name, 'commitVoiceEditSession');
  assert.equal(ui.calls.at(-1).args[1], 7);
}

// Cancellation bypasses the pending model action. Late success/failure cannot
// overwrite a newer edit or clear the newer user's instruction.
for (const fails of [false, true]) {
  const ui = mount();
  const old = deferred();
  const newer = deferred();
  let edits = 0;
  ui.implementations.applyVoiceEditTextInstruction = () => (++edits === 1 ? old.promise : newer.promise);
  ui.input().props.onChange({ target: { value: 'old instruction' } });
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  ui.render();
  ui.button('voiceEdit.cancelInstruction').props.onClick();
  await tick();
  assert.equal(ui.calls[1].name, 'cancelVoiceEditInstruction');
  assert.equal(ui.calls[1].args[1], ui.calls[0].args[2].instructionId);
  assert.ok(!ui.calls.some((call) => call.name === 'cancelVoiceEditSession'));
  ui.render();
  assert.equal(ui.input().props.value, 'old instruction');
  ui.input().props.onChange({ target: { value: 'new instruction' } });
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  if (fails) old.reject(new Error('PRIVATE_LATE_ERROR'));
  else old.resolve(snapshot(99));
  await tick();
  ui.render();
  assert.equal(ui.current().context.previewRevision, 7);
  assert.equal(ui.input().props.value, 'new instruction');
  assert.ok(!ui.text().includes('PRIVATE_LATE_ERROR'));
  newer.resolve(snapshot(8));
  await tick();
  ui.render();
  assert.equal(ui.current().context.previewRevision, 8);
  assert.equal(ui.input().props.value, '');
}

// Unconfirmed cancellation does not hide the outstanding operation's real result.
{
  const ui = mount();
  const pending = deferred();
  ui.implementations.applyVoiceEditTextInstruction = () => pending.promise;
  ui.implementations.cancelVoiceEditInstruction = async () => {
    throw new Error('voiceEditInstructionChanged');
  };
  ui.input().props.onChange({ target: { value: 'retain until success' } });
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  ui.render();
  ui.button('voiceEdit.cancelInstruction').props.onClick();
  await tick();
  pending.resolve(snapshot(8));
  await tick();
  ui.render();
  assert.equal(ui.current().context.previewRevision, 8);
  assert.equal(ui.input().props.value, '');
  assert.ok(!ui.text().includes('voiceEdit.errors.voiceEditUnknownError'));
}

// React's effect cleanup/setup when the embedded panel is hidden and reopened
// must retire old finalizers and reset cancellation presentation state.
{
  const ui = mount();
  const deactivate = ui.activate();
  await tick();
  ui.render();
  const editing = deferred();
  const cancelling = deferred();
  ui.implementations.applyVoiceEditTextInstruction = () => editing.promise;
  ui.implementations.cancelVoiceEditInstruction = () => cancelling.promise;
  ui.input().props.onChange({ target: { value: 'old instruction' } });
  ui.render();
  ui.button('voiceEdit.applyInstruction').props.onClick();
  ui.render();
  ui.button('voiceEdit.cancelInstruction').props.onClick();
  deactivate();
  const deactivateAgain = ui.activate();
  await tick();
  ui.render();
  assert.equal(ui.button('common.cancel').props.disabled, false);
  cancelling.resolve(snapshot());
  editing.resolve(snapshot(99));
  await tick();
  assert.equal(ui.current().context.previewRevision, 7);
  deactivateAgain();
}

// Closing the panel remains explicit whole-session cancellation/dismissal.
{
  const ui = mount();
  ui.close();
  await tick();
  assert.equal(ui.calls[0].name, 'cancelAndCloseVoiceEditSession');
  assert.equal(ui.calls[0].args[0], ui.current().sessionId);
}

// A poll started before a user operation cannot roll back its displayed revision.
{
  const ui = mount();
  const poll = deferred();
  ui.implementations.getVoiceEditState = () => poll.promise;
  const reading = ui.refresh();
  ui.implementations.redoVoiceEditPreview = async () => snapshot(8);
  ui.button('voiceEdit.redo').props.onClick();
  await tick();
  poll.resolve(snapshot(7));
  await reading;
  ui.render();
  assert.equal(ui.current().context.previewRevision, 8);
}

console.log('voice-edit-history-contract.test.mjs passed');

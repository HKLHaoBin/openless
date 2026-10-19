import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

// Exercise the real settings handlers without browser hooks, native IPC or effects.
type Node = { type: unknown; props: Record<string, any> };
let prefs: Record<string, any> = {
  hotkey: { mode: 'toggle' },
  enterStopEnabled: false,
};
let desktop = true;
let saves = 0;
const dependencies: Record<string, any> = {
  useTranslation: () => ({ t: (key: string) => key }),
  useHotkeySettings: () => ({
    prefs,
    capability: { adapter: 'windowsLowLevel' },
    updatePrefs: (next: Record<string, any>) => {
      prefs = next;
      saves += 1;
    },
    refresh: async () => {},
  }),
  useState: (initial: any) => [typeof initial === 'function' ? initial() : initial, () => {}],
  useRef: (initial: unknown) => ({ current: initial }),
  useCallback: (callback: unknown) => callback,
  useEffect: () => {},
  useLayoutEffect: () => {},
  useId: () => 'recording-test',
  getCachedPlatformCapabilities: () => ({
    supportsDesktopHotkey: desktop,
    platform: desktop ? 'windows' : 'android',
  }),
  isHotkeyModeMigrationNoticeActive: () => false,
  isPlainTextInsertionActive: () => false,
  showWindowsOpenlessKeyboardListToggle: () => false,
  showWindowsSendInputNewlineMode: () => false,
  detectOS: () => 'win',
};
const source = readFileSync(new URL('./RecordingInputSection.tsx', import.meta.url), 'utf8');
const parsed = ts.createSourceFile(
  'RecordingInputSection.tsx',
  source,
  ts.ScriptTarget.ES2020,
  true,
  ts.ScriptKind.TSX,
);
const imports = parsed.statements.filter(ts.isImportDeclaration).flatMap((node) => {
  if (node.importClause?.isTypeOnly) return [];
  const bindings = node.importClause?.namedBindings;
  return bindings && ts.isNamedImports(bindings)
    ? bindings.elements.filter((item) => !item.isTypeOnly).map((item) => item.name.text)
    : [];
});
const body = parsed.statements
  .filter((node) => !ts.isImportDeclaration(node))
  .map((node) => node.getFullText(parsed))
  .join('\n');
const compiled = ts.transpileModule(body, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const jsx = (type: unknown, props: Record<string, any>) => ({ type, props });
const render: () => Node = new Function(
  ...imports,
  'exports',
  'require',
  `${compiled};return RecordingInputSection;`,
)(
  ...imports.map((name) => dependencies[name] ?? name),
  {},
  () => ({ jsx, jsxs: jsx }),
);
function pathTo(value: unknown, predicate: (node: Node) => boolean, parents: Node[] = []): Node[] {
  if (Array.isArray(value)) {
    for (const child of value) {
      const found = pathTo(child, predicate, parents);
      if (found.length) return found;
    }
    return [];
  }
  if (!value || typeof value !== 'object' || !('props' in value)) return [];
  const node = value as Node;
  const path = [...parents, node];
  return predicate(node) ? path : pathTo(node.props.children, predicate, path);
}
function enterControl(tree: Node) {
  const path = pathTo(tree, (node) => node.props.label === 'settings.recording.enterStopLabel');
  assert(path.length, 'desktop recording settings must contain the Enter switch');
  const row = path[path.length - 1];
  const control = row.props.children as Node;
  assert.equal(control.type, 'Toggle');
  const group = path.find((node) => Object.prototype.hasOwnProperty.call(node.props, 'aria-hidden'));
  assert(group, 'Enter switch must be in the Toggle-only controls group');
  return { control, group };
}

let tree = render();
let { control, group } = enterControl(tree);
assert.equal(control.props.on, false);
assert.equal(group.props['aria-hidden'], false);
control.props.onToggle(true);
assert.equal(prefs.enterStopEnabled, true);
assert.equal(saves, 1);

for (const mode of ['hold', 'auto', 'toggle']) {
  tree = render();
  const buttonPath = pathTo(
    tree,
    (node) =>
      node.type === 'button' &&
      node.props.children === `settings.recording.mode${mode[0].toUpperCase()}${mode.slice(1)}`,
  );
  assert(buttonPath.length, `expected ${mode} mode control`);
  buttonPath[buttonPath.length - 1].props.onClick();
  assert.equal(prefs.hotkey.mode, mode);
  assert.equal(prefs.enterStopEnabled, true, 'switching mode must retain the Enter preference');
  ({ control, group } = enterControl(render()));
  assert.equal(control.props.on, true);
  assert.equal(group.props['aria-hidden'], mode !== 'toggle');
  assert.equal(Object.prototype.hasOwnProperty.call(group.props, 'inert'), mode !== 'toggle');
}
control.props.onToggle(false);
assert.equal(prefs.enterStopEnabled, false);

// An older frontend snapshot missing the new field must not display it as enabled.
delete prefs.enterStopEnabled;
assert.equal(enterControl(render()).control.props.on, false);
desktop = false;
assert.equal(
  pathTo(render(), (node) => node.props.label === 'settings.recording.enterStopLabel').length,
  0,
  'mobile settings must not offer a desktop keyboard shortcut',
);

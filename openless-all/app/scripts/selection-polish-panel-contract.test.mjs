// 「划词润色结果」不再有独立窗口：它复用选区助手（qa）面板显示第二套 UI
// （可编辑结果 + 「确认并替换」）。本契约锁住这次合并，防止有人把独立窗口加回来
// 或把 /qa 面板拆回去。
//
// 为什么需要它：
//   - 独立窗口 `selection-polish-preview` 曾是第 6 个桌面浮窗（多一个 WebKit 进程
//     + 一套只服务一个弹窗的前端页面）；合并后入口只剩「预览确认」这一个输出模式。
//   - 合并依赖三处配合：后端把窗口指向 qa、前端 QaPanel 认事件、IPC 用 pending
//     标志区分「真的在润色模式」和「残留快照」。任一处回退都会静默失灵（窗口不再
//     出现），所以逐条断言。

import { readFile, stat } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const read = (relativePath) => readFile(`${appRoot}/${relativePath}`, 'utf8');

const violations = [];

const lib = await read('src-tauri/src/lib.rs');
const capabilities = await read('src-tauri/capabilities/default.json');
const commands = await read('src-tauri/src/commands/selection_polish_preview.rs');
const qaPanel = await read('src/pages/QaPanel.tsx');
const appTsx = await read('src/App.tsx');
const mainTsx = await read('src/main.tsx');

// 1. 独立窗口彻底下线：能力表、前端路由、页面文件都不再有它。
const windowCapabilities = JSON.parse(capabilities).windows;
if (windowCapabilities.includes('selection-polish-preview')) {
  violations.push('capabilities/default.json still grants the selection-polish-preview window');
}
if (!windowCapabilities.includes('qa')) {
  violations.push('capabilities/default.json dropped the qa window (the merged panel needs it)');
}
for (const [name, source] of [
  ['src/App.tsx', appTsx],
  ['src/main.tsx', mainTsx],
]) {
  if (source.includes('selection-polish-preview')) {
    violations.push(`${name}: still routes the deleted selection-polish-preview window`);
  }
  if (source.includes('SelectionPolishPreview')) {
    violations.push(`${name}: still imports the deleted SelectionPolishPreview page`);
  }
}
const previewPageExists = await stat(`${appRoot}/src/pages/SelectionPolishPreview.tsx`).then(
  () => true,
  () => false,
);
if (previewPageExists) {
  violations.push('src/pages/SelectionPolishPreview.tsx still exists');
}

// 2. 后端把「润色结果」交给 qa 面板，而不是新窗口。
if (
  !/fn show_selection_polish_preview[\s\S]{0,400}?show_qa_window\(app, "polish-preview"\)/.test(lib)
) {
  violations.push('show_selection_polish_preview no longer opens the qa panel');
}
if (!/emit_to\("qa", SELECTION_POLISH_PREVIEW_SHOWN/.test(lib)) {
  violations.push('the polish-result "shown" event is not emitted to the qa window');
}
if (!/emit_to\("qa", SELECTION_POLISH_PREVIEW_HIDE/.test(lib)) {
  violations.push('the polish-result "hide" event is not emitted to the qa window');
}
if (/WebviewWindowBuilder::new\(\s*app,\s*"selection-polish-preview"/.test(lib)) {
  violations.push('lib.rs builds a selection-polish-preview window again');
}

if (
  !/fn hide_selection_polish_preview[\s\S]{0,400}?clear_selection_polish_preview_pending_for_session/.test(
    lib,
  )
) {
  violations.push('hide_selection_polish_preview no longer clears its matching preview owner');
}

if (
  lib
    .slice(lib.indexOf('macro_rules! app_invoke_handler_mobile'))
    .includes('commands::log_selection_polish_preview_event')
) {
  violations.push('mobile invoke handler registers the desktop-only preview logger');
}

const commandBody = (name) => {
  const start = commands.indexOf(`pub async fn ${name}`);
  if (start < 0) return '';
  const nextCommand = commands.indexOf('\n#[tauri::command]', start + 1);
  return commands.slice(start, nextCommand < 0 ? commands.length : nextCommand);
};
for (const [name, message] of [
  ['get_selection_polish_preview', 'get_selection_polish_preview lost its owner lease gate'],
  [
    'confirm_selection_polish_preview',
    'confirm_selection_polish_preview lost its owner validation',
  ],
  ['cancel_selection_polish_preview', 'cancel_selection_polish_preview lost its owner validation'],
]) {
  if (!commandBody(name).includes('selection_polish_preview_owner()')) violations.push(message);
}

for (const eventName of ['selection-polish-preview:shown', 'selection-polish-preview:hide']) {
  if (!qaPanel.includes(`'${eventName}'`)) {
    violations.push(`QaPanel.tsx does not listen for ${eventName}`);
  }
}
if (!qaPanel.includes('selectionPolishPreview.confirmReplace')) {
  violations.push('QaPanel.tsx dropped the 「确认并替换」 button of the polish-result mode');
}
if (
  !/<textarea[\s\S]*?className="qa-polish-text"[\s\S]*?value=\{polishResult.text\}/.test(qaPanel)
) {
  violations.push('QaPanel.tsx polish-result box is no longer an editable controlled textarea');
}
if (!qaPanel.includes('polishLoadEpochRef') || !qaPanel.includes('polishOwnerRef')) {
  violations.push('QaPanel.tsx dropped preview request/owner guards');
}
if (!qaPanel.includes('polishConfirmInFlightRef')) {
  violations.push('QaPanel.tsx lacks synchronous confirmation single-flight guard');
}
if (
  !qaPanel.includes("event='confirm_ignored'") &&
  !qaPanel.includes("logPolishEvent('confirm_ignored'")
) {
  violations.push('QaPanel.tsx does not diagnose duplicate or empty confirmation clicks');
}
const confirmHandlerStart = qaPanel.indexOf('const onPolishConfirm');
const confirmHandler = qaPanel.slice(
  confirmHandlerStart,
  qaPanel.indexOf('\n  const lastRole', confirmHandlerStart),
);
if (
  confirmHandlerStart < 0 ||
  !/polishConfirmInFlightRef\.current = requestId[\s\S]*?confirmSelectionPolishPreview\(text, owner, requestId\)/.test(
    confirmHandler,
  )
) {
  violations.push('QaPanel.tsx must claim confirmation before invoking native IPC');
}
if (
  !qaPanel.includes('polishDismissedOwnerRef') ||
  !/if\s*\(\s*!polishActiveRef\.current\s*&&\s*currentOwner === null\s*\)/.test(qaPanel) ||
  !qaPanel.includes("reason: 'already_dismissed'")
) {
  violations.push('QaPanel.tsx lost idempotent owner-scoped preview dismissal');
}
if (
  !qaPanel.includes('event.payload') ||
  !/confirmSelectionPolishPreview\(text, owner(?:, requestId)?\)/.test(qaPanel)
) {
  violations.push('QaPanel.tsx no longer carries preview ownership through events and confirm');
}
const escapeCancelStart = qaPanel.indexOf('cancelSelectionPolishPreview(owner, requestId)');
const escapeCancelEnd = qaPanel.indexOf('\n        return;', escapeCancelStart);
const escapeCancelHandler = qaPanel.slice(escapeCancelStart, escapeCancelEnd);
if (
  escapeCancelStart < 0 ||
  !/\.then\(\(\) => \{[\s\S]*?setPolishResult\(null\)[\s\S]*?qaWindowDismiss\(\)/.test(
    escapeCancelHandler,
  ) ||
  !/\.catch\(\(\) => \{[\s\S]*?cancel_error/.test(escapeCancelHandler) ||
  /\.finally\(\(\) => \{[\s\S]*?setPolishResult\(null\)|\.finally\(\(\) => \{[\s\S]*?qaWindowDismiss/.test(
    escapeCancelHandler,
  ) ||
  !escapeCancelHandler.includes('epochAtCancel') ||
  !escapeCancelHandler.includes('ownerStillCurrent') ||
  !/if \(!ownerStillCurrent\)\s*\{[\s\S]*?return;[\s\S]*?setPolishResult\(null\)/.test(
    escapeCancelHandler,
  )
) {
  violations.push('Escape cancellation must close only after successful Core cancellation');
}
const expandedStart = qaPanel.indexOf('const expanded');
if (
  expandedStart < 0 ||
  !qaPanel.slice(expandedStart, expandedStart + 240).includes('polishResult !== null')
) {
  violations.push('QaPanel.tsx expanded layout does not include polishResult');
}

// 6. 五种语言都要有这套文案（合并后由 qa 面板复用同一批 key）。
for (const locale of ['zh-CN', 'zh-TW', 'en', 'ja', 'ko']) {
  const source = await read(`src/i18n/${locale}.ts`);
  if (!source.includes('selectionPolishPreview:')) {
    violations.push(`src/i18n/${locale}.ts lost the selectionPolishPreview strings`);
  }
}

if (violations.length) {
  throw new Error(`Selection polish panel contract failed:\n${violations.join('\n')}`);
}

console.log('selection-polish-panel-contract.test.mjs passed');

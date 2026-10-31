import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { diagnosticBuildArgs } from './run-android-tauri-build.mjs';

const source = readFileSync(new URL('../src/lib/styleZipDiagnostics.ts', import.meta.url), 'utf8');
function load(enabled, invoke) {
  const exports = {};
  const code = ts.transpileModule(source.replace('import.meta.env.VITE_ISSUE1320_DIAGNOSTICS', JSON.stringify(enabled ? '1' : '')), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(code, {
    exports, require: () => ({ invoke }), crypto: { randomUUID: () => '42c8e320-b399-47e7-8664-f31e3c904fcc' },
  });
  return exports;
}
const calls = [];
const api = load(true, async (command, args) => { calls.push({ command, args }); });
const op = api.beginZipDiagnostic();
assert.ok(op);
assert.equal(api.markZipDiagnostic(op, 'picker_selected', { content_uri: true }), undefined);
api.markZipDiagnostic(op, 'ipc_started');
await new Promise((resolve) => setImmediate(resolve));
assert.equal(calls.length, 2);
assert.equal(calls[0].command, 'style_zip_diagnostic_event');
assert.equal(calls[0].args.operationId, op.id);
assert.deepEqual(JSON.parse(JSON.stringify(calls[0].args.facts)), { content_uri: true, source_seq: 1 });
assert.equal(calls[1].args.facts.source_seq, 2);
assert.ok(!JSON.stringify(calls).includes('content://'));

const disabled = load(false, () => { throw new Error('must not invoke'); });
assert.equal(disabled.beginZipDiagnostic(), null);
disabled.markZipDiagnostic(null, 'import_started');
// Rejected diagnostic IPC is consumed; it never rejects/blocks product import.
const failureCalls = [];
const failing = load(true, async (command, args) => {
  if (command === 'export_style_zip_diagnostics') {
    failureCalls.push(args);
    return { path: 'diagnostic.ndjson', dropped: 0, write_failed: false };
  }
  throw new Error('sensitive provider error must not be logged');
});
failing.markZipDiagnostic(failing.beginZipDiagnostic(), 'import_error');
await new Promise((resolve) => setImmediate(resolve));
const failureStatus = await failing.exportZipDiagnostics();
assert.equal(failureCalls[0].frontendFailures, 1);
assert.equal(failureCalls[0].frontendPending, 0);
assert.match(failureStatus, /前端失败 1/);
assert.match(failureStatus, /重载前.*未知/);
assert.ok(!failureStatus.includes('sensitive provider'));
const pending = load(true, (command) => command === 'export_style_zip_diagnostics'
  ? Promise.resolve({ path: 'diagnostic.ndjson', dropped: 0, write_failed: false })
  : new Promise(() => {}));
pending.markZipDiagnostic(pending.beginZipDiagnostic(), 'ipc_started');
assert.match(await pending.exportZipDiagnostics(), /待完成 1/);
assert.deepEqual(diagnosticBuildArgs({}), []);
assert.deepEqual(diagnosticBuildArgs({ OPENLESS_STYLE_ZIP_DIAGNOSTICS: 'true' }), []);
assert.deepEqual(diagnosticBuildArgs({ OPENLESS_STYLE_ZIP_DIAGNOSTICS: '1' }), ['--features', 'issue-1320-diagnostics']);
const workflow = readFileSync(new URL('../../../.github/workflows/android-apk.yml', import.meta.url), 'utf8');
assert.match(workflow, /style_zip_diagnostics:[\s\S]*?default: false/);
assert.match(workflow, /VITE_ISSUE1320_DIAGNOSTICS:.*workflow_dispatch.*inputs.style_zip_diagnostics/);
assert.match(workflow, /OPENLESS_STYLE_ZIP_DIAGNOSTICS:.*workflow_dispatch.*inputs.style_zip_diagnostics/);
console.log('style ZIP diagnostics: opt-in, non-blocking observers, opaque correlation and build flags passed');

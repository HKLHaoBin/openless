import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { diagnosticBuildArgs } from './run-android-tauri-build.mjs';

const source = readFileSync(new URL('../src/lib/styleZipDiagnostics.ts', import.meta.url), 'utf8');
let mockUuidCounter = 0;
function load(enabled, invoke) {
  const exports = {};
  const code = ts.transpileModule(source.replace('import.meta.env?.VITE_ISSUE1320_DIAGNOSTICS', JSON.stringify(enabled ? '1' : '')), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  vm.runInNewContext(code, {
    exports, require: () => ({ invoke }),
    crypto: { randomUUID: () => `42c8e320-b399-47e7-8664-${String(++mockUuidCounter).padStart(12, '0')}` },
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
assert.equal(calls[0].args.producerInstance, api.getZipDiagnosticCoverage().producerInstance);
assert.deepEqual(JSON.parse(JSON.stringify(calls[0].args.facts)), { content_uri: true, source_seq: 1 });
assert.equal(calls[1].args.facts.source_seq, 2);
assert.ok(!JSON.stringify(calls).includes('content://'));

const disabled = load(false, () => { throw new Error('must not invoke'); });
assert.equal(disabled.beginZipDiagnostic(), null);
disabled.markZipDiagnostic(null, 'import_started');
// Failed/pending transports are counted without retaining raw errors or blocking import.
const failing = load(true, async () => { throw new Error('sensitive provider error must not be logged'); });
failing.markZipDiagnostic(failing.beginZipDiagnostic(), 'import_error');
await new Promise((resolve) => setImmediate(resolve));
const failureCoverage = failing.getZipDiagnosticCoverage();
assert.equal(failureCoverage.frontendFailures, 1);
assert.equal(failureCoverage.frontendPending, 0);
assert.ok(!JSON.stringify(failureCoverage).includes('sensitive provider'));
assert.equal(disabled.getZipDiagnosticCoverage(), undefined);
const pending = load(true, () => new Promise(() => {}));
pending.markZipDiagnostic(pending.beginZipDiagnostic(), 'ipc_started');
assert.equal(pending.getZipDiagnosticCoverage().frontendPending, 1);
const remounted = load(true, async () => {});
assert.notEqual(remounted.getZipDiagnosticCoverage().producerInstance, failureCoverage.producerInstance);
assert.equal(remounted.getZipDiagnosticCoverage().frontendFailures, 0);

// The existing settings export is the only UI entry, and passes instance-scoped
// coverage to the ordinary log command so the returned .log includes NDJSON.
const utilitySource = readFileSync(new URL('../src/lib/ipc/utils.ts', import.meta.url), 'utf8');
const utilityExports = {};
const exportCalls = [];
const utilityCode = ts.transpileModule(utilitySource, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
}).outputText;
vm.runInNewContext(utilityCode, {
  exports: utilityExports, navigator: { userAgent: 'Android' },
  require: (name) => {
    if (name === './shared') return { isTauri: true, invokeOrMock: async (command, args) => {
      exportCalls.push({ command, args }); return 'fixture.log';
    } };
    if (name === '../harmonyBridge') return { isHarmonyHost: false };
    if (name === '../styleZipDiagnostics') return failing;
    throw new Error(`unexpected import ${name}`);
  },
});
assert.equal(await utilityExports.exportErrorLog('fixture.log'), 'fixture.log');
assert.equal(exportCalls[0].command, 'export_error_log_to_downloads');
assert.equal(exportCalls[0].args.styleZipCoverage.frontendFailures, 1);
assert.equal(exportCalls[0].args.styleZipCoverage.producerInstance, failureCoverage.producerInstance);
const pageSource = readFileSync(new URL('../src/pages/Style.tsx', import.meta.url), 'utf8');
assert.ok(!pageSource.includes('导出 ZIP 诊断'));
assert.ok(!source.includes('export_style_zip_diagnostics'));
const commands = readFileSync(new URL('../src-tauri/src/commands/style_packs.rs', import.meta.url), 'utf8');
const exportStart = commands.indexOf('pub fn export_style_pack_to_zip(');
const exportEnd = commands.indexOf('// ─────────────────────────── style toggles', exportStart);
assert.ok(exportStart >= 0 && exportEnd > exportStart);
const exportAdapter = commands.slice(exportStart, exportEnd);
assert.match(exportAdapter, /style_zip_transfer::export_zip/);
assert.match(exportAdapter, /write_content_uri\(uri, bytes\)/);
const logCommands = readFileSync(new URL('../src-tauri/src/commands/misc.rs', import.meta.url), 'utf8');
assert.match(logCommands, /append_to_error_log\(&mut bytes, style_zip_coverage\)/);
assert.deepEqual(diagnosticBuildArgs({}), []);
assert.deepEqual(diagnosticBuildArgs({ OPENLESS_STYLE_ZIP_DIAGNOSTICS: 'true' }), []);
assert.deepEqual(diagnosticBuildArgs({ OPENLESS_STYLE_ZIP_DIAGNOSTICS: '1' }), ['--features', 'issue-1320-diagnostics']);
const workflow = readFileSync(new URL('../../../.github/workflows/android-apk.yml', import.meta.url), 'utf8');
assert.match(workflow, /style_zip_diagnostics:[\s\S]*?default: false/);
assert.match(workflow, /VITE_ISSUE1320_DIAGNOSTICS:.*workflow_dispatch.*inputs.style_zip_diagnostics/);
assert.match(workflow, /OPENLESS_STYLE_ZIP_DIAGNOSTICS:.*workflow_dispatch.*inputs.style_zip_diagnostics/);
console.log('style ZIP diagnostics: opt-in, non-blocking observers, opaque correlation and build flags passed');

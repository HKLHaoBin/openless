import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { androidBuildArgs } from './run-android-tauri-build.mjs';

const baseline = ['tauri', 'android', 'build', '--apk', '--split-per-abi', '--target', 'aarch64'];
assert.deepEqual(androidBuildArgs(['aarch64'], '--release', {}), baseline);
for (const value of ['', 'false', '1', 'TRUE']) {
  assert.deepEqual(androidBuildArgs(['aarch64'], '--release', { OPENLESS_STYLE_ZIP_DIAGNOSTICS: value }), baseline);
}
assert.deepEqual(androidBuildArgs(['aarch64'], '--debug', { OPENLESS_STYLE_ZIP_DIAGNOSTICS: 'true' }), [
  ...baseline, '--debug', '--features', 'issue-1320-diagnostics',
]);
assert.deepEqual(androidBuildArgs(['armv7', 'x86_64'], '--release', { OPENLESS_STYLE_ZIP_DIAGNOSTICS: 'true' }), [
  ...baseline.slice(0, -1), 'armv7', 'x86_64', '--features', 'issue-1320-diagnostics',
]);

// Source guards complement (not replace) the executable sink/client tests.
const source = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const style = source('src/pages/Style.tsx');
const handlerStart = style.indexOf('const handleImportZip =');
const handlerEnd = style.indexOf('const handlePublishToMarketplace =', handlerStart);
assert.ok(handlerStart >= 0 && handlerEnd > handlerStart, 'import handler boundaries must exist');
const handler = style.slice(handlerStart, handlerEnd);
assert.match(handler, /filters: \[\{ name: 'Style Pack ZIP', extensions: \['zip'\] \}\]/);
assert.match(handler, /zipPath = typeof picked === 'string' \? picked : null/);
assert.match(handler, /if \(!zipPath\) \{\s*setBusy\(null\);\s*return;/);
assert.match(handler, /finally \{\s*setBusy\(null\)/);
assert.doesNotMatch(handler, /setTimeout|setInterval|sleep|await .*emit\(/);
assert.match(style, /cardRefs\.current\.get\(operation\.packId\)\?\.isConnected/);
assert.match(style, /operation\.appliedPacks === packs/);
assert.doesNotMatch(style, /handleExportZipDiagnostics|diagnosticNotice|diagnosticExporting/);
assert.match(style, /VITE_ISSUE_1320_DIAGNOSTICS === 'true' && isTauri && isAndroid\(\)/);
assert.match(style, /getStyleZipDiagnostics\(/);
const toast = source('src/components/SavedToast.tsx');
assert.match(toast, /durationMs \?\? \(onAction \? 6000 : 800\)/);
assert.match(toast, /onVisibilityChange\(internalVisible && Boolean\(toastRef\.current\?\.isConnected\), saveState\)/);
assert.equal((toast.match(/window\.setTimeout/g) ?? []).length, 1, 'observer must not add a visibility timer');
const backend = source('src-tauri/src/issue1320_diagnostics.rs');
assert.match(backend, /all\(target_os = "android", feature = "issue-1320-diagnostics"\)/);
assert.match(backend, /pub\(crate\) fn include_in_error_log/);
assert.match(backend, /sink\.snapshot_for_export\(facts\)/);
assert.match(backend, /bundle_export\(&bytes, &snapshot\)/);
assert.doesNotMatch(backend, /pub fn export_style_zip_diagnostics|write_public_download/);
const misc = source('src-tauri/src/commands/misc.rs');
assert.match(misc, /pub fn export_error_log_to_downloads\([\s\S]*?include_in_error_log\(bytes, frontend_coverage\)/);
assert.match(source('src/lib/ipc/utils.ts'), /frontendCoverage: zipDiagnosticDelivery\(\)/);
assert.doesNotMatch(backend, /resolve_openless_log_path|openless\.log|log::|sleep\(/);
assert.match(source('src-tauri/Cargo.toml'), /default = \["custom-protocol"\]/);
console.log('style ZIP diagnostic build and observation contracts passed');

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
const handler = style.slice(style.indexOf('const handleImportZip ='), style.indexOf('const handleExportZipDiagnostics ='));
assert.match(handler, /filters: \[\{ name: 'Style Pack ZIP', extensions: \['zip'\] \}\]/);
assert.match(handler, /zipPath = typeof picked === 'string' \? picked : null/);
assert.match(handler, /if \(!zipPath\) \{\s*setBusy\(null\);\s*return;/);
assert.match(handler, /finally \{\s*setBusy\(null\)/);
assert.doesNotMatch(handler, /setTimeout|setInterval|sleep|await .*emit\(/);
assert.match(style, /cardRefs\.current\.get\(operation\.packId\)\?\.isConnected/);
assert.match(style, /zipDiagnostics\.enabled && <div role="status">/);
assert.match(style, /VITE_ISSUE_1320_DIAGNOSTICS === 'true' && isTauri && isAndroid\(\)/);
assert.match(style, /disabled=\{diagnosticExporting\}/);
const toast = source('src/components/SavedToast.tsx');
assert.match(toast, /durationMs \?\? \(onAction \? 6000 : 800\)/);
assert.match(toast, /onVisibilityChange\(internalVisible && Boolean\(toastRef\.current\?\.isConnected\), saveState\)/);
assert.equal((toast.match(/window\.setTimeout/g) ?? []).length, 1, 'observer must not add a visibility timer');
const backend = source('src-tauri/src/issue1320_diagnostics.rs');
assert.match(backend, /all\(target_os = "android", feature = "issue-1320-diagnostics"\)/);
assert.match(backend, /write_public_download\("issue-1320-style-zip\.ndjson", &bytes\)/);
assert.doesNotMatch(backend, /resolve_openless_log_path|openless\.log|log::|sleep\(/);
assert.match(source('src-tauri/Cargo.toml'), /default = \["custom-protocol"\]/);
console.log('style ZIP diagnostic build and observation contracts passed');

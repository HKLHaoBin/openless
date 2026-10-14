import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const read = (path) => readFileSync(join(appRoot, path), 'utf8');
const activity = read('android/kotlin/OpenLessKeyboardSettingsActivity.kt');
const native = read('android/kotlin/OpenLessNative.kt');
const policy = read('android/kotlin/OpenLessMigrationBundlePolicy.kt');
const container = read('crates/openless-core/src/local_migration_bundle/container.rs');
const bridge = read('src-tauri/src/android/native_bridge.rs');
const scaffolding = read('scripts/copy-android-scaffolding.mjs');

assert.match(policy, /openless\.android\.migration-bundle/);
assert.match(activity, /nativeExportLocalMigrationBundle/);
assert.match(activity, /nativeInspectLocalMigrationBundle/);
assert.match(activity, /nativePreviewLocalMigrationBundle/);
assert.match(activity, /nativeImportLocalMigrationBundle/);
assert.match(activity, /launchLegacyImportPicker/);
assert.match(activity, /OpenLessSettingsExport\.import\(/);
assert.match(activity, /OpenLessContentWriter\.writeBytes/);
assert.doesNotMatch(activity, /pendingExportJson/);
assert.match(native, /nativeExportLocalMigrationBundle/);
assert.match(native, /nativePreviewLocalMigrationBundle/);
assert.match(native, /nativeImportLocalMigrationBundle/);
assert.match(container, /BundleMode::ShareRedacted/);
assert.match(container, /CredentialPolicy::PasswordEncrypted/);
assert.match(container, /file\.encrypted\(\)/);
assert.match(container, /safe_entry_name/);
assert.match(bridge, /publish_local_migration_state_changed/);
assert.match(scaffolding, /OpenLessMigrationBundlePolicy\.kt/);
assert.match(scaffolding, /OpenLessMigrationBundleContractTest\.kt/);
assert.match(scaffolding, /OpenLessMigrationBundleInstrumentedTest\.kt/);

console.log('local migration bundle contract passed');

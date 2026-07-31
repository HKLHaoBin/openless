#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const targetPath = fileURLToPath(
  new URL('../src-tauri/gen/android/app/src/main/AndroidManifest.xml', import.meta.url),
);

const RECEIVER_SNIPPET = `<receiver
            android:name=".OpenLessAdbDebugReceiver"
            android:exported="true"
            android:enabled="true">
            <intent-filter>
                <action android:name="com.openless.app.ADB_DUMP_LOG" />
                <action android:name="com.openless.app.ADB_SET_CREDENTIAL" />
                <action android:name="com.openless.app.ADB_SET_ASR_PROVIDER" />
                <action android:name="com.openless.app.ADB_SET_LLM_PROVIDER" />
                <action android:name="com.openless.app.ADB_VALIDATE" />
                <action android:name="com.openless.app.ADB_APPLY_CREDS_JSON" />
            </intent-filter>
        </receiver>`;

function printHelp() {
  console.log(`Usage: node scripts/merge-android-adb-debug-manifest.mjs [options]

Merge ADB specialist debug BroadcastReceiver into generated AndroidManifest.xml.

Options:
  --dry-run   Print planned changes without writing
  --help      Show this help text
`);
}

function parseArgs(argv) {
  let dryRun = false;
  for (const arg of argv) {
    if (arg === '--help' || arg === '-h') {
      printHelp();
      process.exit(0);
    }
    if (arg === '--dry-run') {
      dryRun = true;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return { dryRun };
}

function main() {
  const { dryRun } = parseArgs(process.argv.slice(2));

  if (!existsSync(targetPath)) {
    throw new Error(
      `Generated Android manifest not found: ${targetPath}\nRun "npm run tauri -- android init --ci" first.`,
    );
  }

  let content = readFileSync(targetPath, 'utf8');
  if (content.includes('OpenLessAdbDebugReceiver')) {
    console.log(`ADB specialist receiver already present in ${targetPath}; skipping merge.`);
    return;
  }

  const closingIdx = content.indexOf('</application>');
  if (closingIdx === -1) {
    throw new Error('Target manifest is missing </application>');
  }

  content = `${content.slice(0, closingIdx)}        ${RECEIVER_SNIPPET}\n${content.slice(closingIdx)}`;

  if (dryRun) {
    console.log(`[dry-run] Would merge ADB specialist receiver into ${targetPath}`);
    return;
  }

  writeFileSync(targetPath, content, 'utf8');
  console.log(`Merged ADB specialist receiver into ${targetPath}`);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}

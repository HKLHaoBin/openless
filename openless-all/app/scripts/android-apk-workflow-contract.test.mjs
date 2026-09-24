import assert from 'node:assert/strict';
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
  readFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  ANDROID_ABI_MATRIX,
  parseAndroidAbis,
  toGithubMatrixInclude,
  entryForAbi,
} from './android-abi-matrix.mjs';
import { collectSplitApks } from './collect-android-split-apks.mjs';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const workflowPath = fileURLToPath(
  new URL('../../../.github/workflows/android-apk.yml', import.meta.url),
);

// --- ABI matrix ---
assert.equal(parseAndroidAbis('').map((e) => e.abi).join(','), 'aarch64');
assert.equal(parseAndroidAbis('all').length, 4);
assert.equal(parseAndroidAbis('aarch64,armv7').map((e) => e.abi).join(','), 'aarch64,armv7');
assert.equal(
  parseAndroidAbis('aarch64 aarch64,armv7').map((e) => e.abi).join(','),
  'aarch64,armv7',
);
assert.throws(() => parseAndroidAbis('riscv'), /Unknown Android ABI/);
assert.equal(entryForAbi('x86_64').gradleAbi, 'x86_64');
assert.equal(toGithubMatrixInclude(ANDROID_ABI_MATRIX)[0].rust_target, 'aarch64-linux-android');

const cli = spawnSync(
  process.execPath,
  [join(scriptDir, 'android-abi-matrix.mjs'), 'parse', 'aarch64'],
  { encoding: 'utf8' },
);
assert.equal(cli.status, 0);
assert.equal(JSON.parse(cli.stdout)[0].gradle_abi, 'arm64-v8a');

// --- collectSplitApks with a minimal zip APK ---
function writeMinimalApk(path, abi) {
  const fileName = Buffer.from(`lib/${abi}/libdummy.so`, 'utf8');
  const data = Buffer.from('so');
  const local = Buffer.alloc(30 + fileName.length + data.length);
  local.writeUInt32LE(0x04034b50, 0);
  local.writeUInt16LE(20, 4);
  local.writeUInt16LE(0, 6);
  local.writeUInt16LE(0, 8);
  local.writeUInt16LE(0, 10);
  local.writeUInt16LE(0, 12);
  local.writeUInt32LE(0, 14);
  local.writeUInt32LE(data.length, 18);
  local.writeUInt32LE(data.length, 22);
  local.writeUInt16LE(fileName.length, 26);
  local.writeUInt16LE(0, 28);
  fileName.copy(local, 30);
  data.copy(local, 30 + fileName.length);

  const central = Buffer.alloc(46 + fileName.length);
  central.writeUInt32LE(0x02014b50, 0);
  central.writeUInt16LE(20, 4);
  central.writeUInt16LE(20, 6);
  central.writeUInt16LE(0, 8);
  central.writeUInt16LE(0, 10);
  central.writeUInt16LE(0, 12);
  central.writeUInt16LE(0, 14);
  central.writeUInt32LE(0, 16);
  central.writeUInt32LE(data.length, 20);
  central.writeUInt32LE(data.length, 24);
  central.writeUInt16LE(fileName.length, 28);
  central.writeUInt16LE(0, 30);
  central.writeUInt16LE(0, 32);
  central.writeUInt16LE(0, 34);
  central.writeUInt16LE(0, 36);
  central.writeUInt32LE(0, 38);
  central.writeUInt32LE(0, 42);
  fileName.copy(central, 46);

  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(1, 8);
  eocd.writeUInt16LE(1, 10);
  eocd.writeUInt32LE(central.length, 12);
  eocd.writeUInt32LE(local.length, 16);
  eocd.writeUInt16LE(0, 20);

  writeFileSync(path, Buffer.concat([local, central, eocd]));
}

const tmp = mkdtempSync(join(tmpdir(), 'openless-apk-collect-'));
try {
  const androidRoot = join(tmp, 'android');
  const outDir = join(tmp, 'out');
  const apkDir = join(androidRoot, 'app', 'build', 'outputs', 'apk', 'release');
  mkdirSync(apkDir, { recursive: true });
  writeMinimalApk(join(apkDir, 'app-arm64-v8a-release.apk'), 'arm64-v8a');
  const { outputs } = collectSplitApks({
    mode: 'release',
    label: 'test',
    expectedGradleAbis: ['arm64-v8a'],
    androidRoot,
    outDir,
    version: '9.9.9',
  });
  assert.match(outputs.arm64_v8a_path.replace(/\\/g, '/'), /OpenLess_9\.9\.9_arm64-v8a\.apk$/);
  assert.equal(outputs.arm64_v8a_arch, 'aarch64');
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

// --- workflow contract (#1103) ---
const workflow = readFileSync(workflowPath, 'utf8');
assert.match(workflow, /prefix-key:\s*v1-rust-android-1103/);
assert.doesNotMatch(workflow, /Free disk before artifact upload/);
assert.doesNotMatch(workflow, /rm -rf src-tauri\/target/);
assert.doesNotMatch(workflow, /rm -rf ~\/\.cargo\/registry/);
assert.doesNotMatch(workflow, /rm -rf ~\/\.gradle\/caches/);
assert.match(workflow, /abis:/);
assert.match(workflow, /default:\s*['"]aarch64['"]/);
assert.match(workflow, /fast_profile:/);
assert.match(workflow, /strategy:[\s\S]*matrix:/);
assert.match(workflow, /OPENLESS_ANDROID_TARGETS/);
assert.match(workflow, /CARGO_PROFILE_RELEASE_LTO/);
assert.match(workflow, /first ABI may compile twice|android-studio-script/);
assert.match(workflow, /publish-android-release/);
assert.match(workflow, /download-artifact/);
assert.match(workflow, /Rust cache/);

console.log('android-apk-workflow-contract checks passed');

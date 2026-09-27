import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cargoPath = resolve(appRoot, 'src-tauri/Cargo.toml');
const lockPath = resolve(appRoot, 'src-tauri/Cargo.lock');
const cargo = readFileSync(cargoPath, 'utf8');
const dependency = /^qwen3-asr-rs\s*=\s*\{[^\n]+\}\r?\n/m;

if (!dependency.test(cargo)) {
  throw new Error(`未找到 macOS-only qwen3-asr-rs 依赖：${cargoPath}`);
}

writeFileSync(cargoPath, cargo.replace(dependency, ''));
const lock = readFileSync(lockPath, 'utf8');
if (!lock.includes('name = "qwen3-asr-rs"')) {
  throw new Error(`openless Cargo.lock package 未包含 qwen3-asr-rs：${lockPath}`);
}

// Snapshot the committed Tauri stack before regenerate. `cargo generate-lockfile`
// floats transitive crates (e.g. tauri-runtime 2.12) that break tauri 2.11.2.
const pinPackages = [
  'tauri',
  'tauri-runtime',
  'tauri-runtime-wry',
  'tauri-utils',
  'tauri-build',
  'tauri-macros',
  'tauri-codegen',
  'tauri-plugin',
  'wry',
  'tao',
];
const pins = [];
for (const name of pinPackages) {
  const match = lock.match(new RegExp(`name = "${name}"\\nversion = "([^"]+)"`));
  if (!match) {
    throw new Error(`Cargo.lock missing package to re-pin after qwen3 disable: ${name}`);
  }
  pins.push([name, match[1]]);
}

function runCargo(args) {
  const result = spawnSync('cargo', args, {
    cwd: appRoot,
    stdio: 'inherit',
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`cargo ${args.join(' ')} 失败，退出码：${result.status}`);
  }
}

runCargo(['generate-lockfile', '--manifest-path', cargoPath]);
for (const [name, version] of pins) {
  runCargo([
    'update',
    '--manifest-path',
    cargoPath,
    '-p',
    name,
    '--precise',
    version,
  ]);
}

const regeneratedLock = readFileSync(lockPath, 'utf8');
if (regeneratedLock.includes('name = "qwen3-asr-rs"')) {
  throw new Error(`cargo generate-lockfile 后仍包含 qwen3-asr-rs：${lockPath}`);
}
for (const [name, version] of pins) {
  const match = regeneratedLock.match(new RegExp(`name = "${name}"\\nversion = "([^"]+)"`));
  if (!match || match[1] !== version) {
    throw new Error(
      `re-pin failed for ${name}: expected ${version}, got ${match ? match[1] : 'missing'}`,
    );
  }
}
console.log(
  `[ci] disabled macOS-only qwen3-asr-rs; re-pinned ${pins
    .map(([name, version]) => `${name}@${version}`)
    .join(', ')}`,
);

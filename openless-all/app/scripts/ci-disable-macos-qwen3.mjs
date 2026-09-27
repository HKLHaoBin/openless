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

// Do not run `cargo generate-lockfile`: it floats the Tauri stack (e.g. tauri-runtime
// 2.12 against tauri 2.11.2) and breaks Android/Windows. Surgically drop only the
// macOS-only package entries from the committed lockfile.
const removedNames = new Set();
let next = lock.replace(/\r\n/g, '\n');
const packageBlock =
  /\[\[package\]\]\n(?:(?!\[\[)[^\n]*\n)*?name = "([^"]+)"\n(?:(?!\[\[)[^\n]*\n)*/g;
next = next.replace(packageBlock, (block, name) => {
  if (name === 'qwen3-asr-rs' || name.startsWith('qwen3-asr-rs-')) {
    removedNames.add(name);
    return '';
  }
  return block;
});
if (removedNames.size === 0) {
  throw new Error(`未能从 Cargo.lock 删除 qwen3-asr-rs 包：${lockPath}`);
}

for (const name of removedNames) {
  const depLine = new RegExp(`^\\s*"${name}(?: [^=\\n]+)?",\\n`, 'gm');
  next = next.replace(depLine, '');
}

// Collapse blank runs left by deleted packages so the lockfile stays tidy.
next = next.replace(/\n{3,}/g, '\n\n');
if (!next.endsWith('\n')) {
  next += '\n';
}
writeFileSync(lockPath, next);

const verify = spawnSync(
  'cargo',
  ['metadata', '--locked', '--manifest-path', cargoPath, '--format-version', '1'],
  {
    cwd: appRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  },
);
if (verify.error) {
  throw verify.error;
}
if (verify.status !== 0) {
  throw new Error(
    `cargo metadata --locked 失败（lockfile 手术后不一致）：\n${verify.stderr || verify.stdout}`,
  );
}
if (readFileSync(lockPath, 'utf8').includes('name = "qwen3-asr-rs"')) {
  throw new Error(`lockfile 手术后仍包含 qwen3-asr-rs：${lockPath}`);
}
console.log(
  `[ci] disabled macOS-only qwen3-asr-rs dependency; removed lock packages: ${[
    ...removedNames,
  ].join(', ')}`,
);

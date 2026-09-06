#!/usr/bin/env bash
# Idempotent repository bootstrap for the OpenLess Cloud Agent environment.
# System packages and the Rust toolchain live in the base image/snapshot; this
# script only prepares repository-derived state after checkout.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# The macOS/Linux ASR engines are vendored as git submodules. Cargo's resolver
# reads their manifests on every platform (including Linux), so they must be
# present or `cargo check` fails during dependency resolution.
git submodule update --init --recursive

cd openless-all/app

# Lockfile-pinned frontend dependencies.
npm ci

# The Rust backend's generate_context!() macro requires the built frontend
# (../dist) to exist, so build it before touching cargo.
npm run build

# Pre-fetch and warm the Rust backend so `cargo check`/`cargo test` are fast for
# the agent and to validate the backend compiles as part of setup.
cargo fetch --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml

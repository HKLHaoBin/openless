# Memory Graph — OpenLess live capsule display switching

- slug: openless-capsule-live-display-switch
- path: D:/Dev/worktrees/openless/capsule-live-display-switch
- updated: 2026-10-27

## Summary

Dedicated Windows capsule bugfix worktree: switch from all-screen mirrors to a single display without restarting OpenLess, while preserving continuous streaming transcript presentation.

## Entities and relations

- Main clone (Repo): `D:/Dev/src/openless`; leave its existing checkout and untracked files untouched.
- Worktree (Repo): branch `fix/capsule-live-display-switch`, created from freshly fetched `upstream/beta` at `1fe6ad50d24ba11d4a0fa582e8857ac6c60f083a`.
- Upstream (Repo): `Open-Less/openless`, base `beta`.
- Fork (Repo): `HKLHaoBin/openless`, remote `origin`, intended push destination.
- Tauri host (Service): retains capsule and transcript-rail mirror WebViews; sends window-targeted synthetic idle while hiding unused mirrors.
- Capsule / separate rail (React): receive native presentation events; backend transcript and preference events remain global broadcasts.

## Verified facts

- Tauri Any listeners receive even `emit_to` events addressed to other windows. Global capsule-state subscriptions caused mirror-only idle to exit and clear the primary component in the real-effect VM regression harness.
- Capsule, rail and main-window audio-cue capsule-state subscriptions now use the current WebviewWindow target.
- Native transcript visibility IPC is primary-capsule-only in both the frontend wrapper and Tauri-injected caller validation; retained/closing mirrors cannot hide the primary rail.
- The display-mode restart workaround was removed; unrelated low-memory-mode restart behavior is unchanged.
- Tests cover targeted mirror idle during streaming, mirror reuse, listener cleanup, primary-only visibility ownership, no cue replay, and actual browser DOM retention.
- TypeScript checking, formatting checks, the real-effect regression and focused 11-test Chromium suite passed. Full frontend run: 152/154 scripts passed; the two failures are the untouched Windows/MSYS cache-pruning and emulator-workflow mock path tests. Desktop binaries were not built locally.

## Delivery constraints and remaining gates

- User selected Hermes-native `review-diff-bugs` for the review/fix/re-review gate before commit; then push only to the fork.
- Independent review of the complete uncommitted patch at base `1fe6ad50` returned `no bug found` (delegation `deleg_4e376c0c`). File hashes were checked unchanged after review. The earlier Cursor CLI authentication blocker is no longer applicable; do not require Cursor login for this task.
- Desktop artifacts must be built only by GitHub Actions on this branch, not locally. Existing `release-tauri.yml` supports `workflow_dispatch` with `platform=all` (macOS arm64/x64, Windows x64) and blank `release_tag` for non-publishing branch artifacts.
- After dispatch, use a single `gh run watch RUN_ID --exit-status` per run; on failure retrieve full logs, fix/re-review, push, dispatch again, and watch to completion.
- Native multi-monitor visual acceptance still requires the resulting Windows artifact; frontend tests are not evidence of physical HWND/compositor behavior.
- Full frontend suite on this Windows/MSYS environment has pre-existing cache-pruning and emulator-workflow mock path failures. Their sources/workflow inputs are unchanged from beta; do not broaden the capsule fix to alter them without a causal need.

## Commands

Run frontend checks from `openless-all/app` without invoking local desktop packaging:

- `npx tsc --noEmit`
- `node scripts/capsule-ready-replay.test.mjs`
- `node scripts/windows-ui-config.test.mjs`
- `node scripts/frontend-test-runner.mjs`
- `npx playwright test e2e/capsule-live-display.spec.ts e2e/capsule-display-mode.spec.ts e2e/capsule-reference-motion.spec.ts --project=chromium --reporter=list`

For local browser-only checks, the Playwright preview endpoint can reuse this worktree's Vite dev server on 5325, with fixture server on 5326. No production desktop bundle is needed. The frontend suite regenerates tracked Linux icon assets; restore only those test-generated outputs in this dedicated worktree after the suite, leaving the capsule patch intact.

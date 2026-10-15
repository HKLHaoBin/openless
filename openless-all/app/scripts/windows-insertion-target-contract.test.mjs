import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const adapters = await readFile(
  new URL('../src-tauri/src/core_adapters.rs', import.meta.url),
  'utf8',
);
const events = await readFile(new URL('../src-tauri/src/tauri_events.rs', import.meta.url), 'utf8');
const capsule = await readFile(new URL('../src/components/Capsule.tsx', import.meta.url), 'utf8');
const finalInsert = adapters.match(
  /async fn insert_final\([\s\S]*?(?=\n    async fn copy_fallback)/,
)?.[0];
assert.ok(finalInsert, 'the production final insertion path must exist');

// Native wiring contract: do not touch the system clipboard or steal focus; real window effects
// still require on-device acceptance.
const unavailableTarget = finalInsert.match(
  /if let Err\(error\) = self\.restore_insertion_target\(\) \{([\s\S]*?)\n        \}/,
)?.[1];
assert.ok(
  unavailableTarget,
  'an unavailable original target must reach the configured copy-only fallback',
);
assert.match(
  unavailableTarget,
  /#\[cfg\(target_os = "windows"\)\]\s*if self\.context\.insertion\.allow_non_tsf_fallback\s*\{\s*return self\.copy_fallback\(text\)\.await;\s*\}/,
  'Windows may only copy when fallback is enabled and restoring the original target failed',
);
assert.match(
  unavailableTarget,
  /return Err\(error\);/,
  'disabled fallback must preserve the restore error',
);
assert.doesNotMatch(
  unavailableTarget,
  /insert_final|windows_unicode_fallback|\.insert\(/,
  'a failed target restore must never paste or type into the new focus',
);
assert.match(
  finalInsert,
  /Err\(error\) if error\.is_outcome_unknown\(\) => \{[\s\S]*?return Err\(BackendError::new\(\s*BackendErrorCode::OutcomeUnknown,/,
  'an uncertain TSF delivery must return before any fallback can duplicate it',
);
assert.match(
  events,
  /DictationInsertStatus::CopiedFallback => "已复制，请手动粘贴"/,
  'copied output must not be reported as an insertion failure',
);
assert.match(
  events,
  /BackendEventKind::InsertFallback\(fallback\) => \{[\s\S]*?show_core_insert_fallback\(text, &fallback\.reason\)/,
  'the fallback event must reach the native card',
);
assert.match(
  capsule,
  /'insert:fallback'[\s\S]*?setInsertFallback\(event\.payload \?\? null\)/,
  'the capsule must consume the fallback card payload',
);
assert.match(
  capsule,
  /return <InsertFallbackCard payload=\{insertFallback\} \/>/,
  'the copied text must remain visible in the fallback card',
);

const selection = await readFile(new URL('../src-tauri/src/selection.rs', import.meta.url), 'utf8');
const windowsHost = await readFile(
  new URL('../src-tauri/src/host_document/windows.rs', import.meta.url),
  'utf8',
);
const selectionAnchorRefresh = selection.match(
  /fn refresh_selection_insertion_target_anchor[\s\S]*?(?=\n    \/\/\/|\nfn capture_selection_source_app_hint)/,
)?.[0];
assert.ok(
  selectionAnchorRefresh,
  'selection capture must refresh the UIA anchor after text capture',
);
assert.match(
  selectionAnchorRefresh,
  /selection_insertion_target_is_current\(target\)[\s\S]*?capture_focused_selection_anchor\(&selection\.text\)[\s\S]*?selection_insertion_target_is_current\(target\)[\s\S]*?windows_selection_anchor = Some/,
  'anchor refresh must fence the target before and after the bounded UIA read',
);
assert.match(
  selectionAnchorRefresh,
  /windows_selection_position[\s\S]*?matches_expected\(&selection\.text\)[\s\S]*?capture_focused_selection_anchor\(&selection\.text\)[\s\S]*?matches_anchor\(&anchor\)[\s\S]*?windows_selection_anchor = Some/,
  'anchor refresh must require the pre-capture position and exact offsets before retaining a post-capture anchor',
);
assert.match(
  selection,
  /if let Some\(\(selection, mut insertion_target\)\) = take_prefetched_selection_workspace\(\)[\s\S]*?refresh_selection_insertion_target_anchor\(&mut insertion_target, &selection\)/,
  'a prefetched target must get one final bounded anchor refresh before PreviewConfirm admission',
);
assert.doesNotMatch(
  selectionAnchorRefresh,
  /post_copy_shortcut|simulate_copy_and_read|Ctrl\\+C/,
  'anchor refresh must not simulate another copy or read the clipboard',
);
const captureTarget = selection.match(
  /pub\(crate\) fn capture_selection_insertion_target\([\s\S]*?(?=\n    \/\/\/|\n    pub\(crate\) fn selection_insertion_target_is_captured)/,
)?.[0];
assert.ok(captureTarget, 'the insertion target capture helper must exist');
assert.match(
  captureTarget,
  /capture_selection_insertion_target_impl\(true\)[\s\S]*?capture_focused_selection_position/,
  'the PreviewConfirm capture path must retain a lightweight UIA position before clipboard capture',
);
assert.doesNotMatch(
  captureTarget,
  /capture_focused_selection_anchor/,
  'the initial target snapshot must not depend on an early context-rich UIA anchor read',
);
assert.match(
  selection,
  /pub\(crate\) fn selection_insertion_target_is_captured[\s\S]*?target\.windows\.is_some\(\)/,
  'DirectReplace target capture must remain based on the HWND fingerprint',
);
assert.match(
  selection,
  /pub\(crate\) fn selection_insertion_target_anchor_is_captured[\s\S]*?target\.windows_selection_anchor\.is_some\(\)/,
  'PreviewConfirm must have a separate UIA-anchor readiness predicate',
);
assert.match(
  adapters,
  /fn prepare_preview\([\s\S]*?selection_insertion_target_anchor_is_captured[\s\S]*?target\.preview = true/,
  'PreviewConfirm must reject an anchor-less target before entering preview mode',
);
assert.match(
  adapters,
  /event=capture_bridge_result[\s\S]*?anchor_captured=/,
  'capture diagnostics must expose whether the UIA anchor was retained',
);
assert.match(
  selection,
  /anchor_captured: bool[\s\S]*?target_current=.*anchor_captured=.*capture_id/,
  'capture diagnostics must include anchor_captured metadata without text',
);
const restoreTarget = selection.match(
  /pub\(crate\) fn reactivate_selection_insertion_target[\s\S]*?(?=\n    #\[cfg\(target_os = "macos"\)\])/,
)?.[0];
assert.ok(restoreTarget, 'the production Windows target restore path must exist');
assert.match(
  restoreTarget,
  /if IsIconic\(foreground\)\.as_bool\(\)\s*\{\s*let _ = ShowWindow\(foreground, SW_RESTORE\);\s*\}[\s\S]*?BringWindowToTop\(foreground\)[\s\S]*?SetForegroundWindow\(foreground\)/,
  'a minimized original window must be restored before activation',
);
assert.match(
  restoreTarget,
  /if capture_windows_selection_target\(\)\.as_ref\(\) == Some\(&captured\)/,
  'restoring a window must still verify the original window and control fingerprint',
);
assert.match(
  restoreTarget,
  /AttachThreadInput[\s\S]*?SetFocus\(focused\)/,
  'restoring a window must restore the original focused child',
);
const normalActivationCheck = restoreTarget.indexOf(
  'if capture_windows_selection_target().as_ref() == Some(&captured)',
);
const fallbackFocus = restoreTarget.indexOf('let _ = SetFocus(focused);');
assert.ok(
  normalActivationCheck >= 0 && fallbackFocus > normalActivationCheck,
  'SetFocus must remain a fallback after normal activation is checked so browser editors keep their selection',
);
assert.match(restoreTarget, /for attempt in 0\.\.20/, 'focus restoration must use bounded polling');
assert.match(
  windowsHost,
  /pub\(crate\) struct FocusedSelectionAnchor[\s\S]*?start_offset: i32[\s\S]*?end_offset: i32/,
  'the captured UIA anchor must retain exact document-relative selection offsets',
);
assert.match(
  windowsHost,
  /let moved = range\.MoveEndpointByUnit[\s\S]*?moved < 0 \|\| moved >= \(ObservedInsertion::MAX_DOCUMENT_UTF16 \+ 1\) as i32[\s\S]*?Error::from_win32/,
  'a saturated UIA document offset must fail closed instead of identifying a capped range position',
);
assert.match(
  windowsHost,
  /pub\(crate\) struct FocusedSelectionObservation[\s\S]*?newline_normalized_match/,
  'UIA diagnostics must expose only bounded text-shape metadata',
);
assert.match(
  selection,
  /preview_uia status=.*expected_chars=.*selected_chars=.*newline_normalized_match=/,
  'PreviewConfirm must log selection-shape metadata without logging document text',
);
assert.doesNotMatch(
  selection,
  /preview_uia[^\n]*(?:source_text|replacement_text|expected_selection)/,
  'PreviewConfirm UIA diagnostics must not log selection content',
);

const restoreSelection = windowsHost.match(
  /pub\(crate\) fn restore_focused_selection[\s\S]*?(?=\nunsafe fn runtime_id)/,
)?.[0];
assert.ok(restoreSelection, 'the focused-selection restoration helper must exist');
assert.match(
  restoreSelection,
  /!current_text\.trim\(\)\.is_empty\(\)[\s\S]*?MoveEndpointByUnit[\s\S]*?candidate[\s\S]*?\.Select\(\)/,
  'browser selection restoration must only expand a collapsed UIA caret and select an exact text match',
);
assert.match(
  restoreSelection,
  /let Some\(anchor\) = anchor[\s\S]*?FocusedSelectionRestoreStatus::Unavailable/,
  'selection restoration must fail closed when the original UIA anchor was not captured',
);
assert.match(
  restoreSelection,
  /line_break_units[\s\S]*?unit_counts[\s\S]*?MoveEndpointByUnit/,
  'selection restoration must account for provider-specific newline character units',
);
assert.doesNotMatch(
  restoreSelection,
  /post_copy_shortcut|simulate_copy_and_read/,
  'selection restoration must not fall back to synthetic Ctrl+C',
);
assert.match(
  restoreSelection,
  /Compare\(existing\)[\s\S]*?matching_candidates\.len\(\) > 1[\s\S]*?FocusedSelectionRestoreStatus::Changed[\s\S]*?\.Select\(\)/,
  'selection restoration must fail closed when bounded context matches multiple distinct ranges',
);
assert.doesNotMatch(
  restoreSelection,
  /result=ambiguous[^\n]*source_text|result=ambiguous[^\n]*replacement_text/,
  'ambiguous selection diagnostics must remain metadata-only',
);
assert.match(
  restoreSelection,
  /current[\s\S]*?Compare\(&latest\)[\s\S]*?FocusedSelectionRestoreStatus::Changed[\s\S]*?\.Select\(\)/,
  'selection restoration must revalidate the collapsed caret before selecting a repaired range',
);
assert.match(
  restoreSelection,
  /current_start_offset[\s\S]*?anchor\.start_offset[\s\S]*?current_end_offset[\s\S]*?anchor\.end_offset[\s\S]*?FocusedSelectionRestoreStatus::Changed/,
  'selection restoration must reject a caret or range that moved from the captured document offsets',
);
assert.match(
  adapters,
  /restore_selection_insertion_target_selection\(target, source_text\)[\s\S]*?validate_selection_insertion_target_for_apply\(/,
  'PreviewConfirm must attempt bounded UIA selection restoration before final validation',
);
assert.match(
  adapters,
  /match selection_restore[\s\S]*?SelectionInsertionTargetSelectionRestore::Changed[\s\S]*?BackendErrorCode::Cancelled[\s\S]*?selectionPolishSelectionChanged/,
  'a changed or ambiguous selection restore must fail closed before text-only validation',
);

assert.match(
  adapters,
  /event=capture_bridge_result[\s\S]*?capture_id=[\s\S]*?target_current=/,
  'PreviewConfirm capture diagnostics must correlate the target workspace with an opaque capture id',
);
assert.match(
  adapters,
  /event=selection_restore capture_id=[\s\S]*?event=apply_validation capture_id=/,
  'PreviewConfirm must log selection restoration before final validation for the same capture',
);

console.log('Windows insertion target contract passed');

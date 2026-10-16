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
const diagnosticSources = `${selection}\n${adapters}`;
assert.match(
  diagnosticSources,
  /DEBUG-PREVIEW-1014[\s\S]*?diagnosing-bugs\.v1[\s\S]*?P1[\s\S]*?capture_result/,
  'the instrumented artifact must expose the P1 capture probe',
);
assert.match(
  diagnosticSources,
  /DEBUG-PREVIEW-1014[\s\S]*?P2[\s\S]*?preview_admission/,
  'the instrumented artifact must expose the P2 admission probe',
);
assert.match(
  diagnosticSources,
  /DEBUG-PREVIEW-1014[\s\S]*?P3[\s\S]*?confirmation_validation/,
  'the instrumented artifact must expose the P3 validation probe',
);
assert.match(
  diagnosticSources,
  /DEBUG-PREVIEW-1014[\s\S]*?P4[\s\S]*?insertion_result/,
  'the instrumented artifact must expose the P4 insertion probe',
);
assert.doesNotMatch(
  diagnosticSources,
  /DEBUG-PREVIEW-1014[^\n]*(?:source_text|replacement_text|clipboard|document|window_title|front_app)/,
  'diagnostic probes must not log source, replacement, clipboard, document, or window metadata',
);
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
  /selection_insertion_target_is_current\(target\)[\s\S]*?capture_focused_selection_anchor_with_stage\(&selection\.text\)[\s\S]*?position\.matches_anchor\(&anchor\)[\s\S]*?selection_insertion_target_is_current\(target\)[\s\S]*?windows_selection_anchor = Some/,
  'anchor refresh must fence the target before and after the bounded UIA read and retain only an exact position match',
);
assert.match(
  selectionAnchorRefresh,
  /windows_selection_position[\s\S]*?is_provisional_empty_text\(\)[\s\S]*?matches_expected\(&selection\.text\)[\s\S]*?capture_focused_selection_anchor_with_stage\(&selection\.text\)[\s\S]*?matches_anchor\(&anchor\)/,
  'anchor refresh may bypass only an explicit non-collapsed provisional position and must still require exact post-capture offsets before retaining an anchor',
);
assert.match(
  selectionAnchorRefresh,
  /let should_reacquire_position = target[\s\S]*?windows_selection_position[\s\S]*?map_or\(true, \|position\| position\.is_provisional_empty_text\(\)\);[\s\S]*?if should_reacquire_position[\s\S]*?selection_insertion_target_is_current\(target\)[\s\S]*?capture_focused_selection_position_with_stage\(\)[\s\S]*?windows_selection_position_stage = stage\.as_str\(\)[\s\S]*?windows_selection_position = Some\(position\)[\s\S]*?capture_focused_selection_anchor_with_stage\(&selection\.text\)/,
  'after clipboard capture, a missing or provisional pre-copy position may be reacquired only through a fenced UIA read before exact anchor matching',
);
assert.match(
  selectionAnchorRefresh,
  /let should_reacquire_position = target[\s\S]*?windows_selection_position[\s\S]*?map_or\(true, \|position\| position\.is_provisional_empty_text\(\)\);[\s\S]*?if should_reacquire_position[\s\S]*?capture_focused_selection_position_with_stage\(\)[\s\S]*?windows_selection_position = Some\(position\)/,
  'a provisional empty-text position must be reacquired after clipboard capture instead of being treated as a final anchor candidate',
);
assert.match(
  selectionAnchorRefresh,
  /if should_reacquire_position[\s\S]*?selection_insertion_target_is_current\(target\)[\s\S]*?return "target_changed_before_position"[\s\S]*?capture_focused_selection_position_with_stage\(\)/,
  'position reacquisition must fail closed when the original target changed before the UIA read',
);
assert.match(
  selectionAnchorRefresh,
  /let previous_position = target\.windows_selection_position\.clone\(\)[\s\S]*?previous_position\.matches_position\(&position\)[\s\S]*?return "position_changed_before_anchor"/,
  'a provisional position may only be reacquired when its exact runtime, process, and offset identity is unchanged',
);
assert.match(
  selectionAnchorRefresh,
  /capture_focused_selection_position_with_stage\(\)[\s\S]*?target_changed_before_anchor[\s\S]*?capture_focused_selection_anchor_with_stage\(&selection\.text\)[\s\S]*?target_changed_after_anchor/,
  'reacquired position and anchor must retain the existing pre-read and post-read target fences',
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
  /fn capture_selection_insertion_target_impl\([\s\S]*?(?=\n    \/\/\/|\n    pub\(crate\) fn selection_insertion_target_is_captured)/,
)?.[0];
assert.ok(captureTarget, 'the insertion target capture helper must exist');
assert.match(
  captureTarget,
  /capture_focused_selection_position_with_stage[\s\S]*?windows_selection_position_stage/,
  'the PreviewConfirm capture path must retain the lightweight UIA failure stage before clipboard capture',
);
const positionCapture = windowsHost.match(
  /fn capture_focused_selection_position_once[\s\S]*?(?=\n(?:pub\(crate\) fn capture_focused_selection_position\(\)))/,
)?.[0];
assert.ok(positionCapture, 'the pre-copy UIA position capture helper must exist');
assert.match(
  positionCapture,
  /selection_endpoint_offset[\s\S]*?TextPatternRangeEndpoint_Start[\s\S]*?selection_endpoint_offset[\s\S]*?TextPatternRangeEndpoint_End[\s\S]*?start_offset == end_offset[\s\S]*?start_offset > end_offset/,
  'a collapsed or inverted UIA range must fail closed after exact endpoint capture',
);
assert.match(
  windowsHost,
  /is_position_retryable[\s\S]*?CollapsedSelection/,
  'collapsed UIA ranges must retain the bounded pre-copy retry budget while exposing a distinct stage',
);
assert.match(
  positionCapture,
  /for attempt in 0\.\.FOCUSED_SELECTION_POSITION_ATTEMPTS[\s\S]*?is_position_retryable[\s\S]*?Duration::from_millis/,
  'empty and collapsed UIA selections must use only a bounded pre-copy retry budget',
);
const anchorCapture = windowsHost.match(
  /pub\(crate\) fn capture_focused_selection_anchor_with_stage[\s\S]*?(?=\npub\(crate\) fn capture_focused_selection_anchor\()/,
)?.[0];
assert.ok(anchorCapture, 'the post-copy UIA anchor capture helper must exist');
assert.match(
  anchorCapture,
  /for attempt in 0\.\.FOCUSED_SELECTION_ANCHOR_ATTEMPTS[\s\S]*?EmptySelection[\s\S]*?Duration::from_millis/,
  'transient empty UIA anchors must use only a bounded post-copy retry budget',
);
assert.match(
  anchorCapture,
  /selected_text\.trim\(\)\.is_empty\(\)[\s\S]*?return Err\("empty_selection"\.to_string\(\)\)[\s\S]*?selection_observation\(expected, Some\(&selected_text\)\)/,
  'an empty post-copy UIA text result must enter the bounded empty-selection retry path before expected-text matching',
);
assert.match(
  anchorCapture,
  /selected_text\.trim\(\)\.is_empty\(\)[\s\S]*?selection_range_text\(&pattern, &selected\)/,
  'an empty selected-range text result must be reconstructed from the exact UIA endpoints before failing closed',
);
assert.match(
  windowsHost,
  /unsafe fn selection_range_text[\s\S]*?DocumentRange\(\)[\s\S]*?MoveEndpointByRange[\s\S]*?TextPatternRangeEndpoint_Start[\s\S]*?TextPatternRangeEndpoint_End[\s\S]*?GetText/,
  'UIA range reconstruction must preserve exact document-relative endpoints',
);
assert.doesNotMatch(
  anchorCapture,
  /simulate_copy_and_read|post_copy_shortcut|Ctrl\\+C/,
  'post-copy UIA retries must not touch the clipboard or synthesize input',
);
assert.match(
  positionCapture,
  /let mut provisional_position[\s\S]*?PositionChanged[\s\S]*?matches_position\(current\)[\s\S]*?return \(None, FocusedSelectionCaptureStage::PositionChanged\)/,
  'pre-copy retries must fail closed when a later UIA position changes the provisional identity',
);
assert.doesNotMatch(
  positionCapture,
  /simulate_copy_and_read|post_copy_shortcut|Ctrl\\+C/,
  'pre-copy UIA retries must not touch the clipboard or synthesize input',
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
  /event=capture_bridge_result[\s\S]*?anchor_captured=[\s\S]*?anchor_stage=/,
  'capture diagnostics must expose whether the UIA anchor was retained and why',
);
assert.match(
  selection,
  /position_captured[\s\S]*?anchor_stage/,
  'P1 capture diagnostics must distinguish missing UIA position from missing anchor text',
);
assert.match(
  windowsHost,
  /FocusedSelectionCaptureStage[\s\S]*?focused_element_unavailable[\s\S]*?selection_not_single[\s\S]*?runtime_id_unavailable[\s\S]*?context_after_unavailable[\s\S]*?position_changed/,
  'UIA anchor diagnostics must distinguish metadata-only failure stages',
);
assert.match(
  selection,
  /anchor_captured: bool[\s\S]*?anchor_stage: &\x27static str[\x27[\s\S]*?target_current=.*anchor_captured=.*anchor_stage=.*capture_id/,
  'capture diagnostics must include anchor stage metadata without text',
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
const focusedSelectionObservation = windowsHost.match(
  /pub\(crate\) fn focused_selection_observation[\s\S]*?(?=\npub\(crate\) fn focused_selection_status)/,
)?.[0];
assert.ok(
  focusedSelectionObservation,
  'the focused selection observation helper must exist',
);
assert.match(
  focusedSelectionObservation,
  /GetText[\s\S]*?selected\.trim\(\)\.is_empty\(\)[\s\S]*?selection_range_text\(/,
  'focused selection validation must reconstruct empty selected-range text from exact endpoints',
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
  /current_text\.trim\(\)\.is_empty\(\)[\s\S]*?current_start_offset < current_end_offset[\s\S]*?selection_range_text\(&pattern, &current\)/,
  'an empty non-collapsed UIA selection must reconstruct its text before restore validation',
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
const windowsPreviewValidation = selection.match(
  /pub\(crate\) fn validate_selection_insertion_target_for_apply[\s\S]*?(?=\npub\(crate\) fn restore_selection_insertion_target_selection)/,
)?.[0];
assert.ok(
  windowsPreviewValidation,
  'the PreviewConfirm final validation helper must exist',
);
assert.match(
  windowsPreviewValidation,
  /focused_selection_observation[\s\S]*?focused_selection_matches_anchor[\s\S]*?SelectionChanged/,
  'PreviewConfirm final validation must recheck the exact captured UIA anchor after text validation',
);
assert.match(
  windowsHost,
  /pub\(crate\) fn focused_selection_matches_anchor[\s\S]*?capture_focused_selection_anchor_with_stage[\s\S]*?matches_anchor/,
  'final PreviewConfirm validation must compare a fresh UIA anchor with the captured identity',
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

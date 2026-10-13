import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const imeRoot = join(appRoot, 'windows-ime', 'src');
const textService = readFileSync(join(imeRoot, 'text_service.cpp'), 'utf8');
const editSession = readFileSync(join(imeRoot, 'edit_session.cpp'), 'utf8');
const displayAttribute = readFileSync(join(imeRoot, 'display_attribute.cpp'), 'utf8');
const registry = readFileSync(join(imeRoot, 'registry.cpp'), 'utf8');
const protocol = readFileSync(join(appRoot, 'src-tauri', 'src', 'windows_ime_protocol.rs'), 'utf8');

// The IME runs inside every host process. It must not own threads or block the
// host UI thread: joining a worker from Deactivate deadlocked hosts whenever
// the calling thread held the loader lock.
const imeSources = readdirSync(imeRoot)
  .filter((name) => /\.(cpp|h)$/.test(name))
  .map((name) => [name, readFileSync(join(imeRoot, name), 'utf8')]);
for (const [name, source] of imeSources) {
  assert.doesNotMatch(
    source,
    /std::thread|_beginthreadex|CreateThread\(/,
    `${name}: IME must not create threads inside host processes`,
  );
  assert.doesNotMatch(
    source,
    /CreateNamedPipeW|ConnectNamedPipe/,
    `${name}: IME must not serve named pipes inside host processes`,
  );
  assert.doesNotMatch(
    source,
    /WaitForSingleObject|WaitForMultipleObjects|\.join\(\)|SendMessageTimeoutW\(|SendMessageW\(/,
    `${name}: IME must not block the host UI thread on waits or synchronous sends`,
  );
}
assert.ok(
  !existsSync(join(imeRoot, 'ipc_client.cpp')),
  'the in-host pipe server must stay removed',
);

assert.match(
  textService,
  /message == WM_COPYDATA/,
  'IME should receive submissions as WM_COPYDATA on its message window',
);
assert.match(
  textService,
  /PostMessageW\(message_window_, kRunSubmitMessage/,
  'IME should commit from a posted message at the top of the host message loop',
);
assert.doesNotMatch(
  textService,
  /ChangeWindowMessageFilter/,
  'IME must not let lower-integrity processes inject text into elevated hosts',
);
assert.match(
  textService,
  /STDMETHODIMP OpenLessTextService::Deactivate\(\) \{\s*(?:OpenLessTraceScope trace\(L"Deactivate"\);\s*)?CancelPendingSubmit\(\);\s*DestroyMessageWindow\(\);/,
  'IME deactivation should only cancel the pending submit and destroy its window',
);
assert.match(
  textService,
  /service->AddRef\(\);[\s\S]*service->Release\(\);/,
  'IME message handling should keep the service alive across re-entrant deactivation',
);

// The message protocol is defined twice (C++ and Rust); keep the copies equal.
const sharedConstants = [
  ['kCopyDataSubmit', 'IME_COPYDATA_SUBMIT'],
  ['kCopyDataQuery', 'IME_COPYDATA_QUERY'],
  ['kCopyDataUpdate', 'IME_COPYDATA_UPDATE'],
  ['kCopyDataFinish', 'IME_COPYDATA_FINISH'],
  ['kCopyDataCancel', 'IME_COPYDATA_CANCEL'],
  ['kStatusAccepted', 'IME_STATUS_ACCEPTED'],
  ['kStatusPending', 'IME_STATUS_PENDING'],
  ['kStatusCommitted', 'IME_STATUS_COMMITTED'],
  ['kStatusUnknownToken', 'IME_STATUS_UNKNOWN_TOKEN'],
  ['kStatusBadRequest', 'IME_STATUS_BAD_REQUEST'],
  ['kStatusNotDispatched', 'IME_STATUS_NOT_DISPATCHED'],
  ['kStatusCompositionConflict', 'IME_STATUS_COMPOSITION_CONFLICT'],
  ['kStatusCompositionRecoveryRequired', 'IME_STATUS_COMPOSITION_RECOVERY_REQUIRED'],
];
for (const [nativeName, rustName] of sharedConstants) {
  const nativeValue = textService.match(new RegExp(`${nativeName} = (0x[0-9A-Fa-f]+)`));
  const rustValue = protocol.match(new RegExp(`${rustName}: \\w+ = (0x[0-9A-Fa-f_]+)`));
  assert.ok(nativeValue, `${nativeName} should be defined in text_service.cpp`);
  assert.ok(rustValue, `${rustName} should be defined in windows_ime_protocol.rs`);
  assert.equal(
    Number(rustValue[1].replaceAll('_', '')),
    Number(nativeValue[1]),
    `${nativeName} and ${rustName} must match`,
  );
}
const windowClass = textService.match(/kMessageWindowClassName\[\] = L"([^"]+)"/);
assert.ok(windowClass, 'IME message window class should be defined');
assert.ok(
  protocol.includes(`OPENLESS_IME_MESSAGE_WINDOW_CLASS: &str = "${windowClass[1]}"`),
  'IME message window class must match between C++ and Rust',
);

assert.match(
  editSession,
  /InterlockedIncrement\(&g_object_count\)/,
  'IME edit sessions should keep the COM DLL loaded while TSF holds them',
);
assert.match(
  editSession,
  /InterlockedDecrement\(&g_object_count\)/,
  'IME edit sessions should release the COM DLL lifetime count when destroyed',
);

// Live transcript composition: TSF owns the composition, so the IME must start,
// update, and end it inside edit sessions, underline it, and learn when TSF ends it.
assert.match(
  editSession,
  /context_composition->StartComposition\(/,
  'IME should open the live transcript as a TSF composition',
);
assert.match(
  editSession,
  /composition_->composition->EndComposition\(/,
  'IME should end the composition when the final text replaces it',
);
assert.match(
  editSession,
  /ApplyOpenLessInputDisplayAttribute\(/,
  'IME composition text should carry the OpenLess input display attribute (underline)',
);
assert.match(
  displayAttribute,
  /TF_LS_SOLID[\s\S]*TF_ATTR_INPUT/,
  'IME display attribute should paint a solid input underline',
);
assert.match(
  displayAttribute,
  /InterlockedIncrement\(&g_object_count\)/,
  'IME display attribute COM objects should keep the DLL alive while hosts hold them',
);
assert.match(
  textService,
  /IID_ITfDisplayAttributeProvider/,
  'IME should expose the display attribute provider for composition underline painting',
);
assert.match(
  registry,
  /GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER/,
  'IME registration should advertise the display attribute provider category',
);
assert.match(
  textService,
  /STDMETHODIMP OpenLessTextService::OnCompositionTerminated\(/,
  'IME should learn when TSF ends the composition',
);
assert.match(
  editSession,
  /terminated_text_captured[\s\S]*current_text[\s\S]*!= composition_->terminated_text/,
  'IME must preserve user edits made after TSF terminates a composition',
);
assert.match(
  textService,
  /ReadOpenLessRangeText\(edit_cookie, composition_->terminated_range[\s\S]*terminated_text/,
  'IME must snapshot the terminated composition text before finalization',
);
assert.match(
  textService,
  /OnCompositionTerminated[\s\S]*terminated_text_captured = true/,
  'IME must record whether the terminated composition snapshot was readable',
);
assert.match(
  editSession,
  /if \(!composition_->terminated_text_captured\)[\s\S]*ReadOpenLessRangeText\(edit_cookie, composition_->terminated_range[\s\S]*current_text != composition_->terminated_text[\s\S]*composition_->terminated_range->SetText\(/,
  'IME must verify terminated-range ownership before replacing it with final text',
);
assert.match(
  editSession,
  /else if \(composition_->terminated && composition_->terminated_range != nullptr\)[\s\S]*?else \{[\s\S]*?composition_->Reset\(\);[\s\S]*?return HRESULT_FROM_WIN32\(ERROR_OPERATION_ABORTED\);/,
  'IME cancellation must release latched native state when TSF terminates without a usable range',
);
assert.match(
  editSession,
  /if \(!composition_->terminated_text_captured\)[\s\S]*?return kStatusCompositionConflict;/,
  'IME must reject finalization when the terminated-range ownership snapshot is unavailable',
);

assert.match(
  textService,
  /kCopyDataUpdate[\s\S]*AcceptSubmit\(copy_data, OpenLessEditOp::kUpdate\)/,
  'IME should route composition updates through the submit pipeline',
);
assert.match(
  textService,
  /submit_composition_owner_[\s\S]*CommitTextOnOwnerThread\([\s\S]*submit_composition_owner_/,
  'IME should carry a stable composition owner across submit requests',
);
assert.match(
  editSession,
  /owner_token[\s\S]*composition_owner_/,
  'IME edit sessions should associate composition state with its insertion owner',
);
assert.match(
  textService,
  /submit_composition_process_id_[\s\S]*CommitTextOnOwnerThread\([\s\S]*submit_composition_process_id_/,
  'IME should carry the sender process instance for restart recovery',
);
assert.match(
  textService,
  /kCommitHeaderBytes[\s\S]*composition_process_id[\s\S]*target_window/,
  'plain commits should carry sender process and captured focus for restart recovery',
);
assert.match(
  protocol,
  /encode_commit_submit_payload[\s\S]*sender_process[\s\S]*target_window/,
  'Rust should encode plain-commit recovery metadata',
);
assert.match(
  textService,
  /restart_recovery[\s\S]*kStatusCompositionRecoveryRequired/,
  'IME should require an explicit recovery cancel before restarting a composition',
);
assert.match(
  textService,
  /update_failed_before_composition[\s\S]*dispatched = false/,
  'IME should classify pre-StartComposition update failures as not dispatched',
);
assert.match(
  textService,
  /op == OpenLessEditOp::kCancel[\s\S]*op == OpenLessEditOp::kFinish[\s\S]*accepted async update[\s\S]*dispatched = false/,
  'IME cleanup should clear a latch when async composition never started',
);
assert.doesNotMatch(
  textService,
  /ITfComposition[^\n]*\bWait/,
  'IME composition handling must not wait on the host',
);

// [DEBUG-ISSUE-1320-RESULT] Observer only: never await a probe or log user data.
export const styleZipDiagnosticsEnabled = import.meta.env.VITE_ISSUE1320_DIAGNOSTICS === '1';
export type ZipDiagnosticOperation = { id: string; sequence: number };
type Event =
  | 'import_started' | 'picker_selected' | 'picker_cancelled' | 'ipc_started'
  | 'ipc_ok' | 'import_error' | 'import_settled' | 'list_started'
  | 'list_applied' | 'list_superseded' | 'list_error' | 'row_observed';
type Facts = { count?: number; present?: boolean; visible?: boolean; content_uri?: boolean };
let frontendFailures = 0;
let frontendPending = 0;

export function beginZipDiagnostic(): ZipDiagnosticOperation | null {
  if (!styleZipDiagnosticsEnabled) return null;
  try { return { id: crypto.randomUUID(), sequence: 0 }; } catch { frontendFailures++; return null; }
}

export function markZipDiagnostic(op: ZipDiagnosticOperation | null | undefined, event: Event, facts: Facts = {}): void {
  if (!op || !styleZipDiagnosticsEnabled) return;
  const source_seq = ++op.sequence;
  frontendPending++;
  // Enqueue independently: an unavailable diagnostic IPC must not prevent product work.
  void import('@tauri-apps/api/core').then(({ invoke }) => invoke('style_zip_diagnostic_event', {
    operationId: op.id, event, facts: { ...facts, source_seq },
  })).catch(() => {
    frontendFailures++;
  }).finally(() => { frontendPending--; });
}

export async function exportZipDiagnostics(): Promise<string> {
  const { invoke } = await import('@tauri-apps/api/core');
  const snapshot = { frontendFailures, frontendPending };
  const result = await invoke<{ path: string; dropped: number; write_failed: boolean }>('export_style_zip_diagnostics', snapshot);
  // Counters cover this page lifetime only. Reloads may lose unresolved IPC;
  // explicitly retain that uncertainty instead of claiming complete coverage.
  return `诊断已导出：${result.path}；前端失败 ${snapshot.frontendFailures}，待完成 ${snapshot.frontendPending}；`
    + `后端丢失 ${result.dropped}，写入异常 ${result.write_failed ? '是' : '否'}。页面重载前覆盖未知；请连同此提示反馈。`;
}

// [DEBUG-ISSUE-1320-RESULT] Observer only: never await a probe or log user data.
export const styleZipDiagnosticsEnabled = import.meta.env.VITE_ISSUE1320_DIAGNOSTICS === '1';
export type ZipDiagnosticOperation = { id: string; sequence: number };
type Event =
  | 'import_started' | 'picker_selected' | 'picker_cancelled' | 'ipc_started'
  | 'ipc_ok' | 'import_error' | 'import_settled' | 'list_started'
  | 'list_applied' | 'list_superseded' | 'list_error' | 'row_observed' | 'row_superseded';
type Facts = { count?: number; present?: boolean; visible?: boolean; content_uri?: boolean };
let frontendFailures = 0;
let frontendPending = 0;
let producerInstance: string | null | undefined;

function producerId(): string | null {
  if (producerInstance === undefined) {
    try { producerInstance = crypto.randomUUID(); }
    catch { producerInstance = null; frontendFailures++; }
  }
  return producerInstance;
}

export function beginZipDiagnostic(): ZipDiagnosticOperation | null {
  if (!styleZipDiagnosticsEnabled) return null;
  producerId();
  try { return { id: crypto.randomUUID(), sequence: 0 }; }
  catch { frontendFailures++; return null; }
}

export function markZipDiagnostic(op: ZipDiagnosticOperation | null | undefined, event: Event, facts: Facts = {}): void {
  if (!op || !styleZipDiagnosticsEnabled) return;
  const source_seq = ++op.sequence;
  frontendPending++;
  const producer = producerId();
  // Fire-and-forget: an unavailable diagnostic IPC must not prevent product work.
  void import('@tauri-apps/api/core').then(({ invoke }) => invoke('style_zip_diagnostic_event', {
    operationId: op.id, producerInstance: producer, event, facts: { ...facts, source_seq },
  })).catch(() => { frontendFailures++; }).finally(() => { frontendPending--; });
}

// Only the existing Debug Tools "Export Error Log" exports diagnostic records.
// Backend coverage records declare these counters instance-scoped and previous instances unknown.
export function getZipDiagnosticCoverage(): {
  producerInstance: string | null; frontendFailures: number; frontendPending: number;
} | undefined {
  if (!styleZipDiagnosticsEnabled) return undefined;
  return { producerInstance: producerId(), frontendFailures, frontendPending };
}

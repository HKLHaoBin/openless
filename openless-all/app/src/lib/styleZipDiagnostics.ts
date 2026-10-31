// [DEBUG-STYLE-ZIP-1320] Best-effort probes, never awaited by the product path.
// No queue/retry/timer: missing delivery is a coverage gap, not non-execution.
export type ZipDiagnosticContext = { runId: string; operationId: string };
export type ZipDiagnosticEvent =
  | 'click'
  | 'finished'
  | 'unmounted'
  | 'plugin_loading'
  | 'plugin_ready'
  | 'picker_opening'
  | 'picker_selected'
  | 'picker_cancelled'
  | 'failed'
  | 'list_applied'
  | 'list_superseded'
  | 'list_failed'
  | 'ui_committed'
  | 'feedback_committed';
export type ZipDiagnosticFacts = {
  present?: boolean;
  busy?: boolean;
  count?: number;
  durationMs?: number;
  feedback?: 0 | 1 | 2 | 3;
  stage?: 0 | 1 | 2 | 3;
  errorCode?: 0 | 1 | 2 | 3 | 4;
};

/** Classify locally; never serialize the source exception or arbitrary message. */
export function zipDiagnosticErrorCode(error: unknown): 0 | 1 | 2 | 3 | 4 {
  try {
    const message = (typeof error === 'string' ? error : error instanceof Error ? error.message : '').toLowerCase();
    if (message.includes('no activity found to handle intent')) return 1;
    if (message === 'file picker cancelled') return 2;
    if (message.includes('permission denied') || message.includes('securityexception')) return 3;
    if (message.includes('not allowed') || message.includes('not found') && message.includes('plugin')) return 4;
  } catch {
    // Even hostile/nonstandard exceptions must leave product error handling untouched.
  }
  return 0;
}
export type ZipDiagnosticCoverage = {
  enabled: boolean;
  runId: string | null;
  status: string;
  dropped: number;
  nativeActivityCovered: boolean;
};

type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
const opaque = /^[a-f0-9]{32}$/;

export function zipDiagnosticFeedback(
  operation: ZipDiagnosticContext,
  owner: ZipDiagnosticContext | undefined,
  state: 'idle' | 'saved' | 'failed' | 'saving',
): 0 | 1 | 2 | 3 | undefined {
  if (!owner || operation.runId !== owner.runId || operation.operationId !== owner.operationId)
    return undefined;
  return state === 'saved' ? 1 : state === 'failed' ? 2 : state === 'saving' ? 3 : 0;
}

export function createStyleZipDiagnostics(enabled: boolean, invoke: Invoke) {
  let instanceId: string | undefined;
  if (enabled) {
    try {
      instanceId = crypto.randomUUID().replace(/-/g, '');
    } catch {
      // An unknown instance must not be represented as whole-run accounting.
    }
  }
  let coverage: ZipDiagnosticCoverage | undefined;
  let sequence = 0;
  let skipped = 0;
  let failed = 0;
  let inFlight = 0;
  const summary = () => ({ coverage, skipped, failed, inFlight, instanceId });
  const refresh = async () => {
    if (!enabled) return summary();
    try {
      coverage = await invoke<ZipDiagnosticCoverage>('style_zip_diagnostic_context');
    } catch {
      coverage = undefined;
      failed++;
    }
    return summary();
  };
  const emit = (
    event: ZipDiagnosticEvent,
    correlation: ZipDiagnosticContext | undefined,
    facts: ZipDiagnosticFacts = {},
  ) => {
    if (!enabled) return;
    const eventSeq = ++sequence;
    if (!correlation || !coverage?.enabled || !opaque.test(correlation.operationId)) {
      skipped++;
      return;
    }
    // Explicitly copy allowlisted fields; never spread a caller's error/path/pack data.
    const safe = {
      eventSeq,
      present: facts.present,
      busy: facts.busy,
      count:
        facts.count === undefined
          ? undefined
          : Math.min(1_000_000, Math.max(0, Math.floor(facts.count))),
      durationMs:
        facts.durationMs === undefined
          ? undefined
          : Math.min(4_294_967_295, Math.max(0, Math.floor(facts.durationMs))),
      feedback: facts.feedback,
      stage: facts.stage,
      errorCode: facts.errorCode,
    };
    inFlight++;
    try {
      void invoke('record_style_zip_diagnostic', { event, correlation, facts: safe })
        .catch(() => {
          failed++;
        })
        .finally(() => {
          inFlight--;
        });
    } catch {
      failed++;
      inFlight--;
    }
  };
  const start = (): ZipDiagnosticContext | undefined => {
    if (!enabled) return undefined;
    let correlation: ZipDiagnosticContext | undefined;
    try {
      if (coverage?.enabled && coverage.runId && opaque.test(coverage.runId)) {
        correlation = { runId: coverage.runId, operationId: crypto.randomUUID().replace(/-/g, '') };
      }
    } catch {
      // Missing entropy/context disables observation only, never the import action.
    }
    emit('click', correlation, { busy: true });
    return correlation;
  };
  return { enabled, refresh, emit, start, summary };
}

// One producer per WebView realm; survives Style page unmounts. A WebView reload
// creates a new random producer instance and retains unknown previous coverage.
let shared: ReturnType<typeof createStyleZipDiagnostics> | undefined;
export function getStyleZipDiagnostics(enabled: boolean, invoke: Invoke) {
  if (!enabled) return createStyleZipDiagnostics(false, invoke);
  shared ??= createStyleZipDiagnostics(true, invoke);
  return shared;
}

export function zipDiagnosticDelivery() {
  if (!shared) return undefined;
  const value = shared.summary();
  const bounded = (count: number) => Math.min(4_294_967_295, Math.max(0, count));
  return {
    instanceId: value.instanceId,
    skipped: bounded(value.skipped),
    failed: bounded(value.failed),
    pending: bounded(value.inFlight),
  };
}

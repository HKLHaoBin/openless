import { createStyleZipDiagnostics, getStyleZipDiagnostics, zipDiagnosticDelivery, zipDiagnosticErrorCode, zipDiagnosticFeedback, type ZipDiagnosticFacts } from './styleZipDiagnostics';

const assert = {
  equal(actual: unknown, expected: unknown, message = 'values differ') {
    if (actual !== expected) throw new Error(message);
  },
  ok(value: unknown) {
    if (!value) throw new Error('expected truthy value');
  },
  match(value: string, expression: RegExp) {
    if (!expression.test(value)) throw new Error('pattern mismatch');
  },
  deepEqual(actual: unknown, expected: unknown) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('objects differ');
  },
  doesNotThrow(action: () => unknown, _message: string) { action(); },
};

const runId = '0123456789abcdef0123456789abcdef';
const calls: { command: string; args?: Record<string, unknown> }[] = [];
const invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
  calls.push({ command, args });
  return { enabled: true, runId, status: 'ready', dropped: 0, nativeActivityCovered: false } as T;
};
const off = createStyleZipDiagnostics(false, invoke);
await off.refresh();
assert.equal(off.start(), undefined);
off.emit('failed', undefined);
assert.equal(calls.length, 0, 'default builds must not invoke diagnostic commands');

const client = createStyleZipDiagnostics(true, invoke);
assert.equal(client.start(), undefined, 'missing context cannot block import or invent correlation');
assert.equal(client.summary().skipped, 1);
await client.refresh();
const context = client.start();
assert.ok(context);
if (!context) throw new Error('expected generated context');
assert.match(context.operationId, /^[0-9a-f]{32}$/);
assert.equal(context.runId, runId);
const unsafeFacts = { present: true, count: 2_000_000, durationMs: 42.8, error: 'secret', uri: 'content://private' };
client.emit('ui_committed', context, unsafeFacts as ZipDiagnosticFacts);
await Promise.resolve();
await Promise.resolve();
const records = calls.filter(({ command }) => command === 'record_style_zip_diagnostic');
assert.equal(records.length, 2);
assert.equal(records[0].args?.event, 'click');
assert.equal(records[1].args?.event, 'ui_committed');
assert.deepEqual(records[1].args?.facts, {
  eventSeq: 3, present: true, busy: undefined, count: 1_000_000, durationMs: 42,
  feedback: undefined, stage: undefined, errorCode: undefined,
});
assert.ok(!JSON.stringify(records).includes('secret'));
assert.ok(!JSON.stringify(records).includes('content://'));

const rejected = createStyleZipDiagnostics(true, async <T>(command: string): Promise<T> => {
  if (command === 'style_zip_diagnostic_context') return { enabled: true, runId, status: 'ready' } as T;
  throw new Error('sensitive provider error');
});
await rejected.refresh();
assert.doesNotThrow(() => rejected.start(), 'probe rejection must never reject product import');
await Promise.resolve();
await Promise.resolve();
assert.equal(rejected.summary().failed, 1);

const mismatch = createStyleZipDiagnostics(true, async <T>(): Promise<T> => ({ enabled: false, runId: null, status: 'disabled' } as T));
await mismatch.refresh();
assert.equal(mismatch.start(), undefined, 'frontend-on/backend-off must be fail-safe');
assert.equal(mismatch.summary().skipped, 1);
console.log('style ZIP diagnostic client tests passed');
assert.equal(zipDiagnosticErrorCode('No Activity found to handle Intent { content://private }'), 1);
assert.equal(zipDiagnosticErrorCode(new Error('File picker cancelled')), 2);
assert.equal(zipDiagnosticErrorCode(new Error('Permission denied: secret')), 3);
assert.equal(zipDiagnosticErrorCode('plugin dialog not found'), 4);
assert.equal(zipDiagnosticErrorCode({ message: 'content://private' }), 0);
assert.equal(zipDiagnosticErrorCode('unknown private provider text'), 0);

// Prior/unrelated toasts may stay visible while another picker is pending.
const other = { runId, operationId: 'abcdef0123456789abcdef0123456789' };
assert.equal(zipDiagnosticFeedback(context, other, 'saved'), undefined);
assert.equal(zipDiagnosticFeedback(context, undefined, 'saved'), undefined);
assert.equal(zipDiagnosticFeedback(context, context, 'saved'), 1);
assert.equal(zipDiagnosticFeedback(context, context, 'failed'), 2);

// A remount/recreated WebView starts a new accounting instance, never a new
// claim that the same backend run had no earlier lost diagnostic deliveries.
const remounted = createStyleZipDiagnostics(true, invoke);
assert.match(client.summary().instanceId ?? '', /^[a-f0-9]{32}$/);
assert.match(remounted.summary().instanceId ?? '', /^[a-f0-9]{32}$/);
assert.ok(client.summary().instanceId !== remounted.summary().instanceId);
assert.equal(rejected.summary().failed, 1);
assert.equal(remounted.summary().failed, 0);

// The central export retains the producer across page remounts and reports
// an unresolved transport without waiting for it or hiding it behind backend loss.
assert.equal(zipDiagnosticDelivery(), undefined);
const unresolvedInvoke = async <T>(command: string): Promise<T> => {
  if (command === 'style_zip_diagnostic_context')
    return { enabled: true, runId, status: 'ready' } as T;
  return new Promise<T>(() => {});
};
const sharedClient = getStyleZipDiagnostics(true, unresolvedInvoke);
await sharedClient.refresh();
sharedClient.start();
assert.equal(zipDiagnosticDelivery()?.pending, 1);
assert.equal(getStyleZipDiagnostics(true, unresolvedInvoke), sharedClient);
assert.equal(zipDiagnosticDelivery()?.instanceId, sharedClient.summary().instanceId);

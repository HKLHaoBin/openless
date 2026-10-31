function assertEqual(actual: unknown, expected: unknown): void {
  if (actual !== expected) throw new Error(`expected ${String(expected)}, received ${String(actual)}`);
}
import {
  beginZipDiagnostic,
  getZipDiagnosticCoverage,
  markZipDiagnostic,
  styleZipDiagnosticsEnabled,
} from './styleZipDiagnostics';

// Exercise the real module under Node/tsx, without replacing import.meta.env.
// Shared IPC/locale tests load it outside Vite; absence must be opt-out, not a crash.
assertEqual(import.meta.env, undefined);
assertEqual(styleZipDiagnosticsEnabled, false);
assertEqual(beginZipDiagnostic(), null);
assertEqual(getZipDiagnosticCoverage(), undefined);
markZipDiagnostic(null, 'import_started');
console.log('style ZIP diagnostics: plain Node import defaults safely to disabled');

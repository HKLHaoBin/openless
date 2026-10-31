import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { load } from 'js-yaml';

const workflow = load(
  readFileSync(new URL('../../../.github/workflows/android-apk.yml', import.meta.url), 'utf8'),
);
const input = workflow.on.workflow_dispatch.inputs.style_zip_diagnostics;
assert.equal(input.type, 'boolean');
assert.equal(input.default, false);
const build = workflow.jobs['build-android-apk'];
const expression = build.env.OPENLESS_STYLE_ZIP_DIAGNOSTICS;
assert.equal(build.env.VITE_ISSUE_1320_DIAGNOSTICS, expression);
assert.equal(build.env.OPENLESS_BUILD_ID, '${{ github.sha }}');
// Exercise the actual expression: tags, schedules, and default dispatches must stay off.
assert.match(expression, /^\$\{\{[\s\S]*\}\}$/);
const body = expression.slice(3, -2).trim();
for (const [event, ref, enabled, expected] of [
  ['workflow_dispatch', 'refs/heads/fix/issue-1320', true, 'true'],
  ['workflow_dispatch', 'refs/heads/fix/issue-1320', false, 'false'],
  ['workflow_dispatch', 'refs/heads/fix/issue-1320', undefined, 'false'],
  ['workflow_dispatch', 'refs/tags/v2.0.2-tauri', true, 'false'],
  ['push', 'refs/tags/v2.0.2-tauri', true, 'false'],
  ['schedule', 'refs/heads/beta', true, 'false'],
]) {
  assert.equal(
    runInNewContext(body, {
      github: { event_name: event, ref },
      inputs: { style_zip_diagnostics: enabled },
      startsWith: (value, prefix) => value.startsWith(prefix),
    }),
    expected,
    `${event} ${ref} ${enabled}`,
  );
}
assert.equal(workflow.jobs['publish-android-release'].if, "needs.plan.outputs.is_tag_release == 'true'");
const evidence = build.steps.find((step) => step.name === 'Record APK source and checksum');
assert.ok(evidence);
assert.equal(evidence.env.APK_PATH, '${{ steps.apk.outputs.apk_path }}');
assert.match(evidence.run, /hashlib\.file_digest/);
assert.match(evidence.run, /'source_sha': os\.environ\['GITHUB_SHA'\]/);
assert.match(evidence.run, /'style_zip_diagnostics':/);
const upload = build.steps.find((step) => step.name === 'Upload APK build evidence');
assert.equal(upload.with['if-no-files-found'], 'error');
assert.equal(upload.with.path, 'android-build-evidence.json');

const ci = load(readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8'));
const android = ci.jobs['android-check'];
assert.ok(
  android.steps.some((step) => step.run?.includes('--features issue-1320-diagnostics')),
  'CI must compile the diagnostic Android host, not just feature-off production',
);

assert.ok(
  Object.values(ci.jobs).some((job) =>
    job.steps?.some((step) => step.run === 'python scripts/test-style-zip-diagnostics.py'),
  ),
  'CI must execute the isolated real sink tests',
);
console.log('issue 1320 diagnostic build/workflow contracts passed');

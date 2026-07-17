#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MODE="strict"
MANIFEST="$ROOT/tests/parity/manifest.json"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --automated)
      MODE="automated"
      shift
      ;;
    --manifest)
      [ "$#" -ge 2 ] || { echo "--manifest requires a path" >&2; exit 2; }
      MANIFEST="$2"
      shift 2
      ;;
    *)
      echo "usage: $0 [--automated] [--manifest PATH]" >&2
      exit 2
      ;;
  esac
done

node - "$ROOT" "$MANIFEST" "$MODE" <<'NODE'
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

const [root, manifestArgument, mode] = process.argv.slice(2);
const productionManifest = path.join(root, 'tests/parity/manifest.json');
const manifestPath = path.resolve(manifestArgument);

function fail(message) {
  process.stderr.write(`parity gate failed: ${message}\n`);
  process.exit(1);
}

function regularFile(candidate, label) {
  let stat;
  try {
    stat = fs.lstatSync(candidate);
  } catch {
    fail(`${label} is missing: ${candidate}`);
  }
  if (!stat.isFile() || stat.isSymbolicLink()) {
    fail(`${label} must be a regular non-symlink file: ${candidate}`);
  }
}

function evidencePath(value, label) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) {
    fail(`${label} has an invalid path`);
  }
  const candidate = path.isAbsolute(value) ? value : path.join(root, value);
  regularFile(candidate, label);
  return candidate;
}

regularFile(manifestPath, 'parity manifest');
let manifest;
try {
  manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
} catch {
  fail('parity manifest is not valid JSON');
}

if (manifest.schemaVersion !== 1 || manifest.scope !== 'v1') {
  fail('parity manifest must declare schemaVersion 1 and scope v1');
}
if (!Array.isArray(manifest.automatedSuites) || manifest.automatedSuites.length === 0) {
  fail('automatedSuites must be a non-empty array');
}
if (!Array.isArray(manifest.behaviors) || manifest.behaviors.length === 0) {
  fail('behaviors must be a non-empty array');
}

const suites = new Map();
for (const suite of manifest.automatedSuites) {
  if (!suite || typeof suite.id !== 'string' || !/^[a-z0-9][a-z0-9.-]*$/.test(suite.id)) {
    fail('automated suite has an invalid id');
  }
  if (suites.has(suite.id)) fail(`duplicate automated suite: ${suite.id}`);
  if (!Array.isArray(suite.command) || suite.command.length === 0 ||
      suite.command.some((part) => typeof part !== 'string' || part.length === 0 || part.includes('\0'))) {
    fail(`automated suite ${suite.id} has an invalid command`);
  }
  suites.set(suite.id, suite.command);
}

const seenBehaviors = new Set();
const pendingBehaviors = [];
for (const behavior of manifest.behaviors) {
  if (!behavior || typeof behavior.id !== 'string' || !/^[a-z0-9][a-z0-9._-]*$/.test(behavior.id)) {
    fail('behavior has an invalid id');
  }
  if (seenBehaviors.has(behavior.id)) fail(`duplicate behavior: ${behavior.id}`);
  seenBehaviors.add(behavior.id);
  if (typeof behavior.area !== 'string' || !behavior.area ||
      typeof behavior.requirement !== 'string' || !behavior.requirement) {
    fail(`behavior ${behavior.id} is missing area or requirement`);
  }
  if (!['required', 'excluded'].includes(behavior.disposition)) {
    fail(`behavior ${behavior.id} has an invalid disposition`);
  }
  if (!behavior.evidence || typeof behavior.evidence.kind !== 'string') {
    fail(`behavior ${behavior.id} has no evidence declaration`);
  }

  if (behavior.disposition === 'excluded') {
    if (behavior.status !== 'excluded' || behavior.evidence.kind !== 'approved_exclusion') {
      fail(`excluded behavior ${behavior.id} is not an approved exclusion`);
    }
    if (typeof behavior.target !== 'string' || behavior.target === 'v1' || behavior.target.length === 0) {
      fail(`excluded behavior ${behavior.id} needs a post-v1 target`);
    }
    const source = evidencePath(behavior.evidence.path, `exclusion evidence for ${behavior.id}`);
    const selector = behavior.evidence.selector;
    if (typeof selector !== 'string' || !fs.readFileSync(source, 'utf8').includes(selector)) {
      fail(`exclusion evidence selector is missing for ${behavior.id}`);
    }
    continue;
  }

  if (behavior.status === 'pending') {
    if (!['missing_implementation', 'pending_manual'].includes(behavior.evidence.kind) ||
        typeof behavior.evidence.blocker !== 'string' || behavior.evidence.blocker.length === 0) {
      fail(`pending behavior ${behavior.id} needs an explicit blocker`);
    }
    pendingBehaviors.push(behavior.id);
    continue;
  }
  if (behavior.status !== 'pass' || behavior.evidence.kind !== 'automated_test') {
    fail(`required behavior ${behavior.id} must be pass or explicitly pending`);
  }
  if (!suites.has(behavior.evidence.suite)) {
    fail(`behavior ${behavior.id} references an unknown automated suite`);
  }
  const source = evidencePath(behavior.evidence.path, `automated evidence for ${behavior.id}`);
  const selector = behavior.evidence.selector;
  if (typeof selector !== 'string' || selector.length === 0 || !fs.readFileSync(source, 'utf8').includes(selector)) {
    fail(`automated evidence selector is missing for ${behavior.id}`);
  }
}

if (!manifest.manualAcceptance || !Array.isArray(manifest.manualAcceptance.requiredChecks) ||
    manifest.manualAcceptance.requiredChecks.length === 0 || !Array.isArray(manifest.manualAcceptance.rows)) {
  fail('manualAcceptance must declare requiredChecks and rows');
}
const requiredChecks = manifest.manualAcceptance.requiredChecks;
if (new Set(requiredChecks).size !== requiredChecks.length ||
    requiredChecks.some((check) => typeof check !== 'string' || !/^[a-z][a-z0-9_]*$/.test(check))) {
  fail('manualAcceptance requiredChecks are invalid or duplicated');
}

const seenRows = new Set();
const pendingRows = [];
for (const row of manifest.manualAcceptance.rows) {
  if (!row || typeof row.id !== 'string' || seenRows.has(row.id)) fail('manual row id is invalid or duplicated');
  seenRows.add(row.id);
  if (typeof row.application !== 'string' || !row.application || typeof row.bundleId !== 'string' || !row.bundleId) {
    fail(`manual row ${row.id} is missing application identity`);
  }
  if (row.availability !== 'available') {
    fail(`manual row ${row.id} must be available for the declared acceptance matrix`);
  }
  if (!Array.isArray(row.fixtures) || row.fixtures.length === 0) fail(`manual row ${row.id} has no fixtures`);
  for (const fixture of row.fixtures) evidencePath(fixture, `fixture for ${row.id}`);

  if (row.status === 'pending') {
    if (row.evidence !== null) fail(`pending manual row ${row.id} must not claim evidence`);
    pendingRows.push(row.id);
    continue;
  }
  if (row.status !== 'pass' || typeof row.evidence !== 'string') {
    fail(`manual row ${row.id} must be pending or pass with evidence`);
  }
  const receiptPath = evidencePath(row.evidence, `manual evidence for ${row.id}`);
  let receipt;
  try {
    receipt = JSON.parse(fs.readFileSync(receiptPath, 'utf8'));
  } catch {
    fail(`manual evidence for ${row.id} is not valid JSON`);
  }
  if (receipt.schemaVersion !== 1 || receipt.rowId !== row.id || receipt.result !== 'pass' ||
      !receipt.checks || typeof receipt.checks !== 'object') {
    fail(`manual evidence for ${row.id} has an invalid identity or result`);
  }
  if (!receipt.fixtureSha256 || typeof receipt.fixtureSha256 !== 'object') {
    fail(`manual evidence for ${row.id} lacks fixture hashes`);
  }
  for (const fixture of row.fixtures) {
    const fixturePath = evidencePath(fixture, `fixture for ${row.id}`);
    const declared = receipt.fixtureSha256[fixture] ?? '';
    const actual = crypto.createHash('sha256').update(fs.readFileSync(fixturePath)).digest('hex');
    if (!/^[0-9a-f]{64}$/.test(declared) || declared !== actual) {
      fail(`manual evidence for ${row.id} has the wrong SHA-256 for ${fixture}`);
    }
  }
  if (manifestPath === productionManifest) {
    if (receipt.application !== row.application || receipt.bundleId !== row.bundleId ||
        typeof receipt.applicationVersion !== 'string' || receipt.applicationVersion.length === 0 ||
        !receipt.environment || receipt.environment.platform !== 'macOS' ||
        receipt.environment.architecture !== 'arm64' ||
        typeof receipt.environment.osVersion !== 'string' || receipt.environment.osVersion.length === 0 ||
        typeof receipt.generatedAt !== 'string' || Number.isNaN(Date.parse(receipt.generatedAt))) {
      fail(`manual evidence for ${row.id} lacks the exact application or environment identity`);
    }
    if (!receipt.output || !/^[0-9a-f]{64}$/.test(receipt.output.markdownSha256 ?? '') ||
        !Number.isInteger(receipt.output.bytes) || receipt.output.bytes <= 0 ||
        !Array.isArray(receipt.warnings) || receipt.warnings.some((warning) => typeof warning !== 'string')) {
      fail(`manual evidence for ${row.id} lacks bounded output or warning evidence`);
    }
  }
  for (const check of requiredChecks) {
    const value = receipt.checks[check];
    const pass = value === 'pass';
    const notApplicable = value && value.result === 'not_applicable' &&
      typeof value.reason === 'string' && value.reason.length > 0;
    if (!pass && !notApplicable) fail(`manual evidence for ${row.id} is missing required check ${check}`);
  }
}

if (manifestPath === productionManifest) {
  const requiredBehaviorIds = [
    'desktop.viewer_open', 'desktop.editor_dirty', 'desktop.save', 'desktop.save_as',
    'desktop.close_confirmation', 'desktop.find', 'desktop.preview_gfm',
    'desktop.preferences_theme', 'desktop.export_html', 'desktop.export_pdf',
    'conversion.direct', 'conversion.progress_cancel', 'conversion.warnings',
    'conversion.open_result', 'converter.pdf', 'converter.html', 'converter.csv',
    'converter.json', 'converter.xml', 'converter.zip', 'converter.epub',
    'converter.docx', 'converter.pptx', 'converter.xlsx', 'converter.image_metadata',
    'print.install_repair_uninstall', 'print.closed_start', 'print.warm_start',
    'print.native_save_as', 'print.cancellation_cleanup', 'print.warning_cleanup',
    'output.gfm_assets', 'output.transaction_cleanup', 'converter.image_ocr',
    'converter.scanned_pdf_ocr',
    'exclusion.youtube'
  ];
  const expectedRows = ['safari', 'mail', 'textedit', 'preview', 'word', 'excel', 'powerpoint'];
  for (const id of requiredBehaviorIds) if (!seenBehaviors.has(id)) fail(`required v1 behavior is absent: ${id}`);
  for (const id of expectedRows) if (!seenRows.has(id)) fail(`required manual application row is absent: ${id}`);
  if (seenBehaviors.size !== requiredBehaviorIds.length) fail('production manifest contains an unapproved behavior id');
  if (seenRows.size !== expectedRows.length) fail('production manifest contains an unapproved manual application row');
}

for (const [id, command] of suites) {
  process.stdout.write(`parity suite: ${id}\n`);
  const result = spawnSync(command[0], command.slice(1), { cwd: root, stdio: 'inherit', env: process.env });
  if (result.error || result.status !== 0) fail(`automated suite failed: ${id}`);
}

if (mode === 'strict' && (pendingBehaviors.length > 0 || pendingRows.length > 0)) {
  if (pendingBehaviors.length) process.stderr.write(`pending behaviors: ${pendingBehaviors.join(', ')}\n`);
  if (pendingRows.length) process.stderr.write(`pending manual rows: ${pendingRows.join(', ')}\n`);
  fail('strict parity requires every approved behavior and manual row to pass');
}

process.stdout.write(`${JSON.stringify({
  schemaVersion: 1,
  mode,
  result: 'pass',
  behaviors: manifest.behaviors.length,
  manualRows: manifest.manualAcceptance.rows.length,
  pendingBehaviors,
  pendingManualRows: pendingRows,
})}\n`);
NODE

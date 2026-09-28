import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { basename, dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, '..');
const repoRoot = join(appDir, '..', '..');
const manifest = join(repoRoot, 'models', 'manifest.toml');
const resourcesModels = join(appDir, 'src-tauri', 'resources', 'models');
const baselineStatuses = new Set(['selected', 'selected_derived']);

function parseString(block, key) {
  return block.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, 'm'))?.[1];
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

if (!existsSync(manifest)) {
  throw new Error(`model manifest missing at ${manifest}`);
}

const blocks = readFileSync(manifest, 'utf8')
  .split(/\n\[\[models\]\]\n/)
  .slice(1);
const baseline = blocks
  .map((block) => ({
    id: parseString(block, 'id'),
    status: parseString(block, 'conversion_status'),
    cachePath: parseString(block, 'cache_path'),
    sha256: parseString(block, 'sha256'),
    externalDataSha256: parseString(block, 'external_data_sha256'),
  }))
  .filter((model) => baselineStatuses.has(model.status));

const missing = [];
for (const model of baseline) {
  if (!model.id || !model.cachePath || !model.sha256) {
    throw new Error(`manifest baseline entry is incomplete: ${JSON.stringify(model)}`);
  }
  const source = join(repoRoot, model.cachePath);
  if (!existsSync(source)) {
    missing.push(`${model.id}: missing ${relative(repoRoot, source)}`);
    continue;
  }
  const actual = sha256(source);
  if (actual !== model.sha256) {
    throw new Error(
      `${model.id}: SHA-256 mismatch for ${relative(repoRoot, source)}\nexpected ${model.sha256}\nactual   ${actual}`,
    );
  }
  if (model.externalDataSha256) {
    const sidecar = source.replace(/\.onnx$/, '.data');
    if (!existsSync(sidecar)) {
      missing.push(`${model.id}: missing ${relative(repoRoot, sidecar)}`);
      continue;
    }
    const sidecarActual = sha256(sidecar);
    if (sidecarActual !== model.externalDataSha256) {
      throw new Error(
        `${model.id}: SHA-256 mismatch for ${relative(repoRoot, sidecar)}\nexpected ${model.externalDataSha256}\nactual   ${sidecarActual}`,
      );
    }
  }
}

if (missing.length > 0) {
  throw new Error(
    [
      'Missing verified baseline model files:',
      ...missing.map((line) => `  - ${line}`),
      '',
      'Populate models/cache first: run `cargo xtask fetch-models`, then copy or generate',
      'the derived gesture_embedder/canned_gesture_classifier artifacts with',
      '`tools/training/convert.py convert-gesture` (Python 3.12).',
      'TODO: CI should source this baseline from the signed OTA baseline pack.',
    ].join('\n'),
  );
}

rmSync(resourcesModels, { recursive: true, force: true });
mkdirSync(join(resourcesModels, 'cache'), { recursive: true });
copyFileSync(manifest, join(resourcesModels, 'manifest.toml'));

for (const model of baseline) {
  const source = join(repoRoot, model.cachePath);
  copyFileSync(source, join(resourcesModels, 'cache', basename(source)));
  if (model.externalDataSha256) {
    const sidecar = source.replace(/\.onnx$/, '.data');
    copyFileSync(sidecar, join(resourcesModels, 'cache', basename(sidecar)));
  }
}

console.log(`Prepared ${baseline.length} verified baseline models in ${resourcesModels}`);

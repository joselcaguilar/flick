import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, '..');
const repoRoot = join(appDir, '..', '..');
const uiDir = join(repoRoot, 'ui');
const srcTauri = join(appDir, 'src-tauri');
const bundle = join(repoRoot, 'target', 'debug', 'bundle', 'macos', 'Flick.app');
const macosDir = join(bundle, 'Contents', 'MacOS');
const entitlements = join(srcTauri, 'entitlements.plist');
const identifier = 'app.flick.desktop';

function run(command, args, options = {}) {
  execFileSync(command, args, { cwd: appDir, stdio: 'inherit', ...options });
}

function requirePath(path, description) {
  if (!existsSync(path)) {
    throw new Error(`${description} missing at ${path}`);
  }
}

if (process.platform !== 'darwin') {
  throw new Error('bundle:local is only supported on macOS');
}

run('pnpm', ['--dir', uiDir, 'build']);
run('pnpm', ['tauri', 'build', '--debug', '--bundles', 'app']);

requirePath(bundle, 'Flick.app bundle');
requirePath(entitlements, 'macOS entitlements');

for (const binary of ['flick-engine', 'flick-desktop']) {
  const path = join(macosDir, binary);
  requirePath(path, binary);
  run('codesign', [
    '--force',
    '--sign',
    '-',
    '--options',
    'runtime',
    '--entitlements',
    entitlements,
    '--identifier',
    identifier,
    path,
  ]);
}

run('codesign', [
  '--force',
  '--sign',
  '-',
  '--options',
  'runtime',
  '--entitlements',
  entitlements,
  '--identifier',
  identifier,
  bundle,
]);
run('codesign', ['--verify', '--deep', '--strict', bundle]);
console.log(`Built, signed, and verified ${bundle}`);

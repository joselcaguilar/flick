import { copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, '..');
const repoRoot = join(appDir, '..', '..');
const srcTauri = join(appDir, 'src-tauri');
// Local debug bundles still ship an optimized engine: an unoptimized engine cannot keep up with
// camera-rate decode, inference, and preview encoding.
const release =
  process.argv.includes('--release') ||
  process.env.FLICK_SIDECAR_RELEASE === '1' ||
  process.env.TAURI_ENV_DEBUG !== 'true';

function run(command, args, options = {}) {
  execFileSync(command, args, { cwd: repoRoot, stdio: 'inherit', ...options });
}

const rustc = execFileSync('rustc', ['-vV'], { cwd: repoRoot, encoding: 'utf8' });
const host = rustc.split('\n').find((line) => line.startsWith('host: '))?.slice('host: '.length).trim();
if (!host) throw new Error('could not determine Rust host triple');

const exe = process.platform === 'win32' ? '.exe' : '';
const profile = release ? 'release' : 'debug';
const source = join(repoRoot, 'target', profile, `flick-engine${exe}`);
if (existsSync(source)) {
  rmSync(source, { force: true });
}

run('cargo', ['build', '-p', 'flick-engine', ...(release ? ['--release'] : [])]);

if (!existsSync(source)) throw new Error(`engine binary missing at ${source}`);

const outDir = join(srcTauri, 'binaries');
mkdirSync(outDir, { recursive: true });
const dest = join(outDir, `flick-engine-${host}${exe}`);
copyFileSync(source, dest);
console.log(`Prepared sidecar ${dest}`);

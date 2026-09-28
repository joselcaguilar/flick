import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, '..');
const repoRoot = join(appDir, '..', '..');
const srcTauri = join(appDir, 'src-tauri');
const release = process.argv.includes('--release') || process.env.TAURI_ENV_DEBUG !== 'true';

function run(command, args, options = {}) {
  execFileSync(command, args, { cwd: repoRoot, stdio: 'inherit', ...options });
}

const rustc = execFileSync('rustc', ['-vV'], { cwd: repoRoot, encoding: 'utf8' });
const host = rustc.split('\n').find((line) => line.startsWith('host: '))?.slice('host: '.length).trim();
if (!host) throw new Error('could not determine Rust host triple');

run('cargo', ['build', '-p', 'flick-engine', ...(release ? ['--release'] : [])]);

const exe = process.platform === 'win32' ? '.exe' : '';
const profile = release ? 'release' : 'debug';
const source = join(repoRoot, 'target', profile, `flick-engine${exe}`);
if (!existsSync(source)) throw new Error(`engine binary missing at ${source}`);

const outDir = join(srcTauri, 'binaries');
mkdirSync(outDir, { recursive: true });
const dest = join(outDir, `flick-engine-${host}${exe}`);
copyFileSync(source, dest);
console.log(`Prepared sidecar ${dest}`);

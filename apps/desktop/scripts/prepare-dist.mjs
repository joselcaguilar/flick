import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const appDir = join(here, '..');
const uiDist = join(appDir, '..', '..', 'ui', 'dist');
const placeholder = join(appDir, 'placeholder');
const out = join(appDir, 'dist');
const source = existsSync(join(uiDist, 'index.html')) ? uiDist : placeholder;

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(source, out, { recursive: true });
console.log(`Prepared desktop frontend from ${source}`);

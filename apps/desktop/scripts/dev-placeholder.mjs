import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const port = Number(process.env.FLICK_UI_DEV_PORT ?? 5173);
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, '..', 'placeholder');

async function isServerRunning() {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 250);
  try {
    const response = await fetch(`http://127.0.0.1:${port}/`, { signal: controller.signal });
    return response.ok || response.status < 500;
  } catch {
    return false;
  } finally {
    clearTimeout(timeout);
  }
}

if (await isServerRunning()) {
  console.log(`Using existing UI dev server on ${port}`);
  process.exit(0);
}

const types = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.svg', 'image/svg+xml'],
  ['.png', 'image/png'],
]);

createServer((request, response) => {
  const url = new URL(request.url ?? '/', `http://127.0.0.1:${port}`);
  const requested = normalize(decodeURIComponent(url.pathname)).replace(/^[/\\]+/, '');
  let file = join(root, requested || 'index.html');
  if (relative(root, file).startsWith('..')) file = join(root, 'index.html');
  if (!existsSync(file) || statSync(file).isDirectory()) file = join(root, 'index.html');
  response.setHeader('content-type', types.get(extname(file)) ?? 'application/octet-stream');
  createReadStream(file).pipe(response);
}).listen(port, '127.0.0.1', () => {
  console.log(`Serving Flick placeholder UI on http://127.0.0.1:${port}`);
});

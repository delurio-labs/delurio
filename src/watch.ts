// `page --watch` writes the pages again whenever a graph file under the folder changes; with `--serve` it also shows
// them at http://127.0.0.1:<port>/ and reloads the open page after each write, or shows why the write failed. Local
// only: the server listens on the loopback address and serves the written pages and nothing else.
import { existsSync, readFileSync, statSync, watch } from 'node:fs';
import { createServer, type ServerResponse } from 'node:http';
import { extname, join, normalize, resolve, sep } from 'node:path';

export interface Rendered {
  ok: boolean;
  message: string; // the first error when not ok
}

const WATCHED = new Set(['.yaml', '.yml', '.json']);
const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.json': 'application/json',
  '.mmd': 'text/plain; charset=utf-8',
};

// added to every page the server sends, never to the files on disk
const RELOAD = `<script>(() => {
  const events = new EventSource('/__delurio/events');
  events.addEventListener('reload', () => location.reload());
  events.addEventListener('failed', (e) => {
    let box = document.getElementById('__delurio');
    if (!box) {
      box = document.createElement('pre');
      box.id = '__delurio';
      box.setAttribute('role', 'alert');
      box.style.cssText = 'position:fixed;inset:auto 0 0 0;margin:0;padding:12px 16px;background:#7f1d1d;color:#fff;font:13px/1.5 ui-monospace,Menlo,Consolas,monospace;white-space:pre-wrap;z-index:2147483647';
      document.body.append(box);
    }
    box.textContent = 'delurio page: ' + e.data;
  });
})();</script>`;

// whether a changed file can change a page: a graph file, not one of the pages being written
function counts(root: string, out: string, name: string): boolean {
  const abs = resolve(root, name);
  if (abs === out || abs.startsWith(out + sep)) return false;
  if (abs.split(sep).includes('viz')) return false;
  return WATCHED.has(extname(abs));
}

function serve(out: string, port: number, clients: Set<ServerResponse>): void {
  const server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1');
    if (url.pathname === '/__delurio/events') {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
      res.write(': delurio\n\n');
      clients.add(res);
      req.on('close', () => clients.delete(res));
      return;
    }
    let wanted: string;
    try {
      wanted = decodeURIComponent(url.pathname);
    } catch {
      res.writeHead(400).end();
      return;
    }
    let path = join(out, normalize(wanted));
    if (path !== out && !path.startsWith(out + sep)) {
      res.writeHead(403).end();
      return;
    }
    if (existsSync(path) && statSync(path).isDirectory()) path = join(path, 'index.html');
    if (!existsSync(path)) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' }).end('not found');
      return;
    }
    const type = TYPES[extname(path)] ?? 'application/octet-stream';
    let body: string | Buffer = readFileSync(path);
    if (type.startsWith('text/html')) {
      const html = body.toString('utf8');
      body = html.includes('</body>') ? html.replace('</body>', `${RELOAD}</body>`) : html + RELOAD;
    }
    res.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-store' }).end(body);
  });
  server.listen(port, '127.0.0.1', () => console.log(`serving http://127.0.0.1:${port}/ (Ctrl-C stops)`));
}

// Runs until stopped. `render` writes every page once and says whether it could.
export function watchPages(root: string, out: string, render: () => Promise<Rendered>, port: number | null): Promise<never> {
  const clients = new Set<ServerResponse>();
  const tell = (event: string, data: string): void => {
    for (const c of clients) c.write(`event: ${event}\ndata: ${data.replace(/\n/g, ' ')}\n\n`);
  };
  if (port !== null) serve(out, port, clients);
  let timer: NodeJS.Timeout | null = null;
  let running = false;
  let again = false;
  const run = async (): Promise<void> => {
    if (running) {
      again = true;
      return;
    }
    running = true;
    do {
      again = false;
      const done = await render();
      tell(done.ok ? 'reload' : 'failed', done.ok ? '' : done.message);
    } while (again);
    running = false;
  };
  watch(root, { recursive: true }, (_event, name) => {
    if (name === null || !counts(resolve(root), out, String(name))) return;
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => void run(), 150);
  });
  console.log(`watching ${root} for changes`);
  return new Promise<never>(() => {});
}

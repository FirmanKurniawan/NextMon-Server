#!/usr/bin/env node
// NextMON Server mock: implements the whole control API (docs/SERVER-SPEC.md) with fake, stateful data so ui/ can be
// developed and tested without the Rust server. No dependencies. Usage:
//   node tools/mock-server.mjs [--port 8791] [--token mock-token] [--latency 60] [--no-inject] [--fresh]
// --no-inject : do not inject the token into index.html (exercises the 401 / token dialog)
// --fresh     : start in first-run state (config.onboarded=false) so the onboarding wizard shows
// Mock-only helpers (any method): /__mock/offline?seconds=8, /__mock/crash, /__mock/reset[?nonode=1]
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf('--' + n); return i >= 0 ? (args[i + 1] && !args[i + 1].startsWith('--') ? args[i + 1] : true) : d; };
const PORT = Number(opt('port', process.env.MOCK_PORT || 8791));
let TOKEN = String(opt('token', process.env.MOCK_TOKEN || 'mock-token-0123456789abcdef'));
const LATENCY = Number(opt('latency', 60));
const INJECT = !args.includes('--no-inject');
const FRESH = args.includes('--fresh');
const UI_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'ui');

/* ------------------------------------------------------------------ state */
const now = () => Date.now();
const daysAgo = (n) => new Date(now() - n * 864e5).toISOString();
const rnd = (a, b) => a + Math.random() * (b - a);
const pad = (n) => String(n).padStart(2, '0');
const stamp = () => { const d = new Date(); return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };

function cmp(a, b) { const x = a.split('.').map(Number), y = b.split('.').map(Number); for (let i = 0; i < 3; i++) { if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) - (y[i] || 0); } return 0; }

const EXTERNAL_PORTS = { 3001: 'node.exe (PID 4412)', 8081: 'httpd.exe (PID 2208)', 5432: 'postgres.exe (PID 1840)', 80: 'System (PID 4)' };

function initialState() {
  return {
    version: '0.3.0',
    config: {
      version: 1, language: 'id', theme: 'auto', controlPort: PORT, controlToken: TOKEN, edition: 'php', bind: '127.0.0.1',
      autoStartWithWindows: false, startMinimized: false, autoStartEdition: true, openBrowserOnStart: true,
      dataDir: 'data', logRetentionDays: 14, restartOnCrash: true, maxRestarts: 5, onboarded: !FRESH,
      apps: {
        php: { port: 8080, runtime: '8.4.25', opcache: true, extraIni: { memory_limit: '256M' }, env: { NEXTMON_DEMO: '1', APP_TIMEZONE: 'Asia/Jakarta' } },
        next: { port: 3000, runtime: '24.12.0', env: {} },
      },
      update: { checkOnStart: true, channelPhp: 'stable', channelNode: 'lts', appRepo: 'nextmon/nextmon-server', appChannel: 'stable' },
    },
    proc: { state: 'running', pid: 12044, startedAt: now() - 3 * 3600e3 - 14 * 60e3, restarts: 0, ram: 52 },
    runtimes: {
      php: { installed: [{ version: '8.4.25', sizeMB: 71.4, tsnts: 'nts', arch: 'x64' }, { version: '8.3.28', sizeMB: 69.8, tsnts: 'nts', arch: 'x64' }] },
      node: { installed: [{ version: '24.12.0', sizeMB: 121.0, arch: 'x64' }, { version: '22.20.0', sizeMB: 118.6, arch: 'x64' }] },
    },
    backups: [
      { name: 'nextmon-backup-2026-10-30-0915.zip', sizeMB: 12.4, date: daysAgo(2), kind: 'manual' },
      { name: 'pre-upgrade-php-8.4.25-2026-10-12.zip', sizeMB: 11.9, date: daysAgo(20), kind: 'auto' },
      { name: 'nextmon-backup-rusak-2026-09-02.zip', sizeMB: 3.1, date: daysAgo(60), kind: 'manual' },
    ],
    update: { latest: '0.4.0', date: daysAgo(3), url: 'https://github.com/nextmon/nextmon-server/releases/tag/v0.4.0',
      notes: ['Pemilih edisi PHP / Next.js dengan konfirmasi dan pemasangan runtime otomatis.', 'Rollback otomatis bila health-check runtime gagal setelah upgrade.', 'Perbaikan: log SSE tidak lagi terputus saat tab di latar belakang.'] },
    diag: { 'php-ext-intl': 'fail', webview2: 'warn' },
  };
}
let state = initialState();
const jobs = new Map();
const sseLogs = new Set();
const logs = { server: [], php: [], next: [] };
let offlineUntil = 0;
let jobSeq = 1;

function pushLog(name, line) {
  const l = `${stamp()} ${line}`; logs[name].push(l); if (logs[name].length > 1500) logs[name].shift();
  for (const c of sseLogs) if (c.name === name) c.res.write(`data: ${l}\n\n`);
}
function seedLogs() {
  for (const k of Object.keys(logs)) logs[k] = [];
  const seed = [['server', '[INFO] NextMON Server 0.3.0 started, panel on 127.0.0.1:' + PORT], ['server', '[INFO] config loaded, edition=php, bind=127.0.0.1'], ['server', '[INFO] php runtime 8.4.25 health-check ok'], ['server', '[INFO] app php started pid=12044 port=8080'],
    ['server', '[WARN] update check: new version 0.4.0 available'], ['php', '[INFO] PHP 8.4.25 Development Server (http://127.0.0.1:8080) started']];
  seed.forEach(([n, l]) => { logs[n].push(`${stamp()} ${l}`); });
  for (let i = 0; i < 30; i++) logs.php.push(`${stamp()} [INFO] 127.0.0.1:${50000 + i} [200]: GET ${['/dashboard', '/hotspot/users', '/assets/css/app.css', '/api/stats'][i % 4]}`);
  logs.php.push(`${stamp()} [WARN] Deprecated: Creation of dynamic property in src/Voucher.php on line 88`);
}
const edState = () => state.proc.state;
const editionCfg = (e) => state.config.apps[e];
const kindOf = (e) => (e === 'php' ? 'php' : 'node');

function isInstalled(kind, ver) { return state.runtimes[kind].installed.some((i) => i.version === ver); }
function usedBy(kind, ver) { return ['php', 'next'].filter((e) => kindOf(e) === kind && editionCfg(e).runtime === ver); }
function portOwner(port, edition) {
  if (EXTERNAL_PORTS[port]) return EXTERNAL_PORTS[port];
  for (const e of ['php', 'next']) if (e !== edition && editionCfg(e).port === port && state.config.edition === e && edState() !== 'stopped') return 'NextMON ' + (e === 'php' ? 'PHP' : 'Next.js');
  return null;
}
function appInfo(e) {
  const c = editionCfg(e); const act = state.config.edition === e;
  const p = state.proc; const running = act && (p.state === 'running');
  return { state: act ? p.state : 'stopped', pid: act && p.state !== 'stopped' ? p.pid : null, port: c.port, url: `http://${state.config.bind === '0.0.0.0' ? '127.0.0.1' : state.config.bind}:${c.port}`,
    uptime: running ? Math.floor((now() - p.startedAt) / 1000) : 0, runtime: c.runtime, restarts: act ? p.restarts : 0, ramMB: running ? Math.round(p.ram * 10) / 10 : null };
}

/* ------------------------------------------------------------------ runtime catalog */
const CATALOG = {
  php: [
    ['8.5.2', 'stable', 23, 33], ['8.5.1', 'stable', 53, 33], ['8.4.27', 'stable', 9, 32], ['8.4.26', 'stable', 39, 32], ['8.3.30', 'security', 9, 31], ['8.3.29', 'security', 39, 31],
    ['8.2.31', 'security', 9, 30], ['8.2.30', 'security', 70, 30], ['8.1.34', 'eol', 300, 29],
  ],
  nodeLts: [['24.13.0', 'lts', 11, 33], ['24.12.0', 'lts', 40, 33], ['22.21.0', 'lts', 14, 30], ['22.20.0', 'lts', 45, 30], ['20.19.6', 'lts', 60, 28]],
  nodeCur: [['25.2.0', 'current', 5, 34], ['25.1.0', 'current', 30, 34]],
};
function listRuntime(kind, q) {
  const installed = state.runtimes[kind].installed.map((i) => ({ ...i, path: `runtimes\\${kind}\\${i.version}`, active: usedBy(kind, i.version).length > 0, usedBy: usedBy(kind, i.version) }));
  let src = kind === 'php' ? CATALOG.php : (q.get('channel') === 'current' ? CATALOG.nodeCur : CATALOG.nodeLts);
  const top = installed.map((i) => i.version).sort(cmp).pop() || '0.0.0';
  const available = src.map(([version, channel, d, sizeMB]) => ({ version, channel, date: daysAgo(d), sizeMB, sha256: crypto.createHash('sha256').update(kind + version).digest('hex'),
    url: kind === 'php' ? `https://windows.php.net/downloads/releases/php-${version}-${q.get('variant') === 'ts' ? 'Win32' : 'nts-Win32'}-vs17-x64.zip` : `https://nodejs.org/dist/v${version}/node-v${version}-win-x64.zip`, isNewer: cmp(version, top) > 0 }));
  const stableOnly = available.filter((a) => a.channel !== 'eol');
  const active = state.runtimes[kind].installed.find((i) => usedBy(kind, i.version).length);
  // "latest" = newest release inside the active runtime's major.minor series (PHP) / same LTS line or newest in channel (Node)
  let latest = stableOnly[0];
  if (active) { const mm = active.version.split('.').slice(0, kind === 'php' ? 2 : 1).join('.'); latest = stableOnly.find((a) => a.version.startsWith(mm + '.')) || latest; }
  const updateAvailable = !!(active && latest && cmp(latest.version, active.version) > 0);
  return { installed, available, latest, updateAvailable };
}

/* ------------------------------------------------------------------ jobs */
function createJob(steps, { totalMs = 4000, failAtStep = -1, failCode = 'job_failed', failMsg, onDone } = {}) {
  const id = 'job_' + (jobSeq++);
  const job = { id, state: 'running', progress: 0, step: steps[0], message: '', error: null, subs: new Set() };
  jobs.set(id, job);
  const start = now(); const tick = 150;
  const timer = setInterval(() => {
    const t = Math.min(1, (now() - start) / totalMs);
    const si = Math.min(steps.length - 1, Math.floor(t * steps.length));
    job.progress = Math.round(t * 1000) / 1000; job.step = steps[si];
    if (failAtStep >= 0 && si >= failAtStep && t >= (failAtStep + 0.6) / steps.length) {
      job.state = 'error'; job.message = failMsg?.id || 'Gagal'; job.error = { code: failCode, message: failMsg || { id: 'Job gagal', en: 'Job failed' } }; clearInterval(timer); emitJob(job); return;
    }
    if (t >= 1) { job.state = 'done'; job.progress = 1; clearInterval(timer); try { onDone?.(); } catch (e) { console.error(e); } }
    emitJob(job);
  }, tick);
  emitJob(job);
  return job;
}
const jobView = (j) => ({ id: j.id, state: j.state, progress: j.progress, step: j.step, message: j.message, error: j.error });
function emitJob(job) {
  const data = `data: ${JSON.stringify(jobView(job))}\n\n`;
  for (const res of job.subs) { res.write(data); if (job.state !== 'running') res.end(); }
  if (job.state !== 'running') job.subs.clear();
}

/* ------------------------------------------------------------------ process simulation */
function startApp() {
  const e = state.config.edition; const p = state.proc;
  p.state = 'starting'; pushLog('server', `[INFO] starting ${e} on port ${editionCfg(e).port}`);
  setTimeout(() => {
    if (p.state !== 'starting') return;
    p.state = 'running'; p.startedAt = now(); p.pid = 10000 + Math.floor(Math.random() * 9000); p.ram = e === 'php' ? 46 : 168;
    pushLog('server', `[INFO] app ${e} started pid=${p.pid} port=${editionCfg(e).port}`);
    pushLog(e, e === 'php' ? `[INFO] PHP ${editionCfg(e).runtime} Development Server (http://127.0.0.1:${editionCfg(e).port}) started` : `[INFO] ready - started server on 0.0.0.0:${editionCfg(e).port}`);
  }, 1200);
}
function stopApp(cb) {
  const p = state.proc; if (p.state === 'stopped') { cb?.(); return; }
  p.state = 'stopping'; pushLog('server', `[INFO] stopping ${state.config.edition}`);
  setTimeout(() => { p.state = 'stopped'; p.pid = null; pushLog('server', `[INFO] app ${state.config.edition} stopped`); cb?.(); }, 700);
}
if (!FRESH) { /* running by default */ } else state.proc.state = 'stopped';
seedLogs();

const REQ = ['/dashboard', '/hotspot/users', '/hotspot/vouchers', '/reports/sales', '/api/stats', '/assets/css/app.css', '/login', '/settings'];
setInterval(() => {
  const e = state.config.edition; const p = state.proc;
  if (p.state !== 'running') return;
  p.ram = Math.max(30, p.ram + rnd(-1.2, 1.4));
  const n = Math.random(); const path_ = REQ[Math.floor(Math.random() * REQ.length)];
  if (e === 'php') {
    if (n < 0.05) pushLog('php', `[WARN] Deprecated: Creation of dynamic property in src/Voucher.php on line ${Math.floor(rnd(20, 200))}`);
    else if (n < 0.07) pushLog('php', `[ERROR] RouterOS timeout connecting to 192.168.88.1:8728 (demo off)`);
    else pushLog('php', `[INFO] 127.0.0.1:${Math.floor(rnd(49000, 60000))} [${n < 0.12 ? 404 : 200}]: GET ${path_}`);
  } else {
    if (n < 0.05) pushLog('next', `[WARN] slow render ${path_} took ${Math.floor(rnd(900, 2400))}ms`);
    else pushLog('next', `[INFO] GET ${path_} ${n < 0.1 ? 304 : 200} in ${Math.floor(rnd(8, 90))}ms`);
  }
  if (Math.random() < 0.04) pushLog('server', `[DEBUG] supervisor heartbeat ok pid=${p.pid} rss=${Math.round(p.ram)}MB`);
}, 1100);

/* ------------------------------------------------------------------ http helpers */
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function send(res, status, body, headers = {}) {
  const data = typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': typeof body === 'string' || Buffer.isBuffer(body) ? 'application/octet-stream' : 'application/json; charset=utf-8', 'Cache-Control': 'no-store', ...headers });
  res.end(data);
}
const fail = (res, status, code, id, en) => send(res, status, { error: { code, message: { id, en: en || id } } });
async function readBody(req) { const ch = []; for await (const c of req) ch.push(c); const s = Buffer.concat(ch).toString(); try { return s ? JSON.parse(s) : {}; } catch { return {}; } }

const MIME = { '.html': 'text/html; charset=utf-8', '.css': 'text/css; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png', '.ico': 'image/x-icon', '.woff2': 'font/woff2' };
function serveStatic(req, res, pathname) {
  let rel = decodeURIComponent(pathname); if (rel === '/' || rel === '') rel = '/index.html';
  const file = path.normalize(path.join(UI_DIR, rel));
  if (!file.startsWith(UI_DIR)) return send(res, 403, 'forbidden');
  fs.readFile(file, (err, buf) => {
    if (err) return send(res, 404, 'not found');
    if (path.basename(file) === 'index.html' && INJECT) buf = Buffer.from(buf.toString().replace('{{NM_TOKEN}}', TOKEN));
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-store' });
    res.end(buf);
  });
}

/* ------------------------------------------------------------------ API */
const validPort = (n) => Number.isInteger(n) && n >= 1 && n <= 65535;

async function handleApi(req, res, url) {
  const p = url.pathname.slice(4); // strip /api
  const m = req.method;
  const q = url.searchParams;
  const tok = req.headers['x-token'] || q.get('token');
  if (tok !== TOKEN) return fail(res, 401, 'unauthorized', 'Token tidak valid.', 'Invalid token.');
  await sleep(LATENCY * (0.5 + Math.random()));

  if (m === 'GET' && p === '/status') {
    const c = state.config; const ed = c.edition;
    return send(res, 200, {
      edition: ed, app: appInfo(ed),
      editions: { php: { port: c.apps.php.port, runtime: c.apps.php.runtime, installed: isInstalled('php', c.apps.php.runtime) }, next: { port: c.apps.next.port, runtime: c.apps.next.runtime, installed: isInstalled('node', c.apps.next.runtime) } },
      version: state.version, config: { ...c, controlToken: undefined }, system: { os: 'Windows 11 Pro 10.0.26200', arch: 'x64', ramMB: 16384, diskFreeGB: 212.4 },
      paths: { root: 'C:\\NextMON Server', data: 'C:\\NextMON Server\\data', logs: 'C:\\NextMON Server\\logs' },
      about: { license: 'MIT', commit: 'a1b2c3d', buildDate: '2026-10-28', homepage: 'https://github.com/nextmon/nextmon-server' },
    });
  }
  if (m === 'POST' && p === '/edition') {
    const b = await readBody(req); const e = b.edition;
    if (!['php', 'next'].includes(e)) return fail(res, 422, 'invalid_edition', 'Edisi harus php atau next.', 'Edition must be php or next.');
    if (!isInstalled(kindOf(e), editionCfg(e).runtime)) return fail(res, 409, 'runtime_missing', `Runtime ${kindOf(e) === 'php' ? 'PHP' : 'Node.js'} untuk edisi ini belum terpasang.`, 'The runtime for this edition is not installed.');
    if (e === state.config.edition) return send(res, 200, { ok: true });
    const job = createJob(['Menghentikan edisi lama', 'Mengganti edisi', b.start ? 'Menjalankan edisi baru' : 'Selesai'], { totalMs: 1800, onDone: () => {} });
    stopApp(() => { state.config.edition = e; pushLog('server', `[INFO] edition switched to ${e}`); if (b.start) startApp(); });
    return send(res, 202, { jobId: job.id });
  }
  const am = p.match(/^\/app\/(start|stop|restart)$/);
  if (m === 'POST' && am) {
    const e = state.config.edition; const act = am[1]; const pr = state.proc;
    if (act !== 'stop') {
      if (!isInstalled(kindOf(e), editionCfg(e).runtime)) return fail(res, 409, 'runtime_missing', 'Runtime belum terpasang.', 'Runtime is not installed.');
      const owner = EXTERNAL_PORTS[editionCfg(e).port];
      if (owner && !(act === 'restart')) return fail(res, 409, 'port_in_use', `Port ${editionCfg(e).port} dipakai oleh ${owner}.`, `Port ${editionCfg(e).port} is used by ${owner}.`);
      if (owner) return fail(res, 409, 'port_in_use', `Port ${editionCfg(e).port} dipakai oleh ${owner}.`, `Port ${editionCfg(e).port} is used by ${owner}.`);
    }
    if (act === 'start') { if (pr.state === 'running') return send(res, 200, { ok: true }); startApp(); }
    else if (act === 'stop') stopApp();
    else { pr.restarts = pr.state === 'running' ? pr.restarts : pr.restarts; stopApp(() => startApp()); }
    return send(res, 202, { ok: true });
  }
  if (m === 'POST' && p === '/app/config') {
    const b = await readBody(req); const e = b.edition || state.config.edition;
    if (!['php', 'next'].includes(e)) return fail(res, 422, 'invalid_edition', 'Edisi tidak dikenal.', 'Unknown edition.');
    const c = editionCfg(e);
    if (b.port !== undefined) {
      if (!validPort(b.port)) return fail(res, 422, 'invalid_port', 'Port harus antara 1 dan 65535.', 'Port must be between 1 and 65535.');
      const owner = portOwner(b.port, e);
      if (owner) return fail(res, 409, 'port_in_use', `Port ${b.port} dipakai oleh ${owner}.`, `Port ${b.port} is used by ${owner}.`);
    }
    if (b.runtime !== undefined && b.runtime !== '' && !isInstalled(kindOf(e), b.runtime)) return fail(res, 409, 'runtime_not_installed', `Runtime ${b.runtime} belum terpasang.`, `Runtime ${b.runtime} is not installed.`);
    for (const k of ['port', 'runtime', 'env', 'opcache', 'extraIni']) if (b[k] !== undefined && !(k === 'runtime' && b[k] === '')) c[k] = b[k];
    if (e === 'next') { delete c.opcache; delete c.extraIni; }
    return send(res, 200, c);
  }
  const lm = p.match(/^\/logs\/(server|php|next)(\/stream)?$/);
  if (m === 'GET' && lm) {
    const name = lm[1];
    if (lm[2]) {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' }); res.write(': ok\n\n');
      const c = { res, name }; sseLogs.add(c); const ka = setInterval(() => res.write(': ka\n\n'), 15000);
      req.on('close', () => { sseLogs.delete(c); clearInterval(ka); }); return;
    }
    const n = Math.min(2000, Number(q.get('lines')) || 200);
    return send(res, 200, { name, lines: logs[name].slice(-n) });
  }
  if (p === '/config' && m === 'GET') return send(res, 200, { ...state.config, controlToken: TOKEN });
  if (p === '/config' && m === 'POST') {
    const b = await readBody(req); const c = state.config;
    if (b.language !== undefined && !['id', 'en'].includes(b.language)) return fail(res, 422, 'invalid_config', 'Bahasa tidak valid.', 'Invalid language.');
    if (b.theme !== undefined && !['auto', 'light', 'dark'].includes(b.theme)) return fail(res, 422, 'invalid_config', 'Tema tidak valid.', 'Invalid theme.');
    if (b.bind !== undefined && !['127.0.0.1', '0.0.0.0'].includes(b.bind)) return fail(res, 422, 'invalid_config', 'Bind harus 127.0.0.1 atau 0.0.0.0.', 'Bind must be 127.0.0.1 or 0.0.0.0.');
    if (b.logRetentionDays !== undefined && !(Number.isInteger(b.logRetentionDays) && b.logRetentionDays >= 1 && b.logRetentionDays <= 365)) return fail(res, 422, 'invalid_config', 'Retensi log 1-365 hari.', 'Log retention must be 1-365 days.');
    for (const [k, v] of Object.entries(b)) { if (['apps', 'edition', 'controlToken', 'controlPort', 'version'].includes(k)) continue; if (k === 'update' && typeof v === 'object') c.update = { ...c.update, ...v }; else c[k] = v; }
    return send(res, 200, { ...c, controlToken: undefined });
  }
  if (p === '/config/token/regenerate' && m === 'POST') { TOKEN = 'mock-' + crypto.randomBytes(12).toString('hex'); state.config.controlToken = TOKEN; return send(res, 200, { token: TOKEN }); }
  if (p === '/ports/check' && m === 'GET') {
    const port = Number(q.get('port')); const e = q.get('edition') || state.config.edition;
    if (!validPort(port)) return fail(res, 422, 'invalid_port', 'Port tidak valid.', 'Invalid port.');
    const by = portOwner(port, e); return send(res, 200, by ? { free: false, usedBy: by } : { free: true });
  }
  const rm = p.match(/^\/runtimes\/(php|node)(?:\/(install|activate|uninstall|upgrade))?$/);
  if (rm) {
    const kind = rm[1]; const act = rm[2];
    if (m === 'GET' && !act) return send(res, 200, listRuntime(kind, q));
    if (m === 'POST' && act) {
      const b = await readBody(req); const list = state.runtimes[kind].installed; const label = kind === 'php' ? 'PHP' : 'Node.js';
      if (act === 'install') {
        const cat = listRuntime(kind, new URLSearchParams(b.variant ? { variant: b.variant } : {})); const all = kind === 'php' ? cat.available : listRuntime('node', new URLSearchParams()).available.concat(listRuntime('node', new URLSearchParams({ channel: 'current' })).available);
        const target = all.find((a) => a.version === b.version);
        if (!target) return fail(res, 404, 'version_not_found', `Versi ${b.version} tidak ditemukan.`, `Version ${b.version} not found.`);
        if (isInstalled(kind, b.version)) return fail(res, 409, 'already_installed', `${label} ${b.version} sudah terpasang.`, 'Already installed.');
        const steps = ['Mengunduh', 'Verifikasi sha256', 'Mengekstrak', kind === 'php' ? 'Menyiapkan php.ini' : 'Menyiapkan npm', 'Tes ' + (kind === 'php' ? 'php -v' : 'node -v')];
        const willFail = kind === 'php' && b.version === '8.3.29';
        const job = createJob(steps, { totalMs: 5200, failAtStep: willFail ? 1 : -1, failCode: 'sha256_mismatch', failMsg: { id: 'Checksum sha256 tidak cocok; unduhan dibuang.', en: 'sha256 checksum mismatch; download discarded.' },
          onDone: () => { list.push({ version: b.version, sizeMB: target.sizeMB * 2.2, tsnts: kind === 'php' ? (b.variant === 'ts' ? 'ts' : 'nts') : undefined, arch: 'x64' }); pushLog('server', `[INFO] installed ${kind} ${b.version}`); } });
        return send(res, 202, { jobId: job.id });
      }
      if (act === 'activate') {
        const app = b.app || (kind === 'php' ? 'php' : 'next');
        if (kindOf(app) !== kind) return fail(res, 422, 'invalid_app', 'Runtime tidak cocok dengan edisi.', 'Runtime does not match the edition.');
        if (!isInstalled(kind, b.version)) return fail(res, 409, 'runtime_not_installed', `Runtime ${b.version} belum terpasang.`, 'Runtime is not installed.');
        editionCfg(app).runtime = b.version;
        if (state.config.edition === app && state.proc.state === 'running') stopApp(() => startApp());
        return send(res, 200, { ok: true });
      }
      if (act === 'uninstall') {
        if (usedBy(kind, b.version).length) return fail(res, 409, 'runtime_in_use', `${label} ${b.version} sedang dipakai.`, `${label} ${b.version} is in use.`);
        const i = list.findIndex((x) => x.version === b.version); if (i < 0) return fail(res, 404, 'not_found', 'Runtime tidak ditemukan.', 'Runtime not found.');
        list.splice(i, 1); return send(res, 200, { ok: true });
      }
      if (act === 'upgrade') {
        const info = listRuntime(kind, new URLSearchParams()); const to = b.to || info.latest?.version; const app = b.app || (kind === 'php' ? 'php' : 'next');
        const prev = editionCfg(app).runtime;
        const doFail = kind === 'node' && String(to).startsWith('25.');
        const job = createJob(['Mengunduh ' + to, 'Verifikasi', 'Mengekstrak', 'Mengganti versi', 'Health-check'], { totalMs: 6500, failAtStep: doFail ? 4 : -1, failCode: 'health_check_failed',
          failMsg: { id: `Health-check gagal; dipulihkan ke ${prev} (rollback).`, en: `Health check failed; rolled back to ${prev}.` },
          onDone: () => { if (!isInstalled(kind, to)) list.push({ version: to, sizeMB: 75, tsnts: kind === 'php' ? 'nts' : undefined, arch: 'x64' }); editionCfg(app).runtime = to; if (state.config.edition === app && state.proc.state === 'running') stopApp(() => startApp()); } });
        return send(res, 202, { jobId: job.id });
      }
    }
  }
  const jm = p.match(/^\/jobs\/([\w-]+)(\/stream)?$/);
  if (m === 'GET' && jm) {
    const job = jobs.get(jm[1]); if (!job) return fail(res, 404, 'not_found', 'Job tidak ditemukan.', 'Job not found.');
    if (jm[2]) {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
      res.write(`data: ${JSON.stringify(jobView(job))}\n\n`);
      if (job.state !== 'running') return res.end();
      job.subs.add(res); req.on('close', () => job.subs.delete(res)); return;
    }
    return send(res, 200, jobView(job));
  }
  if (p === '/backup/list' && m === 'GET') return send(res, 200, { backups: state.backups });
  if (p === '/backup/create' && m === 'POST') {
    const d = new Date(); const name = `nextmon-backup-${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.zip`;
    const job = createJob(['Menghitung berkas', 'Mengompres data', 'Menulis arsip'], { totalMs: 3000, onDone: () => state.backups.unshift({ name, sizeMB: Math.round(rnd(9, 14) * 10) / 10, date: new Date().toISOString(), kind: 'manual' }) });
    return send(res, 202, { jobId: job.id });
  }
  if (p === '/backup/restore' && m === 'POST') {
    const b = await readBody(req); if (!state.backups.some((x) => x.name === b.name)) return fail(res, 404, 'not_found', 'Backup tidak ditemukan.', 'Backup not found.');
    const bad = b.name.includes('rusak');
    const job = createJob(['Memeriksa arsip', 'Menghentikan aplikasi', 'Mengekstrak data', 'Menjalankan ulang'], { totalMs: 4000, failAtStep: bad ? 0 : -1, failCode: 'backup_corrupt', failMsg: { id: 'Arsip rusak atau tidak lengkap.', en: 'Archive is corrupt or incomplete.' } });
    return send(res, 202, { jobId: job.id });
  }
  if (p === '/backup/download' && m === 'GET') {
    const name = q.get('name'); if (!state.backups.some((x) => x.name === name)) return fail(res, 404, 'not_found', 'Backup tidak ditemukan.', 'Backup not found.');
    return send(res, 200, crypto.randomBytes(2048), { 'Content-Disposition': `attachment; filename="${name}"` });
  }
  if (p === '/update/check' && m === 'GET') {
    const u = state.update; const available = cmp(u.latest, state.version) > 0;
    return send(res, 200, { current: state.version, latest: u.latest, available, date: u.date, url: u.url, notes: available ? u.notes : [], channel: state.config.update.appChannel });
  }
  if (p === '/update/apply' && m === 'POST') {
    const job = createJob(['Mengunduh pembaruan', 'Verifikasi', 'Backup otomatis', 'Memasang', 'Restart'], { totalMs: 6000, onDone: () => { state.version = state.update.latest; offlineUntil = now() + 3500; pushLog('server', `[INFO] updated to ${state.version}`); } });
    return send(res, 202, { jobId: job.id });
  }
  if (p === '/diagnostics' && m === 'GET') {
    const d = state.diag; const e = state.config.edition;
    const phpVer = editionCfg('php').runtime; const checks = [
      { id: 'vcredist', status: 'pass', title: 'Microsoft Visual C++ Runtime', detail: 'VC++ 2015-2022 x64 14.40 terdeteksi' },
      { id: 'ports', status: EXTERNAL_PORTS[editionCfg(e).port] ? 'fail' : 'pass', title: 'Port tidak bentrok', detail: `Port ${editionCfg(e).port} ${EXTERNAL_PORTS[editionCfg(e).port] ? 'dipakai ' + EXTERNAL_PORTS[editionCfg(e).port] : 'bebas'}`, hint: 'Ganti port di halaman Aplikasi.' },
      { id: 'php-ext-intl', status: d['php-ext-intl'], title: 'Ekstensi PHP wajib', detail: d['php-ext-intl'] === 'pass' ? `Semua ekstensi ada di PHP ${phpVer}` : `Ekstensi intl tidak aktif di PHP ${phpVer}`, hint: 'Aktifkan extension=intl di php.ini runtime.', fixable: d['php-ext-intl'] !== 'pass' },
      { id: 'data-write', status: 'pass', title: 'Izin tulis folder data', detail: 'C:\\NextMON Server\\data dapat ditulis' },
      { id: 'webview2', status: d.webview2, title: 'WebView2 Runtime', detail: d.webview2 === 'pass' ? 'Terpasang' : 'Tidak ditemukan; panel dibuka di browser bawaan.', hint: 'Pasang Microsoft Edge WebView2 agar panel tampil di jendela sendiri.' },
      { id: 'node-min', status: cmp(editionCfg('next').runtime, '20.9.0') >= 0 ? 'pass' : 'fail', title: 'Versi Node.js memenuhi syarat Next.js', detail: `Node ${editionCfg('next').runtime} (minimal 20.9.0)` },
    ];
    return send(res, 200, { checks });
  }
  if (p === '/diagnostics/fix' && m === 'POST') {
    const b = await readBody(req); if (b.id in state.diag) { await sleep(700); state.diag[b.id] = 'pass'; return send(res, 200, { ok: true }); }
    return fail(res, 422, 'not_fixable', 'Masalah ini tidak bisa diperbaiki otomatis.', 'This issue cannot be fixed automatically.');
  }
  if (p === '/open' && m === 'POST') { const b = await readBody(req); pushLog('server', `[INFO] open ${b.what}`); return send(res, 200, { ok: true }); }
  return fail(res, 404, 'not_found', 'Endpoint tidak ditemukan.', 'Endpoint not found.');
}

/* ------------------------------------------------------------------ server */
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://${req.headers.host || 'localhost'}`);
  try {
    if (url.pathname.startsWith('/__mock/')) {
      if (url.pathname === '/__mock/offline') { offlineUntil = now() + Number(url.searchParams.get('seconds') || 8) * 1000; return send(res, 200, { ok: true }); }
      if (url.pathname === '/__mock/crash') { state.proc.state = 'crashed'; state.proc.restarts++; pushLog('server', '[ERROR] app exited with code 3221225477'); return send(res, 200, { ok: true }); }
      if (url.pathname === '/__mock/reset') { state = initialState(); if (url.searchParams.get('nonode')) { state.runtimes.node.installed = []; } seedLogs(); return send(res, 200, { ok: true }); }
      return send(res, 404, 'nope');
    }
    if (url.pathname.startsWith('/api/')) {
      if (now() < offlineUntil) { req.socket.destroy(); return; }
      return await handleApi(req, res, url);
    }
    return serveStatic(req, res, url.pathname);
  } catch (e) { console.error(e); if (!res.headersSent) fail(res, 500, 'internal', 'Kesalahan internal.', 'Internal error.'); }
});
server.listen(PORT, '127.0.0.1', () => {
  console.log(`NextMON Server mock on http://127.0.0.1:${PORT}/`);
  console.log(`token: ${TOKEN}${INJECT ? ' (injected into index.html)' : ' (NOT injected: --no-inject)'}${FRESH ? '  first-run mode' : ''}`);
});

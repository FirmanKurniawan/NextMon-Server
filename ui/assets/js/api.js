// API client: fetch wrapper with X-Token header, uniform errors, and a fetch-based SSE reader (EventSource cannot send headers).
const listeners = { auth: new Set(), net: new Set() };
export const on = (type, fn) => { listeners[type].add(fn); return () => listeners[type].delete(fn); };

let token = '';
export function initToken() {
  const url = new URL(location.href);
  const q = url.searchParams.get('token');
  const meta = document.querySelector('meta[name="nm-token"]')?.content?.trim() || '';
  const metaOk = meta && !/^\{\{.*\}\}$/.test(meta) && !/^__.*__$/.test(meta);
  if (q) {
    token = q; try { localStorage.setItem('nm.token', q); } catch { /* ignore */ }
    url.searchParams.delete('token'); history.replaceState(null, '', url.pathname + url.search + url.hash);
  } else if (metaOk) token = meta;
  else { try { token = localStorage.getItem('nm.token') || ''; } catch { token = ''; } }
}
export const getToken = () => token;
export function setToken(t, persist = true) { token = t; if (persist) { try { localStorage.setItem('nm.token', t); } catch { /* ignore */ } } }

export class ApiError extends Error {
  constructor(status, code, message, body) { super(message || code || 'error'); this.status = status; this.code = code || 'error'; this.body = body; }
}

function pickLang(m) {
  if (m && typeof m === 'object') { const l = document.documentElement.lang || 'id'; return m[l] || m.id || m.en || ''; }
  return m;
}

async function parse(res) {
  const ct = res.headers.get('content-type') || '';
  if (ct.includes('json')) { try { return await res.json(); } catch { return null; } }
  return res.text();
}

export async function api(path, { method = 'GET', body, signal, blob = false } = {}) {
  let res;
  try {
    res = await fetch('/api' + path, {
      method, signal,
      headers: { 'X-Token': token, ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}) },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      cache: 'no-store',
    });
  } catch (e) {
    if (e.name === 'AbortError') throw e;
    listeners.net.forEach((f) => f(true));
    throw new ApiError(0, 'network', 'Network error');
  }
  listeners.net.forEach((f) => f(false));
  if (res.ok) return blob ? res.blob() : parse(res);
  const data = await parse(res);
  const er = data && data.error;
  const err = new ApiError(res.status, er?.code, pickLang(er?.message), data);
  if (res.status === 401) listeners.auth.forEach((f) => f(err));
  throw err;
}

export const get = (p, o) => api(p, o);
export const post = (p, body, o) => api(p, { method: 'POST', body: body ?? {}, ...o });

/** Subscribe to an SSE endpoint. onMessage(dataString). Returns {close()}. Reconnects unless reconnect=false. */
export function sse(path, onMessage, { reconnect = true, onState } = {}) {
  const ctrl = new AbortController();
  let closed = false;
  (async function loop() {
    let delay = 1000;
    while (!closed) {
      try {
        const res = await fetch('/api' + path, { headers: { 'X-Token': token, Accept: 'text/event-stream' }, signal: ctrl.signal, cache: 'no-store' });
        if (res.status === 401) listeners.auth.forEach((f) => f(new ApiError(401, 'unauthorized')));
        if (!res.ok || !res.body) throw new Error('http ' + res.status);
        onState?.('open'); delay = 1000;
        const reader = res.body.getReader(); const dec = new TextDecoder(); let buf = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buf += dec.decode(value, { stream: true });
          let i;
          while ((i = buf.search(/\r?\n\r?\n/)) >= 0) {
            const block = buf.slice(0, i); buf = buf.slice(i).replace(/^\r?\n\r?\n/, '');
            const data = block.split(/\r?\n/).filter((l) => l.startsWith('data:')).map((l) => l.slice(5).replace(/^ /, '')).join('\n');
            if (data !== '' || block.includes('data:')) onMessage(data);
          }
        }
      } catch (e) {
        if (closed || e.name === 'AbortError') return;
      }
      if (!reconnect || closed) { onState?.('closed'); return; }
      onState?.('retry');
      await new Promise((r) => setTimeout(r, delay)); delay = Math.min(delay * 1.6, 8000);
    }
  })();
  return { close() { closed = true; ctrl.abort(); } };
}

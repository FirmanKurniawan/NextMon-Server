// Logs: live tail over SSE with pause/resume, search, level filter, copy, download.
import { get, sse } from '../api.js';
import { h, clear, toast, errText } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S } from '../store.js';

const SOURCES = ['server', 'php', 'next'];
const MAX = 2000;
const levelOf = (l) => (/\b(ERROR|FATAL|CRITICAL|EMERG|ALERT|SEVERE)\b|\bFatal error\b/i.test(l) ? 'err' : /\b(WARN|WARNING|NOTICE)\b/i.test(l) ? 'warn' : /\b(DEBUG|TRACE)\b/i.test(l) ? 'dbg' : 'info');

export function mount(root, params) {
  let src = SOURCES.includes(params[0]) ? params[0] : (S.status?.edition || 'server');
  let lines = [], paused = false, pending = 0, query = '', minLevel = 'all', stream = null, conn = 'connecting', alive = true;
  const box = h('div', { class: 'logbox', role: 'log', 'aria-label': t('logs.title'), 'aria-live': 'off', tabindex: '0' });
  const status = h('span', { 'aria-live': 'polite' });
  const count = h('span');
  const srcSeg = h('div', { class: 'seg', role: 'group', 'aria-label': t('logs.source') });
  const search = h('input', { class: 'input', type: 'search', placeholder: t('logs.search'), 'aria-label': t('logs.search'), onInput: (e) => { query = e.target.value; paint(); } });
  const lvl = h('select', { class: 'select', style: { width: 'auto' }, 'aria-label': t('logs.level'), onChange: (e) => { minLevel = e.target.value; paint(); } },
    ['all', 'warn', 'err'].map((v) => h('option', { value: v }, t('logs.lvl.' + v))));
  const pauseBtn = h('button', { class: 'btn', type: 'button', onClick: togglePause });
  root.append(
    h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('logs.title')), h('p', { class: 'page-desc' }, t('logs.desc')))),
    h('div', { class: 'log-tools' }, srcSeg,
      h('div', { class: 'search' }, icon('search', 14), search), lvl, pauseBtn,
      h('button', { class: 'btn', type: 'button', onClick: copy }, icon('copy', 14), t('logs.copy')),
      h('button', { class: 'btn', type: 'button', onClick: download }, icon('download', 14), t('logs.download')),
      h('button', { class: 'btn btn-ghost', type: 'button', onClick: () => { lines = []; pending = 0; paint(); } }, icon('trash', 14), t('logs.clear'))),
    box, h('div', { class: 'log-status' }, status, count));

  const ranks = { info: 0, dbg: 0, warn: 1, err: 2 };
  const visible = () => lines.filter((l) => (minLevel === 'all' || ranks[l.lv] >= ranks[minLevel === 'warn' ? 'warn' : 'err']) && (!query || l.text.toLowerCase().includes(query.toLowerCase())));
  function lineEl(l) {
    const el = h('div', { class: 'ln ' + l.lv });
    if (query) {
      const q = query.toLowerCase(); let i = 0; const lt = l.text.toLowerCase();
      for (;;) { const j = lt.indexOf(q, i); if (j < 0) { el.append(l.text.slice(i)); break; } el.append(l.text.slice(i, j), h('mark', null, l.text.slice(j, j + q.length))); i = j + q.length; }
    } else el.textContent = l.text;
    return el;
  }
  function paint() {
    const vis = visible(); clear(box);
    if (!vis.length) box.append(h('div', { class: 'empty-l' }, lines.length ? t('logs.nomatch') : t('logs.empty')));
    else { const frag = document.createDocumentFragment(); vis.forEach((l) => frag.append(lineEl(l))); box.append(frag); }
    if (!paused) box.scrollTop = box.scrollHeight;
    meta();
  }
  function meta() {
    pauseBtn.replaceChildren(icon(paused ? 'play' : 'pause', 14), paused ? t('logs.resume') + (pending ? ` (${pending})` : '') : t('logs.pause'));
    pauseBtn.setAttribute('aria-pressed', String(paused));
    status.textContent = paused ? t('logs.paused') : conn === 'open' ? t('logs.live') : conn === 'retry' ? t('logs.reconnecting') : t('logs.connecting');
    count.textContent = t('logs.count', { n: visible().length, total: lines.length });
  }
  function add(text) {
    const l = { text, lv: levelOf(text) }; lines.push(l);
    if (lines.length > MAX) lines.splice(0, lines.length - MAX);
    if (paused) { pending++; meta(); return; }
    const vis = (minLevel === 'all' || ranks[l.lv] >= ranks[minLevel]) && (!query || text.toLowerCase().includes(query.toLowerCase()));
    if (!vis) { meta(); return; }
    if (box.querySelector('.empty-l')) clear(box);
    const stick = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
    box.append(lineEl(l)); while (box.childElementCount > MAX) box.firstChild.remove();
    if (stick) box.scrollTop = box.scrollHeight;
    meta();
  }
  function togglePause() { paused = !paused; if (!paused) { pending = 0; paint(); } else meta(); }
  async function copy() {
    const txt = visible().map((l) => l.text).join('\n');
    try { await navigator.clipboard.writeText(txt); } catch {
      const ta = h('textarea', { style: { position: 'fixed', opacity: 0 } }); ta.value = txt; document.body.append(ta); ta.select(); try { document.execCommand('copy'); } catch { /* ignore */ } ta.remove();
    }
    toast(t('logs.copied', { n: visible().length }), 'ok', { timeout: 2200 });
  }
  function download() {
    const blob = new Blob([lines.map((l) => l.text).join('\n') + '\n'], { type: 'text/plain' });
    const a = h('a', { href: URL.createObjectURL(blob), download: `nextmon-${src}-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.log` });
    document.body.append(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 2000);
  }

  async function open() {
    stream?.close(); lines = []; pending = 0; conn = 'connecting'; paint();
    const mine = src;
    try {
      const res = await get(`/logs/${src}?lines=300`);
      if (mine !== src || !alive) return;
      const arr = Array.isArray(res) ? res : Array.isArray(res?.lines) ? res.lines : String(res?.text ?? res ?? '').split(/\r?\n/);
      lines = arr.filter((x) => x !== '').map((text) => ({ text: String(text), lv: levelOf(String(text)) })); paint();
    } catch (e) { if (alive) toast(errText(e), 'err'); }
    stream = sse(`/logs/${src}/stream`, (d) => { if (d !== '') add(d); }, { onState: (s) => { conn = s; meta(); } });
  }
  function renderSrc() {
    clear(srcSeg);
    SOURCES.forEach((s) => srcSeg.append(h('button', { type: 'button', 'aria-pressed': String(s === src), onClick: () => { src = s; history.replaceState(null, '', '#/logs/' + s); renderSrc(); open(); } }, t('logs.src.' + s))));
  }
  renderSrc(); open();
  return () => { alive = false; stream?.close(); };
}

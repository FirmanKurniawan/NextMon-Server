// DOM helpers: hyperscript, toasts, modal dialogs, formatting.
import { icon } from './icons.js';
import { t, lang } from './i18n.js';

const PROPS = new Set(['value', 'checked', 'disabled', 'selected', 'hidden', 'indeterminate', 'readOnly', 'open']);
export function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === 'dataset') Object.assign(el.dataset, v);
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else if (PROPS.has(k)) el[k] = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  add(el, kids);
  return el;
}
export function add(el, kids) {
  for (const k of kids.flat(Infinity)) {
    if (k == null || k === false) continue;
    el.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return el;
}
export const clear = (el) => { el.replaceChildren(); return el; };
export const $ = (sel, root = document) => root.querySelector(sel);

export function debounce(fn, ms) { let id; return (...a) => { clearTimeout(id); id = setTimeout(() => fn(...a), ms); }; }
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/* ---- formatting ---- */
export function fmtSize(mb) {
  if (mb == null || isNaN(mb)) return '—';
  return mb >= 1024 ? (mb / 1024).toFixed(1) + ' GB' : Math.round(mb) + ' MB';
}
export function fmtBytes(b) {
  if (b == null || isNaN(b)) return '—';
  if (b < 1024) return b + ' B';
  if (b < 1048576) return (b / 1024).toFixed(1) + ' KB';
  if (b < 1073741824) return (b / 1048576).toFixed(1) + ' MB';
  return (b / 1073741824).toFixed(2) + ' GB';
}
export function fmtUptime(sec) {
  if (sec == null || sec < 0) return '—';
  sec = Math.floor(sec);
  const d = Math.floor(sec / 86400), hh = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  const u = (k) => t('u.' + k);
  if (d) return `${d}${u('d')} ${hh}${u('h')}`;
  if (hh) return `${hh}${u('h')} ${m}${u('m')}`;
  if (m) return `${m}${u('m')} ${s}${u('s')}`;
  return `${s}${u('s')}`;
}
export function fmtDate(v, withTime = false) {
  if (!v) return '—';
  const d = new Date(v);
  if (isNaN(d)) return String(v);
  return new Intl.DateTimeFormat(lang(), withTime ? { dateStyle: 'medium', timeStyle: 'short' } : { dateStyle: 'medium' }).format(d);
}
/** URL as reachable from the browser viewing the panel (panel may be opened from a phone on the LAN). */
export function reachableUrl(url) {
  if (!url) return '';
  try {
    const u = new URL(url);
    const local = ['localhost', '127.0.0.1', '::1', '[::1]', '0.0.0.0'];
    if (local.includes(u.hostname) && !local.includes(location.hostname)) u.hostname = location.hostname;
    return u.toString().replace(/\/$/, '');
  } catch { return url; }
}
export const isLoopbackPage = () => ['localhost', '127.0.0.1', '[::1]', '::1'].includes(location.hostname);
export const errText = (e) => (e && e.message && e.code !== 'network') ? e.message : (e?.code === 'network' ? t('err.network') : t('err.generic'));

/* ---- skeleton / empty ---- */
export const skel = (w = '100%', hgt = 14) => h('div', { class: 'skel', style: { width: w, height: hgt + 'px' }, 'aria-hidden': 'true' });
export function skelRows(n = 3) {
  return h('div', { class: 'rows', role: 'status', 'aria-label': t('common.loading') },
    Array.from({ length: n }, () => h('div', { class: 'rrow' }, h('div', { class: 'r-main stack-sm' }, skel('40%', 14), skel('65%', 11)))));
}
export function emptyState(iconName, title, desc, action) {
  return h('div', { class: 'empty' }, icon(iconName, 32), h('b', null, title), desc && h('p', null, desc), action);
}

/* ---- busy button ---- */
export async function busy(btn, fn) {
  if (btn.classList.contains('loading')) return;
  btn.classList.add('loading'); btn.setAttribute('aria-busy', 'true');
  try { return await fn(); } finally { btn.classList.remove('loading'); btn.removeAttribute('aria-busy'); }
}

/* ---- toasts ---- */
let toastBox;
export function toast(message, type = 'info', { timeout = 5000, action } = {}) {
  if (!toastBox) { toastBox = h('div', { class: 'toasts', 'aria-live': 'polite' }); document.body.append(toastBox); }
  const ic = { ok: 'check-circle', err: 'x-circle', warn: 'alert', info: 'info' }[type];
  const el = h('div', { class: 'toast ' + type, role: type === 'err' ? 'alert' : 'status' }, icon(ic, 16),
    h('div', { class: 't-body' }, message),
    action && h('button', { class: 'btn btn-sm btn-ghost', onClick: () => { action.onClick(); close(); } }, action.label),
    h('button', { class: 'btn btn-sm btn-ghost btn-icon', 'aria-label': t('common.close'), onClick: () => close() }, icon('x', 14)));
  const close = () => el.remove();
  toastBox.append(el);
  while (toastBox.children.length > 4) toastBox.firstChild.remove();
  if (timeout) setTimeout(close, timeout);
  return close;
}

/* ---- modal ---- */
/** modal({title, icon, tone, wide, body, buttons:[{label, kind, value, keepOpen, onClick}], dismissible}) -> {close(v), result, body, setButtons} */
export function modal({ title, iconName = 'info', tone = '', wide = false, body, buttons = [], dismissible = true, onClose }) {
  const dlg = h('dialog', { class: wide ? 'wide' : '', 'aria-labelledby': 'dlg-t-' + Math.random().toString(36).slice(2, 7) });
  const tid = dlg.getAttribute('aria-labelledby');
  const bodyEl = h('div', { class: 'dlg-b' }, body);
  const foot = h('div', { class: 'dlg-f' });
  dlg.append(h('div', { class: 'dlg-h' }, h('div', { class: 'dlg-ic ' + tone }, icon(iconName, 18)), h('h2', { id: tid, style: { paddingTop: '6px' } }, title)), bodyEl, foot);
  let resolve; const result = new Promise((r) => { resolve = r; });
  let done = false;
  const close = (v = null) => { if (done) return; done = true; if (dlg.open) dlg.close(); dlg.remove(); onClose?.(); resolve(v); };
  const setButtons = (bs) => {
    foot.replaceChildren(...bs.map((b) => {
      const btn = h('button', { class: 'btn ' + (b.kind === 'primary' ? 'btn-primary' : b.kind === 'danger' ? 'btn-danger' : ''), disabled: !!b.disabled, type: 'button', 'data-auto': b.autofocus ? '1' : null,
        onClick: async () => { if (b.onClick) { await busy(btn, () => b.onClick(close)); } if (!b.keepOpen) close(b.value ?? null); } }, b.label);
      return btn;
    }));
  };
  setButtons(buttons);
  dlg.addEventListener('cancel', (e) => { e.preventDefault(); if (dismissible) close(null); });
  dlg.addEventListener('click', (e) => { if (dismissible && e.target === dlg) close(null); });
  document.body.append(dlg);
  dlg.showModal();
  (dlg.querySelector('[data-auto]') || foot.querySelector('.btn-primary, .btn-danger') || foot.lastElementChild)?.focus();
  return { dlg, close, result, body: bodyEl, setButtons, foot };
}

export function confirmBox({ title, message, confirmLabel, danger = false, tone, iconName, check, extra }) {
  let box; const state = { checked: false };
  const chk = check && h('label', { class: 'check' }, h('input', { type: 'checkbox', onChange: (e) => { state.checked = e.target.checked; box.setButtons(btns()); } }), check);
  const btns = () => [
    { label: t('common.cancel'), value: false },
    { label: confirmLabel || t('common.confirm'), kind: danger ? 'danger' : 'primary', value: true, disabled: !!check && !state.checked },
  ];
  box = modal({ title, iconName: iconName || (danger ? 'alert' : 'info'), tone: tone || (danger ? 'danger' : ''), body: [message instanceof Node ? message : h('p', null, message), extra, chk], buttons: btns() });
  if (danger || check) box.foot.firstElementChild.focus(); else box.foot.lastElementChild.focus();
  return box.result.then((v) => v === true);
}

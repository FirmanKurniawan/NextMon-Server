// Diagnostik: checks with pass/warn/fail, hints and one-click fixes where possible.
import { get, post } from '../api.js';
import { h, clear, toast, errText, busy, skelRows, emptyState } from '../dom.js';
import { icon } from '../icons.js';
import { t, has, lang } from '../i18n.js';
const loc = (v) => (v && typeof v === 'object' ? (v[lang()] || v.id || v.en || '') : v);
import { followJob } from '../store.js';

const SEV_ICON = { pass: 'check', warn: 'alert', fail: 'x' };

export function mount(root) {
  let alive = true;
  const summary = h('div', { class: 'row', style: { marginBottom: '16px' }, 'aria-live': 'polite' });
  const list = h('div');
  const runBtn = h('button', { class: 'btn btn-primary', type: 'button', onClick: (e) => busy(e.currentTarget, run) }, icon('refresh', 14), t('diag.run'));
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('diag.title')), h('p', { class: 'page-desc' }, t('diag.desc'))), h('div', { class: 'page-actions' }, runBtn)),
    summary, h('section', { class: 'card glass' }, list));

  async function run() {
    clear(list); list.append(skelRows(5)); clear(summary);
    try { const r = await get('/diagnostics'); if (!alive) return; render(r.checks || r); }
    catch (e) { if (!alive) return; clear(list); list.append(h('div', { class: 'alert err', role: 'alert', style: { margin: '16px' } }, icon('x-circle', 16), h('div', { class: 'a-body' }, errText(e)))); }
  }
  function render(checks) {
    clear(list); clear(summary);
    const n = (s) => checks.filter((c) => c.status === s).length;
    summary.append(h('span', { class: 'badge badge-ok' }, `${n('pass')} ${t('diag.pass')}`), h('span', { class: 'badge badge-warn' }, `${n('warn')} ${t('diag.warn')}`), h('span', { class: 'badge badge-err' }, `${n('fail')} ${t('diag.fail')}`));
    if (!checks.length) { list.append(emptyState('inbox', t('diag.empty'))); return; }
    list.append(h('div', { class: 'rows' }, checks.map((c) => h('div', { class: 'rrow' },
      h('div', { class: 'sev ' + c.status, role: 'img', 'aria-label': t('diag.' + c.status) }, icon(SEV_ICON[c.status] || 'info', 16)),
      h('div', { class: 'r-main' }, h('div', { class: 'r-title' }, has('diag.c.' + c.id) ? t('diag.c.' + c.id) : loc(c.title)), c.detail && h('div', { class: 'r-sub' }, loc(c.detail)),
        c.hint && c.status !== 'pass' && h('div', { class: 'help', style: { marginTop: '4px' } }, icon('info', 12), ' ', loc(c.hint))),
      h('div', { class: 'r-act' }, c.fixable && c.status !== 'pass' && h('button', { class: 'btn btn-sm', type: 'button', onClick: (e) => fix(c, e.currentTarget) }, icon('tool', 14), t('diag.fix')))))));
  }
  async function fix(c, btn) {
    await busy(btn, async () => {
      try { const r = await post('/diagnostics/fix', { id: c.id }); if (r?.jobId) await followJob(r.jobId); toast(t('diag.fixed'), 'ok'); await run(); }
      catch (e) { toast(t('diag.fixfail') + ' ' + errText(e), 'err'); }
    });
  }
  run();
  return () => { alive = false; };
}

// Backup: create, list, restore (with warning), download.
import { get, post, api } from '../api.js';
import { h, clear, toast, errText, fmtSize, fmtDate, skelRows, emptyState, confirmBox, busy } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { followJob, refreshStatus } from '../store.js';

export function mount(root) {
  let alive = true, items = null, working = null;
  const list = h('div');
  const createBtn = h('button', { class: 'btn btn-primary', type: 'button', onClick: () => create() }, icon('plus', 14), t('backup.create'));
  const prog = h('div', { hidden: true });
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('backup.title')), h('p', { class: 'page-desc' }, t('backup.desc'))), h('div', { class: 'page-actions' }, createBtn)),
    prog, h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('archive', 16), t('backup.list'))), list));

  function showProg(label, job) {
    prog.hidden = false;
    const p = Math.round((job.progress || 0) * 100);
    prog.replaceChildren(h('div', { class: 'alert info', role: 'status', style: { marginBottom: '16px' } }, icon('refresh', 16), h('div', { class: 'a-body prog-wrap' }, h('div', { class: 'a-title' }, label),
      h('div', { class: 'progress', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': p, 'aria-label': label }, h('i', { style: { width: p + '%' } })), h('div', { class: 'prog-meta' }, h('span', null, job.step || '…'), h('span', null, p + '%')))));
  }
  async function runJob(res, label) {
    if (res?.jobId) await followJob(res.jobId, (j) => showProg(label, j));
  }
  async function load() {
    try { const r = await get('/backup/list'); items = (r.backups || r.items || r || []).slice().sort((a, b) => String(b.date).localeCompare(String(a.date))); }
    catch (e) { if (!alive) return; clear(list); list.append(h('div', { class: 'alert err', role: 'alert', style: { margin: '16px' } }, icon('x-circle', 16), h('div', { class: 'a-body' }, errText(e), h('div', { class: 'a-actions' }, h('button', { class: 'btn btn-sm', onClick: load }, t('common.retry')))))); return; }
    if (alive) render();
  }
  function render() {
    clear(list);
    if (!items.length) { list.append(emptyState('archive', t('backup.empty.title'), t('backup.empty.desc'), h('button', { class: 'btn btn-primary', type: 'button', onClick: () => create() }, icon('plus', 14), t('backup.create')))); return; }
    list.append(h('div', { class: 'rows' }, items.map((b) => h('div', { class: 'rrow' },
      h('div', { class: 'r-main' }, h('div', { class: 'r-title mono', style: { fontSize: '13px' } }, b.name, b.kind === 'auto' && h('span', { class: 'badge' }, t('backup.auto'))), h('div', { class: 'r-sub' }, h('span', null, fmtDate(b.date, true)), h('span', null, fmtSize(b.sizeMB)))),
      h('div', { class: 'r-act' },
        h('button', { class: 'btn btn-sm', type: 'button', disabled: !!working, onClick: () => download(b) }, icon('download', 14), t('backup.download')),
        h('button', { class: 'btn btn-sm', type: 'button', disabled: !!working, onClick: () => restore(b) }, icon('upload', 14), t('backup.restore')))))));
  }
  async function create() {
    if (working) return; working = 'create'; createBtn.disabled = true;
    try { const r = await post('/backup/create'); await runJob(r, t('backup.creating')); toast(t('backup.created'), 'ok'); } catch (e) { toast(t('backup.fail') + ' ' + errText(e), 'err'); }
    working = null; createBtn.disabled = false; prog.hidden = true; if (alive) load();
  }
  async function restore(b) {
    const ok = await confirmBox({ title: t('backup.restore.title'), danger: true, iconName: 'alert', confirmLabel: t('backup.restore'), check: t('backup.restore.check'),
      message: h('div', { class: 'stack-sm' }, h('p', null, t('backup.restore.body', { name: b.name })), h('div', { class: 'alert warn' }, icon('alert', 16), h('div', { class: 'a-body' }, t('backup.restore.warn')))) });
    if (!ok) return;
    working = 'restore'; render();
    try { const r = await post('/backup/restore', { name: b.name }); await runJob(r, t('backup.restoring')); toast(t('backup.restored'), 'ok'); await refreshStatus().catch(() => {}); }
    catch (e) { toast(t('backup.restore.fail') + ' ' + errText(e), 'err', { timeout: 9000 }); }
    working = null; prog.hidden = true; if (alive) { render(); load(); }
  }
  async function download(b) {
    try {
      const blob = await api('/backup/download?name=' + encodeURIComponent(b.name), { blob: true });
      const a = h('a', { href: URL.createObjectURL(blob), download: b.name }); document.body.append(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 4000);
    } catch (e) { toast(errText(e), 'err'); }
  }
  list.append(skelRows(3)); load();
  return () => { alive = false; };
}

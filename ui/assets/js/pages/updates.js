// Pembaruan: NextMON app update check/apply + runtime update summary.
import { get, post } from '../api.js';
import { h, clear, toast, errText, fmtDate, busy, confirmBox, skel } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S, cfg, followJob, editionOf } from '../store.js';
import { checkBadges } from '../main.js';

export function mount(root) {
  let alive = true;
  const appBox = h('div', { class: 'card-b' }, skel('50%', 16));
  const rtBox = h('div', { class: 'rows' });
  const progBox = h('div', { hidden: true });
  const checkBtn = h('button', { class: 'btn', type: 'button', onClick: (e) => busy(e.currentTarget, () => checkApp(true)) }, icon('refresh', 14), t('upd.check'));
  const autoSw = h('input', { type: 'checkbox', checked: cfg().update?.checkOnStart !== false, onChange: async (e) => {
    try { await post('/config', { update: { checkOnStart: e.target.checked } }); toast(t('common.saved'), 'ok', { timeout: 1800 }); } catch (er) { e.target.checked = !e.target.checked; toast(errText(er), 'err'); } } });
  const chanSel = h('select', { class: 'select', style: { width: 'auto' }, 'aria-label': t('upd.channel'), onChange: async (e) => {
    try { await post('/config', { update: { appChannel: e.target.value } }); toast(t('common.saved'), 'ok', { timeout: 1800 }); checkApp(true); } catch (er) { toast(errText(er), 'err'); } } },
    ['stable', 'beta'].map((c) => h('option', { value: c, selected: (cfg().update?.appChannel || 'stable') === c }, t('upd.ch.' + c))));
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('upd.title')), h('p', { class: 'page-desc' }, t('upd.desc'))), h('div', { class: 'page-actions' }, checkBtn)),
    progBox,
    h('div', { class: 'stack' },
      h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('box', 16), t('upd.app'))), appBox,
        h('div', { class: 'card-f' }, h('label', { class: 'switch' }, autoSw, h('span', { class: 'track' }), h('span', null, t('upd.auto'))), h('span', { class: 'spacer-r' }), h('label', { class: 'row', style: { gap: '8px' } }, h('span', { class: 'help' }, t('upd.channel')), chanSel))),
      h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('layers', 16), t('upd.runtimes')), h('a', { class: 'btn btn-sm btn-ghost', href: '#/runtimes' }, t('upd.manage'))), rtBox)));

  async function checkApp(manual) {
    try {
      const u = await get('/update/check'); if (!alive) return; paintApp(u);
      if (manual) toast(u.available ? t('upd.found', { v: u.latest }) : t('upd.uptodate'), u.available ? 'info' : 'ok');
      checkBadges();
    } catch (e) { if (!alive) return; clear(appBox); appBox.append(h('div', { class: 'alert err', role: 'alert' }, icon('x-circle', 16), h('div', { class: 'a-body' }, t('upd.fail'), ' ', errText(e)))); }
  }
  function paintApp(u) {
    clear(appBox);
    appBox.append(h('dl', { class: 'kv' }, h('dt', null, t('upd.current')), h('dd', { class: 'mono' }, 'v' + (u.current || S.status?.version || '—')),
      h('dt', null, t('upd.latest')), h('dd', null, u.latest ? h('span', null, h('span', { class: 'mono' }, 'v' + u.latest), u.date && ' · ' + fmtDate(u.date)) : '—')));
    if (u.available) {
      appBox.append(h('div', { class: 'alert info', style: { marginTop: '16px' } }, icon('arrow-up', 16), h('div', { class: 'a-body' },
        h('div', { class: 'a-title' }, t('upd.available', { v: u.latest })),
        u.notes?.length ? [h('p', { class: 'section-title', style: { margin: '12px 0 4px' } }, t('upd.notes')), h('ul', { class: 'notes' }, u.notes.map((n) => h('li', null, n)))] : null,
        h('div', { class: 'a-actions' }, h('button', { class: 'btn btn-primary btn-sm', type: 'button', onClick: (e) => apply(u, e.currentTarget) }, icon('download', 14), t('upd.apply')),
          u.url && h('a', { class: 'btn btn-sm', href: u.url, target: '_blank', rel: 'noopener' }, icon('external', 14), t('upd.release'))))));
    } else appBox.append(h('div', { class: 'alert', style: { marginTop: '16px' } }, icon('check-circle', 16), h('div', { class: 'a-body' }, t('upd.uptodate'))));
  }
  async function apply(u, btn) {
    if (!(await confirmBox({ title: t('upd.apply.title', { v: u.latest }), iconName: 'download', confirmLabel: t('upd.apply'),
      message: h('div', { class: 'stack-sm' }, h('p', null, t('upd.apply.body')), h('ul', { class: 'notes' }, h('li', null, t('upd.apply.a')), h('li', null, t('upd.apply.b')), h('li', null, t('upd.apply.c')))) }))) return;
    await busy(btn, async () => {
      try {
        const r = await post('/update/apply');
        progBox.hidden = false;
        if (r.jobId) await followJob(r.jobId, (j) => {
          const p = Math.round((j.progress || 0) * 100);
          progBox.replaceChildren(h('div', { class: 'alert info', role: 'status', style: { marginBottom: '16px' } }, icon('download', 16), h('div', { class: 'a-body prog-wrap' }, h('div', { class: 'a-title' }, t('upd.applying')),
            h('div', { class: 'progress', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': p, 'aria-label': t('upd.applying') }, h('i', { style: { width: p + '%' } })), h('div', { class: 'prog-meta' }, h('span', null, j.step || '…'), h('span', null, p + '%')))));
        });
        progBox.replaceChildren(h('div', { class: 'alert info', role: 'status', style: { marginBottom: '16px' } }, icon('refresh', 16), h('div', { class: 'a-body' }, t('upd.restarting'))));
        setTimeout(() => location.reload(), 6000);
      } catch (e) { progBox.hidden = true; toast(t('upd.apply.fail') + ' ' + errText(e), 'err', { timeout: 9000 }); }
    });
  }

  async function loadRt() {
    clear(rtBox); rtBox.append(h('div', { class: 'rrow' }, skel('40%', 14)));
    const rows = [];
    for (const kind of ['php', 'node']) {
      let d; try { d = await get('/runtimes/' + kind); } catch { d = null; }
      if (!alive) return;
      const ed = editionOf(kind); const cur = d?.installed?.find((i) => (i.usedBy || []).includes(ed));
      const name = kind === 'php' ? 'PHP' : 'Node.js';
      rows.push(h('div', { class: 'rrow' },
        h('div', { class: 'r-main' }, h('div', { class: 'r-title' }, name), h('div', { class: 'r-sub' }, d ? [h('span', null, t('upd.rt.cur'), ': ', h('span', { class: 'mono' }, cur?.version || '—')), h('span', null, t('upd.latest'), ': ', h('span', { class: 'mono' }, d.latest?.version || '—'))] : t('common.unavailable'))),
        h('div', { class: 'r-act' }, d?.updateAvailable ? [h('span', { class: 'badge badge-info' }, t('rt.new')), h('a', { class: 'btn btn-sm btn-primary', href: '#/runtimes/' + kind }, icon('arrow-up', 14), t('rt.upgrade'))] : d ? h('span', { class: 'badge badge-ok' }, icon('check', 12), t('upd.uptodate.short')) : null)));
    }
    clear(rtBox); rtBox.append(...rows);
  }
  checkApp(false); loadRt();
  return () => { alive = false; };
}

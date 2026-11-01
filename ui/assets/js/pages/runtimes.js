// Runtimes: PHP / Node.js tabs. Installed vs available, install/activate/upgrade/uninstall with job progress.
import { get, post } from '../api.js';
import { h, clear, toast, errText, fmtSize, fmtDate, skelRows, emptyState, modal, confirmBox, busy } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S, jobs, subscribeJobs, trackJob, editionOf, refreshStatus } from '../store.js';
import { edLabel } from '../edition.js';
import { checkBadges } from '../main.js';

const KINDS = ['php', 'node'];
const kindName = (k) => (k === 'php' ? 'PHP' : 'Node.js');

export function mount(root, params) {
  let kind = KINDS.includes(params[0]) ? params[0] : 'php';
  let variant = 'nts', channel = 'lts', showAll = false, data = null, alive = true;
  let unsubJobs = null;
  const tabs = h('div', { class: 'tabs', role: 'tablist', 'aria-label': t('rt.title') });
  const filters = h('div', { class: 'row', style: { marginBottom: '16px' } });
  const banner = h('div');
  const panel = h('div', { class: 'stack', role: 'tabpanel' });
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('rt.title')), h('p', { class: 'page-desc' }, t('rt.desc'))),
    h('div', { class: 'page-actions' }, h('button', { class: 'btn', type: 'button', onClick: (e) => busy(e.currentTarget, () => load(true)) }, icon('refresh', 14), t('common.refresh')))),
    tabs, banner, filters, panel);

  const jobKey = (v) => `${kind}:${v}`;

  function renderTabs() {
    clear(tabs);
    KINDS.forEach((k) => tabs.append(h('button', { role: 'tab', id: 'tab-' + k, 'aria-selected': String(k === kind), tabindex: k === kind ? '0' : '-1', 'aria-controls': 'rt-panel',
      onClick: () => { if (k !== kind) { location.hash = '#/runtimes/' + k; } },
      onKeydown: (e) => { if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { const n = KINDS[(KINDS.indexOf(kind) + 1) % 2]; location.hash = '#/runtimes/' + n; setTimeout(() => document.getElementById('tab-' + n)?.focus(), 150); } } },
      kindName(k))));
    panel.id = 'rt-panel'; panel.setAttribute('aria-labelledby', 'tab-' + kind);
  }
  function renderFilters() {
    clear(filters);
    if (kind === 'php') filters.append(h('span', { class: 'label' }, t('rt.variant')), h('div', { class: 'seg', role: 'group', 'aria-label': t('rt.variant') },
      ['nts', 'ts'].map((v) => h('button', { type: 'button', 'aria-pressed': String(variant === v), onClick: () => { variant = v; showAll = false; renderFilters(); load(); } }, v.toUpperCase()))),
      h('span', { class: 'help' }, t(variant === 'nts' ? 'rt.nts.help' : 'rt.ts.help')));
    else filters.append(h('span', { class: 'label' }, t('rt.channel')), h('div', { class: 'seg', role: 'group', 'aria-label': t('rt.channel') },
      ['lts', 'current'].map((v) => h('button', { type: 'button', 'aria-pressed': String(channel === v), onClick: () => { channel = v; showAll = false; renderFilters(); load(); } }, t('rt.ch.' + v)))),
      h('span', { class: 'help' }, t(channel === 'lts' ? 'rt.lts.help' : 'rt.current.help')));
  }

  async function load(toastDone) {
    try {
      const q = kind === 'php' ? `?variant=${variant}` : `?channel=${channel}`;
      data = await get(`/runtimes/${kind}${q}`);
      if (!alive) return; render();
      if (toastDone) toast(t('common.refreshed'), 'ok', { timeout: 1800 });
    } catch (e) { if (!alive) return; clear(panel); panel.append(h('div', { class: 'alert err', role: 'alert' }, icon('x-circle', 16), h('div', { class: 'a-body' }, errText(e), h('div', { class: 'a-actions' }, h('button', { class: 'btn btn-sm', onClick: () => load() }, t('common.retry')))))); }
  }

  function jobBar(rec) {
    const p = Math.round((rec.progress || 0) * 100);
    return h('div', { class: 'r-prog prog-wrap' },
      h('div', { class: 'progress' + (rec.state === 'error' ? ' err' : ''), role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': p, 'aria-label': rec.step || t('rt.installing') }, h('i', { style: { width: p + '%' } })),
      h('div', { class: 'prog-meta' }, h('span', null, rec.state === 'error' ? rec.error : (rec.step || t('rt.installing'))), h('span', null, p + '%')));
  }

  function render() {
    clear(banner); clear(panel);
    if (!data) { panel.append(skelRows(4)); return; }
    const ed = editionOf(kind);
    if (data.updateAvailable && data.latest) {
      const act = data.installed.find((i) => i.usedBy?.includes(ed) || i.active);
      banner.append(h('div', { class: 'alert info', role: 'status', style: { marginBottom: '16px' } }, icon('arrow-up', 16),
        h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('rt.update.title', { rt: kindName(kind), v: data.latest.version })),
          t('rt.update.body', { cur: act?.version || '—' }),
          h('div', { class: 'a-actions' }, h('button', { class: 'btn btn-sm btn-primary', type: 'button', onClick: () => upgrade() }, icon('arrow-up', 14), t('rt.upgrade.latest'))))));
    }
    /* installed */
    panel.append(h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('box', 16), t('rt.installed'), ' ', h('span', { class: 'badge' }, data.installed.length))),
      data.installed.length ? h('div', { class: 'rows' }, data.installed.slice().sort((a, b) => b.version.localeCompare(a.version, undefined, { numeric: true })).map(installedRow))
        : emptyState('inbox', t('rt.none.title', { rt: kindName(kind) }), t('rt.none.desc'))));
    /* available */
    const list = showAll ? data.available : data.available.slice(0, 8);
    panel.append(h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('download', 16), t('rt.available'), ' ', h('span', { class: 'badge' }, data.available.length))),
      data.available.length ? h('div', { class: 'rows' }, list.map(availableRow)) : emptyState('inbox', t('rt.avail.none')),
      data.available.length > 8 && h('div', { class: 'card-f' }, h('button', { class: 'btn btn-sm', type: 'button', onClick: () => { showAll = !showAll; render(); } }, t(showAll ? 'rt.less' : 'rt.more', { n: data.available.length - 8 })))));
  }

  function installedRow(i) {
    const ed = editionOf(kind); const used = i.usedBy || []; const inUse = used.length > 0;
    const rec = inUse ? jobs.get(`up:${kind}`) : null;
    const canUpgrade = data.updateAvailable && inUse;
    const sub = [fmtSize(i.sizeMB), kind === 'php' && i.tsnts ? i.tsnts.toUpperCase() : null, i.arch].filter(Boolean);
    return h('div', { class: 'rrow' },
      h('div', { class: 'r-main' }, h('div', { class: 'r-title' }, h('span', { class: 'mono' }, i.version),
        inUse && h('span', { class: 'badge badge-ok' }, t('rt.active')), used.map((a) => h('span', { class: 'chip', title: t('rt.usedby') }, edLabel(a)))),
        h('div', { class: 'r-sub' }, sub.map((s) => h('span', null, s)), i.path && h('span', { class: 'mono', title: i.path, style: { maxWidth: '260px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, i.path))),
      h('div', { class: 'r-act' },
        !used.includes(ed) && h('button', { class: 'btn btn-sm', type: 'button', onClick: (e) => activate(i, e.currentTarget) }, icon('check', 14), t('rt.use', { app: edLabel(ed) })),
        canUpgrade && h('button', { class: 'btn btn-sm', type: 'button', disabled: !!rec, onClick: () => upgrade() }, icon('arrow-up', 14), t('rt.upgrade')),
        h('button', { class: 'btn btn-sm btn-danger', type: 'button', disabled: inUse, 'aria-disabled': inUse, title: inUse ? t('rt.delete.inuse') : '', onClick: () => uninstall(i) }, icon('trash', 14), t('rt.delete'))),
      rec && jobBar(rec));
  }

  function availableRow(a) {
    const have = data.installed.some((i) => i.version === a.version);
    const rec = jobs.get(jobKey(a.version));
    return h('div', { class: 'rrow' },
      h('div', { class: 'r-main' }, h('div', { class: 'r-title' }, h('span', { class: 'mono' }, a.version),
        a.isNewer && h('span', { class: 'badge badge-info' }, t('rt.new')), a.channel && h('span', { class: 'badge' }, a.channel)),
        h('div', { class: 'r-sub' }, h('span', null, fmtDate(a.date)), h('span', null, fmtSize(a.sizeMB)))),
      h('div', { class: 'r-act' }, have ? h('span', { class: 'badge badge-ok' }, icon('check', 12), t('rt.installedbadge'))
        : h('button', { class: 'btn btn-sm btn-primary', type: 'button', disabled: !!rec, onClick: () => install(a) }, icon('download', 14), t('rt.install'))),
      rec && jobBar(rec));
  }

  async function install(a) {
    try {
      const r = await post(`/runtimes/${kind}/install`, { version: a.version, ...(kind === 'php' ? { variant } : {}) });
      const p = trackJob(jobKey(a.version), r.jobId, { kind });
      render();
      p.then(async () => { toast(t('rt.installed.ok', { v: a.version }), 'ok'); await afterChange(); }).catch((e) => toast(t('rt.install.fail', { v: a.version }) + ' ' + errText(e), 'err', { timeout: 9000 }));
    } catch (e) { toast(errText(e), 'err'); }
  }
  async function afterChange() { if (alive) await load(); checkBadges(); refreshStatus().catch(() => {}); }

  async function activate(i, btn) {
    const ed = editionOf(kind);
    const running = S.status?.edition === ed && S.status?.app?.state === 'running';
    if (running && !(await confirmBox({ title: t('rt.use.title', { v: i.version }), message: t('rt.use.restart', { app: edLabel(ed) }), confirmLabel: t('rt.use.go') }))) return;
    await busy(btn, async () => {
      try { await post(`/runtimes/${kind}/activate`, { version: i.version, app: ed }); toast(t('rt.use.ok', { v: i.version, app: edLabel(ed) }), 'ok'); await afterChange(); } catch (e) { toast(errText(e), 'err'); }
    });
  }

  async function uninstall(i) {
    if ((i.usedBy || []).length) return;
    if (!(await confirmBox({ title: t('rt.delete.title', { v: i.version }), message: t('rt.delete.body', { rt: kindName(kind), v: i.version, mb: fmtSize(i.sizeMB) }), confirmLabel: t('rt.delete'), danger: true, iconName: 'trash' }))) return;
    try { await post(`/runtimes/${kind}/uninstall`, { version: i.version }); toast(t('rt.delete.ok', { v: i.version }), 'ok'); await afterChange(); } catch (e) { toast(errText(e), 'err'); }
  }

  async function upgrade() {
    const latest = data.latest?.version; const ed = editionOf(kind);
    const cur = data.installed.find((i) => (i.usedBy || []).includes(ed));
    const ok = await confirmBox({ title: t('rt.upgrade.title', { rt: kindName(kind), v: latest }), iconName: 'arrow-up', confirmLabel: t('rt.upgrade'),
      message: h('div', { class: 'stack-sm' }, h('p', null, t('rt.upgrade.body', { cur: cur?.version || '—', v: latest, app: edLabel(ed) })),
        h('div', { class: 'alert info' }, icon('shield', 16), h('div', { class: 'a-body' }, t('rt.upgrade.rollback')))) });
    if (!ok) return;
    try {
      const r = await post(`/runtimes/${kind}/upgrade`, { to: latest, app: ed });
      const p = trackJob(`up:${kind}`, r.jobId, { kind }); render();
      p.then(async () => { toast(t('rt.upgrade.ok', { v: latest }), 'ok'); await afterChange(); })
        .catch(async (e) => { toast(t('rt.upgrade.fail') + ' ' + errText(e), 'err', { timeout: 10000 }); await afterChange(); });
    } catch (e) { toast(errText(e), 'err'); }
  }

  unsubJobs = subscribeJobs(() => { if (alive && data) render(); });
  renderTabs(); renderFilters(); render(); load();
  return () => { alive = false; unsubJobs?.(); };
}

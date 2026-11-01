// Edition switching flow (PHP <-> Next.js). Only one edition runs at a time.
import { get, post } from './api.js';
import { h, modal, toast, errText } from './dom.js';
import { icon } from './icons.js';
import { t } from './i18n.js';
import { S, kindOf, followJob, refreshStatus } from './store.js';

export const edLabel = (ed) => (ed === 'php' ? 'PHP' : 'Next.js');
export const edRuntimeLabel = (ed) => (ed === 'php' ? 'PHP' : 'Node.js');

function progressUI() {
  const bar = h('i', { style: { width: '0%' } });
  const wrap = h('div', { class: 'prog-wrap' },
    h('div', { class: 'progress', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': '0' }, bar),
    h('div', { class: 'prog-meta' }, h('span', { class: 'step' }, '…'), h('span', { class: 'pct' }, '0%')));
  const bg = wrap.querySelector('.progress');
  return { el: wrap, set(job) {
    const p = Math.round((job.progress || 0) * 100);
    bar.style.width = p + '%'; bg.setAttribute('aria-valuenow', p); wrap.querySelector('.pct').textContent = p + '%';
    wrap.querySelector('.step').textContent = job.step || job.message || '…';
  } };
}

/** Ask the user, then switch. Resolves true when the edition was switched. */
export async function switchEdition(target) {
  const st = S.status; if (!st || st.edition === target) return false;
  const from = st.edition; const info = st.editions?.[target] || {};
  const wasRunning = st.app?.state === 'running' || st.app?.state === 'starting';
  const needsRuntime = info.installed === false;
  const startBox = h('input', { type: 'checkbox', checked: wasRunning || !needsRuntime });
  const progress = progressUI();
  const prog = h('div', { hidden: true }, progress.el);
  const err = h('p', { class: 'help err', role: 'alert', hidden: true });

  const body = [
    h('p', null, t('edition.switch.body', { from: edLabel(from), to: edLabel(target) })),
    h('ul', { class: 'notes' },
      h('li', null, t('edition.switch.one')),
      h('li', null, t('edition.switch.data', { from: edLabel(from), to: edLabel(target) })),
      h('li', null, t('edition.switch.stop', { from: edLabel(from) }))),
    needsRuntime && h('div', { class: 'alert warn' }, icon('alert', 16), h('div', { class: 'a-body' }, t('edition.switch.noruntime', { rt: edRuntimeLabel(target) }))),
    h('label', { class: 'check' }, startBox, t('edition.switch.start')),
    prog, err,
  ];

  const m = modal({ title: t('edition.switch.title', { to: edLabel(target) }), iconName: 'swap', body, buttons: [
    { label: t('common.cancel'), value: false },
    { label: needsRuntime ? t('edition.switch.installgo') : t('edition.switch.go'), kind: 'primary', keepOpen: true, onClick: async (close) => {
      err.hidden = true; const cancel = m.foot.firstElementChild; cancel.disabled = true;
      try {
        m.dlg.querySelectorAll('input').forEach((i) => { i.disabled = true; });
        if (needsRuntime) {
          prog.hidden = false;
          const kind = kindOf(target);
          const rt = await get(`/runtimes/${kind}`);
          const ver = rt.latest?.version || rt.available?.[0]?.version;
          if (!ver) throw new Error(t('err.noversion'));
          const job = await post(`/runtimes/${kind}/install`, { version: ver, ...(kind === 'php' ? { variant: 'nts' } : {}) });
          await followJob(job.jobId, (j) => progress.set(j));
          await post(`/runtimes/${kind}/activate`, { version: ver, app: target });
        }
        prog.hidden = false; progress.set({ progress: 0.98, step: t('edition.switch.switching') });
        const res = await post('/edition', { edition: target, start: startBox.checked });
        if (res?.jobId) await followJob(res.jobId, (j) => progress.set(j));
        await refreshStatus();
        toast(t('edition.switched', { to: edLabel(target) }), 'ok');
        close(true);
      } catch (e) {
        cancel.disabled = false; m.dlg.querySelectorAll('input').forEach((i) => { i.disabled = false; });
        prog.hidden = true; err.hidden = false; err.textContent = errText(e);
      }
    } },
  ] });
  return (await m.result) === true;
}

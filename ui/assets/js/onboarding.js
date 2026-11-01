// First-run wizard: edition -> runtime version -> port & network -> install -> finish.
import { get, post } from './api.js';
import { h, clear, modal, toast, errText, debounce, fmtSize, isLoopbackPage, reachableUrl } from './dom.js';
import { icon } from './icons.js';
import { t } from './i18n.js';
import { S, kindOf, followJob, refreshStatus, cfg } from './store.js';
import { edLabel, edRuntimeLabel } from './edition.js';

export function runWizard() {
  const c = cfg();
  const W = { step: 0, edition: c.edition || 'php', version: '', variant: 'nts', port: null, lan: false, rt: null, error: '', portOk: true, portMsg: '', installing: false, progress: { progress: 0, step: '' } };
  const STEPS = ['edition', 'version', 'network', 'install', 'done'];
  const wrap = h('div', { class: 'stack' });
  const m = modal({ title: t('wiz.title'), iconName: 'box', wide: true, dismissible: false, body: wrap, buttons: [] });
  m.dlg.querySelector('.dlg-b').style.padding = '12px 16px';

  const stepper = () => h('ol', { class: 'steps', 'aria-label': t('wiz.steps') }, STEPS.slice(0, 4).map((s, i) => {
    const cls = i < W.step ? 'done' : '';
    return h('li', { class: cls, 'aria-current': i === W.step || (W.step === 4 && i === 3) ? 'step' : null }, h('span', { class: 'n' }, i < W.step ? icon('check', 12) : i + 1), h('span', { class: 't' }, t('wiz.s.' + s)));
  }));

  async function loadRt() {
    W.rt = null; render();
    try {
      W.rt = await get(`/runtimes/${kindOf(W.edition)}` + (kindOf(W.edition) === 'php' ? `?variant=${W.variant}` : ''));
      const inst = W.rt.installed.find((i) => i.version === (cfg().apps?.[W.edition]?.runtime));
      if (!W.version || !W.rt.available.concat(W.rt.installed).some((x) => x.version === W.version)) W.version = W.rt.latest?.version || inst?.version || W.rt.available[0]?.version || '';
    } catch (e) { W.error = errText(e); }
    render();
  }

  const checkPort = debounce(async () => {
    const n = Number(W.port);
    if (!Number.isInteger(n) || n < 1 || n > 65535) { W.portOk = false; W.portMsg = t('apps.port.range'); return render(true); }
    try { const r = await get(`/ports/check?port=${n}&edition=${W.edition}`); W.portOk = !!r.free; W.portMsg = r.free ? t('apps.port.free') : t('apps.port.busy', { by: r.usedBy || '?' }); } catch { W.portOk = true; W.portMsg = t('apps.port.unknown'); }
    render(true);
  }, 350);

  function choice(name, value, checked, title, desc, badge, onChange) {
    return h('label', { class: 'choice' }, h('input', { type: 'radio', name, value, checked, onChange }), h('div', null, h('div', { class: 'c-t' }, title, ' ', badge && h('span', { class: 'badge badge-info' }, badge)), h('div', { class: 'c-d' }, desc)));
  }

  function buttons(back, next, nextLabel, nextDisabled) {
    const bs = [];
    if (W.step === 0) bs.push({ label: t('wiz.skip'), keepOpen: true, onClick: async () => { await markDone(); m.close(false); } });
    if (back) bs.push({ label: t('common.back'), keepOpen: true, onClick: () => { W.step--; if (W.step === 1) loadRt(); else render(); } });
    if (next) bs.push({ label: nextLabel || t('common.next'), kind: 'primary', keepOpen: true, disabled: nextDisabled, onClick: next });
    m.setButtons(bs);
  }

  async function markDone() { try { localStorage.setItem('nm.onboarded', '1'); } catch { /* ignore */ } try { await post('/config', { onboarded: true }); } catch { /* ignore */ } }

  async function runInstall() {
    W.installing = true; W.error = ''; render();
    try {
      const kind = kindOf(W.edition);
      const have = W.rt.installed.some((i) => i.version === W.version);
      if (!have) {
        const job = await post(`/runtimes/${kind}/install`, { version: W.version, ...(kind === 'php' ? { variant: W.variant } : {}) });
        await followJob(job.jobId, (j) => { W.progress = j; renderProgress(); });
      }
      W.progress = { progress: 0.96, step: t('wiz.applying') }; renderProgress();
      await post(`/runtimes/${kind}/activate`, { version: W.version, app: W.edition });
      await post('/app/config', { edition: W.edition, port: Number(W.port) });
      await post('/config', { bind: W.lan ? '0.0.0.0' : '127.0.0.1' });
      if (S.status.edition !== W.edition) { const r = await post('/edition', { edition: W.edition, start: false }); if (r?.jobId) await followJob(r.jobId); }
      await markDone(); await refreshStatus();
      W.installing = false; W.step = 4; render();
    } catch (e) { W.installing = false; W.error = errText(e); render(); }
  }

  function renderProgress() {
    const el = wrap.querySelector('[data-prog]'); if (!el) return;
    const p = Math.round((W.progress.progress || 0) * 100);
    el.querySelector('i').style.width = p + '%'; el.setAttribute('aria-valuenow', p);
    wrap.querySelector('[data-step]').textContent = W.progress.step || '…'; wrap.querySelector('[data-pct]').textContent = p + '%';
  }

  function render(keepFocus) {
    const focusId = keepFocus && document.activeElement?.id; const pos = keepFocus && document.activeElement?.selectionStart;
    clear(wrap); wrap.append(stepper());
    const body = h('div', { class: 'stack', style: { padding: '8px 0' } }); wrap.append(body);
    const heading = (k) => [h('h3', { style: { fontSize: '16px' } }, t(`wiz.${k}.title`)), h('p', { class: 'muted' }, t(`wiz.${k}.desc`))];
    if (W.step === 0) {
      body.append(...heading('edition'),
        choice('ed', 'php', W.edition === 'php', 'NextMON PHP', t('wiz.ed.php'), t('wiz.recommended'), () => { W.edition = 'php'; W.version = ''; W.port = null; }),
        choice('ed', 'next', W.edition === 'next', 'NextMON Next.js', t('wiz.ed.next'), null, () => { W.edition = 'next'; W.version = ''; W.port = null; }),
        h('p', { class: 'help' }, t('wiz.ed.note')));
      buttons(false, () => { W.step = 1; loadRt(); });
    } else if (W.step === 1) {
      body.append(...heading('version'));
      if (W.error) body.append(h('div', { class: 'alert err', role: 'alert' }, icon('x-circle', 16), h('div', { class: 'a-body' }, W.error, h('div', { class: 'a-actions' }, h('button', { class: 'btn btn-sm', onClick: () => { W.error = ''; loadRt(); } }, t('common.retry'))))));
      else if (!W.rt) body.append(h('div', { class: 'empty' }, t('common.loading')));
      else {
        const opts = new Map(); W.rt.installed.forEach((i) => opts.set(i.version, { v: i.version, inst: true, sizeMB: i.sizeMB })); W.rt.available.forEach((a) => { if (!opts.has(a.version)) opts.set(a.version, { v: a.version, sizeMB: a.sizeMB, channel: a.channel }); });
        const sel = h('select', { class: 'select', id: 'wiz-ver', onChange: (e) => { W.version = e.target.value; render(true); } },
          [...opts.values()].map((o) => h('option', { value: o.v, selected: o.v === W.version }, `${o.v}${o.v === W.rt.latest?.version ? ' · ' + t('wiz.recommended') : ''}${o.inst ? ' · ' + t('rt.installedbadge') : ''}`)));
        const cur = opts.get(W.version);
        body.append(h('div', { class: 'field' }, h('label', { for: 'wiz-ver' }, `${edRuntimeLabel(W.edition)} ${t('apps.runtime').toLowerCase()}`), sel,
          h('p', { class: 'help' }, cur?.inst ? t('wiz.ver.have') : t('wiz.ver.download', { mb: fmtSize(cur?.sizeMB) }))));
        if (W.edition === 'php') body.append(h('div', { class: 'field' }, h('span', { class: 'label' }, t('rt.variant')), h('div', { class: 'seg', role: 'group', 'aria-label': t('rt.variant'), style: { alignSelf: 'flex-start' } },
          ['nts', 'ts'].map((v) => h('button', { type: 'button', 'aria-pressed': String(W.variant === v), onClick: () => { W.variant = v; W.version = ''; loadRt(); } }, v.toUpperCase()))), h('p', { class: 'help' }, t('rt.nts.help'))));
      }
      buttons(true, () => { W.step = 2; if (W.port == null) W.port = cfg().apps?.[W.edition]?.port ?? (W.edition === 'php' ? 8080 : 3000); W.lan = cfg().bind === '0.0.0.0'; render(); checkPort(); }, null, !W.version);
    } else if (W.step === 2) {
      if (W.port == null) W.port = cfg().apps?.[W.edition]?.port;
      body.append(...heading('network'),
        h('div', { class: 'field' }, h('label', { for: 'wiz-port' }, t('apps.port')), h('input', { class: 'input', id: 'wiz-port', type: 'number', min: 1, max: 65535, value: W.port, 'aria-invalid': String(!W.portOk), 'aria-describedby': 'wiz-port-h', style: { maxWidth: '160px' }, onInput: (e) => { W.port = e.target.value; checkPort(); } }),
          h('p', { class: 'help ' + (W.portMsg ? (W.portOk ? 'ok' : 'err') : ''), id: 'wiz-port-h', 'aria-live': 'polite' }, W.portMsg || t('apps.port.help'))),
        h('div', { class: 'field' }, h('span', { class: 'label' }, t('apps.bind')),
          choice('bind', 'local', !W.lan, t('apps.bind.local'), t('wiz.net.local'), t('wiz.recommended'), () => { W.lan = false; render(); }),
          choice('bind', 'lan', W.lan, t('apps.bind.lan'), t('wiz.net.lan'), null, () => { W.lan = true; render(); })),
        W.lan ? h('div', { class: 'alert warn' }, icon('alert', 16), h('div', { class: 'a-body' }, t('lan.body'))) : '');
      buttons(true, () => { W.step = 3; render(); runInstall(); }, t('wiz.install'), !W.portOk || !W.port);
    } else if (W.step === 3) {
      body.append(...heading('install'));
      if (W.error) body.append(h('div', { class: 'alert err', role: 'alert' }, icon('x-circle', 16), h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('wiz.install.fail')), W.error)));
      else body.append(h('div', { class: 'prog-wrap', role: 'status' }, h('div', { class: 'progress', 'data-prog': '1', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': '0', 'aria-label': t('wiz.s.install') }, h('i', { style: { width: '0%' } })),
        h('div', { class: 'prog-meta' }, h('span', { 'data-step': '1' }, '…'), h('span', { 'data-pct': '1' }, '0%'))));
      if (W.error) buttons(true, () => runInstall(), t('common.retry')); else m.setButtons([]);
      if (!W.error) renderProgress();
    } else {
      body.append(h('div', { class: 'alert info' }, icon('check-circle', 16), h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('wiz.done.title')), t('wiz.done.body', { ed: edLabel(W.edition), v: W.version, port: W.port }))));
      m.setButtons([
        { label: t('wiz.gotodash'), value: true },
        { label: t('wiz.openapp'), kind: 'primary', keepOpen: true, onClick: async (close) => {
          try {
            await post('/app/start'); await refreshStatus(); close(true);
            const url = reachableUrl(S.status.app?.url || `http://127.0.0.1:${W.port}`);
            if (isLoopbackPage()) await post('/open', { what: 'app:' + W.edition }).catch(() => {}); else window.open(url, '_blank', 'noopener');
          } catch (e) { toast(errText(e), 'err'); }
        } },
      ]);
    }
    if (focusId) { const el = document.getElementById(focusId); if (el) { el.focus(); try { el.setSelectionRange(pos, pos); } catch { /* number input */ } } }
    else wrap.querySelector('input:checked, select, input')?.focus({ preventScroll: true });
  }

  render();
  return m.result;
}

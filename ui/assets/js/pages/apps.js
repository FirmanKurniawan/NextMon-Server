// Aplikasi: per-edition settings (tabs), network exposure (bind), env / opcache / php.ini editors.
import { get, post } from '../api.js';
import { h, clear, toast, errText, debounce, busy, confirmBox } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S, EDITIONS, kindOf, cfg, refreshStatus } from '../store.js';
import { edLabel, edRuntimeLabel } from '../edition.js';

/** Key/value table editor. Returns {el, get(), validate()}. */
function kvEditor({ initial, keyRe, keyLabel, valueLabel, addLabel, onChange, keyPlaceholder = 'KEY', valuePlaceholder = 'value' }) {
  const list = h('div', { class: 'kvlist' });
  const rows = [];
  const err = h('p', { class: 'help err', role: 'alert', hidden: true });
  function addRow(k = '', v = '', focus = false) {
    const ki = h('input', { class: 'input input-sm', value: k, placeholder: keyPlaceholder, 'aria-label': keyLabel, spellcheck: 'false', autocomplete: 'off', onInput: () => { validate(); onChange?.(); } });
    const vi = h('input', { class: 'input input-sm', value: String(v), placeholder: valuePlaceholder, 'aria-label': valueLabel, spellcheck: 'false', autocomplete: 'off', onInput: () => onChange?.() });
    const rm = h('button', { class: 'btn btn-sm btn-ghost btn-icon', type: 'button', 'aria-label': t('common.remove'), onClick: () => { row.remove(); rows.splice(rows.indexOf(rec), 1); validate(); onChange?.(); } }, icon('trash', 14));
    const row = h('div', { class: 'kvrow' }, ki, vi, rm); const rec = { ki, vi, row };
    rows.push(rec); list.append(row); if (focus) ki.focus();
  }
  function validate() {
    const seen = new Set(); let bad = '';
    rows.forEach(({ ki }) => {
      const k = ki.value.trim(); let inv = false;
      if (k && !keyRe.test(k)) { inv = true; bad = bad || t('kv.badkey', { k }); }
      else if (k && seen.has(k)) { inv = true; bad = bad || t('kv.dup', { k }); }
      seen.add(k); ki.setAttribute('aria-invalid', String(inv));
    });
    err.hidden = !bad; err.textContent = bad; return !bad;
  }
  Object.entries(initial || {}).forEach(([k, v]) => addRow(k, v));
  const el = h('div', { class: 'stack-sm' }, list, err, h('div', null, h('button', { class: 'btn btn-sm', type: 'button', onClick: () => addRow('', '', true) }, icon('plus', 14), addLabel)));
  return { el, validate, get() { const o = {}; rows.forEach(({ ki, vi }) => { const k = ki.value.trim(); if (k) o[k] = vi.value; }); return o; } };
}

export function mount(root, params) {
  let ed = EDITIONS.includes(params[0]) ? params[0] : (S.status?.edition || 'php');
  let alive = true;
  const forms = {};
  const tabs = h('div', { class: 'tabs', role: 'tablist', 'aria-label': t('apps.title') });
  const panels = h('div');
  const net = h('div');
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('apps.title')), h('p', { class: 'page-desc' }, t('apps.desc')))), tabs, panels, net);

  function renderTabs() {
    clear(tabs);
    EDITIONS.forEach((e) => tabs.append(h('button', { role: 'tab', id: 'atab-' + e, 'aria-selected': String(e === ed), tabindex: e === ed ? '0' : '-1', 'aria-controls': 'apanel-' + e,
      onClick: () => { ed = e; history.replaceState(null, '', '#/apps/' + e); renderTabs(); showPanel(); },
      onKeydown: (ev) => { if (ev.key === 'ArrowRight' || ev.key === 'ArrowLeft') { ed = EDITIONS[(EDITIONS.indexOf(ed) + 1) % 2]; history.replaceState(null, '', '#/apps/' + ed); renderTabs(); showPanel(); document.getElementById('atab-' + ed).focus(); } } },
      edLabel(e), S.status?.edition === e && h('span', { class: 'badge badge-ok' }, t('apps.active')))));
  }
  function showPanel() { EDITIONS.forEach((e) => { if (forms[e]) forms[e].hidden = e !== ed; }); }

  async function buildForm(e) {
    const kind = kindOf(e);
    const c = cfg().apps?.[e] || {};
    const isActive = () => S.status?.edition === e;
    let rt = null; try { rt = await get(`/runtimes/${kind}`); } catch { /* shown as empty */ }
    const installed = rt?.installed || [];
    const init = { port: c.port ?? '', runtime: c.runtime || '', opcache: c.opcache !== false, env: c.env || {}, extraIni: c.extraIni || {} };

    const dirtyBadge = h('span', { class: 'badge badge-warn', hidden: true }, t('apps.unsaved'));
    const port = h('input', { class: 'input', type: 'number', min: 1, max: 65535, inputmode: 'numeric', id: 'port-' + e, value: init.port, 'aria-describedby': 'port-h-' + e, onInput: () => { onChange(); checkPort(); } });
    const portHelp = h('p', { class: 'help', id: 'port-h-' + e, 'aria-live': 'polite' }, t('apps.port.help'));
    let portOk = true;
    const checkPort = debounce(async () => {
      const n = Number(port.value);
      portHelp.className = 'help';
      if (!Number.isInteger(n) || n < 1 || n > 65535) { portOk = false; port.setAttribute('aria-invalid', 'true'); portHelp.className = 'help err'; portHelp.textContent = t('apps.port.range'); return; }
      portOk = true; port.setAttribute('aria-invalid', 'false');
      if (n === Number(init.port) && isActive() && S.status.app?.state === 'running') { portHelp.className = 'help ok'; portHelp.textContent = t('apps.port.own'); return; }
      try {
        const r = await get(`/ports/check?port=${n}&edition=${e}`);
        if (r.free) { portHelp.className = 'help ok'; portHelp.textContent = t('apps.port.free'); }
        else { portOk = false; port.setAttribute('aria-invalid', 'true'); portHelp.className = 'help err'; portHelp.textContent = t('apps.port.busy', { by: r.usedBy || '?' }); }
      } catch { portHelp.textContent = t('apps.port.unknown'); }
    }, 350);

    const sel = h('select', { class: 'select', id: 'rt-' + e, 'aria-describedby': 'rt-h-' + e, onChange: () => onChange() },
      installed.length ? installed.map((i) => h('option', { value: i.version, selected: i.version === init.runtime }, i.version + (rt.latest?.version === i.version ? ` (${t('apps.latest')})` : '')))
        : h('option', { value: '' }, t('apps.rt.none')));
    if (init.runtime && !installed.some((i) => i.version === init.runtime)) sel.prepend(h('option', { value: init.runtime, selected: true }, init.runtime + ' (' + t('apps.rt.missing') + ')'));
    const rtHelp = h('p', { class: 'help', id: 'rt-h-' + e }, installed.length ? t('apps.rt.help', { rt: edRuntimeLabel(e) }) : [t('apps.rt.nohelp', { rt: edRuntimeLabel(e) }), ' ', h('a', { href: '#/runtimes/' + kind }, t('nav.runtimes'))]);

    const env = kvEditor({ initial: init.env, keyRe: /^[A-Za-z_][A-Za-z0-9_]*$/, keyLabel: t('apps.env.key'), valueLabel: t('apps.env.value'), addLabel: t('apps.env.add'), onChange: () => onChange() });
    let opc, ini;
    if (e === 'php') {
      opc = h('input', { type: 'checkbox', checked: init.opcache, onChange: () => onChange() });
      ini = kvEditor({ initial: init.extraIni, keyRe: /^[A-Za-z0-9_.]+$/, keyLabel: t('apps.ini.key'), valueLabel: t('apps.ini.value'), addLabel: t('apps.ini.add'), keyPlaceholder: 'memory_limit', valuePlaceholder: '256M', onChange: () => onChange() });
    }

    const collect = () => ({ port: Number(port.value), runtime: sel.value, env: env.get(), ...(e === 'php' ? { opcache: opc.checked, extraIni: ini.get() } : {}) });
    const baseline = JSON.stringify({ port: Number(init.port), runtime: init.runtime, env: init.env, ...(e === 'php' ? { opcache: init.opcache, extraIni: init.extraIni } : {}) });
    const save = h('button', { class: 'btn btn-primary', type: 'button', disabled: true, onClick: (ev) => doSave(false, ev.currentTarget) }, icon('check', 14), t('apps.save'));
    const saveRestart = h('button', { class: 'btn', type: 'button', disabled: true, onClick: (ev) => doSave(true, ev.currentTarget) }, icon('restart', 14), t('apps.saverestart'));
    const reset = h('button', { class: 'btn btn-ghost', type: 'button', disabled: true, onClick: () => rebuild() }, t('common.reset'));
    function onChange() {
      const d = JSON.stringify(collect()) !== baseline;
      dirtyBadge.hidden = !d; save.disabled = !d; saveRestart.disabled = !d; reset.disabled = !d;
    }
    async function rebuild() { const nf = await buildForm(e); forms[e].replaceWith(nf); forms[e] = nf; nf.hidden = e !== ed; }
    async function doSave(restart, btn) {
      const okKv = env.validate() & (ini ? ini.validate() : true);
      if (!okKv || !portOk) { toast(t('apps.fixerrors'), 'warn'); return; }
      await busy(btn, async () => {
        try {
          await post('/app/config', { edition: e, ...collect() });
          if (restart && isActive()) await post('/app/restart');
          await refreshStatus();
          toast(t(restart && isActive() ? 'apps.saved.restarted' : 'apps.saved'), 'ok');
          await rebuild();
        } catch (er) {
          if (er.code === 'port_in_use') { portHelp.className = 'help err'; portHelp.textContent = er.message; port.setAttribute('aria-invalid', 'true'); port.focus(); }
          else if (er.code === 'runtime_not_installed') { rtHelp.className = 'help err'; rtHelp.textContent = er.message; sel.focus(); }
          toast(errText(er), 'err');
        }
      });
    }

    const phpOnly = e === 'php' ? [
      h('div', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('apps.php.title'))), h('div', { class: 'card-b stack' },
        h('div', { class: 'setting', style: { padding: 0, border: 0 } }, h('div', { class: 's-text' }, h('div', { class: 's-title' }, 'OPcache'), h('div', { class: 's-desc' }, t('apps.opcache.help'))),
          h('label', { class: 'switch' }, opc, h('span', { class: 'track' }), h('span', { class: 'sr-only' }, 'OPcache'))),
        h('div', { class: 'field' }, h('span', { class: 'label' }, t('apps.ini.title')), h('p', { class: 'help' }, t('apps.ini.help')), ini.el))),
    ] : [];

    return h('div', { class: 'stack', role: 'tabpanel', id: 'apanel-' + e, 'aria-labelledby': 'atab-' + e, hidden: e !== ed },
      !isActive() && h('div', { class: 'alert info' }, icon('info', 16), h('div', { class: 'a-body' }, t('apps.inactive'))),
      h('div', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('apps.general', { app: 'NextMON ' + edLabel(e) })), dirtyBadge),
        h('div', { class: 'card-b' }, h('div', { class: 'form-grid' },
          h('div', { class: 'field' }, h('label', { for: 'port-' + e }, t('apps.port')), port, portHelp),
          h('div', { class: 'field' }, h('label', { for: 'rt-' + e }, t('apps.runtime')), sel, rtHelp)))),
      h('div', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('apps.env.title'))), h('div', { class: 'card-b stack-sm' }, h('p', { class: 'help' }, t('apps.env.help')), env.el)),
      ...phpOnly,
      h('div', { class: 'row' }, save, isActive() && saveRestart, reset));
  }

  function buildNet() {
    clear(net);
    const bind = cfg().bind || '127.0.0.1'; let val = bind;
    const sel = h('select', { class: 'select', id: 'bind', 'aria-describedby': 'bind-h', onChange: () => { val = sel.value; warn.hidden = val !== '0.0.0.0'; btn.disabled = val === bind; } },
      h('option', { value: '127.0.0.1', selected: bind === '127.0.0.1' }, t('apps.bind.local')), h('option', { value: '0.0.0.0', selected: bind === '0.0.0.0' }, t('apps.bind.lan')));
    const warn = h('div', { class: 'alert warn', hidden: bind !== '0.0.0.0' }, icon('alert', 16), h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('lan.title')), t('lan.body')));
    const btn = h('button', { class: 'btn btn-primary', type: 'button', disabled: true, onClick: (ev) => busy(ev.currentTarget, async () => {
      if (val === '0.0.0.0' && !(await confirmBox({ title: t('apps.bind.confirm.title'), message: t('apps.bind.confirm.body'), confirmLabel: t('apps.bind.confirm.go'), tone: 'warn', iconName: 'alert' }))) return;
      try { await post('/config', { bind: val }); if (S.status?.app?.state === 'running') await post('/app/restart'); await refreshStatus(); toast(t('apps.saved'), 'ok'); buildNet(); } catch (er) { toast(errText(er), 'err'); }
    }) }, icon('check', 14), t('apps.save'));
    net.append(h('div', { class: 'card glass', style: { marginTop: '16px' } }, h('div', { class: 'card-h' }, h('h2', null, icon('globe', 16), t('apps.net.title'))),
      h('div', { class: 'card-b stack' }, h('div', { class: 'field', style: { maxWidth: '420px' } }, h('label', { for: 'bind' }, t('apps.bind')), sel, h('p', { class: 'help', id: 'bind-h' }, t('apps.bind.help'))), warn, h('div', null, btn))));
  }

  renderTabs();
  panels.append(h('div', { class: 'empty' }, t('common.loading')));
  (async () => {
    for (const e of EDITIONS) { const f = await buildForm(e); if (!alive) return; forms[e] = f; }
    if (!alive) return;
    clear(panels); EDITIONS.forEach((e) => panels.append(forms[e])); showPanel(); buildNet();
  })();
  return () => { alive = false; };
}

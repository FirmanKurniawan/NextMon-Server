// Dashboard: edition switcher, one large card for the active edition, one small card for the inactive one.
import { get, post, sse } from '../api.js';
import { h, clear, toast, errText, fmtUptime, reachableUrl, isLoopbackPage, busy, skel } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S, subscribe, refreshStatus, EDITIONS, cfg } from '../store.js';
import { switchEdition, edLabel, edRuntimeLabel } from '../edition.js';

const STATE_KEYS = ['running', 'stopped', 'starting', 'stopping', 'crashed', 'failed'];

export function mount(root) {
  let unsub, streams = [], renderedFor = null, refs = null, tail = [];
  const lanSlot = h('div');
  const body = h('div', { class: 'stack' });
  root.append(
    h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('dash.title')), h('p', { class: 'page-desc' }, t('dash.desc')))),
    lanSlot, body);

  const stop = () => { streams.forEach((s) => s.close()); streams = []; };

  function build(st) {
    stop(); tail = [];
    const ed = st.edition, other = EDITIONS.find((e) => e !== ed);
    clear(body);

    /* edition selector */
    const seg = h('div', { class: 'seg', role: 'group', 'aria-label': t('dash.edition') },
      EDITIONS.map((e) => h('button', { type: 'button', 'aria-pressed': String(e === ed), onClick: () => { if (e !== ed) switchEdition(e); } }, edLabel(e))));
    body.append(h('div', { class: 'row', style: { justifyContent: 'space-between' } },
      h('div', { class: 'row' }, h('span', { class: 'label' }, t('dash.edition')), seg),
      h('span', { class: 'help' }, t('dash.oneAtATime'))));

    /* active card */
    const r = {};
    const pill = h('span', { class: 'pill' });
    const btn = (key, ic, kind, onClick) => (r[key] = h('button', { class: 'btn ' + kind, type: 'button', onClick: (e) => onClick(e.currentTarget) }, icon(ic, 14), t('dash.' + key)));
    const act = (path, okKey) => (b) => busy(b, async () => {
      try { await post(path); await refreshStatus(); toast(t(okKey), 'ok'); } catch (e) { toast(errText(e), 'err'); refreshStatus().catch(() => {}); }
    });
    r.pill = pill; r.sub = h('div', { class: 'sub' }); r.port = h('div', { class: 'v' }); r.up = h('div', { class: 'v' }); r.ram = h('div', { class: 'v' }); r.rt = h('div', { class: 'v' });
    r.url = h('a', { target: '_blank', rel: 'noopener', class: 'mono' }); r.urlNone = h('span', { class: 'none' }, t('dash.nourl'));
    r.demo = h('input', { type: 'checkbox', onChange: onDemo });
    r.demoHelp = h('div', { class: 's-desc' }, t('dash.demo.help'));
    r.tail = h('div', { class: 'tail', role: 'log', 'aria-label': t('dash.tail'), tabindex: '0' });

    const stat = (k, el) => h('div', { class: 'stat' }, h('div', { class: 'k' }, t(k)), el);
    body.append(h('section', { class: 'card glass app-card', 'aria-labelledby': 'act-h' },
      h('div', { class: 'head' }, h('div', { class: 'app-tile', 'aria-hidden': 'true' }, ed === 'php' ? 'PHP' : 'N'),
        h('div', { style: { flex: 1, minWidth: 0 } }, h('h2', { id: 'act-h' }, 'NextMON ' + edLabel(ed)), r.sub), pill),
      h('div', { class: 'stats' }, stat('dash.port', r.port), stat('dash.uptime', r.up), stat('dash.ram', r.ram), stat('dash.runtime', r.rt)),
      h('div', { class: 'url-row' }, icon('link', 14), r.url, r.urlNone),
      h('div', { class: 'actions' },
        btn('start', 'play', 'btn-primary', act('/app/start', 'dash.started')),
        btn('stop', 'stop', '', act('/app/stop', 'dash.stopped')),
        btn('restart', 'restart', '', act('/app/restart', 'dash.restarted')),
        btn('open', 'external', '', openApp)),
      h('div', { class: 'demo-row' }, h('div', { class: 's-text' }, h('div', { class: 's-title' }, t('dash.demo')), r.demoHelp),
        h('label', { class: 'switch' }, r.demo, h('span', { class: 'track' }), h('span', { class: 'sr-only' }, t('dash.demo')))),
      h('div', { class: 'tail-h' }, h('span', null, t('dash.tail')), h('a', { href: '#/logs/' + ed }, t('dash.alllogs'))),
      r.tail));

    /* inactive card */
    const oi = st.editions?.[other] || {};
    r.oRt = h('span'); r.oPort = h('span', { class: 'mono' });
    body.append(h('section', { class: 'card glass', 'aria-labelledby': 'ina-h' },
      h('div', { class: 'head', style: { display: 'flex', alignItems: 'center', gap: '12px', padding: '16px', flexWrap: 'wrap' } },
        h('div', { class: 'app-tile', style: { opacity: .7 }, 'aria-hidden': 'true' }, other === 'php' ? 'PHP' : 'N'),
        h('div', { style: { flex: '1 1 180px', minWidth: 0 } },
          h('h2', { id: 'ina-h', style: { fontSize: '14px' } }, 'NextMON ' + edLabel(other), ' ', h('span', { class: 'badge' }, t('dash.inactive'))),
          h('div', { class: 'help', style: { marginTop: '4px' } }, t('dash.port'), ': ', r.oPort, ' · ', r.oRt)),
        h('div', { class: 'row' },
          h('a', { class: 'btn btn-ghost', href: '#/apps/' + other }, icon('sliders', 14), t('nav.apps')),
          h('button', { class: 'btn', type: 'button', onClick: () => switchEdition(other) }, icon('swap', 14), t('dash.switch'))))));
    Object.assign(refs = r, { other });

    /* log tail */
    const push = (line) => { tail.push(line); if (tail.length > 6) tail.shift(); paintTail(); };
    get(`/logs/${ed}?lines=6`).then((res) => { const ls = Array.isArray(res) ? res : (res.lines || String(res).split('\n')); tail = ls.filter(Boolean).slice(-6); paintTail(); }).catch(() => paintTail());
    streams.push(sse(`/logs/${ed}/stream`, (d) => { if (d) push(d); }));
    renderedFor = ed;
  }

  function paintTail() {
    if (!refs) return;
    clear(refs.tail);
    if (!tail.length) refs.tail.append(h('div', { class: 'tn' }, t('dash.notail')));
    else tail.forEach((l) => refs.tail.append(h('div', { title: l }, l)));
    refs.tail.scrollTop = refs.tail.scrollHeight;
  }

  async function onDemo(e) {
    const on = e.target.checked; const ed = S.status.edition;
    const env = { ...(cfg().apps?.[ed]?.env || {}), NEXTMON_DEMO: on ? '1' : '0' };
    try {
      await post('/app/config', { edition: ed, env });
      if (S.status.app?.state === 'running') await post('/app/restart');
      await refreshStatus(); toast(t(on ? 'dash.demo.on' : 'dash.demo.off'), 'ok');
    } catch (err) { e.target.checked = !on; toast(errText(err), 'err'); }
  }

  async function openApp(b) {
    const a = S.status.app;
    if (!isLoopbackPage()) { window.open(reachableUrl(a.url), '_blank', 'noopener'); return; }
    await busy(b, async () => { try { await post('/open', { what: 'app:' + S.status.edition }); } catch (e) { toast(errText(e), 'err'); } });
  }

  function update(st) {
    if (!refs || renderedFor !== st.edition) return;
    const a = st.app || {}; const r = refs; const state = a.state || 'stopped';
    r.pill.className = 'pill ' + state; r.pill.textContent = STATE_KEYS.includes(state) ? t('state.' + state) : state;
    r.sub.textContent = a.runtime ? `${edRuntimeLabel(st.edition)} ${a.runtime}` : t('dash.noruntime');
    r.port.textContent = a.port ?? '—';
    r.up.textContent = state === 'running' ? fmtUptime(a.uptime) : '—';
    r.ram.textContent = state === 'running' && a.ramMB != null ? Math.round(a.ramMB) + ' MB' : '—';
    r.rt.replaceChildren(a.runtime ? h('span', { class: 'chip' }, edRuntimeLabel(st.edition) + ' ' + a.runtime) : '—');
    const url = a.url && state === 'running' ? reachableUrl(a.url) : '';
    r.url.hidden = !url; r.urlNone.hidden = !!url; r.url.textContent = url; r.url.href = url || '#';
    r.urlNone.textContent = state === 'running' ? t('dash.nourl') : t('dash.stoppedurl', { url: a.url ? reachableUrl(a.url) : `:${a.port ?? ''}` });
    const idle = state === 'stopped' || state === 'crashed' || state === 'failed';
    r.start.disabled = !idle; r.stop.disabled = state !== 'running' && state !== 'starting'; r.restart.disabled = state !== 'running'; r.open.disabled = state !== 'running';
    const c = cfg(); const demo = c.apps?.[st.edition]?.env?.NEXTMON_DEMO === '1';
    if (document.activeElement !== r.demo) r.demo.checked = demo;
    const oi = st.editions?.[r.other] || {};
    r.oPort.textContent = oi.port ?? '—';
    r.oRt.replaceChildren(oi.installed === false ? h('span', { class: 'badge badge-warn' }, t('dash.rtmissing', { rt: edRuntimeLabel(r.other) })) : `${edRuntimeLabel(r.other)} ${oi.runtime || ''}`);
    clear(lanSlot);
    if (c.bind === '0.0.0.0') lanSlot.append(h('div', { class: 'alert warn', role: 'note', style: { marginBottom: '16px' } }, icon('alert', 16),
      h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('lan.title')), t('lan.body'), h('div', { class: 'a-actions' }, h('a', { class: 'btn btn-sm', href: '#/apps/' + st.edition }, t('lan.change'))))));
    if (a.restarts > 0 && state === 'crashed') { /* surfaced through the pill + restarts count in subtitle */ }
  }

  function render() {
    const st = S.status;
    if (!st) { if (!body.firstChild) body.append(skel('100%', 280), skel('100%', 72)); return; }
    if (renderedFor !== st.edition) build(st);
    update(st);
  }
  render();
  unsub = subscribe(render);
  return () => { unsub(); stop(); };
}

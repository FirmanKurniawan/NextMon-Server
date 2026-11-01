// Pengaturan: language, theme, startup behaviour, token, data folder, log retention. Changes save immediately.
import { post, getToken, setToken } from '../api.js';
import { h, toast, errText, busy, confirmBox } from '../dom.js';
import { icon } from '../icons.js';
import { t, lang } from '../i18n.js';
import { S, cfg, refreshStatus } from '../store.js';
import { applyTheme, applyLang, getTheme } from '../prefs.js';

export function mount(root) {
  const c = cfg();
  const save = async (patch, quiet) => {
    try { await post('/config', patch); await refreshStatus(); if (!quiet) toast(t('common.saved'), 'ok', { timeout: 1800 }); return true; }
    catch (e) { toast(errText(e), 'err'); return false; }
  };
  const sw = (key, title, desc) => {
    const inp = h('input', { type: 'checkbox', checked: !!c[key], id: 'sw-' + key, 'aria-labelledby': 'st-' + key, 'aria-describedby': 'sd-' + key, onChange: async (e) => { if (!(await save({ [key]: e.target.checked }))) e.target.checked = !e.target.checked; } });
    return h('div', { class: 'setting' }, h('div', { class: 's-text' }, h('div', { class: 's-title', id: 'st-' + key }, title), h('div', { class: 's-desc', id: 'sd-' + key }, desc)),
      h('label', { class: 'switch' }, inp, h('span', { class: 'track' })));
  };

  /* language + theme */
  const langSeg = h('div', { class: 'seg', role: 'group', 'aria-label': t('settings.language') }, ['id', 'en'].map((l) => h('button', { type: 'button', lang: l, 'aria-pressed': String(lang() === l),
    onClick: () => { applyLang(l); post('/config', { language: l }).catch(() => {}); } }, l === 'id' ? 'Bahasa Indonesia' : 'English')));
  const themeSeg = h('div', { class: 'seg', role: 'group', 'aria-label': t('theme.label') }, ['auto', 'light', 'dark'].map((m) => h('button', { type: 'button', 'aria-pressed': String(getTheme() === m),
    onClick: (e) => { applyTheme(m); themeSeg.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b === e.currentTarget))); post('/config', { theme: m }).catch(() => {}); } }, t('theme.' + m))));

  /* token */
  const tokenIn = h('input', { class: 'input mono', type: 'password', readOnly: true, value: getToken() || c.controlToken || '', id: 'tok', 'aria-label': t('settings.token'), 'aria-describedby': 'tok-h' });
  const eye = h('button', { class: 'btn btn-icon', type: 'button', 'aria-label': t('settings.token.show'), 'aria-pressed': 'false', onClick: () => {
    const show = tokenIn.type === 'password'; tokenIn.type = show ? 'text' : 'password'; eye.setAttribute('aria-pressed', String(show)); eye.replaceChildren(icon(show ? 'eye-off' : 'eye', 16));
    eye.setAttribute('aria-label', t(show ? 'settings.token.hide' : 'settings.token.show')); } }, icon('eye', 16));
  const copyTok = h('button', { class: 'btn btn-icon', type: 'button', 'aria-label': t('settings.token.copy'), onClick: async () => { try { await navigator.clipboard.writeText(tokenIn.value); toast(t('common.copied'), 'ok', { timeout: 1800 }); } catch { tokenIn.type = 'text'; tokenIn.select(); } } }, icon('copy', 16));
  const regen = h('button', { class: 'btn btn-danger', type: 'button', onClick: (e) => busy(e.currentTarget, async () => {
    if (!(await confirmBox({ title: t('settings.token.regen.title'), message: t('settings.token.regen.body'), confirmLabel: t('settings.token.regen'), danger: true, iconName: 'key' }))) return;
    try { const r = await post('/config/token/regenerate'); setToken(r.token); tokenIn.value = r.token; toast(t('settings.token.regen.ok'), 'ok'); } catch (er) { toast(errText(er), 'err'); }
  }) }, icon('key', 14), t('settings.token.regen'));

  /* retention */
  const ret = h('input', { class: 'input', type: 'number', min: 1, max: 365, value: c.logRetentionDays ?? 14, id: 'ret', style: { width: '96px' }, 'aria-describedby': 'ret-h',
    onChange: async (e) => { const n = Number(e.target.value); if (!Number.isInteger(n) || n < 1 || n > 365) { e.target.setAttribute('aria-invalid', 'true'); toast(t('settings.ret.bad'), 'warn'); return; } e.target.setAttribute('aria-invalid', 'false'); save({ logRetentionDays: n }); } });

  const dataPath = S.status?.paths?.data || c.dataDir || 'data';
  const openBtn = (what, key) => h('button', { class: 'btn btn-sm', type: 'button', onClick: (e) => busy(e.currentTarget, async () => { try { await post('/open', { what }); } catch (er) { toast(errText(er), 'err'); } }) }, icon('folder', 14), t(key));

  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('settings.title')), h('p', { class: 'page-desc' }, t('settings.desc')))),
    h('div', { class: 'grid-2', style: { alignItems: 'start' } },
      h('div', { class: 'stack' },
        h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('settings.appearance'))), h('div', { class: 'card-b' },
          h('div', { class: 'setting' }, h('div', { class: 's-text' }, h('div', { class: 's-title' }, t('settings.language')), h('div', { class: 's-desc' }, t('settings.language.help'))), langSeg),
          h('div', { class: 'setting' }, h('div', { class: 's-text' }, h('div', { class: 's-title' }, t('theme.label')), h('div', { class: 's-desc' }, t('settings.theme.help'))), themeSeg))),
        h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('settings.startup'))), h('div', { class: 'card-b' },
          sw('autoStartWithWindows', t('settings.autostart'), t('settings.autostart.help')),
          sw('startMinimized', t('settings.minimized'), t('settings.minimized.help')),
          sw('autoStartEdition', t('settings.autoedition'), t('settings.autoedition.help')),
          sw('openBrowserOnStart', t('settings.openbrowser'), t('settings.openbrowser.help')),
          sw('restartOnCrash', t('settings.restartcrash'), t('settings.restartcrash.help'))))),
      h('div', { class: 'stack' },
        h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('key', 16), t('settings.security'))), h('div', { class: 'card-b stack-sm' },
          h('div', { class: 'field' }, h('label', { for: 'tok' }, t('settings.token')), h('div', { class: 'row', style: { flexWrap: 'nowrap' } }, tokenIn, eye, copyTok), h('p', { class: 'help', id: 'tok-h' }, t('settings.token.help'))),
          h('div', null, regen))),
        h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, icon('folder', 16), t('settings.storage'))), h('div', { class: 'card-b stack' },
          h('div', { class: 'field' }, h('span', { class: 'label' }, t('settings.datadir')), h('code', { class: 'mono', style: { overflowWrap: 'anywhere' } }, dataPath), h('p', { class: 'help' }, t('settings.datadir.help')),
            h('div', { class: 'row' }, openBtn('folder:data', 'settings.open.data'), openBtn('folder:logs', 'settings.open.logs'))),
          h('div', { class: 'field' }, h('label', { for: 'ret' }, t('settings.retention')), h('div', { class: 'row' }, ret, h('span', { class: 'help' }, t('settings.days'))), h('p', { class: 'help', id: 'ret-h' }, t('settings.retention.help'))))))));
}

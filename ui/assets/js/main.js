// App shell: hash router, navigation, global banners, auth + onboarding.
import { initToken, on as onApi, setToken, getToken } from './api.js';
import { t, onLang, lang } from './i18n.js';
import { h, clear, modal, toast } from './dom.js';
import { icon } from './icons.js';
import { S, subscribe, startPolling, refreshStatus, cfg } from './store.js';
import { applyTheme, getTheme, applyLang, hasLangPref, hasThemePref } from './prefs.js';

initToken();
applyTheme();

const ROUTES = [
  { id: 'dashboard', icon: 'dashboard', mod: './pages/dashboard.js' },
  { id: 'runtimes', icon: 'layers', mod: './pages/runtimes.js' },
  { id: 'apps', icon: 'sliders', mod: './pages/apps.js' },
  { id: 'logs', icon: 'terminal', mod: './pages/logs.js' },
  { id: 'backup', icon: 'archive', mod: './pages/backup.js' },
  { id: 'updates', icon: 'download', mod: './pages/updates.js' },
  { id: 'diagnostics', icon: 'activity', mod: './pages/diagnostics.js' },
  { id: 'settings', icon: 'settings', mod: './pages/settings.js' },
  { id: 'about', icon: 'info', mod: './pages/about.js' },
];

const $main = document.getElementById('main');
const $nav = document.getElementById('nav');
const $banners = document.getElementById('banners');
const $sidebar = document.getElementById('sidebar');
const $scrim = document.getElementById('scrim');
const $menu = document.getElementById('menu-btn');
let cleanup = null, current = null, navSeq = 0;
export const badges = { updates: 0 };

/* ---- nav ---- */
function renderNav() {
  clear($nav);
  for (const r of ROUTES) {
    const a = h('a', { class: 'nav-link', href: '#/' + r.id, dataset: { route: r.id } }, icon(r.icon, 16), h('span', null, t('nav.' + r.id)));
    if (badges[r.id]) a.append(h('span', { class: 'dot', 'aria-label': String(badges[r.id]) }, badges[r.id]));
    $nav.append(a);
  }
  markCurrent();
}
function markCurrent() {
  $nav.querySelectorAll('.nav-link').forEach((a) => { if (a.dataset.route === current) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current'); });
}

/* ---- topbar tools ---- */
function renderTools() {
  const box = document.getElementById('tb-tools'); clear(box);
  const th = getTheme();
  const nextTheme = { auto: 'light', light: 'dark', dark: 'auto' }[th];
  box.append(
    h('button', { class: 'btn btn-ghost btn-sm', type: 'button', 'aria-label': `${t('theme.label')}: ${t('theme.' + th)}`, title: `${t('theme.label')}: ${t('theme.' + th)}`,
      onClick: () => { applyTheme(nextTheme); renderTools(); } }, icon({ auto: 'monitor', light: 'sun', dark: 'moon' }[th], 16), h('span', { class: 'nowrap' }, t('theme.' + th))),
    h('div', { class: 'seg', role: 'group', 'aria-label': t('settings.language') },
      ['id', 'en'].map((l) => h('button', { type: 'button', 'aria-pressed': String(lang() === l), lang: l, onClick: () => applyLang(l) }, l.toUpperCase()))),
  );
  $menu.replaceChildren(icon('menu', 18)); $menu.setAttribute('aria-label', t('nav.menu'));
  document.getElementById('skip').textContent = t('nav.skip');
  $sidebar.setAttribute('aria-label', t('nav.label'));
}

/* ---- mobile drawer ---- */
const mqMobile = window.matchMedia('(max-width: 899px)');
function syncDrawer() {
  const open = $sidebar.classList.contains('open');
  $scrim.classList.toggle('show', open);
  $menu.setAttribute('aria-expanded', String(open));
  if (mqMobile.matches && !open) $sidebar.setAttribute('inert', ''); else $sidebar.removeAttribute('inert');
}
function setDrawer(open) { $sidebar.classList.toggle('open', open); syncDrawer(); if (open) $nav.querySelector('a')?.focus(); else if (mqMobile.matches) $menu.focus(); }
$menu.addEventListener('click', () => setDrawer(!$sidebar.classList.contains('open')));
$scrim.addEventListener('click', () => setDrawer(false));
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && $sidebar.classList.contains('open')) setDrawer(false); });
mqMobile.addEventListener('change', () => { $sidebar.classList.remove('open'); syncDrawer(); });

/* ---- router ---- */
function parseHash() {
  const parts = location.hash.replace(/^#\/?/, '').split('/').filter(Boolean);
  const id = ROUTES.some((r) => r.id === parts[0]) ? parts[0] : 'dashboard';
  return { id, params: parts.slice(1) };
}
async function route(first = false) {
  const { id, params } = parseHash();
  const seq = ++navSeq;
  const r = ROUTES.find((x) => x.id === id);
  cleanup?.(); cleanup = null;
  current = id; markCurrent();
  document.title = `${t('nav.' + id)} - NextMON Server`;
  clear($main);
  try {
    const mod = await import(r.mod);
    if (seq !== navSeq) return;
    const c = await mod.mount($main, params);
    if (seq !== navSeq) { c?.(); return; }
    cleanup = c;
  } catch (e) {
    console.error(e);
    $main.append(h('div', { class: 'alert err', role: 'alert' }, icon('x-circle', 16), h('div', { class: 'a-body' }, t('err.page'), ' ', h('span', { class: 'mono' }, e.message))));
  }
  setDrawer(false);
  if (!first) { $main.focus({ preventScroll: true }); window.scrollTo(0, 0); }
}
window.addEventListener('hashchange', () => route());

/* ---- global banners ---- */
const offlineBanner = h('div', { class: 'alert err', role: 'alert' });
function renderStatus() {
  const el = document.getElementById('tb-status'); const st = S.status; clear(el);
  if (!st) return;
  const s = st.app?.state || 'stopped';
  el.className = 'tb-status pill ' + s;
  el.textContent = (st.edition === 'php' ? 'PHP' : 'Next.js') + ' · ' + t('state.' + s);
}
function renderBanners() {
  renderStatus();
  const has = $banners.contains(offlineBanner);
  if (S.offline && !has) {
    offlineBanner.replaceChildren(icon('x-circle', 16), h('div', { class: 'a-body' }, h('div', { class: 'a-title' }, t('offline.title')), t('offline.body')));
    $banners.prepend(offlineBanner);
  } else if (!S.offline && has) { offlineBanner.remove(); }
  document.getElementById('conn').classList.toggle('off', S.offline);
  document.getElementById('conn').setAttribute('title', S.offline ? t('offline.title') : t('conn.ok'));
}

/* ---- auth ---- */
let authOpen = false;
onApi('auth', () => {
  if (authOpen) return; authOpen = true;
  const input = h('input', { class: 'input mono', type: 'password', autocomplete: 'off', spellcheck: 'false', 'aria-label': t('auth.token'), placeholder: t('auth.token') });
  const m = modal({ title: t('auth.title'), iconName: 'lock', tone: 'warn', dismissible: false,
    body: [h('p', null, t('auth.body')), h('div', { class: 'field' }, h('label', null, t('auth.token')), input)],
    buttons: [{ label: t('auth.save'), kind: 'primary', keepOpen: true, onClick: async (close) => { if (!input.value.trim()) return; setToken(input.value.trim()); try { await refreshStatus(); close(true); authOpen = false; route(true); } catch { toast(t('auth.bad'), 'err'); input.focus(); } } }] });
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') m.foot.querySelector('button').click(); });
  input.focus();
});

/* ---- boot ---- */
function rerenderAll() { renderNav(); renderTools(); renderBanners(); document.getElementById('sb-ver').textContent = S.status ? `v${S.status.version}` : '—'; }
onLang(() => { rerenderAll(); route(true); });

async function checkBadges() {
  try {
    const { get } = await import('./api.js');
    const [u, p, n] = await Promise.all([get('/update/check').catch(() => null), get('/runtimes/php').catch(() => null), get('/runtimes/node').catch(() => null)]);
    badges.updates = (u?.available ? 1 : 0) + (p?.updateAvailable ? 1 : 0) + (n?.updateAvailable ? 1 : 0);
    renderNav();
  } catch { /* ignore */ }
}

(async function boot() {
  syncDrawer(); rerenderAll();
  try { await refreshStatus(); } catch { /* offline/401 handled */ }
  // Adopt server-side language/theme only when the user has not chosen locally yet.
  const c = cfg();
  if (c.language && !hasLangPref()) applyLang(c.language);
  if (c.theme && !hasThemePref()) { applyTheme(c.theme); }
  rerenderAll();
  startPolling();
  subscribe(() => { renderBanners(); document.getElementById('sb-ver').textContent = S.status ? `v${S.status.version}` : '—'; });
  await route(true);
  if (S.status && !c.onboarded && localStorage.getItem('nm.onboarded') !== '1') {
    const { runWizard } = await import('./onboarding.js');
    runWizard().then(() => route(true));
  }
  checkBadges();
})();
export { checkBadges, route };

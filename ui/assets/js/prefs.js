// Theme + language preferences (localStorage; mirrored to config.json by the Settings page).
import { setLang, lang } from './i18n.js';

const store = (k, v) => { try { if (v === undefined) return localStorage.getItem(k); localStorage.setItem(k, v); } catch { /* ignore */ } return null; };
export const getTheme = () => store('nm.theme') || 'auto';
const mq = window.matchMedia('(prefers-color-scheme: dark)');
export function applyTheme(mode = getTheme()) {
  store('nm.theme', mode);
  const dark = mode === 'dark' || (mode === 'auto' && mq.matches);
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light');
}
mq.addEventListener('change', () => { if (getTheme() === 'auto') applyTheme('auto'); });
export function applyLang(l) { store('nm.lang', l); setLang(l); }
export const hasLangPref = () => !!store('nm.lang');
export const hasThemePref = () => !!store('nm.theme');
export { lang };

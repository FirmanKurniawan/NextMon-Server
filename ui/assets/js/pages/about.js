// Tentang: version, links, license, system info.
import { h } from '../dom.js';
import { icon } from '../icons.js';
import { t } from '../i18n.js';
import { S, cfg } from '../store.js';

export function mount(root) {
  const st = S.status || {}; const about = st.about || {}; const sys = st.system || {};
  const repo = cfg().update?.appRepo; const gh = repo && !repo.startsWith('<') ? `https://github.com/${repo}` : null;
  const row = (k, v) => v != null && v !== '' && [h('dt', null, k), h('dd', null, v)];
  root.append(h('div', { class: 'page-head' }, h('div', null, h('h1', { class: 'page-title' }, t('about.title')))),
    h('div', { class: 'grid-2', style: { alignItems: 'start' } },
      h('section', { class: 'card glass' }, h('div', { class: 'card-b stack' },
        h('div', { class: 'about-hero' }, h('img', { src: 'assets/mark-tile.svg', alt: '', width: 64, height: 64 }),
          h('div', null, h('h2', { style: { fontSize: '18px' } }, 'NextMON Server'), h('p', { class: 'muted' }, t('about.tag')), h('p', { class: 'mono', style: { marginTop: '4px' } }, 'v' + (st.version || '—')))),
        h('p', null, t('about.body')),
        h('div', { class: 'row' }, gh && h('a', { class: 'btn', href: gh, target: '_blank', rel: 'noopener' }, icon('external', 14), 'GitHub'),
          gh && h('a', { class: 'btn', href: gh + '/releases', target: '_blank', rel: 'noopener' }, icon('external', 14), t('about.releases')),
          gh && h('a', { class: 'btn', href: gh + '/issues', target: '_blank', rel: 'noopener' }, icon('external', 14), t('about.issues')),
          about.homepage && h('a', { class: 'btn', href: about.homepage, target: '_blank', rel: 'noopener' }, icon('external', 14), t('about.site'))))),
      h('section', { class: 'card glass' }, h('div', { class: 'card-h' }, h('h2', null, t('about.info'))), h('div', { class: 'card-b' },
        h('dl', { class: 'kv' }, row(t('about.version'), 'v' + (st.version || '—')), row(t('about.license'), about.license || '—'), row(t('about.commit'), about.commit && h('span', { class: 'mono' }, about.commit)), row(t('about.built'), about.buildDate),
          row(t('about.os'), [sys.os, sys.arch].filter(Boolean).join(' / ')), row(t('about.ram'), sys.ramMB && Math.round(sys.ramMB / 1024 * 10) / 10 + ' GB'), row(t('about.disk'), sys.diskFreeGB != null && sys.diskFreeGB + ' GB ' + t('about.free')),
          row(t('about.panelport'), cfg().controlPort && h('span', { class: 'mono' }, cfg().controlPort)))))));
}

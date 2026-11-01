// Runs before first paint: apply stored theme and language to avoid a flash.
(function () {
  var theme = 'auto', lang = '';
  try { theme = localStorage.getItem('nm.theme') || 'auto'; lang = localStorage.getItem('nm.lang') || ''; } catch (e) { /* ignore */ }
  var dark = theme === 'dark' || (theme === 'auto' && window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light');
  if (!lang) lang = (navigator.language || 'id').toLowerCase().indexOf('id') === 0 ? 'id' : (navigator.language || '').toLowerCase().indexOf('en') === 0 ? 'en' : 'id';
  document.documentElement.lang = lang;
})();

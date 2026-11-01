# NextMON Server control panel (ui/)

Static HTML, CSS and vanilla JavaScript (ES modules). No CDN, no build step. The Rust server serves this folder (or embeds it) at `/`
and exposes the control API under `/api` (see `docs/SERVER-SPEC.md`).

## Files
```
index.html                    shell (sidebar, topbar, #main); contains <meta name="nm-token" content="{{NM_TOKEN}}">
assets/mark-tile.svg          logo (final brand mark, copied from brand/)
assets/mark-plain.svg
assets/css/panel.css          tokens, glass panels, components, responsive rules (dark/light, reduced motion)
assets/js/boot.js             sync script: applies stored theme + language before first paint
assets/js/main.js             router (hash), nav, topbar, offline/401 banners, onboarding trigger
assets/js/api.js              fetch wrapper (X-Token header), ApiError, fetch-based SSE reader
assets/js/store.js            status polling (2.5 s), job following (SSE + poll fallback), tracked jobs
assets/js/dom.js              h() hyperscript, toast, modal/confirm dialogs, formatters
assets/js/i18n.js             id/en dictionary, t(key, params)
assets/js/prefs.js            theme + language preferences (localStorage)
assets/js/icons.js            inline SVG icon set
assets/js/edition.js          PHP <-> Next.js switch flow (confirm, optional runtime install, job)
assets/js/onboarding.js       first-run wizard
assets/js/pages/*.js          dashboard, runtimes, apps, logs, backup, updates, diagnostics, settings, about
```
Routes: `#/dashboard`, `#/runtimes[/php|node]`, `#/apps[/php|next]`, `#/logs[/server|php|next]`, `#/backup`, `#/updates`, `#/diagnostics`, `#/settings`, `#/about`.

## Token
The server must replace `{{NM_TOKEN}}` in `index.html` with the control token when serving it. Fallbacks: `?token=...` in the URL (stored,
then removed from the address bar), a token saved earlier in localStorage, and a token dialog shown after any 401.
All requests (including SSE and the backup download) are `fetch` calls with the `X-Token` header; no token goes into URLs.

## Mock server (no Rust needed)
```
node tools/mock-server.mjs [--port 8791] [--token T] [--latency 60] [--no-inject] [--fresh]
```
Open `http://127.0.0.1:8791/`. It serves `ui/` and implements the whole API with stateful fake data (edition switching, start/stop/restart,
runtime install/activate/upgrade/uninstall jobs with progress, SSE logs and jobs, backups, update check/apply, diagnostics with a fixable item).

- `--no-inject` does not inject the token (exercises the token dialog). `--fresh` starts in first-run state (wizard shows).
- Failure cases: installing PHP 8.3.29 fails with `sha256_mismatch`; restoring the `...rusak...` backup fails; ports 3001, 8081, 5432, 80 are
  occupied by fake external processes (port check and save return `port_in_use`); node `25.x` upgrade would fail the health check and roll back.
- Helpers (no token needed): `/__mock/offline?seconds=8` (drops connections), `/__mock/crash` (app state becomes `crashed`),
  `/__mock/reset[?nonode=1]` (reset state; `nonode=1` removes all Node runtimes to test install-then-switch).
- Note for Windows: the default control port 7878 is inside a Hyper-V excluded range on some machines (7780-7979), so listen fails with
  EACCES. The mock defaults to 8791.

# NextMON Server — spesifikasi

Pesaing "Mikhmon Server" (peluncur Windows + PHP bawaan), tetapi jauh lebih lengkap: menjalankan **NextMON versi PHP _atau_ versi Next.js (satu edisi aktif dalam satu waktu, tidak bisa dua-duanya sekaligus)**, dengan
**manajer runtime** untuk memilih, memasang, dan meng-upgrade versi **PHP** dan **Node.js**. Satu `.exe` (Rust), tanpa instalasi, portabel.

## Struktur folder (portabel)
```
nextmon-server/
  NextMON Server.exe          # peluncur (tray + panel kontrol web + CLI)
  nextmon-server.cmd          # pembantu CLI (memanggil exe)
  config.json                 # pengaturan (dibuat saat pertama jalan)
  app/php/                    # salinan nextmon-php (kode aplikasi, read-only untuk pengguna)
  app/next/                   # build standalone nextmon-next (server.js + .next + public)
  runtimes/php/<versi>/       # mis. 8.4.27 (php.exe, php.ini, ext/)  — banyak versi berdampingan
  runtimes/node/<versi>/      # mis. 24.12.0 (node.exe, npm)
  data/                       # data pengguna (NEXTMON_DATA): ikut di-backup, tidak ditimpa saat upgrade
  logs/                       # server.log, php-<tanggal>.log, next-<tanggal>.log
  backups/                    # backup data (zip) + cadangan sebelum upgrade runtime/aplikasi
  ui/                         # aset panel kontrol (HTML/CSS/JS) — ditanam ke exe saat build, juga bisa di-override dari folder
  docs/
```
Semua path relatif terhadap folder exe. Tidak menulis ke registry kecuali opsi "jalankan saat Windows start" (kunci Run milik pengguna, bisa dimatikan).

## config.json (skema)
```json
{
  "version": 1, "language": "id", "theme": "auto", "controlPort": 47878, "controlToken": "<acak, dibuat otomatis>",
  "edition": "php",                    // edisi AKTIF: "php" | "next" — hanya satu yang boleh berjalan; mengganti edisi menghentikan yang lama lebih dulu
  "bind": "127.0.0.1",                 // "127.0.0.1" | "0.0.0.0" (akses LAN; tampilkan peringatan + saran firewall)
  "autoStartWithWindows": false, "startMinimized": false, "autoStartEdition": true, "openBrowserOnStart": true,
  "dataDir": "data",                  // data per edisi di data/php dan data/next (format penyimpanan berbeda, tidak dicampur)
  "logRetentionDays": 14, "restartOnCrash": true, "maxRestarts": 5,
  "onboarded": false,                  // wizard pertama-kali selesai (diisi panel)
  "minNode": "20.9.0",                 // Node.js minimum untuk edisi Next.js
  "apps": {
    "php":  { "port": 8080, "runtime": "8.4.27", "opcache": true, "extraIni": {}, "env": { "NEXTMON_DEMO": "0" } },
    "next": { "port": 3000, "runtime": "24.12.0", "env": {} }
  },
  "update": { "checkOnStart": true, "channelPhp": "stable", "channelNode": "lts", "appRepo": "nextmon/nextmon-server", "appChannel": "stable" }
}
```

> Port kontrol default **47878** (7878 berada di rentang port terkecualikan Hyper-V/WSL 7780-7979 pada sebagian PC Windows -> EACCES). Bila port di config tidak bisa dibuka, server memilih port bebas lain, menyimpannya ke `config.json`, dan mencatatnya di log. `runtime` pada tiap app = versi terpasang ("8.4.27"; kosong = terbaru yang terpasang).

## API kontrol (HTTP JSON, hanya 127.0.0.1:<controlPort>, header `X-Token: <controlToken>`; UI dilayani dari `/` tanpa token tetapi token disisipkan ke halaman)
```
GET  /api/status                      -> { edition, app:{state,pid,port,url,lanUrl?,uptime,runtime,restarts,ramMB,lastError,retryInSecs,bind}, editions:{php:{port,runtime,installed,appPresent,label},next:{...}},
                                          version, appVersion, config (TANPA controlToken, termasuk onboarded), system:{os,arch,ramMB,diskFreeGB}, ports:{control,php,next},
                                          paths:{root,data,logs,backups}, about:{license,commit,buildDate,homepage}, autostart, updateAvailable }
                                          app.state: running|stopped|starting|stopping|crashed|failed  (crashed = proses mati, menunggu restart otomatis dengan backoff 1,2,4..30 dtk)
POST /api/edition                     body {edition, start?}  -> {ok:true} bila sudah aktif; selain itu 202 {jobId}. Edisi lama dihentikan dulu; invarian: tidak pernah dua proses edisi bersamaan.
                                          409 runtime_missing bila runtime edisi itu belum terpasang (error memuat runtime:{kind,installable}); 422 invalid_edition
POST /api/app/start|stop|restart      (selalu edisi aktif)  -> {ok, app}   409 port_in_use / runtime_missing / app_missing / node_too_old
POST /api/app/config                  body datar {edition?, port?, runtime?, opcache?, extraIni?, env?} -> config app edisi itu; me-restart bila edisi aktif sedang jalan
                                          409 port_in_use | runtime_not_installed, 422 invalid_port
GET  /api/ports/check?port=N&edition= -> {free:true} | {free:false, usedBy}
GET  /api/logs/{server|php|next}?lines=200[&format=text|&download=1]  -> {name,file,lines:[...]}
GET  /api/logs/{name}/stream          SSE, hanya baris BARU (tanpa backlog), satu baris per event `data:`
GET  /api/config                      -> config lengkap (+ controlToken, autostart)
POST /api/config                      body parsial (merge dangkal untuk update.*) -> config tanpa token; 422 invalid_config. Kunci apps/edition/controlToken/controlPort/version diabaikan (ada endpoint sendiri).
POST /api/config/token/regenerate     -> {token}   (token lama langsung tidak berlaku)
GET  /api/runtimes/{php|node}[?variant=nts|ts][&channel=lts|current][&refresh=1]
                                      -> { installed:[{version,dirName,path,active,usedBy:["php"|"next"],sizeMB,tsnts,arch}], available:[{version,channel,date,url,sha256,sizeMB,isNewer}], latest, updateAvailable, offline }
POST /api/runtimes/{kind}/install     body {version|"lts"|"latest", variant?} -> 202 {jobId}
POST /api/runtimes/{kind}/activate    body {version, app?}  -> {ok}  409 runtime_not_installed, 422 invalid_app
POST /api/runtimes/{kind}/uninstall   body {version}        -> {ok}  409 runtime_in_use, 404 not_found
POST /api/runtimes/{kind}/upgrade     body {to?, app?}      -> 202 {jobId}; backup data, aktifkan, restart, rollback otomatis (error health_check_failed) bila app tidak naik
GET  /api/jobs/{id}                   -> { id, kind, state: running|done|error, progress 0..1, step, message, error:{code,message}|null, result }
GET  /api/jobs/{id}/stream            SSE: event `data:` berisi snapshot job; ditutup saat selesai
POST /api/backup/create               -> 202 {jobId}       GET /api/backup/list -> {backups:[{name,sizeMB,date,kind:manual|auto}]}
POST /api/backup/restore {name}       -> 202 {jobId} (404/422 langsung bila nama salah; arsip rusak -> job error backup_corrupt)   POST /api/backup/delete {name}
GET  /api/backup/download?name=       -> zip biner
GET  /api/update/check                -> {current,latest,available,date,url,notes:[...],channel,installable}   POST /api/update/apply -> 202 {jobId}
GET  /api/diagnostics                 -> {checks:[{id,status:pass|warn|fail,title,detail,hint?,hintUrl?,fixable}], summary}   id: vcredist, ports, runtime-php|node, app-php|next, php-ext, node-min, data-write, webview2, lan, disk
POST /api/diagnostics/fix {id}        -> {ok} (saat ini hanya php-ext: mengaktifkan ekstensi yang DLL-nya ada)  | 422 not_fixable
POST /api/open {what}                 what: app:php|app:next (hanya edisi aktif) | folder:data|logs|backups|root
POST /api/panel/open   POST /api/shutdown   GET /api/version
```
Keamanan: bind hanya 127.0.0.1; header `Host` harus `127.0.0.1:<port>`/`localhost:<port>`/`[::1]:<port>` (anti DNS-rebinding); `Origin` (bila ada) harus origin sendiri; tidak ada header CORS sama sekali;
token **hanya** lewat header `X-Token` (perbandingan waktu-konstan; tidak pernah di URL). `index.html` dilayani dengan `{{NM_TOKEN}}` diganti token (halaman lain tanpa placeholder mendapat `<meta name="nm-token">`).
UI disajikan dari folder `ui/` bila ada, jika tidak dari salinan yang ditanam di exe.
Semua perubahan state yang lama (download/ekstrak/upgrade/backup/restore/ganti edisi) berupa **job** asinkron dengan progres. Error selalu `{ "error": { "code": "...", "message": { "id": "...", "en": "..." }, ... } }` (field tambahan seperti `port`, `usedBy`, `runtime`, `rolledBack` bila relevan).

## Perilaku server (implementasi)
- Satu proses edisi: `php.exe` (opsi opcache seperti `serve.ps1`, `-c php.ini` runtime, `-S bind:port -t app/php/public app/php/public/index.php`) atau `node.exe app/next/server.js`; env `NEXTMON_DATA=data/<edisi>`, `NEXTMON_BASE_URL`, `NEXTMON_EDITION`, plus `PORT`/`HOSTNAME` (Next). Jendela konsol disembunyikan; stdout/stderr -> `logs/<edisi>-YYYY-MM-DD.log`; log server -> `logs/server.log` (rotasi 2 MB x 5; log aplikasi dihapus setelah `logRetentionDays`).
- Proses anak berada di Windows Job Object (KILL_ON_JOB_CLOSE): mati bersama exe. `run/<edisi>.pid` dipakai membersihkan proses yatim saat start berikutnya (hanya bila path exe cocok). Stop = terminate seluruh job (PHP/Node tidak punya jalur shutdown anggun di Windows).
- Crash: restart otomatis (`restartOnCrash`, `maxRestarts`) dengan backoff 1,2,4,8,16,30 dtk; hitungan di-reset bila proses hidup > 60 dtk; setelah habis -> state `failed`.
- Update aplikasi mengganti `app/`, `ui/`, `docs/` (cadangan `*.bak`), tidak menyentuh `data/`, `config.json`, `runtimes/`; paket harus punya entri di `SHA256SUMS.txt`; rollback otomatis bila app tidak naik. Exe launcher sendiri tidak diganti (belum ada self-replace); `ui/` baru langsung berlaku karena override folder.
- Versi aplikasi: baris pertama `app/VERSION` (jika tidak ada, versi launcher).

## Crate Rust (workspace)
- `crates/runtimes` (lib `nm_runtimes`) — manajer runtime murni (tanpa UI/HTTP server):
  - `php`: ambil `https://windows.php.net/downloads/releases/releases.json` (fallback `/archives/`), pilih varian `nts-Win32-vs17-x64` (default NTS; opsi TS), verifikasi **sha256** dari releases.json, unduh dengan progres + resume, ekstrak zip aman (cegah zip-slip), buat `php.ini` dari `php.ini-production` + penyesuaian (ekstensi wajib NextMON: openssl, mbstring, sockets, curl, gd, pdo_sqlite, sqlite3, zip, fileinfo, intl bila ada; opcache + `opcache.enable_cli=0`; `extension_dir`), tes `php -v` & `php -m`.
  - `node`: `https://nodejs.org/dist/index.json`, zip `node-v<ver>-win-x64.zip`, verifikasi `SHASUMS256.txt`, kanal LTS/Current, filter versi minimum (config `minNode`).
  - Fungsi: `list_installed`, `list_available(channel)`, `install(version, progress_cb)`, `uninstall`, `resolve_latest(channel)`, `health_check(runtime_path)`, `compare_versions`, `is_newer`. Cache daftar online 6 jam (offline-safe: pakai cache bila gagal).
  - Proxy: hormati `HTTPS_PROXY`/`HTTP_PROXY`. Timeout & retry. Tidak butuh hak admin.
- `crates/server` (bin `nextmon-server`) — proses utama: config, supervisor proses (spawn php/node dengan env `NEXTMON_DATA`, `NEXTMON_BASE_URL`, port, bind), restart-on-crash dengan backoff, log rotasi, HTTP API kontrol + file UI, tray icon (menu: Buka panel, Start/Stop tiap app, Buka aplikasi, Keluar), jendela panel (WebView2 via `wry` bila tersedia, fallback buka browser default), CLI, autostart Windows, single-instance (mutex), backup/restore data, cek update aplikasi (GitHub Releases API), diagnostik.
  - Menjalankan PHP: `php.exe [-d opcache...] -S <bind>:<port> -t app/php/public app/php/public/index.php` dengan `-c` ke php.ini runtime; bila `bind=0.0.0.0` tampilkan peringatan.
  - Menjalankan Next: `node.exe app/next/server.js` dengan `PORT`, `HOSTNAME`, `NEXTMON_DATA`.
  - Pastikan port bebas sebelum start; deteksi proses yatim (PID file) dan bersihkan; matikan anak proses (job object Windows) saat exe keluar.
- CLI: `nextmon-server start [php|next|all]`, `stop`, `restart`, `status`, `php list|install <v>|use <v>|upgrade|available`, `node list|install <v|lts|latest>|use <v>|upgrade`, `backup create|list|restore <nama>`, `update check|apply`, `doctor`, `open`, `autostart on|off`, `version`. Keluaran teks rapi + `--json`.

## UI panel kontrol (`ui/`, HTML/CSS/JS murni, tanpa CDN)
Desain sama dengan NextMON (glass terkendali, aksen teal, logo N + sinyal, dark/light, responsif, id/en). Halaman: **Dashboard** (satu kartu besar edisi aktif: state, port, URL, uptime, tombol Start/Stop/Restart/Buka, RAM; di atasnya pemilih edisi PHP | Next.js (segmented control) — ganti edisi meminta konfirmasi, menjelaskan bahwa data kedua edisi terpisah dan hanya satu berjalan; edisi lain tampil sebagai kartu kecil "tidak aktif" dengan tombol "Pindah ke edisi ini"), **Runtime** (tab PHP / Node: terpasang vs tersedia, badge "Baru", tombol Pasang/Pakai/Upgrade/Hapus, progres job, pilih per-app), **Aplikasi** (port, bind, env, opsi opcache/ini, mode demo), **Log** (live tail, filter, salin, unduh), **Backup** (buat/pulihkan/unduh), **Pembaruan** (aplikasi + runtime), **Diagnostik**, **Pengaturan** (bahasa, tema, autostart, tray, token, folder data), **Tentang**.
UI memakai `fetch` ke API di atas; dapat diuji dengan mock server Node tanpa Rust.

## Paket rilis
- `NextMON-Server-vX.Y.Z-win-x64.zip` (lite: exe + app/php + app/next + ui + docs, runtime diunduh saat pertama jalan) dan `NextMON-Server-vX.Y.Z-win-x64-full.zip` (menyertakan PHP stabil terbaru + Node LTS).
- `NextMON-vX.Y.Z-source.zip` (kode sumber tanpa data/build). Skrip: `scripts/package.ps1`, checksum `SHA256SUMS.txt`, catatan rilis `RELEASE_NOTES.md`.

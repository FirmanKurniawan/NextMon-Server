// Small i18n dictionary (id default, en). t(key, {param}) -> string. Missing keys fall back to id, then to the key.
const id = {
  'common.back': 'Kembali', 'common.cancel': 'Batal', 'common.close': 'Tutup', 'common.confirm': 'Konfirmasi', 'common.copied': 'Disalin', 'common.loading': 'Memuat…', 'common.next': 'Lanjut',
  'common.refresh': 'Segarkan', 'common.refreshed': 'Data disegarkan', 'common.remove': 'Hapus baris', 'common.reset': 'Atur ulang', 'common.retry': 'Coba lagi', 'common.saved': 'Tersimpan', 'common.unavailable': 'Tidak tersedia',
  'nav.label': 'Navigasi utama', 'nav.menu': 'Buka menu', 'nav.skip': 'Lompat ke konten',
  'nav.dashboard': 'Dashboard', 'nav.runtimes': 'Runtime', 'nav.apps': 'Aplikasi', 'nav.logs': 'Log', 'nav.backup': 'Backup', 'nav.updates': 'Pembaruan', 'nav.diagnostics': 'Diagnostik', 'nav.settings': 'Pengaturan', 'nav.about': 'Tentang',
  'theme.label': 'Tema', 'theme.auto': 'Otomatis', 'theme.light': 'Terang', 'theme.dark': 'Gelap',
  'u.d': 'hr', 'u.h': 'j', 'u.m': 'mnt', 'u.s': 'dtk',
  'state.running': 'Berjalan', 'state.stopped': 'Berhenti', 'state.starting': 'Memulai', 'state.stopping': 'Menghentikan', 'state.crashed': 'Crash', 'state.failed': 'Gagal',
  'err.generic': 'Terjadi kesalahan. Coba lagi.', 'err.network': 'Tidak dapat terhubung ke NextMON Server.', 'err.noversion': 'Tidak ada versi runtime yang tersedia.', 'err.page': 'Halaman gagal dimuat.',
  'offline.title': 'Server tidak merespons', 'offline.body': 'Panel mencoba menghubungkan kembali secara otomatis. Pastikan NextMON Server masih berjalan.', 'conn.ok': 'Terhubung',
  'auth.title': 'Token diperlukan', 'auth.body': 'Server menolak permintaan karena token kontrol tidak valid. Masukkan token dari config.json (controlToken).', 'auth.token': 'Token kontrol', 'auth.save': 'Simpan token', 'auth.bad': 'Token ditolak oleh server.',
  'lan.title': 'Akses LAN aktif', 'lan.body': 'Aplikasi terbuka untuk semua perangkat di jaringan Anda (0.0.0.0). Panel ini tidak memakai login pengguna; batasi lewat firewall Windows dan gunakan hanya di jaringan tepercaya.', 'lan.change': 'Ubah akses jaringan',

  'dash.title': 'Dashboard', 'dash.desc': 'Satu edisi NextMON berjalan pada satu waktu. Atur dan pantau di sini.',
  'dash.edition': 'Edisi aktif', 'dash.oneAtATime': 'Hanya satu edisi berjalan sekaligus.',
  'dash.port': 'Port', 'dash.uptime': 'Uptime', 'dash.ram': 'RAM', 'dash.runtime': 'Runtime',
  'dash.start': 'Mulai', 'dash.stop': 'Hentikan', 'dash.restart': 'Restart', 'dash.open': 'Buka',
  'dash.started': 'Aplikasi dimulai', 'dash.stopped': 'Aplikasi dihentikan', 'dash.restarted': 'Aplikasi di-restart',
  'dash.nourl': 'Belum berjalan', 'dash.stoppedurl': 'Akan tersedia di {url}', 'dash.noruntime': 'Runtime belum dipilih',
  'dash.demo': 'Mode demo', 'dash.demo.help': 'Memakai data palsu tanpa router. Diterapkan dengan restart otomatis.', 'dash.demo.on': 'Mode demo aktif', 'dash.demo.off': 'Mode demo dimatikan',
  'dash.tail': 'Log terbaru', 'dash.alllogs': 'Semua log', 'dash.notail': 'Belum ada log.',
  'dash.inactive': 'Tidak aktif', 'dash.switch': 'Pindah ke edisi ini', 'dash.rtmissing': '{rt} belum terpasang',

  'edition.switch.title': 'Pindah ke edisi {to}?', 'edition.switch.body': 'Anda akan berpindah dari {from} ke {to}.',
  'edition.switch.one': 'Hanya satu edisi yang berjalan pada satu waktu.', 'edition.switch.data': 'Data {from} dan {to} terpisah (folder data/php dan data/next) dan tidak dicampur. Data tidak dihapus.',
  'edition.switch.stop': '{from} akan dihentikan lebih dulu secara halus.', 'edition.switch.start': 'Jalankan edisi baru setelah pindah',
  'edition.switch.noruntime': '{rt} untuk edisi ini belum terpasang. Versi stabil terbaru akan diunduh dan dipasang lebih dulu.', 'edition.switch.go': 'Pindah edisi', 'edition.switch.installgo': 'Pasang runtime dan pindah',
  'edition.switch.switching': 'Berpindah edisi…', 'edition.switched': 'Sekarang memakai edisi {to}',

  'rt.title': 'Runtime', 'rt.desc': 'Kelola versi PHP dan Node.js. Pasang beberapa versi berdampingan, lalu pilih yang dipakai edisi aktif.',
  'rt.installed': 'Terpasang', 'rt.available': 'Tersedia', 'rt.active': 'Aktif', 'rt.usedby': 'Dipakai oleh', 'rt.new': 'Baru', 'rt.installedbadge': 'Terpasang',
  'rt.variant': 'Varian', 'rt.nts.help': 'NTS: disarankan untuk server bawaan PHP.', 'rt.ts.help': 'TS: thread-safe, hanya bila ekstensi Anda memerlukannya.',
  'rt.channel': 'Kanal', 'rt.ch.lts': 'LTS', 'rt.ch.current': 'Current', 'rt.lts.help': 'LTS: stabil, didukung jangka panjang (disarankan).', 'rt.current.help': 'Current: fitur terbaru, dukungan lebih singkat.',
  'rt.update.title': '{rt} {v} tersedia', 'rt.update.body': 'Versi terpasang saat ini: {cur}.', 'rt.upgrade.latest': 'Upgrade ke terbaru',
  'rt.none.title': '{rt} belum terpasang', 'rt.none.desc': 'Pasang salah satu versi dari daftar tersedia di bawah.', 'rt.avail.none': 'Tidak ada versi tersedia. Periksa koneksi internet.',
  'rt.more': 'Tampilkan {n} versi lainnya', 'rt.less': 'Tampilkan lebih sedikit',
  'rt.install': 'Pasang', 'rt.installing': 'Memasang…', 'rt.installed.ok': '{v} terpasang', 'rt.install.fail': 'Gagal memasang {v}.',
  'rt.use': 'Pakai untuk {app}', 'rt.use.title': 'Pakai versi {v}?', 'rt.use.restart': '{app} sedang berjalan dan akan di-restart untuk memakai versi ini.', 'rt.use.go': 'Pakai dan restart', 'rt.use.ok': '{app} memakai {v}',
  'rt.upgrade': 'Upgrade', 'rt.upgrade.title': 'Upgrade {rt} ke {v}?', 'rt.upgrade.body': 'Versi {cur} akan diganti {v} untuk {app}. Versi lama tetap terpasang.',
  'rt.upgrade.rollback': 'Jika pemeriksaan kesehatan setelah upgrade gagal, NextMON Server otomatis mengembalikan versi sebelumnya (rollback).', 'rt.upgrade.ok': 'Upgrade ke {v} berhasil', 'rt.upgrade.fail': 'Upgrade gagal.',
  'rt.delete': 'Hapus', 'rt.delete.title': 'Hapus versi {v}?', 'rt.delete.body': '{rt} {v} ({mb}) akan dihapus dari disk. Anda bisa memasangnya lagi nanti.', 'rt.delete.inuse': 'Sedang dipakai, tidak bisa dihapus', 'rt.delete.ok': '{v} dihapus',

  'apps.title': 'Aplikasi', 'apps.desc': 'Pengaturan per edisi. Edisi yang tidak aktif tetap bisa diedit tanpa restart.', 'apps.active': 'Aktif',
  'apps.inactive': 'Edisi ini tidak aktif. Perubahan disimpan tanpa restart dan berlaku saat edisi ini diaktifkan.', 'apps.general': 'Pengaturan {app}', 'apps.unsaved': 'Belum disimpan',
  'apps.port': 'Port', 'apps.port.help': 'Port TCP lokal 1-65535 untuk aplikasi.', 'apps.port.range': 'Masukkan port antara 1 dan 65535.', 'apps.port.free': 'Port bebas.', 'apps.port.own': 'Port ini sedang dipakai aplikasi ini.', 'apps.port.busy': 'Port dipakai oleh {by}.', 'apps.port.unknown': 'Tidak dapat memeriksa port.',
  'apps.runtime': 'Versi runtime', 'apps.rt.help': 'Versi {rt} yang menjalankan edisi ini.', 'apps.rt.none': 'Belum ada runtime', 'apps.rt.nohelp': '{rt} belum terpasang.', 'apps.rt.missing': 'tidak terpasang', 'apps.latest': 'terbaru',
  'apps.env.title': 'Variabel lingkungan', 'apps.env.help': 'Diteruskan ke proses aplikasi saat dijalankan. Perubahan berlaku setelah restart.', 'apps.env.add': 'Tambah variabel', 'apps.env.key': 'Nama variabel', 'apps.env.value': 'Nilai variabel',
  'apps.php.title': 'Opsi PHP', 'apps.opcache.help': 'Mempercepat PHP dengan menyimpan bytecode di memori.', 'apps.ini.title': 'Direktif php.ini tambahan', 'apps.ini.help': 'Contoh: memory_limit = 256M. Menimpa php.ini bawaan.', 'apps.ini.add': 'Tambah direktif', 'apps.ini.key': 'Nama direktif', 'apps.ini.value': 'Nilai direktif',
  'apps.save': 'Simpan', 'apps.saverestart': 'Simpan dan restart', 'apps.saved': 'Pengaturan disimpan', 'apps.saved.restarted': 'Pengaturan disimpan, aplikasi di-restart', 'apps.fixerrors': 'Perbaiki kolom yang bermasalah dulu.',
  'apps.net.title': 'Akses jaringan', 'apps.bind': 'Alamat bind', 'apps.bind.local': 'Hanya komputer ini (127.0.0.1)', 'apps.bind.lan': 'Seluruh jaringan LAN (0.0.0.0)', 'apps.bind.help': 'Berlaku untuk aplikasi edisi aktif dan diterapkan dengan restart.',
  'apps.bind.confirm.title': 'Buka akses ke LAN?', 'apps.bind.confirm.body': 'Perangkat lain di jaringan dapat membuka aplikasi. Pastikan firewall membatasi akses dan jaringan Anda tepercaya.', 'apps.bind.confirm.go': 'Buka akses LAN',
  'kv.badkey': 'Nama "{k}" tidak valid.', 'kv.dup': 'Nama "{k}" muncul dua kali.',

  'logs.title': 'Log', 'logs.desc': 'Pantau keluaran server dan aplikasi secara langsung.', 'logs.source': 'Sumber log', 'logs.src.server': 'Server', 'logs.src.php': 'PHP', 'logs.src.next': 'Next.js',
  'logs.search': 'Cari di log', 'logs.level': 'Tingkat', 'logs.lvl.all': 'Semua tingkat', 'logs.lvl.warn': 'Peringatan+', 'logs.lvl.err': 'Hanya error',
  'logs.pause': 'Jeda', 'logs.resume': 'Lanjut', 'logs.copy': 'Salin', 'logs.download': 'Unduh', 'logs.clear': 'Bersihkan', 'logs.copied': '{n} baris disalin',
  'logs.live': 'Langsung', 'logs.paused': 'Dijeda, baris baru ditahan', 'logs.connecting': 'Menghubungkan…', 'logs.reconnecting': 'Terputus, menyambung ulang…', 'logs.count': '{n} dari {total} baris', 'logs.empty': 'Belum ada log untuk sumber ini.', 'logs.nomatch': 'Tidak ada baris yang cocok dengan filter.',

  'backup.title': 'Backup', 'backup.desc': 'Cadangkan folder data (kedua edisi) dan pulihkan bila diperlukan.', 'backup.create': 'Buat backup', 'backup.creating': 'Membuat backup…', 'backup.created': 'Backup dibuat', 'backup.fail': 'Backup gagal.',
  'backup.list': 'Daftar backup', 'backup.auto': 'otomatis', 'backup.empty.title': 'Belum ada backup', 'backup.empty.desc': 'Buat backup pertama agar data aman sebelum upgrade.', 'backup.download': 'Unduh', 'backup.restore': 'Pulihkan',
  'backup.restore.title': 'Pulihkan backup?', 'backup.restore.body': 'Data akan dikembalikan dari {name}.', 'backup.restore.warn': 'Data saat ini akan ditimpa dan aplikasi dihentikan sementara. Buat backup baru dulu bila perlu.', 'backup.restore.check': 'Saya mengerti data saat ini akan ditimpa',
  'backup.restoring': 'Memulihkan backup…', 'backup.restored': 'Backup dipulihkan', 'backup.restore.fail': 'Pemulihan gagal.',

  'upd.title': 'Pembaruan', 'upd.desc': 'Periksa pembaruan NextMON Server dan ringkasan pembaruan runtime.', 'upd.check': 'Periksa pembaruan', 'upd.app': 'NextMON Server', 'upd.current': 'Versi terpasang', 'upd.latest': 'Terbaru',
  'upd.available': 'Versi {v} tersedia', 'upd.notes': 'Catatan rilis', 'upd.apply': 'Perbarui sekarang', 'upd.release': 'Lihat rilis', 'upd.uptodate': 'Anda memakai versi terbaru.', 'upd.uptodate.short': 'Terbaru', 'upd.found': 'Versi {v} tersedia',
  'upd.fail': 'Gagal memeriksa pembaruan.', 'upd.auto': 'Periksa otomatis saat mulai', 'upd.channel': 'Kanal rilis', 'upd.ch.stable': 'Stabil', 'upd.ch.beta': 'Beta',
  'upd.apply.title': 'Perbarui ke v{v}?', 'upd.apply.body': 'Pembaruan akan diunduh dan dipasang. Langkah berikut dijalankan:', 'upd.apply.a': 'Backup otomatis dibuat sebelum memperbarui.', 'upd.apply.b': 'Aplikasi dihentikan sebentar dan NextMON Server di-restart.', 'upd.apply.c': 'Data dan pengaturan Anda tidak ditimpa.',
  'upd.applying': 'Memperbarui…', 'upd.restarting': 'Pembaruan selesai. Server restart, halaman dimuat ulang otomatis.', 'upd.apply.fail': 'Pembaruan gagal.', 'upd.runtimes': 'Pembaruan runtime', 'upd.manage': 'Kelola runtime', 'upd.rt.cur': 'Dipakai',

  'diag.title': 'Diagnostik', 'diag.desc': 'Periksa kebutuhan sistem dan konfigurasi. Beberapa masalah bisa diperbaiki otomatis.', 'diag.run': 'Jalankan ulang', 'diag.pass': 'Lolos', 'diag.warn': 'Peringatan', 'diag.fail': 'Gagal',
  'diag.c.vcredist': 'Microsoft Visual C++ Runtime', 'diag.c.ports': 'Port tidak bentrok', 'diag.c.php-ext': 'Ekstensi PHP wajib', 'diag.c.php-ext-intl': 'Ekstensi PHP wajib', 'diag.c.data-write': 'Izin tulis folder data', 'diag.c.webview2': 'WebView2 Runtime', 'diag.c.node-min': 'Versi Node.js memenuhi syarat Next.js',
  'diag.fix': 'Perbaiki', 'diag.fixed': 'Perbaikan diterapkan', 'diag.fixfail': 'Perbaikan gagal.', 'diag.empty': 'Tidak ada hasil pemeriksaan.',

  'settings.title': 'Pengaturan', 'settings.desc': 'Perubahan disimpan otomatis.', 'settings.appearance': 'Tampilan', 'settings.language': 'Bahasa', 'settings.language.help': 'Bahasa antarmuka panel.', 'settings.theme.help': 'Otomatis mengikuti pengaturan sistem.',
  'settings.startup': 'Peluncuran', 'settings.autostart': 'Jalankan saat Windows mulai', 'settings.autostart.help': 'Menambah kunci Run milik pengguna. Bisa dimatikan kapan saja.', 'settings.minimized': 'Mulai diminimalkan', 'settings.minimized.help': 'Langsung ke tray tanpa membuka jendela panel.',
  'settings.autoedition': 'Jalankan edisi aktif otomatis', 'settings.autoedition.help': 'Edisi aktif ikut berjalan ketika NextMON Server dibuka.', 'settings.openbrowser': 'Buka browser saat mulai', 'settings.openbrowser.help': 'Membuka aplikasi di browser setelah berjalan.',
  'settings.restartcrash': 'Restart bila crash', 'settings.restartcrash.help': 'Menjalankan ulang aplikasi otomatis dengan jeda bertahap.',
  'settings.security': 'Keamanan', 'settings.token': 'Token kontrol', 'settings.token.help': 'Dipakai panel dan CLI untuk mengakses API lokal. Jangan dibagikan.', 'settings.token.show': 'Tampilkan token', 'settings.token.hide': 'Sembunyikan token', 'settings.token.copy': 'Salin token',
  'settings.token.regen': 'Buat token baru', 'settings.token.regen.title': 'Buat token baru?', 'settings.token.regen.body': 'Token lama langsung tidak berlaku. CLI atau tab panel lain harus memakai token baru.', 'settings.token.regen.ok': 'Token baru dibuat',
  'settings.storage': 'Penyimpanan', 'settings.datadir': 'Folder data', 'settings.datadir.help': 'Data tiap edisi disimpan terpisah di subfolder php dan next.', 'settings.open.data': 'Buka folder data', 'settings.open.logs': 'Buka folder log',
  'settings.retention': 'Retensi log', 'settings.days': 'hari', 'settings.retention.help': 'Log lebih lama dari ini dihapus otomatis.', 'settings.ret.bad': 'Masukkan 1 sampai 365 hari.',

  'about.title': 'Tentang', 'about.tag': 'Peluncur lokal untuk NextMON', 'about.body': 'NextMON Server menjalankan NextMON edisi PHP atau Next.js di komputer Anda, lengkap dengan manajer runtime, backup, dan pembaruan.',
  'about.releases': 'Rilis', 'about.issues': 'Laporkan masalah', 'about.site': 'Situs', 'about.info': 'Informasi', 'about.version': 'Versi', 'about.license': 'Lisensi', 'about.commit': 'Commit', 'about.built': 'Tanggal build', 'about.os': 'Sistem', 'about.ram': 'RAM', 'about.disk': 'Disk', 'about.free': 'kosong', 'about.panelport': 'Port panel',

  'wiz.title': 'Selamat datang di NextMON Server', 'wiz.steps': 'Langkah penyiapan', 'wiz.skip': 'Lewati', 'wiz.recommended': 'Disarankan',
  'wiz.s.edition': 'Edisi', 'wiz.s.version': 'Versi', 'wiz.s.network': 'Jaringan', 'wiz.s.install': 'Pasang',
  'wiz.edition.title': 'Pilih edisi', 'wiz.edition.desc': 'Pilih edisi NextMON yang akan dijalankan. Anda bisa berpindah kapan saja.', 'wiz.ed.php': 'Ringan dan mudah. Cocok untuk pemula dan komputer sederhana.', 'wiz.ed.next': 'Untuk fitur realtime dan antarmuka modern. Memerlukan Node.js.', 'wiz.ed.note': 'Hanya satu edisi berjalan sekaligus dan datanya terpisah.',
  'wiz.version.title': 'Pilih versi runtime', 'wiz.version.desc': 'Versi stabil terbaru dipilih untuk Anda. Versi yang belum ada akan diunduh.', 'wiz.ver.have': 'Sudah terpasang, tidak perlu unduh.', 'wiz.ver.download': 'Akan diunduh sekitar {mb}.',
  'wiz.network.title': 'Port dan akses jaringan', 'wiz.network.desc': 'Tentukan port dan siapa yang boleh mengakses aplikasi.', 'wiz.net.local': 'Hanya bisa dibuka dari komputer ini. Paling aman.', 'wiz.net.lan': 'Bisa dibuka dari HP atau komputer lain di jaringan yang sama.',
  'wiz.install': 'Pasang dan selesaikan', 'wiz.install.title': 'Memasang', 'wiz.install.desc': 'Mengunduh dan menyiapkan runtime. Jangan tutup jendela ini.', 'wiz.install.fail': 'Pemasangan gagal', 'wiz.applying': 'Menerapkan pengaturan…',
  'wiz.done.title': 'Siap dipakai', 'wiz.done.body': '{ed} dengan runtime {v} siap di port {port}.', 'wiz.gotodash': 'Ke dashboard', 'wiz.openapp': 'Mulai dan buka aplikasi',
};

const en = {
  'common.back': 'Back', 'common.cancel': 'Cancel', 'common.close': 'Close', 'common.confirm': 'Confirm', 'common.copied': 'Copied', 'common.loading': 'Loading…', 'common.next': 'Next',
  'common.refresh': 'Refresh', 'common.refreshed': 'Data refreshed', 'common.remove': 'Remove row', 'common.reset': 'Reset', 'common.retry': 'Retry', 'common.saved': 'Saved', 'common.unavailable': 'Unavailable',
  'nav.label': 'Main navigation', 'nav.menu': 'Open menu', 'nav.skip': 'Skip to content',
  'nav.dashboard': 'Dashboard', 'nav.runtimes': 'Runtimes', 'nav.apps': 'Application', 'nav.logs': 'Logs', 'nav.backup': 'Backup', 'nav.updates': 'Updates', 'nav.diagnostics': 'Diagnostics', 'nav.settings': 'Settings', 'nav.about': 'About',
  'theme.label': 'Theme', 'theme.auto': 'Auto', 'theme.light': 'Light', 'theme.dark': 'Dark',
  'u.d': 'd', 'u.h': 'h', 'u.m': 'm', 'u.s': 's',
  'state.running': 'Running', 'state.stopped': 'Stopped', 'state.starting': 'Starting', 'state.stopping': 'Stopping', 'state.crashed': 'Crashed', 'state.failed': 'Failed',
  'err.generic': 'Something went wrong. Try again.', 'err.network': 'Cannot reach NextMON Server.', 'err.noversion': 'No runtime version is available.', 'err.page': 'The page failed to load.',
  'offline.title': 'Server is not responding', 'offline.body': 'The panel retries automatically. Make sure NextMON Server is still running.', 'conn.ok': 'Connected',
  'auth.title': 'Token required', 'auth.body': 'The server rejected the request because the control token is invalid. Enter the token from config.json (controlToken).', 'auth.token': 'Control token', 'auth.save': 'Save token', 'auth.bad': 'The server rejected this token.',
  'lan.title': 'LAN access is on', 'lan.body': 'The app is open to every device on your network (0.0.0.0). The panel has no user login; restrict access with the Windows firewall and use only on trusted networks.', 'lan.change': 'Change network access',

  'dash.title': 'Dashboard', 'dash.desc': 'One NextMON edition runs at a time. Control and monitor it here.',
  'dash.edition': 'Active edition', 'dash.oneAtATime': 'Only one edition runs at a time.',
  'dash.port': 'Port', 'dash.uptime': 'Uptime', 'dash.ram': 'RAM', 'dash.runtime': 'Runtime',
  'dash.start': 'Start', 'dash.stop': 'Stop', 'dash.restart': 'Restart', 'dash.open': 'Open',
  'dash.started': 'App started', 'dash.stopped': 'App stopped', 'dash.restarted': 'App restarted',
  'dash.nourl': 'Not running', 'dash.stoppedurl': 'Will be available at {url}', 'dash.noruntime': 'No runtime selected',
  'dash.demo': 'Demo mode', 'dash.demo.help': 'Uses fake data, no router needed. Applied with an automatic restart.', 'dash.demo.on': 'Demo mode on', 'dash.demo.off': 'Demo mode off',
  'dash.tail': 'Recent log', 'dash.alllogs': 'All logs', 'dash.notail': 'No log lines yet.',
  'dash.inactive': 'Inactive', 'dash.switch': 'Switch to this edition', 'dash.rtmissing': '{rt} not installed',

  'edition.switch.title': 'Switch to {to}?', 'edition.switch.body': 'You are moving from {from} to {to}.',
  'edition.switch.one': 'Only one edition runs at a time.', 'edition.switch.data': '{from} and {to} data are kept separate (data/php and data/next folders) and never mixed. Nothing is deleted.',
  'edition.switch.stop': '{from} is stopped gracefully first.', 'edition.switch.start': 'Start the new edition after switching',
  'edition.switch.noruntime': 'The {rt} runtime for this edition is not installed. The latest stable version will be downloaded and installed first.', 'edition.switch.go': 'Switch edition', 'edition.switch.installgo': 'Install runtime and switch',
  'edition.switch.switching': 'Switching edition…', 'edition.switched': 'Now using {to}',

  'rt.title': 'Runtimes', 'rt.desc': 'Manage PHP and Node.js versions. Install several side by side, then pick the one the active edition uses.',
  'rt.installed': 'Installed', 'rt.available': 'Available', 'rt.active': 'Active', 'rt.usedby': 'Used by', 'rt.new': 'New', 'rt.installedbadge': 'Installed',
  'rt.variant': 'Variant', 'rt.nts.help': 'NTS: recommended for the built-in PHP server.', 'rt.ts.help': 'TS: thread-safe, only if your extensions need it.',
  'rt.channel': 'Channel', 'rt.ch.lts': 'LTS', 'rt.ch.current': 'Current', 'rt.lts.help': 'LTS: stable with long-term support (recommended).', 'rt.current.help': 'Current: newest features, shorter support.',
  'rt.update.title': '{rt} {v} is available', 'rt.update.body': 'Installed version: {cur}.', 'rt.upgrade.latest': 'Upgrade to latest',
  'rt.none.title': '{rt} is not installed', 'rt.none.desc': 'Install one of the versions listed below.', 'rt.avail.none': 'No versions available. Check your internet connection.',
  'rt.more': 'Show {n} more versions', 'rt.less': 'Show fewer',
  'rt.install': 'Install', 'rt.installing': 'Installing…', 'rt.installed.ok': '{v} installed', 'rt.install.fail': 'Failed to install {v}.',
  'rt.use': 'Use for {app}', 'rt.use.title': 'Use version {v}?', 'rt.use.restart': '{app} is running and will restart to use this version.', 'rt.use.go': 'Use and restart', 'rt.use.ok': '{app} now uses {v}',
  'rt.upgrade': 'Upgrade', 'rt.upgrade.title': 'Upgrade {rt} to {v}?', 'rt.upgrade.body': 'Version {cur} will be replaced by {v} for {app}. The old version stays installed.',
  'rt.upgrade.rollback': 'If the post-upgrade health check fails, NextMON Server automatically rolls back to the previous version.', 'rt.upgrade.ok': 'Upgraded to {v}', 'rt.upgrade.fail': 'Upgrade failed.',
  'rt.delete': 'Delete', 'rt.delete.title': 'Delete version {v}?', 'rt.delete.body': '{rt} {v} ({mb}) will be removed from disk. You can install it again later.', 'rt.delete.inuse': 'In use, cannot be deleted', 'rt.delete.ok': '{v} deleted',

  'apps.title': 'Application', 'apps.desc': 'Per-edition settings. An inactive edition can still be edited without a restart.', 'apps.active': 'Active',
  'apps.inactive': 'This edition is inactive. Changes are saved without a restart and apply when the edition is activated.', 'apps.general': '{app} settings', 'apps.unsaved': 'Unsaved',
  'apps.port': 'Port', 'apps.port.help': 'Local TCP port 1-65535 for the app.', 'apps.port.range': 'Enter a port between 1 and 65535.', 'apps.port.free': 'Port is free.', 'apps.port.own': 'This app is using this port now.', 'apps.port.busy': 'Port is used by {by}.', 'apps.port.unknown': 'Could not check the port.',
  'apps.runtime': 'Runtime version', 'apps.rt.help': '{rt} version that runs this edition.', 'apps.rt.none': 'No runtime yet', 'apps.rt.nohelp': '{rt} is not installed.', 'apps.rt.missing': 'not installed', 'apps.latest': 'latest',
  'apps.env.title': 'Environment variables', 'apps.env.help': 'Passed to the app process at launch. Changes apply after a restart.', 'apps.env.add': 'Add variable', 'apps.env.key': 'Variable name', 'apps.env.value': 'Variable value',
  'apps.php.title': 'PHP options', 'apps.opcache.help': 'Speeds up PHP by caching bytecode in memory.', 'apps.ini.title': 'Extra php.ini directives', 'apps.ini.help': 'Example: memory_limit = 256M. Overrides the default php.ini.', 'apps.ini.add': 'Add directive', 'apps.ini.key': 'Directive name', 'apps.ini.value': 'Directive value',
  'apps.save': 'Save', 'apps.saverestart': 'Save and restart', 'apps.saved': 'Settings saved', 'apps.saved.restarted': 'Settings saved, app restarted', 'apps.fixerrors': 'Fix the invalid fields first.',
  'apps.net.title': 'Network access', 'apps.bind': 'Bind address', 'apps.bind.local': 'This computer only (127.0.0.1)', 'apps.bind.lan': 'Whole LAN (0.0.0.0)', 'apps.bind.help': 'Applies to the active edition and takes effect with a restart.',
  'apps.bind.confirm.title': 'Open access to the LAN?', 'apps.bind.confirm.body': 'Other devices on the network will be able to open the app. Make sure your firewall limits access and the network is trusted.', 'apps.bind.confirm.go': 'Open LAN access',
  'kv.badkey': 'Name "{k}" is not valid.', 'kv.dup': 'Name "{k}" appears twice.',

  'logs.title': 'Logs', 'logs.desc': 'Watch server and app output live.', 'logs.source': 'Log source', 'logs.src.server': 'Server', 'logs.src.php': 'PHP', 'logs.src.next': 'Next.js',
  'logs.search': 'Search logs', 'logs.level': 'Level', 'logs.lvl.all': 'All levels', 'logs.lvl.warn': 'Warnings+', 'logs.lvl.err': 'Errors only',
  'logs.pause': 'Pause', 'logs.resume': 'Resume', 'logs.copy': 'Copy', 'logs.download': 'Download', 'logs.clear': 'Clear', 'logs.copied': '{n} lines copied',
  'logs.live': 'Live', 'logs.paused': 'Paused, new lines are held', 'logs.connecting': 'Connecting…', 'logs.reconnecting': 'Disconnected, reconnecting…', 'logs.count': '{n} of {total} lines', 'logs.empty': 'No log output for this source yet.', 'logs.nomatch': 'No lines match the filter.',

  'backup.title': 'Backup', 'backup.desc': 'Back up the data folder (both editions) and restore it when needed.', 'backup.create': 'Create backup', 'backup.creating': 'Creating backup…', 'backup.created': 'Backup created', 'backup.fail': 'Backup failed.',
  'backup.list': 'Backups', 'backup.auto': 'automatic', 'backup.empty.title': 'No backups yet', 'backup.empty.desc': 'Create a first backup to keep your data safe before upgrades.', 'backup.download': 'Download', 'backup.restore': 'Restore',
  'backup.restore.title': 'Restore backup?', 'backup.restore.body': 'Data will be restored from {name}.', 'backup.restore.warn': 'Current data will be overwritten and the app is stopped temporarily. Create a new backup first if needed.', 'backup.restore.check': 'I understand current data will be overwritten',
  'backup.restoring': 'Restoring backup…', 'backup.restored': 'Backup restored', 'backup.restore.fail': 'Restore failed.',

  'upd.title': 'Updates', 'upd.desc': 'Check for NextMON Server updates and see a runtime update summary.', 'upd.check': 'Check for updates', 'upd.app': 'NextMON Server', 'upd.current': 'Installed version', 'upd.latest': 'Latest',
  'upd.available': 'Version {v} is available', 'upd.notes': 'Release notes', 'upd.apply': 'Update now', 'upd.release': 'View release', 'upd.uptodate': 'You are on the latest version.', 'upd.uptodate.short': 'Up to date', 'upd.found': 'Version {v} is available',
  'upd.fail': 'Could not check for updates.', 'upd.auto': 'Check automatically on start', 'upd.channel': 'Release channel', 'upd.ch.stable': 'Stable', 'upd.ch.beta': 'Beta',
  'upd.apply.title': 'Update to v{v}?', 'upd.apply.body': 'The update is downloaded and installed. These steps run:', 'upd.apply.a': 'An automatic backup is created before updating.', 'upd.apply.b': 'The app stops briefly and NextMON Server restarts.', 'upd.apply.c': 'Your data and settings are not overwritten.',
  'upd.applying': 'Updating…', 'upd.restarting': 'Update finished. The server is restarting and the page reloads automatically.', 'upd.apply.fail': 'Update failed.', 'upd.runtimes': 'Runtime updates', 'upd.manage': 'Manage runtimes', 'upd.rt.cur': 'In use',

  'diag.title': 'Diagnostics', 'diag.desc': 'Check system requirements and configuration. Some problems can be fixed automatically.', 'diag.run': 'Run again', 'diag.pass': 'Passed', 'diag.warn': 'Warnings', 'diag.fail': 'Failed',
  'diag.c.vcredist': 'Microsoft Visual C++ Runtime', 'diag.c.ports': 'No port conflicts', 'diag.c.php-ext': 'Required PHP extensions', 'diag.c.php-ext-intl': 'Required PHP extensions', 'diag.c.data-write': 'Data folder is writable', 'diag.c.webview2': 'WebView2 Runtime', 'diag.c.node-min': 'Node.js version meets Next.js requirement',
  'diag.fix': 'Fix', 'diag.fixed': 'Fix applied', 'diag.fixfail': 'Fix failed.', 'diag.empty': 'No check results.',

  'settings.title': 'Settings', 'settings.desc': 'Changes are saved automatically.', 'settings.appearance': 'Appearance', 'settings.language': 'Language', 'settings.language.help': 'Panel interface language.', 'settings.theme.help': 'Auto follows your system setting.',
  'settings.startup': 'Startup', 'settings.autostart': 'Start with Windows', 'settings.autostart.help': 'Adds a per-user Run key. You can turn it off at any time.', 'settings.minimized': 'Start minimized', 'settings.minimized.help': 'Go straight to the tray without opening the panel window.',
  'settings.autoedition': 'Auto-start the active edition', 'settings.autoedition.help': 'The active edition runs when NextMON Server opens.', 'settings.openbrowser': 'Open browser on start', 'settings.openbrowser.help': 'Opens the app in your browser once it is running.',
  'settings.restartcrash': 'Restart on crash', 'settings.restartcrash.help': 'Relaunches the app automatically with increasing delay.',
  'settings.security': 'Security', 'settings.token': 'Control token', 'settings.token.help': 'Used by the panel and CLI to reach the local API. Keep it private.', 'settings.token.show': 'Show token', 'settings.token.hide': 'Hide token', 'settings.token.copy': 'Copy token',
  'settings.token.regen': 'Generate new token', 'settings.token.regen.title': 'Generate a new token?', 'settings.token.regen.body': 'The old token stops working immediately. The CLI and other panel tabs must use the new one.', 'settings.token.regen.ok': 'New token generated',
  'settings.storage': 'Storage', 'settings.datadir': 'Data folder', 'settings.datadir.help': 'Each edition stores its data separately in the php and next subfolders.', 'settings.open.data': 'Open data folder', 'settings.open.logs': 'Open logs folder',
  'settings.retention': 'Log retention', 'settings.days': 'days', 'settings.retention.help': 'Logs older than this are deleted automatically.', 'settings.ret.bad': 'Enter 1 to 365 days.',

  'about.title': 'About', 'about.tag': 'Local launcher for NextMON', 'about.body': 'NextMON Server runs the PHP or Next.js edition of NextMON on your computer, with a runtime manager, backups and updates.',
  'about.releases': 'Releases', 'about.issues': 'Report an issue', 'about.site': 'Website', 'about.info': 'Information', 'about.version': 'Version', 'about.license': 'License', 'about.commit': 'Commit', 'about.built': 'Build date', 'about.os': 'System', 'about.ram': 'RAM', 'about.disk': 'Disk', 'about.free': 'free', 'about.panelport': 'Panel port',

  'wiz.title': 'Welcome to NextMON Server', 'wiz.steps': 'Setup steps', 'wiz.skip': 'Skip', 'wiz.recommended': 'Recommended',
  'wiz.s.edition': 'Edition', 'wiz.s.version': 'Version', 'wiz.s.network': 'Network', 'wiz.s.install': 'Install',
  'wiz.edition.title': 'Choose an edition', 'wiz.edition.desc': 'Pick the NextMON edition to run. You can switch at any time.', 'wiz.ed.php': 'Light and simple. Best for beginners and modest computers.', 'wiz.ed.next': 'For realtime features and a modern interface. Requires Node.js.', 'wiz.ed.note': 'Only one edition runs at a time and their data is kept separate.',
  'wiz.version.title': 'Choose a runtime version', 'wiz.version.desc': 'The latest stable version is preselected. Missing versions are downloaded.', 'wiz.ver.have': 'Already installed, no download needed.', 'wiz.ver.download': 'About {mb} will be downloaded.',
  'wiz.network.title': 'Port and network access', 'wiz.network.desc': 'Set the port and who may reach the app.', 'wiz.net.local': 'Reachable from this computer only. Safest.', 'wiz.net.lan': 'Reachable from phones or other computers on the same network.',
  'wiz.install': 'Install and finish', 'wiz.install.title': 'Installing', 'wiz.install.desc': 'Downloading and preparing the runtime. Keep this window open.', 'wiz.install.fail': 'Installation failed', 'wiz.applying': 'Applying settings…',
  'wiz.done.title': 'Ready to go', 'wiz.done.body': '{ed} with runtime {v} is ready on port {port}.', 'wiz.gotodash': 'Go to dashboard', 'wiz.openapp': 'Start and open the app',
};

const DICT = { id, en };
let cur = document.documentElement.lang === 'en' ? 'en' : 'id';
const subs = new Set();
export const lang = () => cur;
export function setLang(l) {
  if (!DICT[l] || l === cur) return;
  cur = l; document.documentElement.lang = l; subs.forEach((f) => f(l));
}
export const onLang = (fn) => { subs.add(fn); return () => subs.delete(fn); };
export const has = (key) => key in DICT[cur];
export function t(key, params) {
  let s = DICT[cur][key] ?? DICT.id[key] ?? key;
  if (params) s = s.replace(/\{(\w+)\}/g, (m, k) => (params[k] ?? m));
  return s;
}
export const _dict = DICT;

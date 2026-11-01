use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("no checksum available for {0}; refusing to install unverified archive")]
    NoChecksum(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("another operation on {0} runtimes is already in progress")]
    Busy(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0} is already installed")]
    AlreadyInstalled(String),
    #[error("runtime {0} is in use")]
    InUse(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("unsafe path in archive: {0}")]
    UnsafeArchivePath(String),
    #[error("archive error: {0}")]
    Archive(String),
    #[error("health check failed: {0}")]
    Health(String),
    #[error("io error: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Network(e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e.to_string())
    }
}
impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::Archive(e.to_string())
    }
}

/// Inner API error: `{ "code", "message" (en), "messageId" (id) }`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    pub message_id: String,
}

/// Wire shape of errors: `{ "error": { "code", "message", "messageId" } }`.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorBody {
    pub error: ApiError,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Network(_) => "network",
            Error::Parse(_) => "parse",
            Error::ChecksumMismatch { .. } => "checksum_mismatch",
            Error::NoChecksum(_) => "no_checksum",
            Error::Cancelled => "cancelled",
            Error::Busy(_) => "busy",
            Error::NotFound(_) => "not_found",
            Error::AlreadyInstalled(_) => "already_installed",
            Error::InUse(_) => "in_use",
            Error::Unsupported(_) => "unsupported",
            Error::UnsafeArchivePath(_) => "unsafe_archive",
            Error::Archive(_) => "archive",
            Error::Health(_) => "health",
            Error::Io(_) => "io",
        }
    }

    pub fn to_api(&self) -> ApiErrorBody {
        let message_id = match self {
            Error::Network(m) => format!("Kesalahan jaringan: {m}"),
            Error::Parse(m) => format!("Gagal membaca data: {m}"),
            Error::ChecksumMismatch { .. } => "Checksum unduhan tidak cocok; berkas dibuang.".into(),
            Error::NoChecksum(v) => format!("Tidak ada checksum untuk {v}; pemasangan tanpa verifikasi ditolak."),
            Error::Cancelled => "Operasi dibatalkan.".into(),
            Error::Busy(k) => format!("Operasi runtime {k} lain sedang berjalan."),
            Error::NotFound(m) => format!("Tidak ditemukan: {m}"),
            Error::AlreadyInstalled(v) => format!("{v} sudah terpasang."),
            Error::InUse(v) => format!("Runtime {v} sedang dipakai; hentikan aplikasinya dulu."),
            Error::Unsupported(m) => format!("Tidak didukung: {m}"),
            Error::UnsafeArchivePath(p) => format!("Jalur berbahaya di dalam arsip: {p}"),
            Error::Archive(m) => format!("Kesalahan arsip: {m}"),
            Error::Health(m) => format!("Pemeriksaan kesehatan gagal: {m}"),
            Error::Io(m) => format!("Kesalahan berkas: {m}"),
        };
        ApiErrorBody { error: ApiError { code: self.code(), message: self.to_string(), message_id } }
    }
}

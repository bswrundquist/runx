use std::fmt;
use std::io;

#[derive(Debug)]
pub enum RunxError {
    InvalidRepoRef(String),
    Git { op: &'static str, detail: String },
    Cache(io::Error),
    TrustDeclined,
    Offline(String),
    ProcessStart(io::Error),
    Release(String),
    Io(io::Error),
}

impl fmt::Display for RunxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRepoRef(s) => write!(f, "invalid repo reference: {s}"),
            Self::Git { op, detail } => write!(f, "{op}: {detail}"),
            Self::Cache(e) => write!(f, "cache: {e}"),
            Self::TrustDeclined => write!(f, "aborted by user"),
            Self::Offline(s) => write!(f, "{s}"),
            Self::ProcessStart(e) => write!(f, "starting process: {e}"),
            Self::Release(s) => write!(f, "release: {s}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunxError {}

impl From<io::Error> for RunxError {
    fn from(e: io::Error) -> Self {
        RunxError::Io(e)
    }
}

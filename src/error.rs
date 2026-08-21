use std::fmt;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotADirectory(String),
    NoTextFiles,
    PathTraversal {
        path: String,
    },
    InvalidBoundary,
    ContainerParse {
        message: String,
    },
    MissingHeader {
        header: String,
    },
    InvalidHeader {
        header: String,
        value: String,
    },
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    IntegrityMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    DuplicatePath {
        path: String,
    },
    FileExists {
        path: String,
    },
    #[cfg(windows)]
    Registry(String),
    #[cfg(not(windows))]
    Platform(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {e}"),
            Error::NotADirectory(p) => write!(f, "not a directory: {p}"),
            Error::NoTextFiles => write!(f, "no text files found"),
            Error::PathTraversal { path } => write!(f, "path traversal rejected: {path}"),
            Error::InvalidBoundary => write!(f, "invalid boundary in container"),
            Error::ContainerParse { message } => write!(f, "container parse error: {message}"),
            Error::MissingHeader { header } => write!(f, "missing header: {header}"),
            Error::InvalidHeader { header, value } => {
                write!(f, "invalid header {header}: {value}")
            }
            Error::SizeMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "size mismatch for {path}: expected {expected} bytes, got {actual}"
            ),
            Error::IntegrityMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "integrity mismatch for {path}: expected sha256 {expected}, got {actual}"
            ),
            Error::DuplicatePath { path } => write!(f, "duplicate path in container: {path}"),
            Error::FileExists { path } => write!(f, "output file already exists: {path}"),
            #[cfg(windows)]
            Error::Registry(msg) => write!(f, "registry error: {msg}"),
            #[cfg(not(windows))]
            Error::Platform(msg) => write!(f, "platform error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

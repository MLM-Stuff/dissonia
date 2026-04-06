use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidArgument(&'static str),
    Unsupported(&'static str),
    InvalidState(&'static str),
    Io(std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(message) => write!(f, "invalid argument: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported operation: {message}"),
            Self::InvalidState(message) => write!(f, "invalid state: {message}"),
            Self::Io(error) => write!(f, "i/o error: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidArgument(_) | Self::Unsupported(_) | Self::InvalidState(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

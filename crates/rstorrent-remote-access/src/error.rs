use std::error::Error;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum RemoteAccessError {
    AuthenticationFailed,
    Capacity(&'static str),
    Conflict(&'static str),
    Corrupt(&'static str),
    Crypto(rstorrent_remote_crypto::RemoteCryptoError),
    Expired,
    InvalidInput(&'static str),
    Io(io::Error),
    NotFound,
    PersistenceUnsupported,
    SimulatedCrash(&'static str),
}

impl fmt::Display for RemoteAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationFailed => formatter.write_str("remote authentication failed"),
            Self::Capacity(limit) => write!(formatter, "remote access {limit} capacity reached"),
            Self::Conflict(reason) => write!(formatter, "remote access conflict: {reason}"),
            Self::Corrupt(reason) => {
                write!(formatter, "remote authority record is invalid: {reason}")
            }
            Self::Crypto(_) => formatter.write_str("remote cryptographic operation failed"),
            Self::Expired => formatter.write_str("remote authorization expired"),
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid remote access input: {reason}")
            }
            Self::Io(error) => write!(formatter, "remote authority persistence: {error}"),
            Self::NotFound => formatter.write_str("remote authorization was not found"),
            Self::PersistenceUnsupported => formatter.write_str(
                "protected remote authority persistence is unsupported on this platform",
            ),
            Self::SimulatedCrash(point) => write!(formatter, "simulated crash at {point}"),
        }
    }
}

impl Error for RemoteAccessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Crypto(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for RemoteAccessError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rstorrent_remote_crypto::RemoteCryptoError> for RemoteAccessError {
    fn from(error: rstorrent_remote_crypto::RemoteCryptoError) -> Self {
        Self::Crypto(error)
    }
}

pub type Result<T> = std::result::Result<T, RemoteAccessError>;

use std::fmt;

#[derive(Debug)]
pub enum RemoteHostError {
    Authority(rstorrent_remote_access::RemoteAccessError),
    Configuration(&'static str),
    Gateway,
    Io(std::io::Error),
    Protocol,
    Relay,
}

impl fmt::Display for RemoteHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authority(error) => write!(formatter, "remote authority: {error}"),
            Self::Configuration(message) => {
                write!(formatter, "remote host configuration: {message}")
            }
            Self::Gateway => formatter.write_str("local application connection failed"),
            Self::Io(error) => write!(formatter, "remote host IO: {error}"),
            Self::Protocol => formatter.write_str("remote authentication failed"),
            Self::Relay => formatter.write_str("remote relay unavailable"),
        }
    }
}

impl std::error::Error for RemoteHostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Authority(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rstorrent_remote_access::RemoteAccessError> for RemoteHostError {
    fn from(error: rstorrent_remote_access::RemoteAccessError) -> Self {
        Self::Authority(error)
    }
}

impl From<std::io::Error> for RemoteHostError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, RemoteHostError>;

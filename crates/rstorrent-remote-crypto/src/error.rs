use core::fmt;

/// Errors intentionally avoid carrying passwords, keys, plaintexts, or
/// dependency error strings across the public boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteCryptoError {
    InvalidUsername,
    InvalidPassphrase,
    InvalidIdentifier,
    InvalidMessage,
    AuthenticationFailed,
    HostIdentityChanged,
    RandomnessUnavailable,
    KeyDerivationFailed,
    RecordTooLarge,
    InvalidRecord,
    RecordSequenceExhausted,
    ChannelClosed,
}

impl fmt::Display for RemoteCryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidUsername => "invalid username",
            Self::InvalidPassphrase => "invalid passphrase",
            Self::InvalidIdentifier => "invalid relay or host identifier",
            Self::InvalidMessage => "invalid protocol message",
            Self::AuthenticationFailed => "authentication failed",
            Self::HostIdentityChanged => "host identity changed",
            Self::RandomnessUnavailable => "secure randomness is unavailable",
            Self::KeyDerivationFailed => "key derivation failed",
            Self::RecordTooLarge => "encrypted record is too large",
            Self::InvalidRecord => "invalid encrypted record",
            Self::RecordSequenceExhausted => "encrypted record sequence exhausted",
            Self::ChannelClosed => "encrypted channel is closed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for RemoteCryptoError {}

pub type Result<T> = core::result::Result<T, RemoteCryptoError>;

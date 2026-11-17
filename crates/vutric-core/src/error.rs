/// Errors produced while decoding or validating protocol data.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("frame of {len} bytes exceeds the {max}-byte limit")]
    FrameTooLarge { len: usize, max: usize },
    #[error("malformed message: {0}")]
    Malformed(String),
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u16),
    #[error("invalid pairing ticket: {0}")]
    InvalidTicket(&'static str),
    #[error("preview is {len} bytes, the limit is {max}")]
    PreviewTooLarge { len: usize, max: usize },
}

pub type Result<T, E = Error> = core::result::Result<T, E>;

impl From<postcard::Error> for Error {
    fn from(err: postcard::Error) -> Self {
        Self::Malformed(err.to_string())
    }
}

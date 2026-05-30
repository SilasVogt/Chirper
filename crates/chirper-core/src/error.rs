use std::fmt::{Display, Formatter};

pub type ChirperResult<T> = Result<T, ChirperError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChirperError {
    Audio(String),
    Transcription(String),
    Formatting(String),
    Insertion(String),
    Configuration(String),
    Unsupported(String),
}

impl Display for ChirperError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Audio(message) => write!(f, "audio error: {message}"),
            Self::Transcription(message) => write!(f, "transcription error: {message}"),
            Self::Formatting(message) => write!(f, "formatting error: {message}"),
            Self::Insertion(message) => write!(f, "insertion error: {message}"),
            Self::Configuration(message) => write!(f, "configuration error: {message}"),
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

impl std::error::Error for ChirperError {}

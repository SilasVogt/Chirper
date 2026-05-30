use std::path::PathBuf;

use crate::{ChirperResult, DictationMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAudio {
    pub path: PathBuf,
    pub sample_rate_hz: u32,
    pub channels: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertionTarget {
    pub app_id: Option<String>,
    pub window_title: Option<String>,
}

pub trait AudioSource {
    fn start_recording(&mut self) -> ChirperResult<()>;
    fn stop_recording(&mut self) -> ChirperResult<CapturedAudio>;
}

pub trait AsrEngine {
    fn transcribe(&self, audio: &CapturedAudio) -> ChirperResult<Transcript>;
}

pub trait Formatter {
    fn format(&self, transcript: &Transcript, mode: DictationMode) -> ChirperResult<String>;
}

pub trait TextInserter {
    fn insert(&self, text: &str, target: Option<&InsertionTarget>) -> ChirperResult<()>;
}

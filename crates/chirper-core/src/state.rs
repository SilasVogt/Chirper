#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowState {
    Idle,
    Recording,
    Transcribing,
    Formatting,
    Inserting,
    Error,
}

impl Default for WorkflowState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationMode {
    Auto,
    Standard,
    Email,
    Command,
    Code,
}

impl Default for DictationMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceCommand {
    Toggle,
    StartRecording,
    StopRecording,
    SetMode(DictationMode),
    OpenSettings,
    GetStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowEvent {
    StateChanged(WorkflowState),
    TranscriptReady(String),
    TextInserted,
    Error(String),
}

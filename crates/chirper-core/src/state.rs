#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkflowState {
    #[default]
    Idle,
    Recording,
    Transcribing,
    Formatting,
    Inserting,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DictationMode {
    #[default]
    Auto,
    Standard,
    Email,
    Command,
    Code,
}

impl DictationMode {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Standard => "standard",
            Self::Email => "email",
            Self::Command => "command",
            Self::Code => "code",
        }
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

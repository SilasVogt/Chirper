use std::{
    fs,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
};

use chirper_api::{
    default_socket_path, ApiRequest, ApiResponse, AudioCaptureKind, AudioCaptureTarget,
};
use chirper_asr_whispercpp::{WhisperCppAsr, WhisperCppOptions};
use chirper_audio_pipewire::{PipeWireRecorder, PipeWireRecorderOptions};
use chirper_core::{
    AsrEngine, AudioSource, ChirperConfig, FormatterBackend, TextInserter, Transcript,
    WorkflowState,
};
use chirper_formatter_codex::{CodexFormatter, CodexOptions};
use chirper_formatter_ollama::{OllamaFormatter, OllamaOptions};
use chirper_formatter_rules::{format_spoken_rules_with_vocabulary, learn_spelling_vocabulary};
use chirper_insertion_clipboard::ClipboardInserter;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let socket_path = default_socket_path();
    prepare_socket_path(&socket_path)?;

    let listener = UnixListener::bind(&socket_path)
        .map_err(|source| format!("failed to bind {}: {source}", socket_path.display()))?;
    let mut state = DaemonState::default();

    println!("chirper-daemon listening on {}", socket_path.display());

    for connection in listener.incoming() {
        let stream = match connection {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("failed to accept API connection: {error}");
                continue;
            }
        };

        if handle_connection(stream, &mut state) {
            break;
        }
    }

    let _ = fs::remove_file(&socket_path);
    Ok(())
}

fn prepare_socket_path(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|source| format!("failed to create {}: {source}", parent.display()))?;
    }

    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(format!(
                "daemon socket already appears active at {}",
                path.display()
            ));
        }

        fs::remove_file(path)
            .map_err(|source| format!("failed to remove stale {}: {source}", path.display()))?;
    }

    Ok(())
}

fn handle_connection(mut stream: UnixStream, state: &mut DaemonState) -> bool {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let response = ApiResponse::error(state_name(state.workflow), error);
            let _ = write_response(&mut stream, &response);
            return false;
        }
    };

    let (response, should_shutdown) = handle_request(request, state);
    if let Err(error) = write_response(&mut stream, &response) {
        eprintln!("{error}");
    }

    should_shutdown
}

fn read_request(stream: &mut UnixStream) -> Result<ApiRequest, String> {
    let mut payload = String::new();
    stream
        .read_to_string(&mut payload)
        .map_err(|source| format!("failed to read API request: {source}"))?;

    if payload.trim().is_empty() {
        return Err("empty API request".to_string());
    }

    serde_json::from_str(payload.trim())
        .map_err(|source| format!("failed to decode API request: {source}"))
}

fn write_response(stream: &mut UnixStream, response: &ApiResponse) -> Result<(), String> {
    let mut payload = serde_json::to_vec(response)
        .map_err(|source| format!("failed to encode API response: {source}"))?;
    payload.push(b'\n');

    stream
        .write_all(&payload)
        .map_err(|source| format!("failed to write API response: {source}"))
}

fn handle_request(request: ApiRequest, state: &mut DaemonState) -> (ApiResponse, bool) {
    let should_shutdown = matches!(request, ApiRequest::Shutdown);
    let response = match request {
        ApiRequest::Status => status_response(state),
        ApiRequest::Toggle { audio } => {
            if state.workflow == WorkflowState::Recording {
                stop_recording(state)
            } else {
                start_recording(state, audio)
            }
        }
        ApiRequest::StartRecording { audio } => start_recording(state, audio),
        ApiRequest::StopRecording => stop_recording(state),
        ApiRequest::Shutdown => ApiResponse::ok(state_name(state.workflow), "daemon shutting down"),
    };

    (response, should_shutdown)
}

#[derive(Debug)]
struct DaemonState {
    workflow: WorkflowState,
    recorder: Option<PipeWireRecorder>,
    active_audio: Option<ActiveAudioTarget>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            workflow: WorkflowState::Idle,
            recorder: None,
            active_audio: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ActiveAudioTarget {
    target: Option<String>,
    label: String,
}

fn status_response(state: &DaemonState) -> ApiResponse {
    let mut response = ApiResponse::ok(state_name(state.workflow), "daemon ready");
    response.recording_path = active_recording_path(state);
    apply_active_audio(&mut response, state.active_audio.as_ref());
    response
}

fn start_recording(
    state: &mut DaemonState,
    requested_audio: Option<AudioCaptureTarget>,
) -> ApiResponse {
    if state.workflow != WorkflowState::Idle {
        return ApiResponse::error(
            state_name(state.workflow),
            format!(
                "cannot start recording while state is {}",
                state_name(state.workflow)
            ),
        );
    }

    let config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            state.workflow = WorkflowState::Idle;
            return ApiResponse::error(state_name(state.workflow), error.to_string());
        }
    };
    let active_audio = resolve_audio_target(&config, requested_audio);
    let mut options = PipeWireRecorderOptions::from_config(&config);
    options.target = active_audio.target.clone();

    let mut recorder = PipeWireRecorder::new(options);
    if let Err(error) = recorder.start_recording() {
        state.workflow = WorkflowState::Idle;
        state.active_audio = None;
        return ApiResponse::error(state_name(state.workflow), error.to_string());
    }

    let recording_path = recorder
        .active_path()
        .map(|path| path.display().to_string());
    state.recorder = Some(recorder);
    state.active_audio = Some(active_audio);
    state.workflow = WorkflowState::Recording;

    let mut response = ApiResponse::ok(state_name(state.workflow), "recording started");
    response.recording_path = recording_path;
    apply_active_audio(&mut response, state.active_audio.as_ref());
    response
}

fn stop_recording(state: &mut DaemonState) -> ApiResponse {
    if state.workflow != WorkflowState::Recording {
        return ApiResponse::error(
            state_name(state.workflow),
            format!(
                "cannot stop recording while state is {}",
                state_name(state.workflow)
            ),
        );
    }

    let Some(mut recorder) = state.recorder.take() else {
        state.workflow = WorkflowState::Idle;
        state.active_audio = None;
        return ApiResponse::error(state_name(state.workflow), "recording state was missing");
    };

    let audio = match recorder.stop_recording() {
        Ok(audio) => audio,
        Err(error) => {
            state.workflow = WorkflowState::Idle;
            state.active_audio = None;
            return ApiResponse::error(state_name(state.workflow), error.to_string());
        }
    };
    let recording_path = Some(audio.path.display().to_string());
    let active_audio = state.active_audio.clone();
    let mut config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            state.workflow = WorkflowState::Idle;
            let mut response = ApiResponse::error(state_name(state.workflow), error.to_string());
            response.recording_path = recording_path;
            apply_active_audio(&mut response, active_audio.as_ref());
            state.active_audio = None;
            return response;
        }
    };

    state.workflow = WorkflowState::Transcribing;
    let transcript = match transcribe_audio(&config, &audio) {
        Ok(transcript) => transcript,
        Err(error) => {
            state.workflow = WorkflowState::Idle;
            let mut response = ApiResponse::error(state_name(state.workflow), error);
            response.recording_path = recording_path;
            apply_active_audio(&mut response, active_audio.as_ref());
            state.active_audio = None;
            return response;
        }
    };
    learn_vocabulary_from_transcript(&mut config, &transcript);

    state.workflow = WorkflowState::Formatting;
    let formatted = match format_transcript(&config, &transcript) {
        Ok(formatted) => formatted,
        Err(error) => {
            state.workflow = WorkflowState::Idle;
            let mut response = ApiResponse::error(state_name(state.workflow), error);
            response.recording_path = recording_path;
            response.transcript = Some(transcript.text);
            apply_active_audio(&mut response, active_audio.as_ref());
            state.active_audio = None;
            return response;
        }
    };

    if formatted.trim().is_empty() {
        state.workflow = WorkflowState::Idle;
        let mut response = ApiResponse::ok(state_name(state.workflow), "no speech detected");
        response.recording_path = recording_path;
        response.transcript = Some(transcript.text);
        response.formatted = Some(formatted);
        apply_active_audio(&mut response, active_audio.as_ref());
        state.active_audio = None;
        return response;
    }

    state.workflow = WorkflowState::Inserting;
    if let Err(error) = copy_text(&formatted) {
        state.workflow = WorkflowState::Idle;
        let mut response = ApiResponse::error(state_name(state.workflow), error);
        response.recording_path = recording_path;
        response.transcript = Some(transcript.text);
        response.formatted = Some(formatted);
        apply_active_audio(&mut response, active_audio.as_ref());
        state.active_audio = None;
        return response;
    }

    state.workflow = WorkflowState::Idle;
    state.active_audio = None;
    let mut response =
        ApiResponse::ok(state_name(state.workflow), "transcript copied to clipboard");
    response.recording_path = recording_path;
    response.transcript = Some(transcript.text);
    response.formatted = Some(formatted);
    response.copied = true;
    apply_active_audio(&mut response, active_audio.as_ref());
    response
}

fn resolve_audio_target(
    config: &ChirperConfig,
    requested_audio: Option<AudioCaptureTarget>,
) -> ActiveAudioTarget {
    if let Some(audio) = requested_audio {
        let fallback = match audio.kind {
            AudioCaptureKind::Input => "Microphone".to_string(),
            AudioCaptureKind::ScreenAudio => "Screen audio".to_string(),
        };
        return ActiveAudioTarget {
            target: audio.target.filter(|target| !target.trim().is_empty()),
            label: audio.label.unwrap_or(fallback),
        };
    }

    ActiveAudioTarget {
        target: config.pipewire_target.clone(),
        label: config
            .pipewire_target
            .as_ref()
            .map(|target| format!("Input: {target}"))
            .unwrap_or_else(|| "Default microphone".to_string()),
    }
}

fn apply_active_audio(response: &mut ApiResponse, active_audio: Option<&ActiveAudioTarget>) {
    if let Some(active_audio) = active_audio {
        response.audio_target = active_audio.target.clone();
        response.audio_label = Some(active_audio.label.clone());
    }
}

fn learn_vocabulary_from_transcript(config: &mut ChirperConfig, transcript: &Transcript) {
    for entry in learn_spelling_vocabulary(&transcript.text) {
        if let Err(error) =
            ChirperConfig::save_default_vocabulary_entry(&entry.spoken, &entry.written)
        {
            eprintln!(
                "failed to save vocabulary entry `{}` -> `{}`: {error}",
                entry.spoken, entry.written
            );
            continue;
        }

        if let Some(existing) = config
            .vocabulary
            .iter_mut()
            .find(|existing| existing.spoken == entry.spoken)
        {
            existing.written = entry.written;
        } else {
            config.vocabulary.push(entry);
        }
    }
}

fn transcribe_audio(
    config: &ChirperConfig,
    audio: &chirper_core::CapturedAudio,
) -> Result<Transcript, String> {
    let options = WhisperCppOptions::from_config(config).map_err(|error| error.to_string())?;
    let asr = WhisperCppAsr::new(options);

    asr.transcribe(audio).map_err(|error| error.to_string())
}

fn format_transcript(config: &ChirperConfig, transcript: &Transcript) -> Result<String, String> {
    match config.formatter_backend {
        FormatterBackend::None => Ok(transcript.text.clone()),
        FormatterBackend::Rules => format_with_rules(config, transcript),
        FormatterBackend::Ollama => {
            let preformatted = format_with_rules(config, transcript)?;
            OllamaFormatter::new(OllamaOptions::from_config(config))
                .format_with_context(transcript, &preformatted, config.dictation_mode)
                .map_err(|error| error.to_string())
        }
        FormatterBackend::Codex => {
            let preformatted = format_with_rules(config, transcript)?;
            CodexFormatter::new(CodexOptions::from_config(config))
                .format_with_context(transcript, &preformatted, config.dictation_mode)
                .map_err(|error| error.to_string())
        }
        FormatterBackend::LlamaCpp => {
            eprintln!(
                "formatter backend {:?} is not implemented yet; using raw transcript",
                config.formatter_backend
            );
            Ok(transcript.text.clone())
        }
    }
}

fn format_with_rules(config: &ChirperConfig, transcript: &Transcript) -> Result<String, String> {
    Ok(format_spoken_rules_with_vocabulary(
        &transcript.text,
        config.dictation_mode,
        &config.vocabulary,
    ))
}

fn copy_text(text: &str) -> Result<(), String> {
    let inserter = ClipboardInserter::detect().map_err(|error| error.to_string())?;
    inserter
        .insert(text, None)
        .map_err(|error| error.to_string())
}

fn active_recording_path(state: &DaemonState) -> Option<String> {
    state
        .recorder
        .as_ref()
        .and_then(PipeWireRecorder::active_path)
        .map(|path| path.display().to_string())
}

fn state_name(state: WorkflowState) -> &'static str {
    match state {
        WorkflowState::Idle => "idle",
        WorkflowState::Recording => "recording",
        WorkflowState::Transcribing => "transcribing",
        WorkflowState::Formatting => "formatting",
        WorkflowState::Inserting => "inserting",
        WorkflowState::Error => "error",
    }
}

use std::{
    env, fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chirper_api::{send_request, ApiRequest, ApiResponse};
use chirper_asr_whispercpp::{WhisperCppAsr, WhisperCppOptions};
use chirper_audio_pipewire::{DetachedRecording, PipeWireRecorder, PipeWireRecorderOptions};
use chirper_core::{
    AsrEngine, AudioSource, ChirperConfig, DictationMode, Formatter, FormatterBackend,
    ServiceCommand, TextInserter, WorkflowState,
};
use chirper_formatter_rules::RuleFormatter;
use chirper_insertion_clipboard::ClipboardInserter;
use chirper_platform::{PlatformDiagnostics, RuntimeDiagnostics};

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    if let Some(request) = parse_daemon_request(first.as_deref()) {
        call_daemon(request);
        return;
    }

    if matches!(first.as_deref(), Some("record-test")) {
        record_test(args.next().as_deref());
        return;
    }

    if matches!(first.as_deref(), Some("transcribe-file")) {
        transcribe_file(args.next(), args.next());
        return;
    }

    if matches!(first.as_deref(), Some("diagnose")) {
        diagnose();
        return;
    }

    if matches!(first.as_deref(), Some("copy-test")) {
        copy_test(args.collect::<Vec<_>>().join(" "));
        return;
    }

    if matches!(first.as_deref(), Some("format-test")) {
        format_test(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("dictate-test")) {
        dictate_test(args.next().as_deref());
        return;
    }

    let command = parse_command(first.into_iter().chain(args));

    match command {
        ServiceCommand::GetStatus => print_status(),
        ServiceCommand::Toggle => toggle(),
        ServiceCommand::StartRecording => {
            println!("start recording requested");
            println!("daemon control is not implemented yet");
        }
        ServiceCommand::StopRecording => {
            println!("stop recording requested");
            println!("daemon control is not implemented yet");
        }
        ServiceCommand::SetMode(mode) => {
            println!("mode change requested: {mode:?}");
            println!("daemon control is not implemented yet");
        }
        ServiceCommand::OpenSettings => {
            println!("settings app is not implemented yet");
        }
    }
}

fn parse_daemon_request(command: Option<&str>) -> Option<ApiRequest> {
    match command {
        Some("daemon-status") => Some(ApiRequest::Status),
        Some("daemon-toggle") => Some(ApiRequest::Toggle),
        Some("daemon-start") => Some(ApiRequest::StartRecording),
        Some("daemon-stop") => Some(ApiRequest::StopRecording),
        Some("daemon-shutdown") => Some(ApiRequest::Shutdown),
        _ => None,
    }
}

fn call_daemon(request: ApiRequest) {
    let response = match send_request(&request) {
        Ok(response) => response,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    print_api_response(&response);

    if !response.ok {
        std::process::exit(1);
    }
}

fn print_api_response(response: &ApiResponse) {
    println!("state: {}", response.state);
    println!("message: {}", response.message);

    if let Some(path) = &response.recording_path {
        println!("recording_path: {path}");
    }

    if let Some(transcript) = &response.transcript {
        println!("transcript: {transcript}");
    }

    if let Some(formatted) = &response.formatted {
        println!("formatted: {formatted}");
    }

    println!("copied: {}", response.copied);
}

fn parse_command(mut args: impl Iterator<Item = String>) -> ServiceCommand {
    match args.next().as_deref() {
        None | Some("status") => ServiceCommand::GetStatus,
        Some("toggle") => ServiceCommand::Toggle,
        Some("start") => ServiceCommand::StartRecording,
        Some("stop") => ServiceCommand::StopRecording,
        Some("settings") => ServiceCommand::OpenSettings,
        Some("mode") => parse_mode(args.next().as_deref()),
        Some(_) => ServiceCommand::GetStatus,
    }
}

fn record_test(seconds: Option<&str>) {
    let seconds = seconds
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3);

    let mut recorder = PipeWireRecorder::default();

    println!("recording for {seconds}s...");
    if let Err(error) = recorder.start_recording() {
        eprintln!("{error}");
        std::process::exit(1);
    }

    std::thread::sleep(Duration::from_secs(seconds));

    let audio = match recorder.stop_recording() {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    println!("recorded: {}", audio.path.display());
    println!("sample_rate_hz: {}", audio.sample_rate_hz);
    println!("channels: {}", audio.channels);
}

fn transcribe_file(audio_path: Option<String>, model_path: Option<String>) {
    let Some(audio_path) = audio_path else {
        eprintln!("usage: chirper transcribe-file <audio.wav> [model.bin]");
        std::process::exit(1);
    };

    let mut config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Some(model_path) = model_path {
        config.whispercpp_model_path = Some(model_path.into());
    }

    let options = match WhisperCppOptions::from_config(&config) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let asr = WhisperCppAsr::new(options);
    let audio = chirper_core::CapturedAudio {
        path: audio_path.into(),
        sample_rate_hz: 16_000,
        channels: 1,
    };

    let transcript = match asr.transcribe(&audio) {
        Ok(transcript) => transcript,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    println!("{}", transcript.text);
}

fn diagnose() {
    let config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let diagnostics = PlatformDiagnostics::detect();
    let runtime =
        RuntimeDiagnostics::detect(&config.whispercpp_command, config.whispercpp_model_path);

    println!("tools:");
    for tool in diagnostics.tools {
        let status = tool
            .path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".to_string());
        println!("  {}: {status}", tool.name);
    }

    println!("gpu:");
    println!("  amd_gpu_detected: {}", diagnostics.gpu.amd_gpu_detected);
    println!(
        "  render_node_detected: {}",
        diagnostics.gpu.render_node_detected
    );
    println!("  kfd_detected: {}", diagnostics.gpu.kfd_detected);
    println!(
        "  vulkan_loader_detected: {}",
        diagnostics.gpu.vulkan_loader_detected
    );
    println!(
        "  vulkan_radeon_detected: {}",
        diagnostics.gpu.vulkan_radeon_detected
    );
    println!(
        "  rocm_path_detected: {}",
        diagnostics.gpu.rocm_path_detected
    );
    println!(
        "  rocm_tool_detected: {}",
        diagnostics.gpu.rocm_tool_detected
    );
    println!(
        "  suggested_gpu_backend: {:?}",
        diagnostics.gpu.suggested_gpu_backend
    );

    println!("runtime:");
    print_path_status(&runtime.whispercpp_command);
    print_path_status(&runtime.whispercpp_model_path);
}

fn print_path_status(status: &chirper_platform::PathStatus) {
    let path = status
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unset>".to_string());
    println!("  {}: {} ({})", status.label, path, status.exists);
}

fn copy_test(text: String) {
    if text.is_empty() {
        eprintln!("usage: chirper copy-test <text>");
        std::process::exit(1);
    }

    let inserter = match ClipboardInserter::detect() {
        Ok(inserter) => inserter,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = inserter.insert(&text, None) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("copied {} bytes to clipboard", text.len());
}

fn format_test(args: Vec<String>) {
    let (mode, text) = parse_format_test_args(args);

    if text.is_empty() {
        eprintln!("usage: chirper format-test [--mode auto|standard|email|command|code] <text>");
        std::process::exit(1);
    }

    println!("{}", format_text(&text, mode));
}

fn dictate_test(seconds: Option<&str>) {
    let seconds = seconds
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3);

    let mut recorder = PipeWireRecorder::default();
    println!("recording for {seconds}s...");
    if let Err(error) = recorder.start_recording() {
        eprintln!("{error}");
        std::process::exit(1);
    }

    std::thread::sleep(Duration::from_secs(seconds));

    let audio = match recorder.stop_recording() {
        Ok(audio) => audio,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    println!("transcribing {}...", audio.path.display());
    let mode = configured_mode();
    let transcript = transcribe_audio(audio);
    let formatted = format_text(&transcript.text, mode);
    println!("transcript: {}", transcript.text);
    println!("formatted: {formatted}");
    if formatted.trim().is_empty() {
        println!("no speech detected");
        return;
    }

    let inserter = match ClipboardInserter::detect() {
        Ok(inserter) => inserter,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = inserter.insert(&formatted, None) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("copied transcript to clipboard");
}

fn toggle() {
    let state_path = toggle_state_path();

    if let Some(recording) = read_toggle_state(&state_path) {
        let audio = match PipeWireRecorder::stop_detached(&recording) {
            Ok(audio) => audio,
            Err(error) => {
                let _ = fs::remove_file(&state_path);
                eprintln!("{error}");
                std::process::exit(1);
            }
        };
        let _ = fs::remove_file(&state_path);

        println!("stopped recording: {}", audio.path.display());
        println!("transcribing...");
        let mode = configured_mode();
        let transcript = transcribe_audio(audio);
        let formatted = format_text(&transcript.text, mode);
        println!("transcript: {}", transcript.text);
        println!("formatted: {formatted}");
        if formatted.trim().is_empty() {
            println!("no speech detected");
            return;
        }

        let inserter = match ClipboardInserter::detect() {
            Ok(inserter) => inserter,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };

        if let Err(error) = inserter.insert(&formatted, None) {
            eprintln!("{error}");
            std::process::exit(1);
        }

        println!("copied transcript to clipboard");
        return;
    }

    let recording = match PipeWireRecorder::start_detached(PipeWireRecorderOptions::default()) {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = write_toggle_state(&state_path, &recording) {
        let _ = PipeWireRecorder::stop_detached(&recording);
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("started recording: {}", recording.audio.path.display());
}

fn parse_mode(value: Option<&str>) -> ServiceCommand {
    let mode = match value {
        Some("standard") => DictationMode::Standard,
        Some("email") => DictationMode::Email,
        Some("command") => DictationMode::Command,
        Some("code") => DictationMode::Code,
        _ => DictationMode::Auto,
    };

    ServiceCommand::SetMode(mode)
}

fn print_status() {
    let config_path = ChirperConfig::default_path();
    let config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let state = WorkflowState::default();

    println!("state: {state:?}");
    println!("config_path: {}", config_path.display());
    println!("audio_backend: {:?}", config.audio_backend);
    println!("asr_backend: {:?}", config.asr_backend);
    println!("gpu_backend: {:?}", config.gpu_backend);
    println!("formatter_backend: {:?}", config.formatter_backend);
    println!("insertion_backend: {:?}", config.insertion_backend);
    println!("dictation_mode: {:?}", config.dictation_mode);
    println!("whisper_model: {}", config.whisper_model);
    println!("whispercpp_command: {}", config.whispercpp_command);
    println!(
        "whispercpp_model_path: {}",
        config
            .whispercpp_model_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unset>".to_string())
    );
    println!(
        "whisper_language: {}",
        config.whisper_language.as_deref().unwrap_or("auto")
    );
}

fn parse_format_test_args(args: Vec<String>) -> (DictationMode, String) {
    let mut mode = configured_mode();
    let mut text = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if let Some(value) = arg.strip_prefix("--mode=") {
            mode = parse_mode_name(value).unwrap_or(mode);
            index += 1;
        } else if arg == "--mode" {
            if let Some(value) = args.get(index + 1) {
                mode = parse_mode_name(value).unwrap_or(mode);
                index += 2;
            } else {
                index += 1;
            }
        } else {
            text.extend(args[index..].iter().cloned());
            break;
        }
    }

    (mode, text.join(" "))
}

fn configured_mode() -> DictationMode {
    match ChirperConfig::load_default() {
        Ok(config) => config.dictation_mode,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn parse_mode_name(value: &str) -> Option<DictationMode> {
    match value {
        "auto" => Some(DictationMode::Auto),
        "standard" | "text" | "prose" => Some(DictationMode::Standard),
        "email" => Some(DictationMode::Email),
        "command" | "shell" | "terminal" => Some(DictationMode::Command),
        "code" | "programming" => Some(DictationMode::Code),
        _ => None,
    }
}

fn format_text(text: &str, mode: DictationMode) -> String {
    let config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    match config.formatter_backend {
        FormatterBackend::None => text.to_string(),
        FormatterBackend::Rules => {
            let transcript = chirper_core::Transcript {
                text: text.to_string(),
                language: None,
            };
            match RuleFormatter.format(&transcript, mode) {
                Ok(text) => text,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        FormatterBackend::Ollama | FormatterBackend::LlamaCpp => {
            eprintln!(
                "formatter backend {:?} is not implemented yet; using raw transcript",
                config.formatter_backend
            );
            text.to_string()
        }
    }
}

fn transcribe_audio(audio: chirper_core::CapturedAudio) -> chirper_core::Transcript {
    let config = match ChirperConfig::load_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let options = match WhisperCppOptions::from_config(&config) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let asr = WhisperCppAsr::new(options);
    match asr.transcribe(&audio) {
        Ok(transcript) => transcript,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn toggle_state_path() -> PathBuf {
    runtime_dir().join("toggle-state")
}

fn runtime_dir() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("chirper");
    }

    env::temp_dir().join("chirper")
}

fn read_toggle_state(path: &PathBuf) -> Option<DetachedRecording> {
    let content = fs::read_to_string(path).ok()?;
    let mut pid = None;
    let mut audio_path = None;
    let mut sample_rate_hz = None;
    let mut channels = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key {
            "pid" => pid = value.parse::<u32>().ok(),
            "path" => audio_path = Some(PathBuf::from(value)),
            "sample_rate_hz" => sample_rate_hz = value.parse::<u32>().ok(),
            "channels" => channels = value.parse::<u16>().ok(),
            _ => {}
        }
    }

    let recording = DetachedRecording {
        pid: pid?,
        audio: chirper_core::CapturedAudio {
            path: audio_path?,
            sample_rate_hz: sample_rate_hz?,
            channels: channels?,
        },
    };

    if process_is_running(recording.pid) {
        Some(recording)
    } else {
        let _ = fs::remove_file(path);
        None
    }
}

fn write_toggle_state(
    path: &PathBuf,
    recording: &DetachedRecording,
) -> chirper_core::ChirperResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            chirper_core::ChirperError::Configuration(format!(
                "failed to create runtime directory {}: {source}",
                parent.display()
            ))
        })?;
    }

    let started_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    let content = format!(
        "pid={}\npath={}\nsample_rate_hz={}\nchannels={}\nstarted_at_ms={started_at_ms}\n",
        recording.pid,
        recording.audio.path.display(),
        recording.audio.sample_rate_hz,
        recording.audio.channels
    );

    fs::write(path, content).map_err(|source| {
        chirper_core::ChirperError::Configuration(format!(
            "failed to write toggle state {}: {source}",
            path.display()
        ))
    })
}

fn process_is_running(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0
}

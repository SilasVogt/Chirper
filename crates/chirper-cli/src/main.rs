use std::{
    collections::BTreeMap,
    env,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chirper_api::{send_request, ApiRequest, ApiResponse};
use chirper_asr_whispercpp::{WhisperCppAsr, WhisperCppOptions};
use chirper_audio_pipewire::{DetachedRecording, PipeWireRecorder, PipeWireRecorderOptions};
use chirper_core::{
    AsrEngine, AudioSource, ChirperConfig, CodexProfileConfig, DictationMode, FormatterBackend,
    ServiceCommand, TextInserter, WorkflowState, WHISPER_MODEL_NAMES,
};
use chirper_formatter_codex::{CodexFormatter, CodexOptions, CodexPromptInput};
use chirper_formatter_ollama::{
    list_ollama_models, OllamaFormatter, OllamaModel, OllamaOptions, OllamaPromptInput,
};
use chirper_formatter_rules::format_spoken_rules_with_vocabulary;
use chirper_insertion_clipboard::ClipboardInserter;
use chirper_platform::{PlatformDiagnostics, RuntimeDiagnostics};

const WHISPER_LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("auto", "Auto detect"),
    ("en", "English"),
    ("id", "Indonesian"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("sv", "Swedish"),
    ("no", "Norwegian"),
    ("da", "Danish"),
    ("fi", "Finnish"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("hi", "Hindi"),
    ("ar", "Arabic"),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    if let Some(request) = parse_daemon_request(first.as_deref()) {
        call_daemon(request);
        return;
    }

    if matches!(first.as_deref(), Some("daemon-start-screen")) {
        daemon_start_screen();
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

    if matches!(first.as_deref(), Some("model-current")) {
        model_current();
        return;
    }

    if matches!(first.as_deref(), Some("model-list")) {
        model_list(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("model-use")) {
        model_use(args.next());
        return;
    }

    if matches!(first.as_deref(), Some("model-download")) {
        model_download(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("language-current")) {
        language_current(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("language-list")) {
        language_list(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("language-use")) {
        language_use(args.next());
        return;
    }

    if matches!(first.as_deref(), Some("audio-current")) {
        audio_current();
        return;
    }

    if matches!(first.as_deref(), Some("audio-list")) {
        audio_list(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("audio-use")) {
        audio_use(args.next());
        return;
    }

    if matches!(first.as_deref(), Some("formatter-current")) {
        formatter_current(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("formatter-use")) {
        formatter_use(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("ollama-list")) {
        ollama_list(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("ollama-use")) {
        ollama_use(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("codex-current")) {
        codex_current(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("codex-use")) {
        codex_use(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("codex-list") | Some("codex-models")) {
        codex_list(args.collect());
        return;
    }

    if matches!(
        first.as_deref(),
        Some("codex-profile-add") | Some("codex-profile-set")
    ) {
        codex_profile_add(args.collect());
        return;
    }

    if matches!(
        first.as_deref(),
        Some("codex-profile-remove") | Some("codex-profile-delete")
    ) {
        codex_profile_remove(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("codex-profiles")) {
        codex_profiles(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("vocab-list")) {
        vocab_list(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("vocab-add")) {
        vocab_add(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("vocab-remove")) {
        vocab_remove(args.collect());
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

    if matches!(first.as_deref(), Some("format-compare")) {
        format_compare(args.collect());
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
        Some("daemon-toggle") => Some(ApiRequest::Toggle { audio: None }),
        Some("daemon-start") => Some(ApiRequest::StartRecording { audio: None }),
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

    let config = load_config_or_exit();
    let mut recorder = PipeWireRecorder::new(PipeWireRecorderOptions::from_config(&config));

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

fn model_current() {
    let config = load_config_or_exit();
    let path = config
        .whispercpp_model_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<unset>".to_string());

    println!("model: {}", config.whisper_model);
    println!("path: {path}");
}

fn model_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let installed = installed_models();
    let current_installed = config
        .whispercpp_model_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(false);

    if json {
        let installed_json = installed
            .values()
            .map(|model| {
                serde_json::json!({
                    "name": model.name,
                    "path": model.path,
                    "bytes": model.bytes,
                })
            })
            .collect::<Vec<_>>();
        let available_json = WHISPER_MODEL_NAMES
            .iter()
            .map(|model| {
                serde_json::json!({
                    "name": model,
                    "installed": installed.contains_key(*model),
                    "path": ChirperConfig::default_model_path(model),
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "current": {
                "name": config.whisper_model,
                "path": config.whispercpp_model_path,
                "installed": current_installed,
            },
            "model_dir": ChirperConfig::default_model_dir(),
            "installed": installed_json,
            "available": available_json,
        });

        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("current:");
    println!("  model: {}", config.whisper_model);
    println!(
        "  path: {}",
        config
            .whispercpp_model_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<unset>".to_string())
    );
    println!("  installed: {current_installed}");
    println!();
    println!("installed:");

    if installed.is_empty() {
        println!(
            "  none found in {}",
            ChirperConfig::default_model_dir().display()
        );
    } else {
        for model in installed.values() {
            println!(
                "  {:<24} {:>8}  {}",
                model.name,
                format_bytes(model.bytes),
                model.path.display()
            );
        }
    }

    println!();
    println!("download examples:");
    for model in ["base", "small", "medium", "large-v3-turbo"] {
        println!("  chirper model-download {model} --select");
    }
}

fn model_use(selection: Option<String>) {
    let Some(selection) = selection else {
        eprintln!("usage: chirper model-use <model-name|/path/to/ggml-model.bin>");
        std::process::exit(1);
    };

    let (model, path) = match resolve_model_selection(&selection) {
        Ok(selection) => selection,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = ChirperConfig::save_default_model_selection(&model, &path) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected whisper model: {model}");
    println!("path: {}", path.display());
    println!("the daemon will use this for the next transcription");
}

fn model_download(args: Vec<String>) {
    let mut model = None;
    let mut select = false;

    for arg in args {
        if arg == "--select" {
            select = true;
        } else if model.is_none() {
            model = Some(arg);
        } else {
            eprintln!("usage: chirper model-download <model-name> [--select]");
            std::process::exit(1);
        }
    }

    let Some(model) = model else {
        eprintln!("usage: chirper model-download <model-name> [--select]");
        std::process::exit(1);
    };

    if !WHISPER_MODEL_NAMES.contains(&model.as_str()) {
        eprintln!("unknown whisper model: {model}");
        eprintln!("run `chirper model-list` for common model names");
        std::process::exit(1);
    }

    let script = whispercpp_download_script();
    if !script.exists() {
        eprintln!(
            "whisper.cpp download script not found: {}",
            script.display()
        );
        eprintln!("run `scripts/setup-whispercpp.sh --backend vulkan --model {model}` first");
        std::process::exit(1);
    }

    let model_dir = ChirperConfig::default_model_dir();
    if let Err(error) = fs::create_dir_all(&model_dir) {
        eprintln!(
            "failed to create model directory {}: {error}",
            model_dir.display()
        );
        std::process::exit(1);
    }

    let status = match Command::new(&script)
        .arg(&model)
        .arg(&model_dir)
        .stdin(Stdio::null())
        .status()
    {
        Ok(status) => status,
        Err(error) => {
            eprintln!("failed to run {}: {error}", script.display());
            std::process::exit(1);
        }
    };

    if !status.success() {
        eprintln!("model download failed with status {status}");
        std::process::exit(1);
    }

    let path = ChirperConfig::default_model_path(&model);
    println!("downloaded whisper model: {model}");
    println!("path: {}", path.display());

    if select {
        if let Err(error) = ChirperConfig::save_default_model_selection(&model, &path) {
            eprintln!("{error}");
            std::process::exit(1);
        }

        println!("selected whisper model: {model}");
    }
}

fn language_current(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let code = current_language_code(&config);
    let label = language_label(&code);

    if json {
        let value = serde_json::json!({
            "code": code,
            "label": label,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("language: {code}");
    println!("label: {label}");
}

fn language_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let current = current_language_code(&config);

    if json {
        let languages = WHISPER_LANGUAGE_OPTIONS
            .iter()
            .map(|(code, label)| {
                serde_json::json!({
                    "code": code,
                    "label": label,
                    "selected": *code == current,
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "current": {
                "code": current,
                "label": language_label(&current),
            },
            "languages": languages,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("current: {} ({})", current, language_label(&current));
    println!("languages:");
    for (code, label) in WHISPER_LANGUAGE_OPTIONS {
        let marker = if *code == current { "*" } else { " " };
        println!(" {marker} {:<6} {label}", code);
    }
}

fn language_use(selection: Option<String>) {
    let Some(selection) = selection else {
        eprintln!("usage: chirper language-use <auto|language-code|language-name>");
        std::process::exit(1);
    };
    let Some(code) = resolve_language_selection(&selection) else {
        eprintln!("unknown language: {selection}");
        eprintln!("run `chirper language-list` to see common language codes");
        std::process::exit(1);
    };
    let language = (code != "auto").then_some(code);

    if let Err(error) = ChirperConfig::save_default_language_selection(language) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected whisper language: {code}");
    println!("label: {}", language_label(code));
    println!("the daemon will use this for the next transcription");
}

fn current_language_code(config: &ChirperConfig) -> String {
    config
        .whisper_language
        .as_deref()
        .and_then(resolve_language_selection)
        .unwrap_or("auto")
        .to_string()
}

fn language_label(code: &str) -> &str {
    WHISPER_LANGUAGE_OPTIONS
        .iter()
        .find_map(|(candidate, label)| (*candidate == code).then_some(*label))
        .unwrap_or("Custom")
}

fn resolve_language_selection(selection: &str) -> Option<&'static str> {
    let normalized_selection = normalize_language_selection(selection);

    if normalized_selection.is_empty()
        || matches!(
            normalized_selection.as_str(),
            "auto" | "default" | "detect" | "autodetect" | "none"
        )
    {
        return Some("auto");
    }

    if let Some((code, _label)) = WHISPER_LANGUAGE_OPTIONS.iter().find(|(code, label)| {
        normalize_language_selection(code) == normalized_selection
            || normalize_language_selection(label) == normalized_selection
    }) {
        return Some(*code);
    }

    None
}

fn normalize_language_selection(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "")
}

fn audio_current() {
    let config = load_config_or_exit();
    let nodes = pipewire_audio_nodes().unwrap_or_default();
    let label = current_audio_label(&config, &nodes);
    let target = config.pipewire_target.as_deref().unwrap_or("auto");

    println!("target: {target}");
    println!("label: {label}");
}

fn audio_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let nodes = match pipewire_audio_nodes() {
        Ok(nodes) => nodes,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let current_label = current_audio_label(&config, &nodes);

    if json {
        let sources = nodes
            .iter()
            .filter(|node| node.kind == AudioNodeKind::Input)
            .map(|node| audio_node_json(node, config.pipewire_target.as_deref()))
            .collect::<Vec<_>>();
        let sinks = nodes
            .iter()
            .filter(|node| node.kind == AudioNodeKind::Output)
            .map(|node| audio_node_json(node, None))
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "current": {
                "target": config.pipewire_target,
                "label": current_label,
            },
            "sources": sources,
            "sinks": sinks,
        });

        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("current:");
    println!(
        "  target: {}",
        config.pipewire_target.as_deref().unwrap_or("auto")
    );
    println!("  label: {current_label}");
    println!();
    println!("microphone inputs:");
    println!("  {:<8} {:<8} {:<42} Description", "id", "serial", "target");
    println!(
        "  {:<8} {:<8} {:<42} {}",
        "-", "-", "auto", "Default microphone"
    );

    for node in nodes
        .iter()
        .filter(|node| node.kind == AudioNodeKind::Input)
    {
        println!(
            "  {:<8} {:<8} {:<42} {}",
            node.id, node.serial, node.name, node.description
        );
    }

    println!();
    println!("screen audio outputs:");
    println!("  {:<8} {:<8} {:<42} Description", "id", "serial", "target");
    for node in nodes
        .iter()
        .filter(|node| node.kind == AudioNodeKind::Output)
    {
        println!(
            "  {:<8} {:<8} {:<42} {}",
            node.id, node.serial, node.name, node.description
        );
    }
}

fn audio_use(selection: Option<String>) {
    let Some(selection) = selection else {
        eprintln!("usage: chirper audio-use <auto|source-id|source-name>");
        std::process::exit(1);
    };

    if matches!(selection.as_str(), "auto" | "default" | "none") {
        if let Err(error) = ChirperConfig::save_default_audio_target(None) {
            eprintln!("{error}");
            std::process::exit(1);
        }

        println!("selected audio input: Default microphone");
        println!("target: auto");
        return;
    }

    let nodes = match pipewire_audio_nodes() {
        Ok(nodes) => nodes,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let Some(node) = nodes
        .iter()
        .filter(|node| node.kind == AudioNodeKind::Input)
        .find(|node| node.matches_selection(&selection))
    else {
        eprintln!("audio input not found: {selection}");
        eprintln!("run `chirper audio-list` to see available inputs");
        std::process::exit(1);
    };

    if let Err(error) = ChirperConfig::save_default_audio_target(Some(&node.name)) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected audio input: {}", node.description);
    println!("target: {}", node.name);
}

fn formatter_current(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();

    if json {
        let value = serde_json::json!({
            "backend": config.formatter_backend.as_config_value(),
            "ollama_model": config.ollama_model,
            "ollama_command": config.ollama_command,
            "codex_command": config.codex_command,
            "codex_model": config.codex_model,
            "codex_profile": config.codex_profile,
            "codex_reasoning_effort": config.codex_reasoning_effort,
            "codex_service_tier": config.codex_service_tier,
            "codex_config_overrides": config.codex_config_overrides,
        });

        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("backend: {}", config.formatter_backend.as_config_value());
    println!("ollama_command: {}", config.ollama_command);
    println!("ollama_model: {}", config.ollama_model);
    println!("codex_command: {}", config.codex_command);
    println!(
        "codex_model: {}",
        config.codex_model.as_deref().unwrap_or("<codex default>")
    );
    println!(
        "codex_profile: {}",
        config.codex_profile.as_deref().unwrap_or("<none>")
    );
    println!(
        "codex_reasoning_effort: {}",
        config
            .codex_reasoning_effort
            .as_deref()
            .unwrap_or("<default>")
    );
    println!(
        "codex_service_tier: {}",
        config.codex_service_tier.as_deref().unwrap_or("<default>")
    );
    println!("vocabulary_entries: {}", config.vocabulary.len());
}

fn formatter_use(args: Vec<String>) {
    let Some(selection) = args.first() else {
        eprintln!("usage: chirper formatter-use <none|rules|ollama|codex> [model]");
        std::process::exit(1);
    };

    let backend = match selection.parse::<FormatterBackend>() {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let model = args.get(1).map(String::as_str);

    if backend == FormatterBackend::Ollama {
        let config = load_config_or_exit();
        let selected_model = model.unwrap_or(&config.ollama_model);
        if let Err(error) = ensure_ollama_model_available(&config.ollama_command, selected_model) {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }

    if backend == FormatterBackend::Codex {
        if let Some(model) = model {
            if let Err(error) = ChirperConfig::save_default_codex_selection(
                Some(model),
                None,
                None,
                None,
                &[],
                false,
            ) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }

    let ollama_model = (backend == FormatterBackend::Ollama)
        .then_some(model)
        .flatten();
    if let Err(error) = ChirperConfig::save_default_formatter_selection(backend, ollama_model) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected formatter: {}", backend.as_config_value());
    if let Some(model) = model.filter(|_| backend == FormatterBackend::Ollama) {
        println!("ollama_model: {model}");
    } else if let Some(model) = model.filter(|_| backend == FormatterBackend::Codex) {
        println!("codex_model: {model}");
    }
    println!("the daemon will use this for the next transcription");
}

fn ollama_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();

    match list_ollama_models(&config.ollama_command) {
        Ok(models) => {
            if json {
                let value = ollama_status_json(&config, true, None, &models);
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }

            println!("formatter: {}", config.formatter_backend.as_config_value());
            println!("ollama_command: {}", config.ollama_command);
            println!("current_model: {}", config.ollama_model);
            println!();
            println!("installed Ollama models:");
            if models.is_empty() {
                println!("  none");
            } else {
                for model in models {
                    let marker = if model.name == config.ollama_model {
                        "*"
                    } else {
                        " "
                    };
                    println!(" {marker} {}", model.name);
                }
            }
        }
        Err(error) => {
            if json {
                let value = ollama_status_json(&config, false, Some(error.to_string()), &[]);
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }

            eprintln!("{error}");
            eprintln!("install Ollama or set `ollama_command` in ~/.config/chirper/config.toml");
            std::process::exit(1);
        }
    }
}

fn ollama_use(args: Vec<String>) {
    let mut model = None;
    let mut enable = true;

    for arg in args {
        match arg.as_str() {
            "--no-enable" => enable = false,
            "--enable" => enable = true,
            _ if model.is_none() => model = Some(arg),
            _ => {
                eprintln!("usage: chirper ollama-use <model> [--enable|--no-enable]");
                std::process::exit(1);
            }
        }
    }

    let Some(model) = model else {
        eprintln!("usage: chirper ollama-use <model> [--enable|--no-enable]");
        std::process::exit(1);
    };
    let config = load_config_or_exit();

    if let Err(error) = ensure_ollama_model_available(&config.ollama_command, &model) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    let backend = if enable {
        FormatterBackend::Ollama
    } else {
        config.formatter_backend
    };

    if let Err(error) = ChirperConfig::save_default_formatter_selection(backend, Some(&model)) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected Ollama model: {model}");
    println!("formatter: {}", backend.as_config_value());
    println!("the daemon will use this for the next transcription");
}

fn ensure_ollama_model_available(command: &str, selected_model: &str) -> Result<(), String> {
    let models = list_ollama_models(command).map_err(|error| error.to_string())?;

    if models.iter().any(|model| model.name == selected_model) {
        return Ok(());
    }

    Err(format!(
        "Ollama model `{selected_model}` is not installed; run `ollama pull {selected_model}` or choose one from `chirper ollama-list`"
    ))
}

fn ollama_status_json(
    config: &ChirperConfig,
    available: bool,
    error: Option<String>,
    models: &[OllamaModel],
) -> serde_json::Value {
    let models_json = models
        .iter()
        .map(|model| {
            serde_json::json!({
                "name": model.name,
                "selected": model.name == config.ollama_model,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "available": available,
        "error": error,
        "formatter": config.formatter_backend.as_config_value(),
        "command": config.ollama_command,
        "current": {
            "model": config.ollama_model,
            "selected_installed": models.iter().any(|model| model.name == config.ollama_model),
        },
        "models": models_json,
    })
}

fn codex_current(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let available = command_available(&config.codex_command);

    if json {
        let value = serde_json::json!({
            "available": available,
            "formatter": config.formatter_backend.as_config_value(),
            "command": config.codex_command,
            "current": codex_options_json(&CodexOptions::from_config(&config)),
            "profiles": config.codex_profiles.iter().map(codex_profile_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("formatter: {}", config.formatter_backend.as_config_value());
    println!("codex_available: {available}");
    println!("codex_command: {}", config.codex_command);
    print_codex_options("current", &CodexOptions::from_config(&config));

    if !config.codex_profiles.is_empty() {
        println!();
        println!("configured profiles:");
        for profile in &config.codex_profiles {
            println!("  {}", format_codex_profile_summary(profile));
        }
    }
}

fn codex_use(args: Vec<String>) {
    let mut model = None;
    let mut profile = None;
    let mut reasoning_effort = None;
    let mut service_tier = None;
    let mut config_overrides = Vec::new();
    let mut enable = true;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--model=") {
            model = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--model" {
            model = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            profile = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--profile" {
            profile = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg
            .strip_prefix("--effort=")
            .or_else(|| arg.strip_prefix("--reasoning-effort="))
        {
            reasoning_effort = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--effort" || arg == "--reasoning-effort" {
            reasoning_effort = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg
            .strip_prefix("--service-tier=")
            .or_else(|| arg.strip_prefix("--tier="))
        {
            service_tier = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--service-tier" || arg == "--tier" {
            service_tier = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--config=") {
            push_config_override(&mut config_overrides, value);
            index += 1;
        } else if arg == "--config" {
            if let Some(value) = args.get(index + 1) {
                push_config_override(&mut config_overrides, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--fast" {
            service_tier = Some("priority".to_string());
            index += 1;
        } else if arg == "--extra-high" || arg == "--xhigh" {
            reasoning_effort = Some("xhigh".to_string());
            index += 1;
        } else if arg == "--high" || arg == "--medium" || arg == "--low" {
            reasoning_effort = Some(arg.trim_start_matches("--").to_string());
            index += 1;
        } else if arg == "--no-enable" {
            enable = false;
            index += 1;
        } else if arg == "--enable" {
            enable = true;
            index += 1;
        } else if model.is_none() {
            model = normalize_optional_cli_value(arg);
            index += 1;
        } else {
            eprintln!(
                "usage: chirper codex-use [MODEL] [--effort low|medium|high|xhigh] [--service-tier priority] [--fast] [--profile CODEX_PROFILE] [--config key=value] [--enable|--no-enable]"
            );
            std::process::exit(1);
        }
    }

    if let Err(error) = ChirperConfig::save_default_codex_selection(
        model.as_deref(),
        profile.as_deref(),
        reasoning_effort.as_deref(),
        service_tier.as_deref(),
        &config_overrides,
        enable,
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected Codex formatter settings");
    println!("formatter_enabled: {enable}");
    println!("model: {}", model.as_deref().unwrap_or("<codex default>"));
    println!("profile: {}", profile.as_deref().unwrap_or("<none>"));
    println!(
        "reasoning_effort: {}",
        reasoning_effort.as_deref().unwrap_or("<default>")
    );
    println!(
        "service_tier: {}",
        service_tier.as_deref().unwrap_or("<default>")
    );
    if !config_overrides.is_empty() {
        println!("config_overrides: {}", config_overrides.join(", "));
    }
}

fn codex_profiles(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();

    if json {
        let value = serde_json::json!({
            "profiles": config.codex_profiles.iter().map(codex_profile_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    if config.codex_profiles.is_empty() {
        println!("no Codex profiles configured");
        println!("run `chirper codex-profile-add fast --model gpt-5.5 --effort low --fast`");
        return;
    }

    println!("Codex profiles:");
    for profile in &config.codex_profiles {
        println!("  {}", format_codex_profile_summary(profile));
    }
}

fn codex_profile_add(args: Vec<String>) {
    let mut name = None;
    let mut model = None;
    let mut profile = None;
    let mut reasoning_effort = None;
    let mut service_tier = None;
    let mut config_overrides = Vec::new();
    let mut json = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--json" {
            json = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--name=") {
            name = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--name" {
            name = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--model=") {
            model = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--model" {
            model = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            profile = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--profile" {
            profile = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--effort=") {
            reasoning_effort = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--effort" || arg == "--reasoning-effort" {
            reasoning_effort = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--service-tier=") {
            service_tier = normalize_optional_cli_value(value);
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--tier=") {
            service_tier = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--service-tier" || arg == "--tier" {
            service_tier = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--config=") {
            push_config_override(&mut config_overrides, value);
            index += 1;
        } else if arg == "--config" {
            if let Some(value) = args.get(index + 1) {
                push_config_override(&mut config_overrides, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--fast" {
            service_tier = Some("priority".to_string());
            index += 1;
        } else if arg == "--extra-high" || arg == "--xhigh" {
            reasoning_effort = Some("xhigh".to_string());
            index += 1;
        } else if arg == "--high" || arg == "--medium" || arg == "--low" {
            reasoning_effort = Some(arg.trim_start_matches("--").to_string());
            index += 1;
        } else if name.is_none() {
            name = normalize_optional_cli_value(arg);
            index += 1;
        } else {
            eprintln!(
                "usage: chirper codex-profile-add NAME [--model MODEL] [--effort low|medium|high|xhigh] [--service-tier priority] [--fast] [--profile CODEX_PROFILE] [--config key=value]"
            );
            std::process::exit(1);
        }
    }

    let Some(name) = name else {
        eprintln!(
            "usage: chirper codex-profile-add NAME [--model MODEL] [--effort low|medium|high|xhigh] [--service-tier priority] [--fast] [--profile CODEX_PROFILE] [--config key=value]"
        );
        std::process::exit(1);
    };
    let profile_config = CodexProfileConfig {
        name,
        model,
        profile,
        reasoning_effort,
        service_tier,
        config_overrides,
    };

    if let Err(error) = ChirperConfig::save_default_codex_profile(profile_config.clone()) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&codex_profile_json(&profile_config)).unwrap()
        );
        return;
    }

    println!(
        "saved Codex profile {}",
        format_codex_profile_summary(&profile_config)
    );
}

fn codex_profile_remove(args: Vec<String>) {
    let mut name = None;
    let mut json = false;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--json" {
            json = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--name=") {
            name = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--name" {
            name = args
                .get(index + 1)
                .and_then(|value| normalize_optional_cli_value(value));
            index += 2;
        } else if name.is_none() {
            name = normalize_optional_cli_value(arg);
            index += 1;
        } else {
            eprintln!("usage: chirper codex-profile-remove NAME");
            std::process::exit(1);
        }
    }

    let Some(name) = name else {
        eprintln!("usage: chirper codex-profile-remove NAME");
        std::process::exit(1);
    };

    let removed = match ChirperConfig::remove_default_codex_profile(&name) {
        Ok(removed) => removed,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if json {
        let value = serde_json::json!({
            "name": name,
            "removed": removed,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    if removed {
        println!("removed Codex profile {name}");
    } else {
        println!("Codex profile {name} was not configured");
    }
}

fn codex_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let models = match codex_model_catalog(&config.codex_command) {
        Ok(models) => models,
        Err(error) => {
            if json {
                let value = serde_json::json!({
                    "available": false,
                    "error": error,
                    "models": [],
                });
                println!("{}", serde_json::to_string_pretty(&value).unwrap());
                return;
            }

            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if json {
        let value = serde_json::json!({
            "available": true,
            "models": models,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("available Codex models:");
    for model in models {
        let slug = model
            .get("slug")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let display = model
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(slug);
        let default_effort = model
            .get("default_reasoning_level")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("default");
        let efforts = model
            .get("supported_reasoning_levels")
            .and_then(serde_json::Value::as_array)
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|level| level.get("effort").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let tiers = model
            .get("service_tiers")
            .and_then(serde_json::Value::as_array)
            .map(|tiers| {
                tiers
                    .iter()
                    .filter_map(|tier| tier.get("id").and_then(serde_json::Value::as_str))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();

        println!("  {slug:<24} {display}");
        println!("    default_effort: {default_effort}");
        if !efforts.is_empty() {
            println!("    efforts: {efforts}");
        }
        if !tiers.is_empty() {
            println!("    service_tiers: {tiers}");
        }
    }
}

fn codex_model_catalog(command: &str) -> Result<Vec<serde_json::Value>, String> {
    let output = Command::new(command)
        .arg("debug")
        .arg("models")
        .stdin(Stdio::null())
        .output()
        .map_err(|source| format!("failed to run `{command} debug models`: {source}"))?;

    if !output.status.success() {
        return Err(format!(
            "`{command} debug models` exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let value = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|source| format!("failed to parse Codex model catalog JSON: {source}"))?;
    let models = value
        .get("models")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Codex model catalog did not contain a models array".to_string())?;

    Ok(models
        .iter()
        .map(|model| {
            serde_json::json!({
                "slug": model.get("slug").and_then(serde_json::Value::as_str),
                "display_name": model.get("display_name").and_then(serde_json::Value::as_str),
                "default_reasoning_level": model.get("default_reasoning_level").and_then(serde_json::Value::as_str),
                "supported_reasoning_levels": model.get("supported_reasoning_levels").cloned().unwrap_or_else(|| serde_json::json!([])),
                "additional_speed_tiers": model.get("additional_speed_tiers").cloned().unwrap_or_else(|| serde_json::json!([])),
                "service_tiers": model.get("service_tiers").cloned().unwrap_or_else(|| serde_json::json!([])),
                "supported_in_api": model.get("supported_in_api").and_then(serde_json::Value::as_bool),
            })
        })
        .collect())
}

fn command_available(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn normalize_optional_cli_value(value: &str) -> Option<String> {
    let value = value.trim();

    if value.is_empty() || matches!(value, "default" | "none" | "unset" | "auto") {
        None
    } else {
        Some(value.to_string())
    }
}

fn push_config_override(config_overrides: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        config_overrides.push(value.to_string());
    }
}

fn codex_options_json(options: &CodexOptions) -> serde_json::Value {
    serde_json::json!({
        "model": options.model.as_deref(),
        "profile": options.profile.as_deref(),
        "reasoning_effort": options.reasoning_effort.as_deref(),
        "service_tier": options.service_tier.as_deref(),
        "config_overrides": options.config_overrides.as_slice(),
        "label": options.label(),
    })
}

fn codex_profile_json(profile: &CodexProfileConfig) -> serde_json::Value {
    serde_json::json!({
        "name": profile.name.as_str(),
        "model": profile.model.as_deref(),
        "profile": profile.profile.as_deref(),
        "reasoning_effort": profile.reasoning_effort.as_deref(),
        "service_tier": profile.service_tier.as_deref(),
        "config_overrides": profile.config_overrides.as_slice(),
    })
}

fn print_codex_options(label: &str, options: &CodexOptions) {
    println!("{label}: {}", options.label());
    println!(
        "  model: {}",
        options.model.as_deref().unwrap_or("<codex default>")
    );
    println!(
        "  profile: {}",
        options.profile.as_deref().unwrap_or("<none>")
    );
    println!(
        "  reasoning_effort: {}",
        options.reasoning_effort.as_deref().unwrap_or("<default>")
    );
    println!(
        "  service_tier: {}",
        options.service_tier.as_deref().unwrap_or("<default>")
    );
    if !options.config_overrides.is_empty() {
        println!(
            "  config_overrides: {}",
            options.config_overrides.join(", ")
        );
    }
}

fn format_codex_profile_summary(profile: &CodexProfileConfig) -> String {
    let options = CodexOptions {
        command: "codex".to_string(),
        model: profile.model.clone(),
        profile: profile.profile.clone(),
        reasoning_effort: profile.reasoning_effort.clone(),
        service_tier: profile.service_tier.clone(),
        config_overrides: profile.config_overrides.clone(),
        vocabulary: Vec::new(),
    };

    format!("{}: {}", profile.name, options.label())
}

fn vocab_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();

    if json {
        let entries = config
            .vocabulary
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "spoken": entry.spoken,
                    "written": entry.written,
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({ "entries": entries });

        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    if config.vocabulary.is_empty() {
        println!("no vocabulary entries configured");
        println!("example: chirper vocab-add \"silas on linux\" SilasOnLinux");
        return;
    }

    println!("vocabulary:");
    for entry in config.vocabulary {
        println!("  {:<28} -> {}", entry.spoken, entry.written);
    }
}

fn vocab_add(args: Vec<String>) {
    if args.len() != 2 {
        eprintln!("usage: chirper vocab-add <spoken phrase> <written form>");
        eprintln!("example: chirper vocab-add \"silas on linux\" SilasOnLinux");
        std::process::exit(1);
    }

    if let Err(error) = ChirperConfig::save_default_vocabulary_entry(&args[0], &args[1]) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("added vocabulary entry: {} -> {}", args[0], args[1]);
}

fn vocab_remove(args: Vec<String>) {
    if args.len() != 1 {
        eprintln!("usage: chirper vocab-remove <spoken phrase>");
        std::process::exit(1);
    }

    let removed = match ChirperConfig::remove_default_vocabulary_entry(&args[0]) {
        Ok(removed) => removed,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if removed {
        println!("removed vocabulary entry: {}", args[0]);
    } else {
        println!("vocabulary entry not found: {}", args[0]);
    }
}

fn daemon_start_screen() {
    let nodes = match pipewire_audio_nodes() {
        Ok(nodes) => nodes,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let Some(node) = nodes.iter().find(|node| node.kind == AudioNodeKind::Output) else {
        eprintln!("no screen audio outputs found");
        std::process::exit(1);
    };

    call_daemon(ApiRequest::StartRecording {
        audio: Some(chirper_api::AudioCaptureTarget {
            kind: chirper_api::AudioCaptureKind::ScreenAudio,
            target: Some(node.name.clone()),
            label: Some(format!("Screen audio: {}", node.description)),
        }),
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioNodeKind {
    Input,
    Output,
}

#[derive(Debug, Clone)]
struct PipeWireAudioNode {
    id: u64,
    serial: u64,
    name: String,
    description: String,
    kind: AudioNodeKind,
}

impl PipeWireAudioNode {
    fn matches_selection(&self, selection: &str) -> bool {
        selection == self.name
            || selection == self.description
            || selection == self.id.to_string()
            || selection == self.serial.to_string()
    }
}

fn pipewire_audio_nodes() -> Result<Vec<PipeWireAudioNode>, String> {
    let output = Command::new("pw-dump")
        .stdin(Stdio::null())
        .output()
        .map_err(|source| format!("failed to run pw-dump: {source}"))?;

    if !output.status.success() {
        return Err(format!("pw-dump exited with status {}", output.status));
    }

    let value = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|source| format!("failed to parse pw-dump JSON: {source}"))?;
    let Some(items) = value.as_array() else {
        return Err("pw-dump returned unexpected JSON".to_string());
    };

    let mut nodes = Vec::new();
    for item in items {
        if item.get("type").and_then(serde_json::Value::as_str) != Some("PipeWire:Interface:Node") {
            continue;
        }

        let Some(props) = item
            .get("info")
            .and_then(|info| info.get("props"))
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        let media_class = props
            .get("media.class")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let kind = match media_class {
            "Audio/Source" => AudioNodeKind::Input,
            "Audio/Sink" => AudioNodeKind::Output,
            _ => continue,
        };
        let id = item
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let serial = props
            .get("object.serial")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(id);
        let name = props
            .get("node.name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();

        if name.is_empty() {
            continue;
        }

        let description = props
            .get("node.description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&name)
            .to_string();

        nodes.push(PipeWireAudioNode {
            id,
            serial,
            name,
            description,
            kind,
        });
    }

    nodes.sort_by(|a, b| {
        (a.kind == AudioNodeKind::Output)
            .cmp(&(b.kind == AudioNodeKind::Output))
            .then_with(|| a.description.cmp(&b.description))
    });
    Ok(nodes)
}

fn audio_node_json(node: &PipeWireAudioNode, selected_target: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "id": node.id,
        "serial": node.serial,
        "target": node.name,
        "name": node.name,
        "description": node.description,
        "label": node.description,
        "selected": selected_target
            .map(|target| node.matches_selection(target))
            .unwrap_or(false),
    })
}

fn current_audio_label(config: &ChirperConfig, nodes: &[PipeWireAudioNode]) -> String {
    let Some(target) = config.pipewire_target.as_deref() else {
        return "Default microphone".to_string();
    };

    nodes
        .iter()
        .find(|node| node.kind == AudioNodeKind::Input && node.matches_selection(target))
        .map(|node| node.description.clone())
        .unwrap_or_else(|| target.to_string())
}

#[derive(Debug, Clone)]
struct InstalledModel {
    name: String,
    path: PathBuf,
    bytes: u64,
}

fn installed_models() -> BTreeMap<String, InstalledModel> {
    let mut models = BTreeMap::new();
    let Ok(entries) = fs::read_dir(ChirperConfig::default_model_dir()) else {
        return models;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = ChirperConfig::model_name_from_path(&path) else {
            continue;
        };
        let bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);

        models.insert(name.clone(), InstalledModel { name, path, bytes });
    }

    models
}

fn resolve_model_selection(selection: &str) -> Result<(String, PathBuf), String> {
    let path = expand_user_path(selection);
    let looks_like_path = selection.contains('/') || selection.ends_with(".bin");

    if looks_like_path {
        if !path.exists() {
            return Err(format!("model file not found: {}", path.display()));
        }

        let model = ChirperConfig::model_name_from_path(&path).unwrap_or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("custom")
                .to_string()
        });

        return Ok((model, path));
    }

    let model = selection.to_string();
    let path = ChirperConfig::default_model_path(&model);

    if path.exists() {
        return Ok((model, path));
    }

    if WHISPER_MODEL_NAMES.contains(&selection) {
        Err(format!(
            "model `{selection}` is not installed at {}\nrun `chirper model-download {selection} --select`",
            path.display()
        ))
    } else {
        Err(format!(
            "unknown or missing model `{selection}`\nrun `chirper model-list` to see installed models"
        ))
    }
}

fn expand_user_path(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }

    PathBuf::from(value)
}

fn whispercpp_download_script() -> PathBuf {
    ChirperConfig::default_data_dir().join("src/whisper.cpp/models/download-ggml-model.sh")
}

fn format_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;

    if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} B")
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct FormatCompareArgs {
    mode: DictationMode,
    models: Vec<String>,
    include_ollama: bool,
    force_all_ollama: bool,
    include_codex_current: bool,
    codex_profiles: Vec<String>,
    all_codex_profiles: bool,
    include_rules: bool,
    keep_loaded: bool,
    prompt_input: ComparePromptInput,
    prompt_note: Option<String>,
    report_dir: Option<PathBuf>,
    progress_json: bool,
    json: bool,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparePromptInput {
    RawOnly,
    RawAndPreprocessed,
}

impl ComparePromptInput {
    fn as_ollama_input(self) -> OllamaPromptInput {
        match self {
            Self::RawOnly => OllamaPromptInput::RawOnly,
            Self::RawAndPreprocessed => OllamaPromptInput::RawAndPreprocessed,
        }
    }

    fn as_codex_input(self) -> CodexPromptInput {
        match self {
            Self::RawOnly => CodexPromptInput::RawOnly,
            Self::RawAndPreprocessed => CodexPromptInput::RawAndPreprocessed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::RawOnly => "raw",
            Self::RawAndPreprocessed => "raw+preprocessed",
        }
    }
}

fn format_compare(args: Vec<String>) {
    let args = parse_format_compare_args(args);

    if args.text.is_empty() {
        eprintln!(
            "usage: chirper format-compare [--mode auto|standard|email|command|code] [--model MODEL] [--models MODEL1,MODEL2] [--codex] [--codex-profile NAME] [--all-codex-profiles] [--prompt-input raw|both] [--prompt-note TEXT] [--prompt-file PATH] [--no-preprocessor] [--report-dir PATH] [--json] <text>"
        );
        std::process::exit(1);
    }

    let config = load_config_or_exit();
    let transcript = chirper_core::Transcript {
        text: args.text.clone(),
        language: None,
    };
    let preformatted = match format_with_rules(&config, &transcript, args.mode) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let codex_requested =
        args.include_codex_current || args.all_codex_profiles || !args.codex_profiles.is_empty();
    let load_all_ollama = args.include_ollama
        && args.models.is_empty()
        && (!codex_requested || args.force_all_ollama);
    let models = if load_all_ollama {
        match list_ollama_models(&config.ollama_command) {
            Ok(models) => models
                .into_iter()
                .map(|model| model.name)
                .collect::<Vec<_>>(),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    } else if args.include_ollama {
        args.models.clone()
    } else {
        Vec::new()
    };
    let codex_runs = resolve_codex_compare_runs(&config, &args);

    if models.is_empty() && codex_runs.is_empty() && !args.include_rules {
        eprintln!(
            "no formatter targets selected; run `chirper ollama-list`, pass `--codex`, or enable rules output"
        );
        std::process::exit(1);
    }

    let hardware = collect_hardware_snapshot(&config.ollama_command);
    let total_targets = models.len() + codex_runs.len();
    let compare_started = Instant::now();
    let mut results = Vec::new();
    emit_compare_progress(
        &args,
        serde_json::json!({
            "type": "started",
            "total": total_targets,
            "include_rules": args.include_rules,
            "hardware": hardware_json(&hardware),
        }),
    );

    if args.include_rules {
        results.push(FormatCompareResult {
            name: "rules".to_string(),
            elapsed_ms: 0,
            metrics: ResourceMetrics::default(),
            output: Some(preformatted.clone()),
            error: None,
        });
    }

    let mut target_index = 0usize;

    for model in models {
        target_index += 1;
        emit_compare_progress(
            &args,
            serde_json::json!({
                "type": "target_started",
                "index": target_index,
                "total": total_targets,
                "name": model.as_str(),
                "elapsed_ms": compare_started.elapsed().as_millis(),
            }),
        );
        let formatter = OllamaFormatter::new(OllamaOptions {
            command: config.ollama_command.clone(),
            model: model.clone(),
            vocabulary: config.vocabulary.clone(),
        });
        let started = Instant::now();
        let (result, metrics) = run_with_resource_sampling(|| {
            formatter.format_with_prompt_input_and_note(
                &transcript,
                &preformatted,
                args.mode,
                args.prompt_input.as_ollama_input(),
                args.prompt_note.as_deref(),
            )
        });
        let elapsed_ms = started.elapsed().as_millis();
        if !args.keep_loaded {
            stop_ollama_model_silent(&config.ollama_command, &model);
        }

        let result = match result {
            Ok(output) => FormatCompareResult {
                name: model,
                elapsed_ms,
                metrics,
                output: Some(output),
                error: None,
            },
            Err(error) => FormatCompareResult {
                name: model,
                elapsed_ms,
                metrics,
                output: None,
                error: Some(error.to_string()),
            },
        };
        emit_compare_progress(
            &args,
            serde_json::json!({
                "type": "target_finished",
                "index": target_index,
                "total": total_targets,
                "name": result.name.as_str(),
                "ok": result.error.is_none(),
                "elapsed_ms": result.elapsed_ms,
                "total_elapsed_ms": compare_started.elapsed().as_millis(),
                "metrics": metrics_json(&result.metrics),
                "error": result.error.as_deref(),
            }),
        );
        results.push(result);
    }

    for (name, options) in codex_runs {
        target_index += 1;
        emit_compare_progress(
            &args,
            serde_json::json!({
                "type": "target_started",
                "index": target_index,
                "total": total_targets,
                "name": name.as_str(),
                "elapsed_ms": compare_started.elapsed().as_millis(),
            }),
        );
        let formatter = CodexFormatter::new(options);
        let started = Instant::now();
        let (result, metrics) = run_with_resource_sampling(|| {
            formatter.format_with_prompt_input_and_note(
                &transcript,
                &preformatted,
                args.mode,
                args.prompt_input.as_codex_input(),
                args.prompt_note.as_deref(),
            )
        });
        let elapsed_ms = started.elapsed().as_millis();

        let result = match result {
            Ok(output) => FormatCompareResult {
                name,
                elapsed_ms,
                metrics,
                output: Some(output),
                error: None,
            },
            Err(error) => FormatCompareResult {
                name,
                elapsed_ms,
                metrics,
                output: None,
                error: Some(error.to_string()),
            },
        };
        emit_compare_progress(
            &args,
            serde_json::json!({
                "type": "target_finished",
                "index": target_index,
                "total": total_targets,
                "name": result.name.as_str(),
                "ok": result.error.is_none(),
                "elapsed_ms": result.elapsed_ms,
                "total_elapsed_ms": compare_started.elapsed().as_millis(),
                "metrics": metrics_json(&result.metrics),
                "error": result.error.as_deref(),
            }),
        );
        results.push(result);
    }

    let total_elapsed_ms = compare_started.elapsed().as_millis();
    let tested_models = tested_model_count(&results);
    let report_path = args.report_dir.as_ref().map(|directory| {
        write_format_compare_report(
            directory,
            &hardware,
            args.mode,
            args.prompt_input,
            args.prompt_note.as_deref(),
            total_elapsed_ms,
            &transcript.text,
            &preformatted,
            &results,
        )
    });
    emit_compare_progress(
        &args,
        serde_json::json!({
            "type": "finished",
            "total": total_targets,
            "tested_models": tested_models,
            "elapsed_ms": total_elapsed_ms,
            "report_path": report_path.as_ref().and_then(|result| result.as_ref().ok().map(|path| path.display().to_string())),
        }),
    );

    if args.json {
        let value = serde_json::json!({
            "mode": format!("{:?}", args.mode),
            "prompt_input": args.prompt_input.label(),
            "prompt_note": args.prompt_note.as_deref(),
            "tested_models": tested_models,
            "total_elapsed_ms": total_elapsed_ms,
            "preprocessed_sent_to_model": args.prompt_input == ComparePromptInput::RawAndPreprocessed,
            "preprocessed": preformatted,
            "hardware": hardware_json(&hardware),
            "report_path": report_path.as_ref().map(|result| result.as_ref().ok().map(|path| path.display().to_string())),
            "results": results.iter().map(format_compare_result_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        if let Some(Err(error)) = report_path {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    println!("mode: {:?}", args.mode);
    println!("prompt_input: {}", args.prompt_input.label());
    println!(
        "summary: {}",
        format_tested_summary(tested_models, total_elapsed_ms)
    );
    if let Some(prompt_note) = args.prompt_note.as_deref() {
        println!("prompt_note: {prompt_note}");
    }
    println!("hardware:");
    print_hardware_snapshot(&hardware);
    if args.prompt_input == ComparePromptInput::RawAndPreprocessed {
        println!("preprocessed:");
    } else {
        println!("preprocessed (not sent to model):");
    }
    println!("{}", preformatted);

    for result in results {
        println!();
        println!(
            "=== {} ({}, {}) ===",
            result.name,
            format_elapsed(result.elapsed_ms),
            format_metrics_summary(&result.metrics)
        );
        if let Some(output) = result.output {
            println!("{output}");
        } else if let Some(error) = result.error {
            println!("ERROR: {error}");
        }
    }

    if let Some(report_result) = report_path {
        match report_result {
            Ok(path) => println!("\nreport: {}", path.display()),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}

fn parse_format_compare_args(args: Vec<String>) -> FormatCompareArgs {
    let mut mode = configured_mode();
    let mut models = Vec::new();
    let mut include_ollama = true;
    let mut force_all_ollama = false;
    let mut include_codex_current = false;
    let mut codex_profiles = Vec::new();
    let mut all_codex_profiles = false;
    let mut include_rules = true;
    let mut keep_loaded = false;
    let mut prompt_input = ComparePromptInput::RawAndPreprocessed;
    let mut prompt_note = None;
    let mut report_dir = None;
    let mut progress_json = false;
    let mut json = false;
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
        } else if let Some(value) = arg.strip_prefix("--model=") {
            push_model_values(&mut models, value);
            index += 1;
        } else if arg == "--model" {
            if let Some(value) = args.get(index + 1) {
                push_model_values(&mut models, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--models=") {
            push_model_values(&mut models, value);
            index += 1;
        } else if arg == "--models" {
            if let Some(value) = args.get(index + 1) {
                push_model_values(&mut models, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--no-ollama" {
            include_ollama = false;
            index += 1;
        } else if arg == "--all-ollama" {
            force_all_ollama = true;
            include_ollama = true;
            index += 1;
        } else if arg == "--codex" {
            include_codex_current = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--codex-profile=") {
            push_model_values(&mut codex_profiles, value);
            index += 1;
        } else if arg == "--codex-profile" {
            if let Some(value) = args.get(index + 1) {
                push_model_values(&mut codex_profiles, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--codex-profiles=") {
            push_model_values(&mut codex_profiles, value);
            index += 1;
        } else if arg == "--codex-profiles" {
            if let Some(value) = args.get(index + 1) {
                push_model_values(&mut codex_profiles, value);
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--all-codex-profiles" {
            all_codex_profiles = true;
            index += 1;
        } else if arg == "--no-rules" {
            include_rules = false;
            index += 1;
        } else if arg == "--rules" {
            include_rules = true;
            index += 1;
        } else if arg == "--keep-loaded" {
            keep_loaded = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--prompt-input=") {
            prompt_input = parse_compare_prompt_input(value).unwrap_or(prompt_input);
            index += 1;
        } else if arg == "--prompt-input" {
            if let Some(value) = args.get(index + 1) {
                prompt_input = parse_compare_prompt_input(value).unwrap_or(prompt_input);
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--raw-only" {
            prompt_input = ComparePromptInput::RawOnly;
            index += 1;
        } else if arg == "--no-preprocessor" {
            prompt_input = ComparePromptInput::RawOnly;
            include_rules = false;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--prompt-note=") {
            prompt_note = Some(value.to_string());
            index += 1;
        } else if arg == "--prompt-note" || arg == "--prompt" {
            if let Some(value) = args.get(index + 1) {
                prompt_note = Some(value.to_string());
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--prompt-file=") {
            prompt_note = Some(read_prompt_note_file(value));
            index += 1;
        } else if arg == "--prompt-file" {
            if let Some(value) = args.get(index + 1) {
                prompt_note = Some(read_prompt_note_file(value));
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--report-dir=") {
            report_dir = Some(expand_user_path(value));
            index += 1;
        } else if arg == "--report-dir" || arg == "--report" {
            if let Some(value) = args.get(index + 1) {
                report_dir = Some(expand_user_path(value));
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--json" {
            json = true;
            index += 1;
        } else if arg == "--progress-json" {
            progress_json = true;
            index += 1;
        } else {
            text.extend(args[index..].iter().cloned());
            break;
        }
    }

    FormatCompareArgs {
        mode,
        models,
        include_ollama,
        force_all_ollama,
        include_codex_current,
        codex_profiles,
        all_codex_profiles,
        include_rules,
        keep_loaded,
        prompt_input,
        prompt_note,
        report_dir,
        progress_json,
        json,
        text: text.join(" "),
    }
}

fn parse_compare_prompt_input(value: &str) -> Option<ComparePromptInput> {
    match value.trim().to_ascii_lowercase().as_str() {
        "raw" | "raw-only" | "transcript" | "none" | "off" => Some(ComparePromptInput::RawOnly),
        "both" | "preprocessed" | "raw+preprocessed" | "with-preprocessed" | "default" => {
            Some(ComparePromptInput::RawAndPreprocessed)
        }
        _ => None,
    }
}

fn read_prompt_note_file(path: &str) -> String {
    let path = expand_user_path(path);
    fs::read_to_string(&path).unwrap_or_else(|source| {
        eprintln!("failed to read prompt file {}: {source}", path.display());
        std::process::exit(1);
    })
}

fn emit_compare_progress(args: &FormatCompareArgs, value: serde_json::Value) {
    if args.progress_json {
        eprintln!("{}", serde_json::to_string(&value).unwrap());
    }
}

fn push_model_values(models: &mut Vec<String>, value: &str) {
    for model in value.split(',') {
        let model = model.trim();
        if !model.is_empty() {
            models.push(model.to_string());
        }
    }
}

fn resolve_codex_compare_runs(
    config: &ChirperConfig,
    args: &FormatCompareArgs,
) -> Vec<(String, CodexOptions)> {
    let mut runs = Vec::new();

    if args.include_codex_current {
        runs.push((
            format!("codex:{}", CodexOptions::from_config(config).label()),
            CodexOptions::from_config(config),
        ));
    }

    if args.all_codex_profiles {
        for profile in &config.codex_profiles {
            runs.push((
                format!("codex:{}", profile.name),
                CodexOptions::from_named_profile(config, profile),
            ));
        }
    }

    for profile_name in &args.codex_profiles {
        let Some(profile) = config
            .codex_profiles
            .iter()
            .find(|profile| profile.name == *profile_name)
        else {
            eprintln!("unknown Codex profile: {profile_name}");
            eprintln!("run `chirper codex-profiles` to see configured profiles");
            std::process::exit(1);
        };

        runs.push((
            format!("codex:{}", profile.name),
            CodexOptions::from_named_profile(config, profile),
        ));
    }

    runs
}

#[derive(Debug, Clone, PartialEq)]
struct FormatCompareResult {
    name: String,
    elapsed_ms: u128,
    metrics: ResourceMetrics,
    output: Option<String>,
    error: Option<String>,
}

fn format_compare_result_json(result: &FormatCompareResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "elapsed_ms": result.elapsed_ms,
        "metrics": metrics_json(&result.metrics),
        "ok": result.error.is_none(),
        "output": result.output,
        "error": result.error,
    })
}

fn tested_model_count(results: &[FormatCompareResult]) -> usize {
    results
        .iter()
        .filter(|result| result.name != "rules")
        .count()
}

fn format_tested_summary(tested_models: usize, elapsed_ms: u128) -> String {
    let noun = if tested_models == 1 {
        "Model"
    } else {
        "Models"
    };
    format!(
        "Tested {tested_models} {noun} in {}",
        format_elapsed_words(elapsed_ms)
    )
}

#[derive(Debug, Clone, Default, PartialEq)]
struct ResourceMetrics {
    samples: usize,
    avg_cpu_percent: Option<f64>,
    avg_ram_used_bytes: Option<u64>,
    avg_gpu_percent: Option<f64>,
    avg_vram_used_bytes: Option<u64>,
    vram_total_bytes: Option<u64>,
    avg_gpu_power_watts: Option<f64>,
    avg_gpu_temp_celsius: Option<f64>,
}

#[derive(Debug, Clone, Default)]
struct ResourceAccumulator {
    samples: usize,
    cpu_sum: f64,
    cpu_count: usize,
    ram_sum: u128,
    ram_count: usize,
    gpu_sum: f64,
    gpu_count: usize,
    vram_sum: u128,
    vram_count: usize,
    vram_total_bytes: Option<u64>,
    power_sum: f64,
    power_count: usize,
    temp_sum: f64,
    temp_count: usize,
}

#[derive(Debug, Clone, Default)]
struct ResourceSample {
    cpu_percent: Option<f64>,
    ram_used_bytes: Option<u64>,
    gpu_percent: Option<f64>,
    vram_used_bytes: Option<u64>,
    vram_total_bytes: Option<u64>,
    gpu_power_watts: Option<f64>,
    gpu_temp_celsius: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

#[derive(Debug, Clone, Default)]
struct HardwareSnapshot {
    os: Option<String>,
    kernel: Option<String>,
    cpu_model: Option<String>,
    ram_total_bytes: Option<u64>,
    gpu: Option<GpuHardware>,
    ollama_models: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct GpuHardware {
    card: String,
    pci_bus: Option<String>,
    name: Option<String>,
    vendor_id: Option<String>,
    device_id: Option<String>,
    vram_total_bytes: Option<u64>,
    gtt_total_bytes: Option<u64>,
    current_sclk_mhz: Option<u64>,
    current_mclk_mhz: Option<u64>,
    temperature_celsius: Option<f64>,
    power_watts: Option<f64>,
    device_path: PathBuf,
    hwmon_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct GpuProbe {
    card: String,
    device_path: PathBuf,
    hwmon_path: Option<PathBuf>,
}

impl ResourceAccumulator {
    fn add(&mut self, sample: ResourceSample) {
        self.samples += 1;

        if let Some(value) = sample.cpu_percent {
            self.cpu_sum += value;
            self.cpu_count += 1;
        }

        if let Some(value) = sample.ram_used_bytes {
            self.ram_sum += value as u128;
            self.ram_count += 1;
        }

        if let Some(value) = sample.gpu_percent {
            self.gpu_sum += value;
            self.gpu_count += 1;
        }

        if let Some(value) = sample.vram_used_bytes {
            self.vram_sum += value as u128;
            self.vram_count += 1;
        }

        if sample.vram_total_bytes.is_some() {
            self.vram_total_bytes = sample.vram_total_bytes;
        }

        if let Some(value) = sample.gpu_power_watts {
            self.power_sum += value;
            self.power_count += 1;
        }

        if let Some(value) = sample.gpu_temp_celsius {
            self.temp_sum += value;
            self.temp_count += 1;
        }
    }

    fn finish(self) -> ResourceMetrics {
        ResourceMetrics {
            samples: self.samples,
            avg_cpu_percent: average_f64(self.cpu_sum, self.cpu_count),
            avg_ram_used_bytes: average_u64(self.ram_sum, self.ram_count),
            avg_gpu_percent: average_f64(self.gpu_sum, self.gpu_count),
            avg_vram_used_bytes: average_u64(self.vram_sum, self.vram_count),
            vram_total_bytes: self.vram_total_bytes,
            avg_gpu_power_watts: average_f64(self.power_sum, self.power_count),
            avg_gpu_temp_celsius: average_f64(self.temp_sum, self.temp_count),
        }
    }
}

fn average_f64(sum: f64, count: usize) -> Option<f64> {
    (count > 0).then_some(sum / count as f64)
}

fn average_u64(sum: u128, count: usize) -> Option<u64> {
    (count > 0).then_some((sum / count as u128) as u64)
}

fn run_with_resource_sampling<T>(operation: impl FnOnce() -> T) -> (T, ResourceMetrics) {
    let stop = Arc::new(AtomicBool::new(false));
    let sampler_stop = Arc::clone(&stop);
    let probe = detect_primary_gpu();
    let sampler = thread::spawn(move || sample_resources_until(sampler_stop, probe));
    let result = operation();

    stop.store(true, Ordering::SeqCst);
    let metrics = sampler.join().unwrap_or_default();

    (result, metrics)
}

fn sample_resources_until(stop: Arc<AtomicBool>, probe: Option<GpuProbe>) -> ResourceMetrics {
    let mut accumulator = ResourceAccumulator::default();
    let mut previous_cpu = read_cpu_times();

    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(250));
        let sample = read_resource_sample(&mut previous_cpu, probe.as_ref());
        accumulator.add(sample);
    }

    accumulator.finish()
}

fn read_resource_sample(
    previous_cpu: &mut Option<CpuTimes>,
    probe: Option<&GpuProbe>,
) -> ResourceSample {
    let cpu_percent = match (*previous_cpu, read_cpu_times()) {
        (Some(previous), Some(current)) => {
            *previous_cpu = Some(current);
            cpu_usage_percent(previous, current)
        }
        (_, current) => {
            *previous_cpu = current;
            None
        }
    };
    let (ram_used_bytes, _ram_total_bytes) = read_memory_usage();
    let gpu_percent = probe.and_then(read_gpu_busy_percent);
    let vram_used_bytes =
        probe.and_then(|probe| read_u64_file(probe.device_path.join("mem_info_vram_used")));
    let vram_total_bytes =
        probe.and_then(|probe| read_u64_file(probe.device_path.join("mem_info_vram_total")));
    let gpu_power_watts = probe.and_then(read_gpu_power_watts);
    let gpu_temp_celsius = probe.and_then(read_gpu_temp_celsius);

    ResourceSample {
        cpu_percent,
        ram_used_bytes,
        gpu_percent,
        vram_used_bytes,
        vram_total_bytes,
        gpu_power_watts,
        gpu_temp_celsius,
    }
}

fn read_cpu_times() -> Option<CpuTimes> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().find(|line| line.starts_with("cpu "))?;
    let values = line
        .split_whitespace()
        .skip(1)
        .filter_map(|value| value.parse::<u64>().ok())
        .collect::<Vec<_>>();

    if values.len() < 4 {
        return None;
    }

    let idle = values.get(3).copied().unwrap_or(0) + values.get(4).copied().unwrap_or(0);
    let total = values.iter().sum();

    Some(CpuTimes { idle, total })
}

fn cpu_usage_percent(previous: CpuTimes, current: CpuTimes) -> Option<f64> {
    let total = current.total.checked_sub(previous.total)?;
    let idle = current.idle.checked_sub(previous.idle)?;

    if total == 0 {
        return None;
    }

    Some(((total - idle) as f64 / total as f64) * 100.0)
}

fn read_memory_usage() -> (Option<u64>, Option<u64>) {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(content) => content,
        Err(_) => return (None, None),
    };
    let total = meminfo_bytes(&content, "MemTotal:");
    let available = meminfo_bytes(&content, "MemAvailable:");
    let used = match (total, available) {
        (Some(total), Some(available)) => total.checked_sub(available),
        _ => None,
    };

    (used, total)
}

fn meminfo_bytes(content: &str, key: &str) -> Option<u64> {
    let line = content.lines().find(|line| line.starts_with(key))?;
    let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;

    Some(kib * 1024)
}

fn detect_primary_gpu() -> Option<GpuProbe> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;
    let mut cards = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let number = name.strip_prefix("card")?;
            if number.is_empty() || !number.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            Some((name, entry.path().join("device")))
        })
        .collect::<Vec<_>>();
    cards.sort_by(|left, right| left.0.cmp(&right.0));

    for (card, device_path) in cards {
        let vendor = read_string_file(device_path.join("vendor")).unwrap_or_default();
        if vendor.trim() != "0x1002" && !device_path.join("gpu_busy_percent").exists() {
            continue;
        }

        return Some(GpuProbe {
            card,
            hwmon_path: detect_gpu_hwmon(&device_path),
            device_path,
        });
    }

    None
}

fn detect_gpu_hwmon(device_path: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(device_path.join("hwmon")).ok()?;

    entries.flatten().map(|entry| entry.path()).find(|path| {
        read_string_file(path.join("name"))
            .map(|name| name.trim() == "amdgpu")
            .unwrap_or(false)
    })
}

fn read_gpu_busy_percent(probe: &GpuProbe) -> Option<f64> {
    read_u64_file(probe.device_path.join("gpu_busy_percent")).map(|value| value as f64)
}

fn read_gpu_power_watts(probe: &GpuProbe) -> Option<f64> {
    let hwmon = probe.hwmon_path.as_ref()?;
    read_u64_file(hwmon.join("power1_average"))
        .or_else(|| read_u64_file(hwmon.join("power1_input")))
        .map(|microwatts| microwatts as f64 / 1_000_000.0)
}

fn read_gpu_temp_celsius(probe: &GpuProbe) -> Option<f64> {
    let hwmon = probe.hwmon_path.as_ref()?;
    read_u64_file(hwmon.join("temp1_input")).map(|millicelsius| millicelsius as f64 / 1000.0)
}

fn collect_hardware_snapshot(ollama_command: &str) -> HardwareSnapshot {
    HardwareSnapshot {
        os: os_pretty_name(),
        kernel: command_stdout("uname", &["-r"]).map(|value| value.trim().to_string()),
        cpu_model: cpu_model_name(),
        ram_total_bytes: read_memory_usage().1,
        gpu: collect_gpu_hardware(),
        ollama_models: list_ollama_models(ollama_command)
            .map(|models| models.into_iter().map(|model| model.name).collect())
            .unwrap_or_default(),
    }
}

fn collect_gpu_hardware() -> Option<GpuHardware> {
    let probe = detect_primary_gpu()?;
    let pci_bus = fs::canonicalize(&probe.device_path).ok().and_then(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    });

    Some(GpuHardware {
        card: probe.card.clone(),
        pci_bus: pci_bus.clone(),
        name: pci_bus.as_deref().and_then(gpu_name_from_lspci),
        vendor_id: read_string_file(probe.device_path.join("vendor"))
            .map(|value| value.trim().to_string()),
        device_id: read_string_file(probe.device_path.join("device"))
            .map(|value| value.trim().to_string()),
        vram_total_bytes: read_u64_file(probe.device_path.join("mem_info_vram_total")),
        gtt_total_bytes: read_u64_file(probe.device_path.join("mem_info_gtt_total")),
        current_sclk_mhz: active_dpm_mhz(&probe.device_path.join("pp_dpm_sclk")),
        current_mclk_mhz: active_dpm_mhz(&probe.device_path.join("pp_dpm_mclk")),
        temperature_celsius: read_gpu_temp_celsius(&probe),
        power_watts: read_gpu_power_watts(&probe),
        device_path: probe.device_path,
        hwmon_path: probe.hwmon_path,
    })
}

fn os_pretty_name() -> Option<String> {
    let content = fs::read_to_string("/etc/os-release").ok()?;
    let value = content
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))?;

    Some(value.trim_matches('"').to_string())
}

fn cpu_model_name() -> Option<String> {
    let content = fs::read_to_string("/proc/cpuinfo").ok()?;
    content.lines().find_map(|line| {
        line.strip_prefix("model name").and_then(|value| {
            value
                .split_once(':')
                .map(|(_, name)| name.trim().to_string())
        })
    })
}

fn gpu_name_from_lspci(pci_bus: &str) -> Option<String> {
    let output = command_stdout("lspci", &["-D"])?;
    output
        .lines()
        .find(|line| line.starts_with(pci_bus))
        .map(|line| line.to_string())
}

fn active_dpm_mhz(path: &Path) -> Option<u64> {
    let content = fs::read_to_string(path).ok()?;
    let line = content.lines().find(|line| line.contains('*'))?;
    let mhz = line.split_whitespace().find(|part| part.ends_with("Mhz"))?;

    mhz.trim_end_matches("Mhz").parse().ok()
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .ok()?;

    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

fn read_u64_file(path: impl AsRef<Path>) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_string_file(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn metrics_json(metrics: &ResourceMetrics) -> serde_json::Value {
    serde_json::json!({
        "samples": metrics.samples,
        "avg_cpu_percent": metrics.avg_cpu_percent,
        "avg_ram_used_bytes": metrics.avg_ram_used_bytes,
        "avg_gpu_percent": metrics.avg_gpu_percent,
        "avg_vram_used_bytes": metrics.avg_vram_used_bytes,
        "vram_total_bytes": metrics.vram_total_bytes,
        "avg_gpu_power_watts": metrics.avg_gpu_power_watts,
        "avg_gpu_temp_celsius": metrics.avg_gpu_temp_celsius,
    })
}

fn hardware_json(hardware: &HardwareSnapshot) -> serde_json::Value {
    serde_json::json!({
        "os": hardware.os.as_deref(),
        "kernel": hardware.kernel.as_deref(),
        "cpu_model": hardware.cpu_model.as_deref(),
        "ram_total_bytes": hardware.ram_total_bytes,
        "gpu": hardware.gpu.as_ref().map(|gpu| serde_json::json!({
            "card": gpu.card.as_str(),
            "pci_bus": gpu.pci_bus.as_deref(),
            "name": gpu.name.as_deref(),
            "vendor_id": gpu.vendor_id.as_deref(),
            "device_id": gpu.device_id.as_deref(),
            "vram_total_bytes": gpu.vram_total_bytes,
            "gtt_total_bytes": gpu.gtt_total_bytes,
            "current_sclk_mhz": gpu.current_sclk_mhz,
            "current_mclk_mhz": gpu.current_mclk_mhz,
            "temperature_celsius": gpu.temperature_celsius,
            "power_watts": gpu.power_watts,
            "device_path": gpu.device_path.display().to_string(),
            "hwmon_path": gpu.hwmon_path.as_ref().map(|path| path.display().to_string()),
        })),
        "ollama_models": hardware.ollama_models.as_slice(),
    })
}

fn write_format_compare_report(
    directory: &Path,
    hardware: &HardwareSnapshot,
    mode: DictationMode,
    prompt_input: ComparePromptInput,
    prompt_note: Option<&str>,
    total_elapsed_ms: u128,
    raw_transcript: &str,
    preprocessed: &str,
    results: &[FormatCompareResult],
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|source| {
        format!(
            "failed to create report directory {}: {source}",
            directory.display()
        )
    })?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let path = directory.join(format!("chirper-format-compare-{timestamp}.txt"));
    let mut report = String::new();

    let _ = writeln!(report, "Chirper format comparison");
    let _ = writeln!(report, "generated_unix_seconds: {timestamp}");
    let _ = writeln!(
        report,
        "{}",
        format_tested_summary(tested_model_count(results), total_elapsed_ms)
    );
    let _ = writeln!(report, "total_elapsed_ms: {total_elapsed_ms}");
    let _ = writeln!(report, "mode: {mode:?}");
    let _ = writeln!(report, "prompt_input: {}", prompt_input.label());
    if let Some(prompt_note) = prompt_note.map(str::trim).filter(|note| !note.is_empty()) {
        let _ = writeln!(report, "prompt_note:");
        let _ = writeln!(report, "{prompt_note}");
    }
    let _ = writeln!(report);
    let _ = writeln!(report, "Hardware:");
    write_hardware_snapshot(&mut report, hardware);
    let _ = writeln!(report);
    let _ = writeln!(report, "Raw transcript:");
    let _ = writeln!(report, "{raw_transcript}");
    let _ = writeln!(report);
    if prompt_input == ComparePromptInput::RawAndPreprocessed {
        let _ = writeln!(report, "Preprocessed draft (sent to model):");
    } else {
        let _ = writeln!(report, "Preprocessed draft (not sent to model):");
    }
    let _ = writeln!(report, "{preprocessed}");

    for result in results {
        let _ = writeln!(report);
        let _ = writeln!(
            report,
            "=== {} ({}, {}) ===",
            result.name,
            format_elapsed(result.elapsed_ms),
            format_metrics_summary(&result.metrics)
        );
        if let Some(output) = &result.output {
            let _ = writeln!(report, "{output}");
        } else if let Some(error) = &result.error {
            let _ = writeln!(report, "ERROR: {error}");
        }
    }

    fs::write(&path, report)
        .map_err(|source| format!("failed to write report {}: {source}", path.display()))?;

    Ok(path)
}

fn print_hardware_snapshot(hardware: &HardwareSnapshot) {
    let mut output = String::new();
    write_hardware_snapshot(&mut output, hardware);
    print!("{output}");
}

fn write_hardware_snapshot(output: &mut String, hardware: &HardwareSnapshot) {
    let _ = writeln!(
        output,
        "  os: {}",
        hardware.os.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  kernel: {}",
        hardware.kernel.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  cpu: {}",
        hardware.cpu_model.as_deref().unwrap_or("unknown")
    );
    let _ = writeln!(
        output,
        "  ram_total: {}",
        format_optional_bytes(hardware.ram_total_bytes)
    );

    if let Some(gpu) = &hardware.gpu {
        let _ = writeln!(output, "  gpu_card: {}", gpu.card);
        if let Some(name) = &gpu.name {
            let _ = writeln!(output, "  gpu_name: {name}");
        }
        if let Some(pci_bus) = &gpu.pci_bus {
            let _ = writeln!(output, "  gpu_pci_bus: {pci_bus}");
        }
        if let Some(vendor_id) = &gpu.vendor_id {
            let _ = writeln!(output, "  gpu_vendor_id: {vendor_id}");
        }
        if let Some(device_id) = &gpu.device_id {
            let _ = writeln!(output, "  gpu_device_id: {device_id}");
        }
        let _ = writeln!(
            output,
            "  gpu_vram_total: {}",
            format_optional_bytes(gpu.vram_total_bytes)
        );
        let _ = writeln!(
            output,
            "  gpu_gtt_total: {}",
            format_optional_bytes(gpu.gtt_total_bytes)
        );
        let _ = writeln!(
            output,
            "  gpu_sclk: {}",
            format_optional_mhz(gpu.current_sclk_mhz)
        );
        let _ = writeln!(
            output,
            "  gpu_mclk: {}",
            format_optional_mhz(gpu.current_mclk_mhz)
        );
        let _ = writeln!(
            output,
            "  gpu_power_now: {}",
            format_optional_watts(gpu.power_watts)
        );
        let _ = writeln!(
            output,
            "  gpu_temp_now: {}",
            format_optional_celsius(gpu.temperature_celsius)
        );
        let _ = writeln!(output, "  gpu_device_path: {}", gpu.device_path.display());
        if let Some(hwmon_path) = &gpu.hwmon_path {
            let _ = writeln!(output, "  gpu_hwmon_path: {}", hwmon_path.display());
        }
    } else {
        let _ = writeln!(output, "  gpu: unavailable");
    }

    if hardware.ollama_models.is_empty() {
        let _ = writeln!(output, "  ollama_models: none detected");
    } else {
        let _ = writeln!(
            output,
            "  ollama_models: {}",
            hardware.ollama_models.join(", ")
        );
    }
}

fn format_metrics_summary(metrics: &ResourceMetrics) -> String {
    if metrics.samples == 0 {
        return "telemetry unavailable".to_string();
    }

    let mut parts = Vec::new();
    parts.push(format!("samples {}", metrics.samples));

    if let Some(value) = metrics.avg_cpu_percent {
        parts.push(format!("cpu {:.1}%", value));
    }
    if let Some(value) = metrics.avg_ram_used_bytes {
        parts.push(format!("ram {}", format_bytes_decimal(value)));
    }
    if let Some(value) = metrics.avg_gpu_percent {
        parts.push(format!("gpu {:.1}%", value));
    }
    if let Some(value) = metrics.avg_vram_used_bytes {
        let vram = match metrics.vram_total_bytes {
            Some(total) => format!(
                "{}/{}",
                format_bytes_decimal(value),
                format_bytes_decimal(total)
            ),
            None => format_bytes_decimal(value),
        };
        parts.push(format!("vram {vram}"));
    }
    if let Some(value) = metrics.avg_gpu_power_watts {
        parts.push(format!("gpu power {:.0} W", value));
    }
    if let Some(value) = metrics.avg_gpu_temp_celsius {
        parts.push(format!("gpu temp {:.0} C", value));
    }

    if parts.len() == 1 {
        "telemetry unavailable".to_string()
    } else {
        parts.join(", ")
    }
}

fn format_optional_bytes(value: Option<u64>) -> String {
    value
        .map(format_bytes_decimal)
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_optional_mhz(value: Option<u64>) -> String {
    value
        .map(|value| format!("{value} MHz"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_optional_watts(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0} W"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_optional_celsius(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.0} C"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_bytes_decimal(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn format_elapsed(elapsed_ms: u128) -> String {
    if elapsed_ms >= 1000 {
        format!("{:.2}s", elapsed_ms as f64 / 1000.0)
    } else {
        format!("{elapsed_ms}ms")
    }
}

fn format_elapsed_words(elapsed_ms: u128) -> String {
    let mut seconds = (elapsed_ms / 1000) as u64;
    let hours = seconds / 3600;
    seconds %= 3600;
    let minutes = seconds / 60;
    seconds %= 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else if elapsed_ms >= 1000 {
        format!("{seconds}s")
    } else {
        format!("{elapsed_ms}ms")
    }
}

fn stop_ollama_model_silent(command: &str, model: &str) {
    let _ = Command::new(command)
        .arg("stop")
        .arg(model)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn dictate_test(seconds: Option<&str>) {
    let seconds = seconds
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3);

    let config = load_config_or_exit();
    let mut recorder = PipeWireRecorder::new(PipeWireRecorderOptions::from_config(&config));
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
    println!("ollama_command: {}", config.ollama_command);
    println!("ollama_model: {}", config.ollama_model);
    println!("codex_command: {}", config.codex_command);
    println!(
        "codex_model: {}",
        config.codex_model.as_deref().unwrap_or("<codex default>")
    );
    println!(
        "codex_reasoning_effort: {}",
        config
            .codex_reasoning_effort
            .as_deref()
            .unwrap_or("<default>")
    );
    println!(
        "codex_service_tier: {}",
        config.codex_service_tier.as_deref().unwrap_or("<default>")
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
    load_config_or_exit().dictation_mode
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
    let config = load_config_or_exit();
    let transcript = chirper_core::Transcript {
        text: text.to_string(),
        language: None,
    };

    match format_transcript_with_config(&config, &transcript, mode) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn format_transcript_with_config(
    config: &ChirperConfig,
    transcript: &chirper_core::Transcript,
    mode: DictationMode,
) -> Result<String, String> {
    match config.formatter_backend {
        FormatterBackend::None => Ok(transcript.text.clone()),
        FormatterBackend::Rules => format_with_rules(config, transcript, mode),
        FormatterBackend::Ollama => {
            let preformatted = format_with_rules(config, transcript, mode)?;
            OllamaFormatter::new(OllamaOptions::from_config(config))
                .format_with_context(transcript, &preformatted, mode)
                .map_err(|error| error.to_string())
        }
        FormatterBackend::Codex => {
            let preformatted = format_with_rules(config, transcript, mode)?;
            CodexFormatter::new(CodexOptions::from_config(config))
                .format_with_context(transcript, &preformatted, mode)
                .map_err(|error| error.to_string())
        }
        FormatterBackend::LlamaCpp => {
            eprintln!("formatter backend llama.cpp is not implemented yet; using raw transcript");
            Ok(transcript.text.clone())
        }
    }
}

fn format_with_rules(
    config: &ChirperConfig,
    transcript: &chirper_core::Transcript,
    mode: DictationMode,
) -> Result<String, String> {
    Ok(format_spoken_rules_with_vocabulary(
        &transcript.text,
        mode,
        &config.vocabulary,
    ))
}

fn transcribe_audio(audio: chirper_core::CapturedAudio) -> chirper_core::Transcript {
    let config = load_config_or_exit();

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

fn load_config_or_exit() -> ChirperConfig {
    match ChirperConfig::load_default() {
        Ok(config) => config,
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

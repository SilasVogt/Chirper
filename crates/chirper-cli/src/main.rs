use std::{
    collections::BTreeMap,
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chirper_api::{send_request, ApiRequest, ApiResponse};
use chirper_asr_whispercpp::{WhisperCppAsr, WhisperCppOptions};
use chirper_audio_pipewire::{DetachedRecording, PipeWireRecorder, PipeWireRecorderOptions};
use chirper_core::{
    AsrEngine, AudioSource, ChirperConfig, DictationMode, FormatterBackend, ServiceCommand,
    TextInserter, WorkflowState, WHISPER_MODEL_NAMES,
};
use chirper_formatter_ollama::{list_ollama_models, OllamaFormatter, OllamaModel, OllamaOptions};
use chirper_formatter_rules::format_spoken_rules_with_vocabulary;
use chirper_insertion_clipboard::ClipboardInserter;
use chirper_platform::{PlatformDiagnostics, RuntimeDiagnostics};

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
        });

        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("backend: {}", config.formatter_backend.as_config_value());
    println!("ollama_command: {}", config.ollama_command);
    println!("ollama_model: {}", config.ollama_model);
    println!("vocabulary_entries: {}", config.vocabulary.len());
}

fn formatter_use(args: Vec<String>) {
    let Some(selection) = args.first() else {
        eprintln!("usage: chirper formatter-use <none|rules|ollama> [ollama-model]");
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

    if let Err(error) = ChirperConfig::save_default_formatter_selection(backend, model) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected formatter: {}", backend.as_config_value());
    if let Some(model) = model {
        println!("ollama_model: {model}");
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

use std::{
    fs,
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
use chirper_formatter_ollama::{list_ollama_models, OllamaFormatter, OllamaOptions};
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
    preload_ollama_for_recording(&config);
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
            let formatter = OllamaFormatter::new(OllamaOptions::from_config(config));
            let started = Instant::now();
            let (result, metrics) =
                run_with_resource_sampling(|| formatter.format_ai_prompt(transcript));
            stop_ollama_model_silent(&config.ollama_command, &config.ollama_model);

            match result {
                Ok(run) => {
                    let elapsed_ms = started.elapsed().as_millis();
                    if let Err(error) = write_prompt_log(
                        config,
                        transcript,
                        &run.prompt,
                        &run.output,
                        &metrics,
                        elapsed_ms,
                    ) {
                        eprintln!("{error}");
                    }
                    if let Err(error) = prune_prompt_logs(config) {
                        eprintln!("{error}");
                    }

                    Ok(run.output)
                }
                Err(error) => {
                    if let Err(prune_error) = prune_prompt_logs(config) {
                        eprintln!("{prune_error}");
                    }
                    Err(error.to_string())
                }
            }
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

fn preload_ollama_for_recording(config: &ChirperConfig) {
    if config.formatter_backend != FormatterBackend::Ollama || !config.ollama_preload_on_recording {
        return;
    }

    let command = config.ollama_command.clone();
    let model = config.ollama_model.clone();
    thread::spawn(move || {
        let status = Command::new(&command)
            .arg("run")
            .arg("--nowordwrap")
            .arg("--hidethinking")
            .arg("--keepalive")
            .arg("10m")
            .arg(&model)
            .arg("")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        if let Err(error) = status {
            eprintln!("failed to preload Ollama model `{model}`: {error}");
        }
    });
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

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Default)]
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

#[derive(Debug, Clone, Copy)]
struct CpuTimes {
    idle: u64,
    total: u64,
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
        accumulator.add(read_resource_sample(&mut previous_cpu, probe.as_ref()));
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

    ResourceSample {
        cpu_percent,
        ram_used_bytes,
        gpu_percent: probe.and_then(read_gpu_busy_percent),
        vram_used_bytes: probe
            .and_then(|probe| read_u64_file(probe.device_path.join("mem_info_vram_used"))),
        vram_total_bytes: probe
            .and_then(|probe| read_u64_file(probe.device_path.join("mem_info_vram_total"))),
        gpu_power_watts: probe.and_then(read_gpu_power_watts),
        gpu_temp_celsius: probe.and_then(read_gpu_temp_celsius),
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

fn write_prompt_log(
    config: &ChirperConfig,
    transcript: &Transcript,
    prompt: &str,
    output: &str,
    metrics: &ResourceMetrics,
    elapsed_ms: u128,
) -> Result<(), String> {
    if config.format_log_retention_days == 0 {
        return Ok(());
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let directory = ChirperConfig::default_prompt_log_dir()
        .join(format!("chirper-format-{timestamp}-{}", std::process::id()));

    fs::create_dir_all(&directory).map_err(|source| {
        format!(
            "failed to create prompt log directory {}: {source}",
            directory.display()
        )
    })?;
    fs::write(directory.join("raw-transcript.txt"), &transcript.text)
        .map_err(|source| format!("failed to write prompt log raw transcript: {source}"))?;
    fs::write(directory.join("prompt.txt"), prompt)
        .map_err(|source| format!("failed to write prompt log prompt: {source}"))?;
    fs::write(directory.join("output.txt"), output)
        .map_err(|source| format!("failed to write prompt log output: {source}"))?;

    let hardware = collect_hardware_snapshot(&config.ollama_command);
    let metadata = serde_json::json!({
        "generated_unix_seconds": timestamp,
        "model": config.ollama_model,
        "hardware_tier": config.ai_hardware_tier.as_config_value(),
        "hardware_tier_label": config.ai_hardware_tier.label(),
        "hardware_tier_description": config.ai_hardware_tier.description(),
        "formatter_backend": config.formatter_backend.as_config_value(),
        "elapsed_ms": elapsed_ms,
        "log_retention_days": config.format_log_retention_days,
        "prompt_file": "prompt.txt",
        "raw_transcript_file": "raw-transcript.txt",
        "output_file": "output.txt",
        "metrics": metrics_json(metrics),
        "hardware": hardware_json(&hardware),
    });
    let metadata = serde_json::to_string_pretty(&metadata)
        .map_err(|source| format!("failed to encode prompt log metadata: {source}"))?;
    fs::write(directory.join("metadata.json"), format!("{metadata}\n"))
        .map_err(|source| format!("failed to write prompt log metadata: {source}"))?;

    Ok(())
}

fn prune_prompt_logs(config: &ChirperConfig) -> Result<(), String> {
    if config.format_log_retention_days == 0 {
        return Ok(());
    }

    let directory = ChirperConfig::default_prompt_log_dir();
    if !directory.exists() {
        return Ok(());
    }

    let retention = Duration::from_secs(config.format_log_retention_days.saturating_mul(86_400));
    let now = SystemTime::now();
    let entries = fs::read_dir(&directory).map_err(|source| {
        format!(
            "failed to read prompt log directory {}: {source}",
            directory.display()
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < retention {
            continue;
        }

        let result = if metadata.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = result {
            eprintln!(
                "failed to remove old prompt log {}: {error}",
                path.display()
            );
        }
    }

    Ok(())
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

use std::{
    collections::BTreeMap,
    env,
    fmt::Write,
    fs,
    io::{self, Write as IoWrite},
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
    AsrEngine, AudioSource, ChirperConfig, ChirperResult, CodexProfileConfig, DictationMode,
    FormatterBackend, GuiProfile, ServiceCommand, TextInserter, TranscriptionProfile,
    WorkflowState, WHISPER_MODEL_NAMES,
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
const ONBOARDING_WHISPER_MODELS: &[&str] = &["medium", "large-v3-turbo", "large-v3"];
const ONBOARDING_OLLAMA_MODELS: &[&str] = &["granite4.1:3b", "granite4.1:8b", "olmo2:7b"];

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();

    if matches!(first.as_deref(), Some("-h" | "--help" | "help")) {
        print_help();
        return;
    }

    if matches!(first.as_deref(), Some("-V" | "--version" | "version")) {
        println!("chirper {}", env!("CARGO_PKG_VERSION"));
        return;
    }

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

    if matches!(first.as_deref(), Some("record-start")) {
        record_start(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("record-stop")) {
        record_stop(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("transcribe-file")) {
        transcribe_file(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("diagnose")) {
        diagnose();
        return;
    }

    if matches!(first.as_deref(), Some("update-check")) {
        update_check(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("update")) {
        update(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("uninstall")) {
        uninstall(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("onboarding-check")) {
        onboarding_check(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("setup-status")) {
        setup_status(args.collect());
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

    if matches!(
        first.as_deref(),
        Some("transcription-current") | Some("transcription-profile-current")
    ) {
        transcription_current(args.collect());
        return;
    }

    if matches!(
        first.as_deref(),
        Some("transcription-list") | Some("transcription-profile-list")
    ) {
        transcription_list(args.collect());
        return;
    }

    if matches!(
        first.as_deref(),
        Some("transcription-use") | Some("transcription-profile-use")
    ) {
        transcription_use(args.next());
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

    if matches!(first.as_deref(), Some("gui-current")) {
        gui_current();
        return;
    }

    if matches!(first.as_deref(), Some("gui-use")) {
        gui_use(args.next());
        return;
    }

    if matches!(first.as_deref(), Some("ai-format-current")) {
        ai_format_current(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("ai-format-use")) {
        ai_format_use(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("ai-format-logs")) {
        ai_format_logs(args.collect());
        return;
    }

    if matches!(first.as_deref(), Some("ai-format-preload")) {
        ai_format_preload(args.collect());
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
        ServiceCommand::StartRecording => call_daemon(ApiRequest::StartRecording { audio: None }),
        ServiceCommand::StopRecording => call_daemon(ApiRequest::StopRecording),
        ServiceCommand::SetMode(mode) => set_mode(mode),
        ServiceCommand::OpenSettings => open_settings(),
    }
}

fn print_help() {
    println!(
        "\
chirper {}

Usage:
  chirper <command> [options]

Core commands:
  status                       Show local config status
  start | stop | toggle         Control dictation
  daemon-status                Check the user daemon
  diagnose                     Check local runtime dependencies
  setup-status                 Check whether onboarding setup is complete
  audio-list                   List PipeWire audio targets
  model-list                   List local Whisper models
  formatter-current            Show formatter config
  gui-current                  Show installed GUI profile
  gui-use                      Select installed GUI profile
  settings                     Open the installed GUI settings app
  update-check                 Check for release updates
  update                       Install an available update
  uninstall                    Remove user-local install artifacts

Run a subcommand with --help where supported for detailed options.",
        env!("CARGO_PKG_VERSION")
    );
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

#[derive(Debug)]
struct UpdateCheckOptions {
    source_dir: Option<PathBuf>,
    branch: Option<String>,
    mode: UpdateMode,
    json: bool,
}

#[derive(Debug)]
struct UpdateOptions {
    source_dir: Option<PathBuf>,
    branch: Option<String>,
    mode: UpdateMode,
    profile: String,
    gui: GuiProfile,
    with_whispercpp: bool,
    with_service: bool,
    whisper_backend: Option<String>,
    whisper_model: Option<String>,
    reinstall: bool,
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateMode {
    Releases,
    Canary,
}

impl UpdateMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Releases => "releases",
            Self::Canary => "canary",
        }
    }
}

#[derive(Debug)]
enum UpdateStatus {
    Releases(ReleaseUpdateStatus),
    Canary(CanaryUpdateStatus),
}

impl UpdateStatus {
    fn source_dir(&self) -> &Path {
        match self {
            Self::Releases(status) => &status.source_dir,
            Self::Canary(status) => &status.source_dir,
        }
    }

    fn dirty(&self) -> bool {
        match self {
            Self::Releases(status) => status.dirty,
            Self::Canary(status) => status.dirty,
        }
    }

    fn update_available(&self) -> bool {
        match self {
            Self::Releases(status) => status.update_available,
            Self::Canary(status) => status.behind > 0,
        }
    }

    fn target_ref(&self) -> Result<UpdateTargetRef, String> {
        match self {
            Self::Releases(status) => {
                let latest = status.latest.as_ref().ok_or_else(|| {
                    "no release tags found; use `chirper update --mode canary` for main".to_string()
                })?;
                Ok(UpdateTargetRef::Ref(latest.tag.clone()))
            }
            Self::Canary(status) => Ok(UpdateTargetRef::Branch(status.branch.clone())),
        }
    }
}

#[derive(Debug)]
enum UpdateTargetRef {
    Branch(String),
    Ref(String),
}

#[derive(Debug)]
struct CanaryUpdateStatus {
    source_dir: PathBuf,
    branch: String,
    upstream: String,
    local_sha: String,
    upstream_sha: String,
    ahead: u32,
    behind: u32,
    dirty: bool,
}

#[derive(Debug)]
struct ReleaseUpdateStatus {
    source_dir: PathBuf,
    local_sha: String,
    current: Option<ReleaseTag>,
    latest: Option<ReleaseTag>,
    dirty: bool,
    update_available: bool,
}

#[derive(Debug, Clone)]
struct ReleaseTag {
    tag: String,
    version: ReleaseVersion,
    sha: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

fn update_check(args: Vec<String>) {
    let options = match parse_update_check_args(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: chirper update-check [--json] [--mode releases|canary] [--source-dir PATH] [--branch NAME]");
            std::process::exit(2);
        }
    };

    let status = match update_status(
        options.mode,
        options.source_dir.as_deref(),
        options.branch.as_deref(),
    ) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if options.json {
        let value = update_status_json(&status);
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    print_update_status(&status);
}

fn update(args: Vec<String>) {
    let options = match parse_update_args(args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: chirper update [--mode releases|canary] [--source-dir PATH] [--branch NAME] [--profile debug|release] [--gui gnome|none] [--with-whispercpp] [--no-service] [--reinstall] [--dry-run]");
            std::process::exit(2);
        }
    };

    let status = match update_status(
        options.mode,
        options.source_dir.as_deref(),
        options.branch.as_deref(),
    ) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    print_update_status(&status);

    if status.dirty() && !options.dry_run {
        let _ = io::stdout().flush();
        eprintln!();
        eprintln!("source checkout has local changes; commit or stash them before updating");
        std::process::exit(1);
    }

    if !status.update_available() && !options.reinstall {
        println!();
        if matches!(&status, UpdateStatus::Releases(status) if status.latest.is_none()) {
            println!("no release tags found; use `chirper update --mode canary` for main");
        } else {
            println!("already up to date; pass --reinstall to rebuild and reinstall anyway");
        }
        return;
    }

    let target_ref = match status.target_ref() {
        Ok(target_ref) => target_ref,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    let install_script = status.source_dir().join("scripts/install.sh");
    if !install_script.is_file() {
        eprintln!("installer not found: {}", install_script.display());
        std::process::exit(1);
    }

    let mut command = Command::new(&install_script);
    command
        .current_dir(env::temp_dir())
        .arg("--source-dir")
        .arg(status.source_dir())
        .arg("--profile")
        .arg(&options.profile)
        .arg("--gui")
        .arg(options.gui.as_config_value());

    match target_ref {
        UpdateTargetRef::Branch(branch) => {
            command.arg("--branch").arg(branch);
        }
        UpdateTargetRef::Ref(ref_name) => {
            command.arg("--ref").arg(ref_name);
        }
    }

    if !options.with_whispercpp {
        command.arg("--no-whispercpp");
    }

    if !options.with_service {
        command.arg("--no-service");
    }

    if let Some(backend) = &options.whisper_backend {
        command.arg("--whisper-backend").arg(backend);
    }

    if let Some(model) = &options.whisper_model {
        command.arg("--whisper-model").arg(model);
    }

    if options.dry_run {
        println!();
        println!("would run:");
        println!("  {}", shell_command_preview(&command));
        return;
    }

    println!();
    println!("running updater:");
    println!("  {}", shell_command_preview(&command));

    let status = match command.status() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("failed to run {}: {error}", install_script.display());
            std::process::exit(1);
        }
    };

    if !status.success() {
        eprintln!("update failed with status {status}");
        std::process::exit(1);
    }

    println!();
    println!("Chirper update finished.");
    println!("If GNOME extension UI changed, log out and back in on Wayland to load the new extension code.");
}

fn uninstall(args: Vec<String>) {
    let source_dir = match resolve_source_dir(None) {
        Ok(source_dir) => source_dir,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let uninstall_script = source_dir.join("scripts/uninstall.sh");

    if !uninstall_script.is_file() {
        eprintln!("uninstaller not found: {}", uninstall_script.display());
        std::process::exit(1);
    }

    let status = match Command::new(&uninstall_script).args(args).status() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("failed to run {}: {error}", uninstall_script.display());
            std::process::exit(1);
        }
    };

    if !status.success() {
        eprintln!("uninstall failed with status {status}");
        std::process::exit(1);
    }
}

fn parse_update_check_args(args: Vec<String>) -> Result<UpdateCheckOptions, String> {
    let mut options = UpdateCheckOptions {
        source_dir: None,
        branch: None,
        mode: default_update_mode()?,
        json: false,
    };
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--json" => {
                options.json = true;
                index += 1;
            }
            "--source-dir" => {
                options.source_dir = Some(PathBuf::from(
                    args.get(index + 1)
                        .ok_or_else(|| "--source-dir requires a path".to_string())?,
                ));
                index += 2;
            }
            "--branch" => {
                options.branch = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "--branch requires a name".to_string())?
                        .to_string(),
                );
                index += 2;
            }
            "--mode" | "--channel" => {
                options.mode = parse_update_mode(
                    args.get(index + 1)
                        .ok_or_else(|| "--mode requires releases or canary".to_string())?,
                )?;
                index += 2;
            }
            "-h" | "--help" => {
                println!(
                    "usage: chirper update-check [--json] [--mode releases|canary] [--source-dir PATH] [--branch NAME]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown update-check argument: {other}")),
        }
    }

    Ok(options)
}

fn parse_update_args(args: Vec<String>) -> Result<UpdateOptions, String> {
    let mut options = UpdateOptions {
        source_dir: None,
        branch: None,
        mode: default_update_mode()?,
        profile: env::var("CHIRPER_BUILD_PROFILE").unwrap_or_else(|_| "release".to_string()),
        gui: default_gui_profile()?,
        with_whispercpp: false,
        with_service: true,
        whisper_backend: None,
        whisper_model: None,
        reinstall: false,
        dry_run: false,
    };
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--source-dir" => {
                options.source_dir = Some(PathBuf::from(
                    args.get(index + 1)
                        .ok_or_else(|| "--source-dir requires a path".to_string())?,
                ));
                index += 2;
            }
            "--branch" => {
                options.branch = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "--branch requires a name".to_string())?
                        .to_string(),
                );
                index += 2;
            }
            "--mode" | "--channel" => {
                options.mode = parse_update_mode(
                    args.get(index + 1)
                        .ok_or_else(|| "--mode requires releases or canary".to_string())?,
                )?;
                index += 2;
            }
            "--profile" => {
                options.profile = args
                    .get(index + 1)
                    .ok_or_else(|| "--profile requires debug or release".to_string())?
                    .to_string();
                index += 2;
            }
            "--gui" => {
                options.gui = parse_gui_profile(
                    args.get(index + 1)
                        .ok_or_else(|| "--gui requires gnome or none".to_string())?,
                )?;
                index += 2;
            }
            "--with-whispercpp" => {
                options.with_whispercpp = true;
                index += 1;
            }
            "--whisper-backend" => {
                options.with_whispercpp = true;
                options.whisper_backend = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "--whisper-backend requires a backend".to_string())?
                        .to_string(),
                );
                index += 2;
            }
            "--whisper-model" => {
                options.with_whispercpp = true;
                options.whisper_model = Some(
                    args.get(index + 1)
                        .ok_or_else(|| "--whisper-model requires a model name".to_string())?
                        .to_string(),
                );
                index += 2;
            }
            "--no-service" => {
                options.with_service = false;
                index += 1;
            }
            "--no-gui" | "--no-gnome-extension" => {
                options.gui = GuiProfile::None;
                index += 1;
            }
            "--reinstall" => {
                options.reinstall = true;
                index += 1;
            }
            "--dry-run" => {
                options.dry_run = true;
                index += 1;
            }
            "-h" | "--help" => {
                println!("usage: chirper update [--mode releases|canary] [--source-dir PATH] [--branch NAME] [--profile debug|release] [--gui gnome|none] [--with-whispercpp] [--no-service] [--reinstall] [--dry-run]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown update argument: {other}")),
        }
    }

    match options.profile.as_str() {
        "debug" | "release" => {}
        other => return Err(format!("unsupported profile: {other}")),
    }

    Ok(options)
}

fn default_update_mode() -> Result<UpdateMode, String> {
    match env::var("CHIRPER_UPDATE_MODE") {
        Ok(value) if !value.trim().is_empty() => parse_update_mode(&value),
        _ => Ok(UpdateMode::Releases),
    }
}

fn parse_update_mode(value: &str) -> Result<UpdateMode, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "release" | "releases" | "stable" => Ok(UpdateMode::Releases),
        "canary" | "main" | "source" => Ok(UpdateMode::Canary),
        other => Err(format!(
            "unsupported update mode `{other}`; use releases or canary"
        )),
    }
}

fn default_gui_profile() -> Result<GuiProfile, String> {
    match env::var("CHIRPER_GUI") {
        Ok(value) if !value.trim().is_empty() => parse_gui_profile(&value),
        _ => ChirperConfig::load_default()
            .map(|config| config.gui_profile)
            .map_err(|error| error.to_string()),
    }
}

fn parse_gui_profile(value: &str) -> Result<GuiProfile, String> {
    value
        .parse::<GuiProfile>()
        .map_err(|error| error.to_string())
}

fn update_status(
    mode: UpdateMode,
    source_dir: Option<&Path>,
    branch: Option<&str>,
) -> Result<UpdateStatus, String> {
    match mode {
        UpdateMode::Releases => release_update_status(source_dir).map(UpdateStatus::Releases),
        UpdateMode::Canary => canary_update_status(source_dir, branch).map(UpdateStatus::Canary),
    }
}

fn canary_update_status(
    source_dir: Option<&Path>,
    branch: Option<&str>,
) -> Result<CanaryUpdateStatus, String> {
    let source_dir = resolve_source_dir(source_dir)?;
    ensure_source_checkout(&source_dir)?;
    let branch = branch
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "main".to_string());

    git_stdout(&source_dir, &["fetch", "--prune", "origin", &branch])?;
    let upstream = format!("origin/{branch}");

    let local_sha = git_stdout(&source_dir, &["rev-parse", "HEAD"])?;
    let upstream_sha = git_stdout(&source_dir, &["rev-parse", &upstream])?;
    let counts = git_stdout(
        &source_dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("HEAD...{upstream}"),
        ],
    )?;
    let (ahead, behind) = parse_ahead_behind(&counts)?;
    let dirty = !git_stdout(&source_dir, &["status", "--porcelain"])?.is_empty();

    Ok(CanaryUpdateStatus {
        source_dir,
        branch,
        upstream,
        local_sha,
        upstream_sha,
        ahead,
        behind,
        dirty,
    })
}

fn release_update_status(source_dir: Option<&Path>) -> Result<ReleaseUpdateStatus, String> {
    let source_dir = resolve_source_dir(source_dir)?;
    ensure_source_checkout(&source_dir)?;
    git_stdout(&source_dir, &["fetch", "--prune", "--tags", "origin"])?;

    let local_sha = git_stdout(&source_dir, &["rev-parse", "HEAD"])?;
    let current = current_release_tag(&source_dir)?;
    let latest = latest_release_tag(&source_dir)?;
    let dirty = !git_stdout(&source_dir, &["status", "--porcelain"])?.is_empty();
    let update_available = match (&current, &latest) {
        (_, None) => false,
        (Some(current), Some(latest)) if current.sha == latest.sha => false,
        (Some(current), Some(latest)) => latest.version > current.version,
        (None, Some(latest)) => latest.sha != local_sha,
    };

    Ok(ReleaseUpdateStatus {
        source_dir,
        local_sha,
        current,
        latest,
        dirty,
        update_available,
    })
}

fn current_release_tag(source_dir: &Path) -> Result<Option<ReleaseTag>, String> {
    let tags = git_stdout(source_dir, &["tag", "--points-at", "HEAD"])?;
    let mut releases = Vec::new();

    for tag in tags.lines().map(str::trim).filter(|tag| !tag.is_empty()) {
        if let Some(version) = parse_release_version_tag(tag) {
            let sha = git_stdout(source_dir, &["rev-list", "-n", "1", tag])?;
            releases.push(ReleaseTag {
                tag: tag.to_string(),
                version,
                sha,
            });
        }
    }

    releases.sort_by_key(|release| release.version);
    Ok(releases.pop())
}

fn latest_release_tag(source_dir: &Path) -> Result<Option<ReleaseTag>, String> {
    let tags = git_stdout(source_dir, &["tag", "--list"])?;
    let mut releases = Vec::new();

    for tag in tags.lines().map(str::trim).filter(|tag| !tag.is_empty()) {
        if let Some(version) = parse_release_version_tag(tag) {
            let sha = git_stdout(source_dir, &["rev-list", "-n", "1", tag])?;
            releases.push(ReleaseTag {
                tag: tag.to_string(),
                version,
                sha,
            });
        }
    }

    releases.sort_by_key(|release| release.version);
    Ok(releases.pop())
}

fn parse_release_version_tag(tag: &str) -> Option<ReleaseVersion> {
    let value = tag.strip_prefix('v').unwrap_or(tag);
    let mut parts = value.split('.');
    let major = parse_version_part(parts.next()?)?;
    let minor = parse_version_part(parts.next()?)?;
    let patch = parse_version_part(parts.next()?)?;

    if parts.next().is_some() {
        return None;
    }

    Some(ReleaseVersion {
        major,
        minor,
        patch,
    })
}

fn parse_version_part(value: &str) -> Option<u64> {
    if value.is_empty() || !value.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    value.parse().ok()
}

fn resolve_source_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }

    if let Some(path) = env::var_os("CHIRPER_SOURCE_DIR") {
        return Ok(PathBuf::from(path));
    }

    if let Some(path) = env::current_exe()
        .ok()
        .and_then(|path| fs::canonicalize(path).ok())
        .and_then(|path| find_source_ancestor(&path))
    {
        return Ok(path);
    }

    if let Some(path) = env::current_dir()
        .ok()
        .and_then(|path| find_source_ancestor(&path))
    {
        return Ok(path);
    }

    Ok(ChirperConfig::default_data_dir().join("source"))
}

fn find_source_ancestor(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.join("Cargo.toml").is_file() && ancestor.join("scripts/install.sh").is_file() {
            return Some(ancestor.to_path_buf());
        }
    }

    None
}

fn ensure_source_checkout(source_dir: &Path) -> Result<(), String> {
    if !source_dir.join(".git").exists() {
        return Err(format!(
            "source checkout not found at {}; install Chirper first or pass --source-dir",
            source_dir.display()
        ));
    }

    if !source_dir.join("scripts/install.sh").is_file() {
        return Err(format!(
            "Chirper installer not found at {}",
            source_dir.join("scripts/install.sh").display()
        ));
    }

    Ok(())
}

fn parse_ahead_behind(value: &str) -> Result<(u32, u32), String> {
    let mut parts = value.split_whitespace();
    let ahead = parts
        .next()
        .ok_or_else(|| format!("unexpected git ahead/behind output: {value}"))?
        .parse::<u32>()
        .map_err(|_| format!("unexpected git ahead count: {value}"))?;
    let behind = parts
        .next()
        .ok_or_else(|| format!("unexpected git ahead/behind output: {value}"))?
        .parse::<u32>()
        .map_err(|_| format!("unexpected git behind count: {value}"))?;

    Ok((ahead, behind))
}

fn update_status_json(status: &UpdateStatus) -> serde_json::Value {
    match status {
        UpdateStatus::Releases(status) => serde_json::json!({
            "mode": UpdateMode::Releases.as_str(),
            "source_dir": status.source_dir.display().to_string(),
            "branch": "releases",
            "upstream": status.latest.as_ref().map(|release| release.tag.as_str()),
            "local_sha": status.local_sha.as_str(),
            "upstream_sha": status.latest.as_ref().map(|release| release.sha.as_str()),
            "ahead": 0,
            "behind": if status.update_available { 1 } else { 0 },
            "dirty": status.dirty,
            "current_tag": status.current.as_ref().map(|release| release.tag.as_str()),
            "latest_tag": status.latest.as_ref().map(|release| release.tag.as_str()),
            "update_available": status.update_available,
        }),
        UpdateStatus::Canary(status) => serde_json::json!({
            "mode": UpdateMode::Canary.as_str(),
            "source_dir": status.source_dir.display().to_string(),
            "branch": status.branch.as_str(),
            "upstream": status.upstream.as_str(),
            "local_sha": status.local_sha.as_str(),
            "upstream_sha": status.upstream_sha.as_str(),
            "ahead": status.ahead,
            "behind": status.behind,
            "dirty": status.dirty,
            "current_tag": null,
            "latest_tag": null,
            "update_available": status.behind > 0,
        }),
    }
}

fn print_update_status(status: &UpdateStatus) {
    match status {
        UpdateStatus::Releases(status) => {
            println!("mode: releases");
            println!("source_dir: {}", status.source_dir.display());
            println!(
                "current_tag: {}",
                status
                    .current
                    .as_ref()
                    .map(|release| release.tag.as_str())
                    .unwrap_or("none")
            );
            println!(
                "latest_tag: {}",
                status
                    .latest
                    .as_ref()
                    .map(|release| release.tag.as_str())
                    .unwrap_or("none")
            );
            println!("local: {}", short_commit(&status.local_sha));
            println!(
                "remote: {}",
                status
                    .latest
                    .as_ref()
                    .map(|release| short_commit(&release.sha))
                    .unwrap_or("none")
            );
            println!("dirty: {}", status.dirty);

            if status.latest.is_none() {
                println!("status: no release tags found");
            } else if status.update_available {
                println!("status: update available");
            } else {
                println!("status: up to date");
            }
        }
        UpdateStatus::Canary(status) => {
            println!("mode: canary");
            println!("source_dir: {}", status.source_dir.display());
            println!("branch: {}", status.branch);
            println!("upstream: {}", status.upstream);
            println!("local: {}", short_commit(&status.local_sha));
            println!("remote: {}", short_commit(&status.upstream_sha));
            println!("ahead: {}", status.ahead);
            println!("behind: {}", status.behind);
            println!("dirty: {}", status.dirty);

            if status.behind > 0 {
                println!("status: update available");
            } else if status.ahead > 0 {
                println!("status: local checkout is ahead of upstream");
            } else {
                println!("status: up to date");
            }
        }
    }
}

fn short_commit(value: &str) -> &str {
    value.get(..7).unwrap_or(value)
}

fn git_stdout(source_dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(source_dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("git {:?} failed with status {}", args, output.status)
        } else {
            stderr
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn shell_command_preview(command: &Command) -> String {
    let mut preview = shell_quote(command.get_program().to_string_lossy().as_ref());
    for arg in command.get_args() {
        preview.push(' ');
        preview.push_str(&shell_quote(arg.to_string_lossy().as_ref()));
    }
    preview
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=+".contains(character))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_command(mut args: impl Iterator<Item = String>) -> ServiceCommand {
    match args.next().as_deref() {
        None | Some("status") => ServiceCommand::GetStatus,
        Some("toggle") => ServiceCommand::Toggle,
        Some("start") => ServiceCommand::StartRecording,
        Some("stop") => ServiceCommand::StopRecording,
        Some("settings") => ServiceCommand::OpenSettings,
        Some("mode") => parse_mode(args.next().as_deref()),
        Some(command) => {
            eprintln!("unknown command: {command}");
            eprintln!("run `chirper --help` for available commands");
            std::process::exit(2);
        }
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

fn record_start(args: Vec<String>) {
    let (json, state_path) = parse_record_state_args(args, "record-start");

    if read_toggle_state(&state_path).is_some() {
        eprintln!("recording is already active for {}", state_path.display());
        std::process::exit(1);
    }

    let config = load_config_or_exit();
    let recording =
        match PipeWireRecorder::start_detached(PipeWireRecorderOptions::from_config(&config)) {
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

    if json {
        print_recording_json(&recording, &state_path);
    } else {
        println!("started recording: {}", recording.audio.path.display());
        println!("pid: {}", recording.pid);
        println!("state_path: {}", state_path.display());
    }
}

fn record_stop(args: Vec<String>) {
    let (json, state_path) = parse_record_state_args(args, "record-stop");
    let Some(recording) = read_toggle_state(&state_path) else {
        eprintln!("no active recording found at {}", state_path.display());
        std::process::exit(1);
    };

    let audio = match PipeWireRecorder::stop_detached(&recording) {
        Ok(audio) => audio,
        Err(error) => {
            let _ = fs::remove_file(&state_path);
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let _ = fs::remove_file(&state_path);

    if json {
        let value = serde_json::json!({
            "path": audio.path,
            "sample_rate_hz": audio.sample_rate_hz,
            "channels": audio.channels,
            "state_path": state_path,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
    } else {
        println!("stopped recording: {}", audio.path.display());
        println!("sample_rate_hz: {}", audio.sample_rate_hz);
        println!("channels: {}", audio.channels);
        println!("state_path: {}", state_path.display());
    }
}

fn parse_record_state_args(args: Vec<String>, command: &str) -> (bool, PathBuf) {
    let mut json = false;
    let mut state_path = manual_record_state_path();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--json" {
            json = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--state=") {
            state_path = expand_user_path(value);
            index += 1;
        } else if arg == "--state" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("usage: chirper {command} [--json] [--state PATH]");
                std::process::exit(1);
            };
            state_path = expand_user_path(value);
            index += 2;
        } else {
            eprintln!("usage: chirper {command} [--json] [--state PATH]");
            std::process::exit(1);
        }
    }

    (json, state_path)
}

fn print_recording_json(recording: &DetachedRecording, state_path: &Path) {
    let value = serde_json::json!({
        "pid": recording.pid,
        "path": recording.audio.path,
        "sample_rate_hz": recording.audio.sample_rate_hz,
        "channels": recording.audio.channels,
        "state_path": state_path,
    });
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}

fn transcribe_file(args: Vec<String>) {
    let (json, audio_path, model_path, profile) = match parse_transcribe_file_args(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("{error}");
            eprintln!(
                "usage: chirper transcribe-file [--json] [--profile balanced|fast] [--fast] <audio.wav> [model.bin]"
            );
            std::process::exit(1);
        }
    };
    let Some(audio_path) = audio_path else {
        eprintln!(
            "usage: chirper transcribe-file [--json] [--profile balanced|fast] [--fast] <audio.wav> [model.bin]"
        );
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

    if let Some(profile) = profile {
        config.transcription_profile = profile;
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

    let ((transcript, elapsed_ms), metrics) = run_with_resource_sampling(|| {
        let started = Instant::now();
        let transcript = asr.transcribe(&audio);
        let elapsed_ms = started.elapsed().as_millis();
        (transcript, elapsed_ms)
    });
    let transcript = match transcript {
        Ok(transcript) => transcript,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if json {
        let value = serde_json::json!({
            "text": transcript.text,
            "language": transcript.language,
            "elapsed_ms": elapsed_ms,
            "metrics": metrics_json(&metrics),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("{}", transcript.text);
}

type TranscribeFileArgs = (
    bool,
    Option<String>,
    Option<String>,
    Option<TranscriptionProfile>,
);

fn parse_transcribe_file_args(args: Vec<String>) -> Result<TranscribeFileArgs, String> {
    let mut json = false;
    let mut profile = None;
    let mut positional = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];

        if arg == "--json" {
            json = true;
            index += 1;
        } else if arg == "--fast" {
            profile = Some(TranscriptionProfile::Fast);
            index += 1;
        } else if arg == "--balanced" {
            profile = Some(TranscriptionProfile::Balanced);
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            profile = Some(
                parse_transcription_profile(value)
                    .ok_or_else(|| format!("unknown transcription profile: {value}"))?,
            );
            index += 1;
        } else if arg == "--profile" {
            if let Some(value) = args.get(index + 1) {
                profile = Some(
                    parse_transcription_profile(value)
                        .ok_or_else(|| format!("unknown transcription profile: {value}"))?,
                );
                index += 2;
            } else {
                return Err("--profile requires balanced or fast".to_string());
            }
        } else {
            positional.push(arg.clone());
            index += 1;
        }
    }

    if positional.len() > 2 {
        return Err("too many positional arguments".to_string());
    }

    Ok((
        json,
        positional.first().cloned(),
        positional.get(1).cloned(),
        profile,
    ))
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

fn onboarding_check(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let installed_whisper_models = installed_models();
    let ollama_result = list_ollama_models(&config.ollama_command);
    let (ollama_available, ollama_error, ollama_models) = match ollama_result {
        Ok(models) => (true, None, models),
        Err(error) => (false, Some(error.to_string()), Vec::new()),
    };
    let ollama_model_names = ollama_models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();

    let value = serde_json::json!({
        "commands": {
            "pw_record": {
                "command": "pw-record",
                "available": command_available("pw-record"),
            },
            "whisper": {
                "command": config.whispercpp_command,
                "available": command_available(&config.whispercpp_command),
            },
            "ollama": {
                "command": config.ollama_command,
                "available": ollama_available,
                "error": ollama_error,
            },
            "codex": {
                "command": config.codex_command,
                "available": command_available(&config.codex_command),
            },
        },
        "whisper_models": ONBOARDING_WHISPER_MODELS
            .iter()
            .map(|name| {
                let installed = installed_whisper_models.get(*name);
                serde_json::json!({
                    "name": name,
                    "installed": installed.is_some(),
                    "path": installed
                        .map(|model| model.path.clone())
                        .unwrap_or_else(|| ChirperConfig::default_model_path(name)),
                    "bytes": installed.map(|model| model.bytes),
                })
            })
            .collect::<Vec<_>>(),
        "ollama_models": ONBOARDING_OLLAMA_MODELS
            .iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "installed": ollama_model_names.contains(name),
                })
            })
            .collect::<Vec<_>>(),
        "current": {
            "whisper_model": config.whisper_model,
            "whisper_model_path": config.whispercpp_model_path,
            "formatter_backend": config.formatter_backend.as_config_value(),
            "ollama_model": config.ollama_model,
            "codex_model": config.codex_model,
            "codex_reasoning_effort": config.codex_reasoning_effort,
        },
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("onboarding checks:");
    for (key, command) in value["commands"].as_object().unwrap() {
        let available = command["available"].as_bool().unwrap_or(false);
        let command_name = command["command"].as_str().unwrap_or(key);
        println!(
            "  {key}: {} ({command_name})",
            if available { "ok" } else { "missing" }
        );
    }
    println!("whisper models:");
    for model in value["whisper_models"].as_array().unwrap() {
        println!(
            "  {}: {}",
            model["name"].as_str().unwrap_or("unknown"),
            if model["installed"].as_bool().unwrap_or(false) {
                "installed"
            } else {
                "missing"
            }
        );
    }
    println!("ollama models:");
    for model in value["ollama_models"].as_array().unwrap() {
        println!(
            "  {}: {}",
            model["name"].as_str().unwrap_or("unknown"),
            if model["installed"].as_bool().unwrap_or(false) {
                "installed"
            } else {
                "missing"
            }
        );
    }
}

fn setup_status(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let whisper_model_path = config.whispercpp_model_path.clone();
    let whisper_model_configured = whisper_model_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let formatter_configured = config.formatter_backend != FormatterBackend::None;
    let mut missing = Vec::new();

    if !whisper_model_configured {
        missing.push("whisper_model");
    }
    if !formatter_configured {
        missing.push("formatter");
    }

    let setup_required = !missing.is_empty();

    if json {
        let value = serde_json::json!({
            "setup_required": setup_required,
            "missing": missing,
            "whisper": {
                "model": config.whisper_model,
                "path": whisper_model_path,
                "configured": whisper_model_configured,
            },
            "formatter": {
                "backend": config.formatter_backend.as_config_value(),
                "configured": formatter_configured,
                "ollama_model": config.ollama_model,
                "codex_model": config.codex_model,
            },
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!(
        "setup: {}",
        if setup_required {
            "incomplete"
        } else {
            "complete"
        }
    );
    println!("whisper_model_configured: {whisper_model_configured}");
    println!("formatter_configured: {formatter_configured}");
    if !missing.is_empty() {
        println!("missing: {}", missing.join(", "));
        println!("run: chirper-onboarding");
    }
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

fn transcription_current(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let profile = config.transcription_profile;

    if json {
        let value = serde_json::json!({
            "profile": profile.as_config_value(),
            "label": profile.label(),
            "description": profile.description(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("profile: {}", profile.as_config_value());
    println!("label: {}", profile.label());
    println!("description: {}", profile.description());
}

fn transcription_list(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let current = config.transcription_profile;

    if json {
        let profiles = TranscriptionProfile::all()
            .iter()
            .map(|profile| {
                serde_json::json!({
                    "name": profile.as_config_value(),
                    "label": profile.label(),
                    "description": profile.description(),
                    "selected": *profile == current,
                })
            })
            .collect::<Vec<_>>();
        let value = serde_json::json!({
            "current": {
                "profile": current.as_config_value(),
                "label": current.label(),
                "description": current.description(),
            },
            "profiles": profiles,
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!(
        "current: {} ({})",
        current.as_config_value(),
        current.label()
    );
    println!("profiles:");
    for profile in TranscriptionProfile::all() {
        let marker = if *profile == current { "*" } else { " " };
        println!(
            " {marker} {:<10} {}",
            profile.as_config_value(),
            profile.description()
        );
    }
}

fn transcription_use(selection: Option<String>) {
    let Some(selection) = selection else {
        eprintln!("usage: chirper transcription-use <balanced|fast>");
        std::process::exit(1);
    };

    let profile = match parse_transcription_profile(&selection) {
        Some(profile) => profile,
        None => {
            eprintln!("unknown transcription profile: {selection}");
            eprintln!("usage: chirper transcription-use <balanced|fast>");
            std::process::exit(1);
        }
    };

    if let Err(error) = ChirperConfig::save_default_transcription_profile(profile) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!(
        "selected transcription profile: {}",
        profile.as_config_value()
    );
    println!("description: {}", profile.description());
    println!("the daemon will use this for the next transcription");
}

fn parse_transcription_profile(value: &str) -> Option<TranscriptionProfile> {
    value.parse::<TranscriptionProfile>().ok()
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
    println!("  {:<8} {:<8} {:<42} Default microphone", "-", "-", "auto");

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
            "format_log_retention_days": config.format_log_retention_days,
            "ollama_preload_on_recording": config.ollama_preload_on_recording,
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
    println!(
        "format_log_retention_days: {}",
        config.format_log_retention_days
    );
    println!(
        "ollama_preload_on_recording: {}",
        config.ollama_preload_on_recording
    );
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

fn ai_format_current(args: Vec<String>) {
    let json = args.iter().any(|arg| arg == "--json");
    let config = load_config_or_exit();
    let enabled = config.formatter_backend.is_ai();

    if json {
        let value = serde_json::json!({
            "enabled": enabled,
            "backend": config.formatter_backend.as_config_value(),
            "last_enabled_backend": config
                .last_ai_formatter_backend
                .map(FormatterBackend::as_config_value),
            "model": config.ollama_model,
            "ollama_command": config.ollama_command,
            "preload_on_recording": config.ollama_preload_on_recording,
            "log_retention_days": config.format_log_retention_days,
            "prompt_log_dir": ChirperConfig::default_prompt_log_dir().display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        return;
    }

    println!("enabled: {enabled}");
    println!("backend: {}", config.formatter_backend.as_config_value());
    println!(
        "last_enabled_backend: {}",
        config
            .last_ai_formatter_backend
            .map(FormatterBackend::as_config_value)
            .unwrap_or("<none>")
    );
    println!("ollama_model: {}", config.ollama_model);
    println!(
        "preload_on_recording: {}",
        config.ollama_preload_on_recording
    );
    println!("log_retention_days: {}", config.format_log_retention_days);
    println!(
        "prompt_log_dir: {}",
        ChirperConfig::default_prompt_log_dir().display()
    );
}

fn ai_format_use(args: Vec<String>) {
    let Some(selection) = args.first() else {
        eprintln!("usage: chirper ai-format-use <off|on>");
        std::process::exit(1);
    };

    if matches!(selection.as_str(), "off" | "none" | "disable" | "disabled") {
        if let Err(error) =
            ChirperConfig::save_default_formatter_selection(FormatterBackend::Rules, None)
        {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("AI formatting disabled");
        println!("formatter: rules");
        return;
    }

    if !matches!(selection.as_str(), "on" | "enable" | "enabled" | "ollama") {
        eprintln!("usage: chirper ai-format-use <off|on>");
        std::process::exit(1);
    }

    let config = load_config_or_exit();

    if let Err(error) = ensure_ollama_model_available(&config.ollama_command, &config.ollama_model)
    {
        eprintln!("{error}");
        std::process::exit(1);
    }

    if let Err(error) =
        ChirperConfig::save_default_formatter_selection(FormatterBackend::Ollama, None)
    {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("AI formatting enabled");
    println!("formatter: ollama");
    println!("ollama_model: {}", config.ollama_model);
}

fn ai_format_logs(args: Vec<String>) {
    let Some(value) = args.first() else {
        eprintln!("usage: chirper ai-format-logs <off|0|1|7|30|days>");
        std::process::exit(1);
    };

    let days = match value.as_str() {
        "off" | "none" | "disable" | "disabled" => 0,
        _ => match value.parse::<u64>() {
            Ok(days) => days,
            Err(_) => {
                eprintln!("usage: chirper ai-format-logs <off|0|1|7|30|days>");
                std::process::exit(1);
            }
        },
    };

    if let Err(error) = ChirperConfig::save_default_ai_formatting(None, None, Some(days), None) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("AI prompt log retention days: {days}");
}

fn ai_format_preload(args: Vec<String>) {
    let Some(value) = args.first() else {
        eprintln!("usage: chirper ai-format-preload <on|off>");
        std::process::exit(1);
    };

    let enabled = match value.as_str() {
        "on" | "true" | "yes" | "enable" | "enabled" => true,
        "off" | "false" | "no" | "disable" | "disabled" => false,
        _ => {
            eprintln!("usage: chirper ai-format-preload <on|off>");
            std::process::exit(1);
        }
    };

    if let Err(error) = ChirperConfig::save_default_ai_formatting(None, None, None, Some(enabled)) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("AI model preload on recording: {enabled}");
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

fn required_cli_value(args: &[String], index: usize, flag: &str) -> String {
    let value = args.get(index + 1).map(|value| value.trim());
    let Some(value) = value.filter(|value| !value.is_empty() && !value.starts_with('-')) else {
        eprintln!("{flag} requires a value");
        std::process::exit(1);
    };

    value.to_string()
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
    codex_model: Option<String>,
    codex_reasoning_effort: Option<String>,
    codex_service_tier: Option<String>,
    codex_config_overrides: Vec<String>,
    include_rules: bool,
    keep_loaded: bool,
    prompt_input: ComparePromptInput,
    prompt_note: Option<String>,
    custom_prompts: Vec<NamedPromptTemplate>,
    include_default_prompt: bool,
    transcripts: Vec<NamedTranscript>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct NamedPromptTemplate {
    name: String,
    template: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NamedTranscript {
    name: String,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparePromptVariant {
    name: String,
    template: Option<String>,
}

impl ComparePromptVariant {
    fn built_in() -> Self {
        Self {
            name: "chirper".to_string(),
            template: None,
        }
    }

    fn is_custom(&self) -> bool {
        self.template.is_some()
    }
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

    if args.text.is_empty() && args.transcripts.is_empty() {
        eprintln!(
            "usage: chirper format-compare [--mode auto|standard|email|command|code] [--model MODEL] [--models MODEL1,MODEL2] [--codex] [--codex-model MODEL] [--codex-effort low|medium|high|xhigh] [--codex-service-tier TIER] [--codex-tier TIER] [--codex-config KEY=VALUE] [--codex-profile NAME] [--all-codex-profiles] [--prompt-input raw|both] [--prompt-note TEXT] [--custom-prompt NAME=TEXT] [--custom-prompt-file NAME=PATH] [--transcript NAME=TEXT] [--transcript-file NAME=PATH] [--include-default-prompt] [--no-preprocessor] [--report-dir PATH] [--json] [text]"
        );
        std::process::exit(1);
    }

    let config = load_config_or_exit();
    let transcript_cases = resolve_compare_transcripts(&args);
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
    let prompt_variants = resolve_compare_prompt_variants(&args);

    if models.is_empty() && codex_runs.is_empty() {
        eprintln!("no formatter targets selected; run `chirper ollama-list` or pass `--codex`");
        std::process::exit(1);
    }

    let hardware = collect_hardware_snapshot(&config.ollama_command);
    let prompt_variant_count = prompt_variants.len();
    let transcript_count = transcript_cases.len();
    let total_targets = (models.len() + codex_runs.len()) * prompt_variant_count * transcript_count;
    let compare_started = Instant::now();
    let mut results = Vec::new();
    emit_compare_progress(
        &args,
        serde_json::json!({
            "type": "started",
            "total": total_targets,
            "include_rules": args.include_rules,
            "prompt_variants": prompt_variants.iter().map(|variant| variant.name.as_str()).collect::<Vec<_>>(),
            "transcripts": transcript_cases.iter().map(|case| case.name.as_str()).collect::<Vec<_>>(),
            "hardware": hardware_json(&hardware),
        }),
    );

    let mut target_index = 0usize;

    for transcript_case in &transcript_cases {
        let transcript = chirper_core::Transcript {
            text: transcript_case.text.clone(),
            language: None,
        };
        let preformatted = match format_with_rules(&config, &transcript, args.mode) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        };

        for model in &models {
            for prompt_variant in &prompt_variants {
                target_index += 1;
                let run_name = format_compare_run_name(
                    model,
                    prompt_variant,
                    prompt_variant_count,
                    transcript_case,
                    transcript_count,
                );
                emit_compare_progress(
                    &args,
                    serde_json::json!({
                        "type": "target_started",
                        "index": target_index,
                        "total": total_targets,
                        "name": run_name.as_str(),
                        "model": model.as_str(),
                        "prompt": prompt_variant.name.as_str(),
                        "transcript": transcript_case.name.as_str(),
                        "elapsed_ms": compare_started.elapsed().as_millis(),
                    }),
                );
                let formatter = OllamaFormatter::new(OllamaOptions {
                    command: config.ollama_command.clone(),
                    model: model.clone(),
                    vocabulary: config.vocabulary.clone(),
                });
                let ((result, elapsed_ms), metrics) = run_with_resource_sampling(|| {
                    let started = Instant::now();
                    let result = format_with_ollama_prompt_variant(
                        &formatter,
                        &transcript,
                        &preformatted,
                        &config,
                        &args,
                        prompt_variant,
                    );
                    let elapsed_ms = started.elapsed().as_millis();
                    (result, elapsed_ms)
                });
                if !args.keep_loaded {
                    stop_ollama_model_silent(&config.ollama_command, model);
                }

                let result = match result {
                    Ok(output) => FormatCompareResult {
                        name: run_name,
                        prompt_name: prompt_variant.name.clone(),
                        transcript_name: transcript_case.name.clone(),
                        elapsed_ms,
                        metrics,
                        output: Some(output),
                        error: None,
                    },
                    Err(error) => FormatCompareResult {
                        name: run_name,
                        prompt_name: prompt_variant.name.clone(),
                        transcript_name: transcript_case.name.clone(),
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
        }

        // Compare mode must report Codex errors as Codex results, not fallback output.
        for (name, options) in &codex_runs {
            for prompt_variant in &prompt_variants {
                target_index += 1;
                let run_name = format_compare_run_name(
                    name,
                    prompt_variant,
                    prompt_variant_count,
                    transcript_case,
                    transcript_count,
                );
                emit_compare_progress(
                    &args,
                    serde_json::json!({
                        "type": "target_started",
                        "index": target_index,
                        "total": total_targets,
                        "name": run_name.as_str(),
                        "model": name.as_str(),
                        "prompt": prompt_variant.name.as_str(),
                        "transcript": transcript_case.name.as_str(),
                        "elapsed_ms": compare_started.elapsed().as_millis(),
                    }),
                );
                let formatter = CodexFormatter::new(options.clone());
                let ((result, elapsed_ms), metrics) = run_with_resource_sampling(|| {
                    let started = Instant::now();
                    let result = format_with_codex_prompt_variant(
                        &formatter,
                        &transcript,
                        &preformatted,
                        &config,
                        &args,
                        prompt_variant,
                    );
                    let elapsed_ms = started.elapsed().as_millis();
                    (result, elapsed_ms)
                });

                let result = match result {
                    Ok(output) => FormatCompareResult {
                        name: run_name,
                        prompt_name: prompt_variant.name.clone(),
                        transcript_name: transcript_case.name.clone(),
                        elapsed_ms,
                        metrics,
                        output: Some(output),
                        error: None,
                    },
                    Err(error) => FormatCompareResult {
                        name: run_name,
                        prompt_name: prompt_variant.name.clone(),
                        transcript_name: transcript_case.name.clone(),
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
        }
    }

    let total_elapsed_ms = compare_started.elapsed().as_millis();
    let tested_models = tested_model_count(&results);
    let report_paths = args.report_dir.as_ref().map(|directory| {
        let context = FormatCompareReportContext {
            hardware: &hardware,
            mode: args.mode,
            prompt_input: args.prompt_input,
            prompt_note: args.prompt_note.as_deref(),
            total_elapsed_ms,
            prompt_variants: &prompt_variants,
            transcripts: &transcript_cases,
            results: &results,
        };
        write_format_compare_reports(directory, context)
    });
    emit_compare_progress(
        &args,
        serde_json::json!({
            "type": "finished",
            "total": total_targets,
            "tested_models": tested_models,
            "elapsed_ms": total_elapsed_ms,
            "report_paths": report_paths.as_ref().and_then(|result| result.as_ref().ok().map(|paths| paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>())),
            "report_path": report_paths.as_ref().and_then(|result| result.as_ref().ok().and_then(|paths| paths.first().map(|path| path.display().to_string()))),
        }),
    );

    if args.json {
        let value = serde_json::json!({
            "mode": format!("{:?}", args.mode),
            "prompt_input": args.prompt_input.label(),
            "prompt_note": args.prompt_note.as_deref(),
            "custom_prompts": args.custom_prompts.iter().map(|prompt| prompt.name.as_str()).collect::<Vec<_>>(),
            "transcripts": transcript_cases.iter().map(|case| serde_json::json!({"name": case.name.as_str(), "text": case.text.as_str()})).collect::<Vec<_>>(),
            "tested_models": tested_models,
            "total_elapsed_ms": total_elapsed_ms,
            "preprocessed_sent_to_model": args.prompt_input == ComparePromptInput::RawAndPreprocessed,
            "hardware": hardware_json(&hardware),
            "report_paths": report_paths.as_ref().map(|result| result.as_ref().ok().map(|paths| paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>())),
            "report_path": report_paths.as_ref().map(|result| result.as_ref().ok().and_then(|paths| paths.first().map(|path| path.display().to_string()))),
            "results": results.iter().map(format_compare_result_json).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&value).unwrap());
        if let Some(Err(error)) = report_paths {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    println!("mode: {:?}", args.mode);
    println!("prompt_input: {}", args.prompt_input.label());
    println!(
        "transcripts: {}",
        transcript_cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "summary: {}",
        format_tested_summary(tested_models, total_elapsed_ms)
    );
    if let Some(prompt_note) = args.prompt_note.as_deref() {
        println!("prompt_note: {prompt_note}");
    }
    if !args.custom_prompts.is_empty() {
        println!(
            "custom_prompts: {}",
            args.custom_prompts
                .iter()
                .map(|prompt| prompt.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("hardware:");
    print_hardware_snapshot(&hardware);

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

    if let Some(report_result) = report_paths {
        match report_result {
            Ok(paths) => {
                for path in paths {
                    println!("\nreport: {}", path.display());
                }
            }
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
    let mut codex_model = None;
    let mut codex_reasoning_effort = None;
    let mut codex_service_tier = None;
    let mut codex_config_overrides = Vec::new();
    let mut include_rules = true;
    let mut keep_loaded = false;
    let mut prompt_input = ComparePromptInput::RawAndPreprocessed;
    let mut prompt_note = None;
    let mut custom_prompts = Vec::new();
    let mut include_default_prompt = false;
    let mut transcripts = Vec::new();
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
        } else if let Some(value) = arg.strip_prefix("--codex-model=") {
            codex_model = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--codex-model" {
            let value = required_cli_value(&args, index, arg);
            codex_model = normalize_optional_cli_value(&value);
            index += 2;
        } else if let Some(value) = arg
            .strip_prefix("--codex-effort=")
            .or_else(|| arg.strip_prefix("--codex-reasoning-effort="))
        {
            codex_reasoning_effort = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--codex-effort" || arg == "--codex-reasoning-effort" {
            let value = required_cli_value(&args, index, arg);
            codex_reasoning_effort = normalize_optional_cli_value(&value);
            index += 2;
        } else if let Some(value) = arg
            .strip_prefix("--codex-service-tier=")
            .or_else(|| arg.strip_prefix("--codex-tier="))
        {
            codex_service_tier = normalize_optional_cli_value(value);
            index += 1;
        } else if arg == "--codex-service-tier" || arg == "--codex-tier" {
            let value = required_cli_value(&args, index, arg);
            codex_service_tier = normalize_optional_cli_value(&value);
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--codex-config=") {
            push_config_override(&mut codex_config_overrides, value);
            index += 1;
        } else if arg == "--codex-config" {
            let value = required_cli_value(&args, index, arg);
            push_config_override(&mut codex_config_overrides, &value);
            index += 2;
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
        } else if let Some(value) = arg.strip_prefix("--custom-prompt=") {
            custom_prompts.push(parse_named_prompt_template(value, custom_prompts.len() + 1));
            index += 1;
        } else if arg == "--custom-prompt" || arg == "--model-prompt" {
            if let Some(value) = args.get(index + 1) {
                custom_prompts.push(parse_named_prompt_template(value, custom_prompts.len() + 1));
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--custom-prompt-file=") {
            custom_prompts.push(read_named_prompt_template_file(
                value,
                custom_prompts.len() + 1,
            ));
            index += 1;
        } else if arg == "--custom-prompt-file" || arg == "--model-prompt-file" {
            if let Some(value) = args.get(index + 1) {
                custom_prompts.push(read_named_prompt_template_file(
                    value,
                    custom_prompts.len() + 1,
                ));
                index += 2;
            } else {
                index += 1;
            }
        } else if arg == "--include-default-prompt" || arg == "--with-default-prompt" {
            include_default_prompt = true;
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--transcript=") {
            transcripts.push(parse_named_transcript(value, transcripts.len() + 1));
            index += 1;
        } else if arg == "--transcript" || arg == "--case" {
            if let Some(value) = args.get(index + 1) {
                transcripts.push(parse_named_transcript(value, transcripts.len() + 1));
                index += 2;
            } else {
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--transcript-file=") {
            transcripts.push(read_named_transcript_file(value, transcripts.len() + 1));
            index += 1;
        } else if arg == "--transcript-file" || arg == "--case-file" {
            if let Some(value) = args.get(index + 1) {
                transcripts.push(read_named_transcript_file(value, transcripts.len() + 1));
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
        codex_model,
        codex_reasoning_effort,
        codex_service_tier,
        codex_config_overrides,
        include_rules,
        keep_loaded,
        prompt_input,
        prompt_note,
        custom_prompts,
        include_default_prompt,
        transcripts,
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

fn parse_named_prompt_template(value: &str, index: usize) -> NamedPromptTemplate {
    let (name, template) = value
        .split_once('=')
        .map(|(name, template)| (name.to_string(), template.to_string()))
        .unwrap_or_else(|| (format!("prompt-{index}"), value.to_string()));
    let name = name.trim();
    let template = template.trim();

    if template.is_empty() {
        eprintln!("custom prompt `{name}` cannot be empty");
        std::process::exit(1);
    }

    NamedPromptTemplate {
        name: if name.is_empty() {
            format!("prompt-{index}")
        } else {
            name.to_string()
        },
        template: template.to_string(),
    }
}

fn read_named_prompt_template_file(value: &str, index: usize) -> NamedPromptTemplate {
    let (name, path) = value
        .split_once('=')
        .map(|(name, path)| (name.to_string(), path.to_string()))
        .unwrap_or_else(|| (format!("prompt-{index}"), value.to_string()));
    let path = expand_user_path(path.trim());
    let template = fs::read_to_string(&path).unwrap_or_else(|source| {
        eprintln!(
            "failed to read custom prompt file {}: {source}",
            path.display()
        );
        std::process::exit(1);
    });

    parse_named_prompt_template(
        &format!(
            "{}={}",
            if name.trim().is_empty() {
                format!("prompt-{index}")
            } else {
                name.trim().to_string()
            },
            template
        ),
        index,
    )
}

fn parse_named_transcript(value: &str, index: usize) -> NamedTranscript {
    let (name, text) = value
        .split_once('=')
        .map(|(name, text)| (name.to_string(), text.to_string()))
        .unwrap_or_else(|| (format!("transcript-{index}"), value.to_string()));
    let name = name.trim();
    let text = text.trim();

    if text.is_empty() {
        eprintln!("transcript `{name}` cannot be empty");
        std::process::exit(1);
    }

    NamedTranscript {
        name: if name.is_empty() {
            format!("transcript-{index}")
        } else {
            name.to_string()
        },
        text: text.to_string(),
    }
}

fn read_named_transcript_file(value: &str, index: usize) -> NamedTranscript {
    let (name, path) = value
        .split_once('=')
        .map(|(name, path)| (name.to_string(), path.to_string()))
        .unwrap_or_else(|| (format!("transcript-{index}"), value.to_string()));
    let path = expand_user_path(path.trim());
    let text = fs::read_to_string(&path).unwrap_or_else(|source| {
        eprintln!(
            "failed to read transcript file {}: {source}",
            path.display()
        );
        std::process::exit(1);
    });

    parse_named_transcript(
        &format!(
            "{}={}",
            if name.trim().is_empty() {
                format!("transcript-{index}")
            } else {
                name.trim().to_string()
            },
            text
        ),
        index,
    )
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

fn resolve_compare_transcripts(args: &FormatCompareArgs) -> Vec<NamedTranscript> {
    let mut transcripts = Vec::new();

    if !args.text.trim().is_empty() {
        transcripts.push(NamedTranscript {
            name: "transcript-1".to_string(),
            text: args.text.trim().to_string(),
        });
    }

    transcripts.extend(args.transcripts.iter().cloned());

    transcripts
}

fn resolve_compare_prompt_variants(args: &FormatCompareArgs) -> Vec<ComparePromptVariant> {
    let mut variants = Vec::new();

    if args.custom_prompts.is_empty() || args.include_default_prompt {
        variants.push(ComparePromptVariant::built_in());
    }

    variants.extend(
        args.custom_prompts
            .iter()
            .map(|prompt| ComparePromptVariant {
                name: prompt.name.clone(),
                template: Some(prompt.template.clone()),
            }),
    );

    variants
}

fn format_compare_run_name(
    model_name: &str,
    prompt_variant: &ComparePromptVariant,
    prompt_variant_count: usize,
    transcript: &NamedTranscript,
    transcript_count: usize,
) -> String {
    let mut name = model_name.to_string();

    if prompt_variant_count > 1 || prompt_variant.is_custom() {
        name.push_str(" / ");
        name.push_str(&prompt_variant.name);
    }

    if transcript_count > 1 {
        name.push_str(" / ");
        name.push_str(&transcript.name);
    }

    name
}

fn format_with_ollama_prompt_variant(
    formatter: &OllamaFormatter,
    transcript: &chirper_core::Transcript,
    preformatted: &str,
    config: &ChirperConfig,
    args: &FormatCompareArgs,
    prompt_variant: &ComparePromptVariant,
) -> ChirperResult<String> {
    if let Some(template) = prompt_variant.template.as_deref() {
        let prompt = render_custom_prompt(
            template,
            &transcript.text,
            preformatted,
            args.mode,
            args.prompt_input,
            &config.vocabulary,
            args.prompt_note.as_deref(),
        );
        return formatter.format_custom_prompt(
            &prompt,
            compare_non_empty_input(transcript, preformatted, args.prompt_input),
        );
    }

    formatter.format_with_prompt_input_and_note(
        transcript,
        preformatted,
        args.mode,
        args.prompt_input.as_ollama_input(),
        args.prompt_note.as_deref(),
    )
}

fn format_with_codex_prompt_variant(
    formatter: &CodexFormatter,
    transcript: &chirper_core::Transcript,
    preformatted: &str,
    config: &ChirperConfig,
    args: &FormatCompareArgs,
    prompt_variant: &ComparePromptVariant,
) -> ChirperResult<String> {
    if let Some(template) = prompt_variant.template.as_deref() {
        let prompt = render_custom_prompt(
            template,
            &transcript.text,
            preformatted,
            args.mode,
            args.prompt_input,
            &config.vocabulary,
            args.prompt_note.as_deref(),
        );
        return formatter.format_custom_prompt(
            &prompt,
            compare_non_empty_input(transcript, preformatted, args.prompt_input),
        );
    }

    formatter.format_with_prompt_input_and_note(
        transcript,
        preformatted,
        args.mode,
        args.prompt_input.as_codex_input(),
        args.prompt_note.as_deref(),
    )
}

fn compare_non_empty_input<'a>(
    transcript: &'a chirper_core::Transcript,
    preformatted: &'a str,
    prompt_input: ComparePromptInput,
) -> &'a str {
    match prompt_input {
        ComparePromptInput::RawOnly => &transcript.text,
        ComparePromptInput::RawAndPreprocessed => preformatted,
    }
}

fn render_custom_prompt(
    template: &str,
    raw_text: &str,
    preprocessed_text: &str,
    mode: DictationMode,
    prompt_input: ComparePromptInput,
    vocabulary: &[chirper_core::VocabularyEntry],
    prompt_note: Option<&str>,
) -> String {
    let preprocessed_for_prompt = match prompt_input {
        ComparePromptInput::RawOnly => "",
        ComparePromptInput::RawAndPreprocessed => preprocessed_text,
    };
    let vocabulary_text = if vocabulary.is_empty() {
        String::new()
    } else {
        vocabulary
            .iter()
            .map(|entry| format!("{} => {}", entry.spoken, entry.written))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut prompt = template.trim().to_string();
    let had_placeholder = [
        "{transcript}",
        "{raw_transcript}",
        "{preprocessed}",
        "{preprocessed_draft}",
        "{mode}",
        "{vocabulary}",
    ]
    .iter()
    .any(|placeholder| prompt.contains(placeholder));

    prompt = prompt
        .replace("{transcript}", raw_text)
        .replace("{raw_transcript}", raw_text)
        .replace("{preprocessed}", preprocessed_for_prompt)
        .replace("{preprocessed_draft}", preprocessed_for_prompt)
        .replace("{mode}", &format!("{mode:?}"))
        .replace("{vocabulary}", &vocabulary_text);

    if let Some(prompt_note) = prompt_note.map(str::trim).filter(|note| !note.is_empty()) {
        prompt.push_str("\n\nAdditional compare-run instructions:\n");
        prompt.push_str(prompt_note);
    }

    if had_placeholder {
        return prompt;
    }

    prompt.push_str("\n\nRaw transcript:\n<<<\n");
    prompt.push_str(raw_text);
    prompt.push_str("\n>>>\n");

    if prompt_input == ComparePromptInput::RawAndPreprocessed {
        prompt.push_str("\nPreprocessed draft:\n<<<\n");
        prompt.push_str(preprocessed_text);
        prompt.push_str("\n>>>\n");
    }

    prompt
}

fn resolve_codex_compare_runs(
    config: &ChirperConfig,
    args: &FormatCompareArgs,
) -> Vec<(String, CodexOptions)> {
    let mut runs = Vec::new();

    if args.include_codex_current {
        let options = codex_compare_options_from_args(config, args);
        runs.push((format!("codex:{}", options.label()), options));
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

fn codex_compare_options_from_args(
    config: &ChirperConfig,
    args: &FormatCompareArgs,
) -> CodexOptions {
    let mut options = CodexOptions::from_config(config);

    if let Some(model) = &args.codex_model {
        options.model = Some(model.clone());
    }
    if let Some(reasoning_effort) = &args.codex_reasoning_effort {
        options.reasoning_effort = Some(reasoning_effort.clone());
    }
    if let Some(service_tier) = &args.codex_service_tier {
        options.service_tier = Some(service_tier.clone());
    }
    if !args.codex_config_overrides.is_empty() {
        options.config_overrides = args.codex_config_overrides.clone();
    }

    options
}

#[derive(Debug, Clone, PartialEq)]
struct FormatCompareResult {
    name: String,
    prompt_name: String,
    transcript_name: String,
    elapsed_ms: u128,
    metrics: ResourceMetrics,
    output: Option<String>,
    error: Option<String>,
}

fn format_compare_result_json(result: &FormatCompareResult) -> serde_json::Value {
    serde_json::json!({
        "name": result.name,
        "prompt_name": result.prompt_name,
        "transcript_name": result.transcript_name,
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
        "Model Run"
    } else {
        "Model Runs"
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

struct FormatCompareReportContext<'a> {
    hardware: &'a HardwareSnapshot,
    mode: DictationMode,
    prompt_input: ComparePromptInput,
    prompt_note: Option<&'a str>,
    total_elapsed_ms: u128,
    prompt_variants: &'a [ComparePromptVariant],
    transcripts: &'a [NamedTranscript],
    results: &'a [FormatCompareResult],
}

fn write_format_compare_reports(
    directory: &Path,
    context: FormatCompareReportContext<'_>,
) -> Result<Vec<PathBuf>, String> {
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

    let mut paths = Vec::new();

    for (prompt_index, prompt_variant) in context.prompt_variants.iter().enumerate() {
        let prompt_results = context
            .results
            .iter()
            .filter(|result| result.prompt_name == prompt_variant.name)
            .collect::<Vec<_>>();
        if prompt_results.is_empty() {
            continue;
        }

        let file_name = format!(
            "chirper-format-compare-{timestamp}-{:02}-{}.txt",
            prompt_index + 1,
            slugify_report_part(&prompt_variant.name)
        );
        let path = directory.join(file_name);
        let mut report = String::new();
        let prompt_elapsed_ms = prompt_results
            .iter()
            .map(|result| result.elapsed_ms)
            .sum::<u128>();

        let _ = writeln!(report, "Chirper format comparison");
        let _ = writeln!(report, "generated_unix_seconds: {timestamp}");
        let _ = writeln!(report, "prompt: {}", prompt_variant.name);
        let _ = writeln!(
            report,
            "{}",
            format_tested_summary(prompt_results.len(), prompt_elapsed_ms)
        );
        let _ = writeln!(report, "prompt_elapsed_ms: {prompt_elapsed_ms}");
        let _ = writeln!(report, "full_run_elapsed_ms: {}", context.total_elapsed_ms);
        let _ = writeln!(report, "mode: {:?}", context.mode);
        let _ = writeln!(report, "prompt_input: {}", context.prompt_input.label());
        if let Some(prompt_note) = context
            .prompt_note
            .map(str::trim)
            .filter(|note| !note.is_empty())
        {
            let _ = writeln!(report, "prompt_note:");
            let _ = writeln!(report, "{prompt_note}");
        }
        if let Some(template) = prompt_variant.template.as_deref() {
            let _ = writeln!(report);
            let _ = writeln!(report, "Custom prompt template:");
            let _ = writeln!(report, "{template}");
        }
        let _ = writeln!(report);
        let _ = writeln!(report, "Hardware:");
        write_hardware_snapshot(&mut report, context.hardware);
        let _ = writeln!(report);
        let _ = writeln!(report, "Transcripts:");
        for transcript in context.transcripts {
            let _ = writeln!(report);
            let _ = writeln!(report, "--- {} ---", transcript.name);
            let _ = writeln!(report, "{}", transcript.text);
        }

        for result in prompt_results {
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
        paths.push(path);
    }

    Ok(paths)
}

fn slugify_report_part(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        "prompt".to_string()
    } else {
        slug
    }
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
    let Some(value) = value else {
        eprintln!("usage: chirper mode <auto|standard|email|command|code>");
        std::process::exit(1);
    };
    let Some(mode) = parse_mode_name(value) else {
        eprintln!("unknown dictation mode: {value}");
        eprintln!("usage: chirper mode <auto|standard|email|command|code>");
        std::process::exit(1);
    };

    ServiceCommand::SetMode(mode)
}

fn set_mode(mode: DictationMode) {
    if let Err(error) = ChirperConfig::save_default_dictation_mode(mode) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected dictation mode: {}", mode.as_config_value());
    println!("the daemon will use this for the next transcription");
}

fn gui_current() {
    let config = load_config_or_exit();
    println!("gui_profile: {}", config.gui_profile.as_config_value());
}

fn gui_use(profile: Option<String>) {
    let Some(profile) = profile else {
        eprintln!("usage: chirper gui-use <gnome|none>");
        std::process::exit(1);
    };
    let profile = match parse_gui_profile(&profile) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: chirper gui-use <gnome|none>");
            std::process::exit(1);
        }
    };

    if let Err(error) = ChirperConfig::save_default_gui_profile(profile) {
        eprintln!("{error}");
        std::process::exit(1);
    }

    println!("selected GUI profile: {}", profile.as_config_value());
}

fn open_settings() {
    let config = load_config_or_exit();
    let Some(launcher) = settings_launcher(config.gui_profile) else {
        eprintln!(
            "no GUI settings app is installed for profile `{}`",
            config.gui_profile.as_config_value()
        );
        eprintln!("install one with `scripts/install.sh --gui gnome` or select it with `chirper gui-use gnome` after installation");
        std::process::exit(1);
    };

    if chirper_platform::find_executable(launcher).is_none() {
        eprintln!("settings launcher not found: {launcher}");
        eprintln!(
            "reinstall with `scripts/install.sh --gui {}`",
            config.gui_profile.as_config_value()
        );
        std::process::exit(1);
    }

    match Command::new(launcher)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(error) => {
            eprintln!("failed to open {launcher}: {error}");
            std::process::exit(1);
        }
    }
}

fn settings_launcher(profile: GuiProfile) -> Option<&'static str> {
    match profile {
        GuiProfile::Gnome => Some("chirper-settings"),
        GuiProfile::None => None,
    }
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
    println!(
        "transcription_profile: {}",
        config.transcription_profile.as_config_value()
    );
    println!("gpu_backend: {:?}", config.gpu_backend);
    println!("formatter_backend: {:?}", config.formatter_backend);
    println!("insertion_backend: {:?}", config.insertion_backend);
    println!("dictation_mode: {:?}", config.dictation_mode);
    println!("gui_profile: {}", config.gui_profile.as_config_value());
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
            match CodexFormatter::new(CodexOptions::from_config(config)).format_with_context(
                transcript,
                &preformatted,
                mode,
            ) {
                Ok(text) => Ok(text),
                Err(codex_error) => {
                    eprintln!(
                        "Codex formatter failed; trying Ollama fallback `{}`: {codex_error}",
                        config.ollama_model
                    );
                    OllamaFormatter::new(OllamaOptions::from_config(config))
                        .format_with_context(transcript, &preformatted, mode)
                        .map_err(|fallback_error| {
                            format!(
                                "Codex formatter failed: {codex_error}; Ollama fallback failed: {fallback_error}"
                            )
                        })
                }
            }
        }
        FormatterBackend::LlamaCpp => {
            Err("formatter backend llama.cpp is not available in Chirper 0.1.0".to_string())
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

fn manual_record_state_path() -> PathBuf {
    runtime_dir().join("record-state")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_release_tags() {
        assert_eq!(
            parse_release_version_tag("v1.2.3"),
            Some(ReleaseVersion {
                major: 1,
                minor: 2,
                patch: 3,
            })
        );
        assert_eq!(
            parse_release_version_tag("1.10.0"),
            Some(ReleaseVersion {
                major: 1,
                minor: 10,
                patch: 0,
            })
        );
        assert!(parse_release_version_tag("v1.2").is_none());
        assert!(parse_release_version_tag("v1.2.3-beta").is_none());
        assert!(parse_release_version_tag("nightly").is_none());
    }

    #[test]
    fn compares_release_tags_numerically() {
        let older = parse_release_version_tag("v1.9.0").unwrap();
        let newer = parse_release_version_tag("v1.10.0").unwrap();

        assert!(newer > older);
    }
}

use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{ChirperError, ChirperResult, DictationMode};

pub const WHISPER_MODEL_NAMES: &[&str] = &[
    "tiny",
    "tiny.en",
    "tiny-q5_1",
    "tiny.en-q5_1",
    "tiny-q8_0",
    "base",
    "base.en",
    "base-q5_1",
    "base.en-q5_1",
    "base-q8_0",
    "small",
    "small.en",
    "small.en-tdrz",
    "small-q5_1",
    "small.en-q5_1",
    "small-q8_0",
    "medium",
    "medium.en",
    "medium-q5_0",
    "medium.en-q5_0",
    "medium-q8_0",
    "large-v1",
    "large-v2",
    "large-v2-q5_0",
    "large-v2-q8_0",
    "large-v3",
    "large-v3-q5_0",
    "large-v3-turbo",
    "large-v3-turbo-q5_0",
    "large-v3-turbo-q8_0",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChirperConfig {
    pub audio_backend: AudioBackend,
    pub pipewire_target: Option<String>,
    pub asr_backend: AsrBackend,
    pub transcription_profile: TranscriptionProfile,
    pub gpu_backend: GpuBackend,
    pub formatter_backend: FormatterBackend,
    pub last_ai_formatter_backend: Option<FormatterBackend>,
    pub insertion_backend: InsertionBackend,
    pub dictation_mode: DictationMode,
    pub gui_profile: GuiProfile,
    pub whisper_model: String,
    pub whispercpp_command: String,
    pub whispercpp_model_path: Option<PathBuf>,
    pub whisper_language: Option<String>,
    pub ollama_command: String,
    pub ollama_model: String,
    pub ai_hardware_tier: AiHardwareTier,
    pub format_log_retention_days: u64,
    pub ollama_preload_on_recording: bool,
    pub codex_command: String,
    pub codex_model: Option<String>,
    pub codex_profile: Option<String>,
    pub codex_reasoning_effort: Option<String>,
    pub codex_service_tier: Option<String>,
    pub codex_config_overrides: Vec<String>,
    pub codex_profiles: Vec<CodexProfileConfig>,
    pub vocabulary: Vec<VocabularyEntry>,
}

impl Default for ChirperConfig {
    fn default() -> Self {
        Self {
            audio_backend: AudioBackend::PipeWire,
            pipewire_target: None,
            asr_backend: AsrBackend::WhisperCpp,
            transcription_profile: TranscriptionProfile::Balanced,
            gpu_backend: GpuBackend::Auto,
            formatter_backend: FormatterBackend::None,
            last_ai_formatter_backend: None,
            insertion_backend: InsertionBackend::Clipboard,
            dictation_mode: DictationMode::Auto,
            gui_profile: GuiProfile::Gnome,
            whisper_model: "base".to_string(),
            whispercpp_command: "whisper-cli".to_string(),
            whispercpp_model_path: None,
            whisper_language: None,
            ollama_command: "ollama".to_string(),
            ollama_model: AiHardwareTier::High.ollama_model().to_string(),
            ai_hardware_tier: AiHardwareTier::High,
            format_log_retention_days: 7,
            ollama_preload_on_recording: true,
            codex_command: "codex".to_string(),
            codex_model: None,
            codex_profile: None,
            codex_reasoning_effort: None,
            codex_service_tier: None,
            codex_config_overrides: Vec::new(),
            codex_profiles: Vec::new(),
            vocabulary: Vec::new(),
        }
    }
}

impl ChirperConfig {
    pub fn load_default() -> ChirperResult<Self> {
        let path = Self::default_path();

        if path.exists() {
            Self::load_from_path(path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> ChirperResult<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to read config file {}: {source}",
                path.display()
            ))
        })?;

        Self::from_toml_str(&content)
    }

    pub fn from_toml_str(content: &str) -> ChirperResult<Self> {
        let table = content.parse::<toml::Table>().map_err(|source| {
            ChirperError::Configuration(format!("failed to parse config TOML: {source}"))
        })?;

        let mut config = Self::default();

        if let Some(value) = table.get("audio_backend") {
            config.audio_backend = parse_config_value("audio_backend", value)?;
        }

        if let Some(value) = table.get("pipewire_target") {
            config.pipewire_target = parse_optional_string("pipewire_target", value)?;
        }

        if let Some(value) = table.get("asr_backend") {
            config.asr_backend = parse_config_value("asr_backend", value)?;
        }

        if let Some(value) = table.get("transcription_profile") {
            config.transcription_profile = parse_config_value("transcription_profile", value)?;
        }

        if let Some(value) = table.get("gpu_backend") {
            config.gpu_backend = parse_config_value("gpu_backend", value)?;
        }

        if let Some(value) = table.get("formatter_backend") {
            config.formatter_backend = parse_config_value("formatter_backend", value)?;
        }

        if let Some(value) = table.get("last_ai_formatter_backend") {
            config.last_ai_formatter_backend = parse_last_ai_formatter_backend(value)?;
        }

        if let Some(value) = table.get("insertion_backend") {
            config.insertion_backend = parse_config_value("insertion_backend", value)?;
        }

        if let Some(value) = table.get("dictation_mode") {
            config.dictation_mode = parse_config_value("dictation_mode", value)?;
        }

        if let Some(value) = table.get("gui_profile") {
            config.gui_profile = parse_config_value("gui_profile", value)?;
        }

        if let Some(value) = table.get("whisper_model") {
            config.whisper_model = parse_string("whisper_model", value)?.to_string();
        }

        if let Some(value) = table.get("whispercpp_command") {
            config.whispercpp_command = parse_string("whispercpp_command", value)?.to_string();
        }

        if let Some(value) = table.get("whispercpp_model_path") {
            config.whispercpp_model_path = parse_optional_path("whispercpp_model_path", value)?;
        }

        if let Some(value) = table.get("whisper_language") {
            config.whisper_language = parse_optional_string("whisper_language", value)?;
        }

        if let Some(value) = table.get("ollama_command") {
            config.ollama_command = parse_string("ollama_command", value)?.to_string();
        }

        if let Some(value) = table.get("ollama_model") {
            config.ollama_model = parse_string("ollama_model", value)?.to_string();
        }

        if let Some(value) = table.get("ai_hardware_tier") {
            config.ai_hardware_tier = parse_config_value("ai_hardware_tier", value)?;
        }

        if let Some(value) = table.get("format_log_retention_days") {
            config.format_log_retention_days = parse_u64("format_log_retention_days", value)?;
        }

        if let Some(value) = table.get("ollama_preload_on_recording") {
            config.ollama_preload_on_recording = parse_bool("ollama_preload_on_recording", value)?;
        }

        if let Some(value) = table.get("codex_command") {
            config.codex_command = parse_string("codex_command", value)?.to_string();
        }

        if let Some(value) = table.get("codex_model") {
            config.codex_model = parse_optional_string("codex_model", value)?;
        }

        if let Some(value) = table.get("codex_profile") {
            config.codex_profile = parse_optional_string("codex_profile", value)?;
        }

        if let Some(value) = table.get("codex_reasoning_effort") {
            config.codex_reasoning_effort = parse_optional_string("codex_reasoning_effort", value)?;
        }

        if let Some(value) = table.get("codex_service_tier") {
            config.codex_service_tier = parse_optional_string("codex_service_tier", value)?;
        }

        if let Some(value) = table
            .get("codex_config_overrides")
            .or_else(|| table.get("codex_config"))
        {
            config.codex_config_overrides = parse_string_array("codex_config_overrides", value)?;
        }

        if let Some(value) = table.get("codex_profiles") {
            config.codex_profiles = parse_codex_profiles(value)?;
        }

        if let Some(value) = table.get("vocabulary") {
            config.vocabulary = parse_vocabulary(value)?;
        }

        Ok(config)
    }

    pub fn default_path() -> PathBuf {
        if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(config_home).join("chirper/config.toml");
        }

        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(".config/chirper/config.toml");
        }

        PathBuf::from("chirper/config.toml")
    }

    pub fn default_data_dir() -> PathBuf {
        if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data_home).join("chirper");
        }

        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/chirper");
        }

        PathBuf::from("chirper")
    }

    pub fn default_model_dir() -> PathBuf {
        Self::default_data_dir().join("models")
    }

    pub fn default_prompt_log_dir() -> PathBuf {
        Self::default_path()
            .parent()
            .map(|parent| parent.join("prompt-logs"))
            .unwrap_or_else(|| PathBuf::from("chirper/prompt-logs"))
    }

    pub fn default_model_path(model: &str) -> PathBuf {
        Self::default_model_dir().join(format!("ggml-{model}.bin"))
    }

    pub fn model_name_from_path(path: impl AsRef<Path>) -> Option<String> {
        let filename = path.as_ref().file_name()?.to_str()?;
        let name = filename.strip_prefix("ggml-")?.strip_suffix(".bin")?;

        Some(name.to_string())
    }

    pub fn save_model_selection(
        path: impl AsRef<Path>,
        model: &str,
        model_path: impl AsRef<Path>,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = if path.exists() {
            let content = fs::read_to_string(path).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to read config file {}: {source}",
                    path.display()
                ))
            })?;

            content.parse::<toml::Table>().map_err(|source| {
                ChirperError::Configuration(format!("failed to parse config TOML: {source}"))
            })?
        } else {
            toml::Table::new()
        };

        table.insert(
            "whisper_model".to_string(),
            toml::Value::String(model.to_string()),
        );
        table.insert(
            "whispercpp_model_path".to_string(),
            toml::Value::String(model_path.as_ref().display().to_string()),
        );

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to create config directory {}: {source}",
                    parent.display()
                ))
            })?;
        }

        let content = toml::to_string_pretty(&table).map_err(|source| {
            ChirperError::Configuration(format!("failed to encode config TOML: {source}"))
        })?;
        fs::write(path, content).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to write config file {}: {source}",
                path.display()
            ))
        })
    }

    pub fn save_whispercpp_setup(
        path: impl AsRef<Path>,
        model: &str,
        command: &str,
        model_path: impl AsRef<Path>,
        gpu_backend: GpuBackend,
    ) -> ChirperResult<()> {
        let model = model.trim();
        let command = command.trim();
        let model_path = model_path.as_ref();

        if model.is_empty() {
            return Err(ChirperError::Configuration(
                "whisper model cannot be empty".to_string(),
            ));
        }
        if command.is_empty() {
            return Err(ChirperError::Configuration(
                "whisper.cpp command cannot be empty".to_string(),
            ));
        }
        if model_path.as_os_str().is_empty() {
            return Err(ChirperError::Configuration(
                "whisper.cpp model path cannot be empty".to_string(),
            ));
        }

        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        table.insert(
            "asr_backend".to_string(),
            toml::Value::String("whisper-cpp".to_string()),
        );
        table.insert(
            "gpu_backend".to_string(),
            toml::Value::String(gpu_backend.as_config_value().to_string()),
        );
        table.insert(
            "whisper_model".to_string(),
            toml::Value::String(model.to_string()),
        );
        table.insert(
            "whispercpp_command".to_string(),
            toml::Value::String(command.to_string()),
        );
        table.insert(
            "whispercpp_model_path".to_string(),
            toml::Value::String(model_path.display().to_string()),
        );
        table
            .entry("whisper_language".to_string())
            .or_insert_with(|| toml::Value::String("auto".to_string()));

        write_config_table(path, &table)
    }

    pub fn save_audio_target(path: impl AsRef<Path>, target: Option<&str>) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = if path.exists() {
            let content = fs::read_to_string(path).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to read config file {}: {source}",
                    path.display()
                ))
            })?;

            content.parse::<toml::Table>().map_err(|source| {
                ChirperError::Configuration(format!("failed to parse config TOML: {source}"))
            })?
        } else {
            toml::Table::new()
        };

        match target {
            Some(target) if !target.trim().is_empty() => {
                table.insert(
                    "pipewire_target".to_string(),
                    toml::Value::String(target.to_string()),
                );
            }
            _ => {
                table.remove("pipewire_target");
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to create config directory {}: {source}",
                    parent.display()
                ))
            })?;
        }

        let content = toml::to_string_pretty(&table).map_err(|source| {
            ChirperError::Configuration(format!("failed to encode config TOML: {source}"))
        })?;
        fs::write(path, content).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to write config file {}: {source}",
                path.display()
            ))
        })
    }

    pub fn save_formatter_selection(
        path: impl AsRef<Path>,
        backend: FormatterBackend,
        ollama_model: Option<&str>,
    ) -> ChirperResult<()> {
        if backend == FormatterBackend::LlamaCpp {
            return Err(ChirperError::Configuration(
                "formatter backend llama.cpp is not available in Chirper 0.1.0".to_string(),
            ));
        }

        let path = path.as_ref();
        let mut table = if path.exists() {
            let content = fs::read_to_string(path).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to read config file {}: {source}",
                    path.display()
                ))
            })?;

            content.parse::<toml::Table>().map_err(|source| {
                ChirperError::Configuration(format!("failed to parse config TOML: {source}"))
            })?
        } else {
            toml::Table::new()
        };

        table.insert(
            "formatter_backend".to_string(),
            toml::Value::String(backend.as_config_value().to_string()),
        );

        if backend.is_ai() {
            table.insert(
                "last_ai_formatter_backend".to_string(),
                toml::Value::String(backend.as_config_value().to_string()),
            );
        }

        if let Some(model) = ollama_model
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            table.insert(
                "ollama_model".to_string(),
                toml::Value::String(model.to_string()),
            );
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                ChirperError::Configuration(format!(
                    "failed to create config directory {}: {source}",
                    parent.display()
                ))
            })?;
        }

        let content = toml::to_string_pretty(&table).map_err(|source| {
            ChirperError::Configuration(format!("failed to encode config TOML: {source}"))
        })?;
        fs::write(path, content).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to write config file {}: {source}",
                path.display()
            ))
        })
    }

    pub fn save_language_selection(
        path: impl AsRef<Path>,
        language: Option<&str>,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        let language = language
            .map(str::trim)
            .filter(|language| !language.is_empty() && !language.eq_ignore_ascii_case("auto"));

        table.insert(
            "whisper_language".to_string(),
            toml::Value::String(language.unwrap_or("auto").to_string()),
        );

        write_config_table(path, &table)
    }

    pub fn save_transcription_profile(
        path: impl AsRef<Path>,
        profile: TranscriptionProfile,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;

        table.insert(
            "transcription_profile".to_string(),
            toml::Value::String(profile.as_config_value().to_string()),
        );

        write_config_table(path, &table)
    }

    pub fn save_dictation_mode(path: impl AsRef<Path>, mode: DictationMode) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;

        table.insert(
            "dictation_mode".to_string(),
            toml::Value::String(mode.as_config_value().to_string()),
        );

        write_config_table(path, &table)
    }

    pub fn save_gui_profile(path: impl AsRef<Path>, profile: GuiProfile) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;

        table.insert(
            "gui_profile".to_string(),
            toml::Value::String(profile.as_config_value().to_string()),
        );

        write_config_table(path, &table)
    }

    pub fn save_codex_selection(
        path: impl AsRef<Path>,
        model: Option<&str>,
        profile: Option<&str>,
        reasoning_effort: Option<&str>,
        service_tier: Option<&str>,
        config_overrides: &[String],
        enable: bool,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;

        set_optional_string(&mut table, "codex_model", model);
        set_optional_string(&mut table, "codex_profile", profile);
        set_optional_string(&mut table, "codex_reasoning_effort", reasoning_effort);
        set_optional_string(&mut table, "codex_service_tier", service_tier);

        if config_overrides.is_empty() {
            table.remove("codex_config_overrides");
        } else {
            table.insert(
                "codex_config_overrides".to_string(),
                toml::Value::Array(
                    config_overrides
                        .iter()
                        .map(|value| toml::Value::String(value.to_string()))
                        .collect(),
                ),
            );
        }

        if enable {
            table.insert(
                "formatter_backend".to_string(),
                toml::Value::String(FormatterBackend::Codex.as_config_value().to_string()),
            );
            table.insert(
                "last_ai_formatter_backend".to_string(),
                toml::Value::String(FormatterBackend::Codex.as_config_value().to_string()),
            );
        }

        write_config_table(path, &table)
    }

    pub fn save_ai_formatting(
        path: impl AsRef<Path>,
        enabled: Option<bool>,
        hardware_tier: Option<AiHardwareTier>,
        log_retention_days: Option<u64>,
        preload_on_recording: Option<bool>,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;

        if let Some(enabled) = enabled {
            let backend = if enabled {
                FormatterBackend::Ollama
            } else {
                FormatterBackend::Rules
            };
            table.insert(
                "formatter_backend".to_string(),
                toml::Value::String(backend.as_config_value().to_string()),
            );
            if backend.is_ai() {
                table.insert(
                    "last_ai_formatter_backend".to_string(),
                    toml::Value::String(backend.as_config_value().to_string()),
                );
            }
        }

        if let Some(hardware_tier) = hardware_tier {
            table.insert(
                "ai_hardware_tier".to_string(),
                toml::Value::String(hardware_tier.as_config_value().to_string()),
            );
            table.insert(
                "ollama_model".to_string(),
                toml::Value::String(hardware_tier.ollama_model().to_string()),
            );
        }

        if let Some(days) = log_retention_days {
            table.insert(
                "format_log_retention_days".to_string(),
                toml::Value::Integer(days as i64),
            );
        }

        if let Some(preload) = preload_on_recording {
            table.insert(
                "ollama_preload_on_recording".to_string(),
                toml::Value::Boolean(preload),
            );
        }

        write_config_table(path, &table)
    }

    pub fn save_codex_profile(
        path: impl AsRef<Path>,
        profile: CodexProfileConfig,
    ) -> ChirperResult<()> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        let name = profile.name.trim();

        if name.is_empty() {
            return Err(ChirperError::Configuration(
                "Codex profile name cannot be empty".to_string(),
            ));
        }

        if !table.contains_key("codex_profiles") {
            table.insert(
                "codex_profiles".to_string(),
                toml::Value::Table(toml::Table::new()),
            );
        }

        let profiles = table
            .get_mut("codex_profiles")
            .and_then(toml::Value::as_table_mut)
            .ok_or_else(|| {
                ChirperError::Configuration(
                    "config key `codex_profiles` must be a table".to_string(),
                )
            })?;
        let mut profile_table = toml::Table::new();

        set_optional_string(&mut profile_table, "model", profile.model.as_deref());
        set_optional_string(&mut profile_table, "profile", profile.profile.as_deref());
        set_optional_string(
            &mut profile_table,
            "reasoning_effort",
            profile.reasoning_effort.as_deref(),
        );
        set_optional_string(
            &mut profile_table,
            "service_tier",
            profile.service_tier.as_deref(),
        );

        if !profile.config_overrides.is_empty() {
            profile_table.insert(
                "config_overrides".to_string(),
                toml::Value::Array(
                    profile
                        .config_overrides
                        .iter()
                        .map(|value| toml::Value::String(value.to_string()))
                        .collect(),
                ),
            );
        }

        profiles.insert(name.to_string(), toml::Value::Table(profile_table));
        write_config_table(path, &table)
    }

    pub fn remove_codex_profile(path: impl AsRef<Path>, name: &str) -> ChirperResult<bool> {
        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        let name = name.trim();

        if name.is_empty() {
            return Err(ChirperError::Configuration(
                "Codex profile name cannot be empty".to_string(),
            ));
        }

        let removed = if let Some(value) = table.get_mut("codex_profiles") {
            let profiles = value.as_table_mut().ok_or_else(|| {
                ChirperError::Configuration(
                    "config key `codex_profiles` must be a table".to_string(),
                )
            })?;
            let removed = profiles.remove(name).is_some();
            if profiles.is_empty() {
                table.remove("codex_profiles");
            }
            removed
        } else {
            false
        };

        write_config_table(path, &table)?;
        Ok(removed)
    }

    pub fn save_vocabulary_entry(
        path: impl AsRef<Path>,
        spoken: &str,
        written: &str,
    ) -> ChirperResult<()> {
        let spoken = spoken.trim();
        let written = written.trim();

        if spoken.is_empty() || written.is_empty() {
            return Err(ChirperError::Configuration(
                "vocabulary entries require spoken and written text".to_string(),
            ));
        }

        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        let mut vocabulary = remove_vocabulary_table(&mut table)?;
        vocabulary.insert(
            spoken.to_ascii_lowercase(),
            toml::Value::String(written.to_string()),
        );
        table.insert("vocabulary".to_string(), toml::Value::Table(vocabulary));

        write_config_table(path, &table)
    }

    pub fn remove_vocabulary_entry(path: impl AsRef<Path>, spoken: &str) -> ChirperResult<bool> {
        let spoken = spoken.trim();

        if spoken.is_empty() {
            return Err(ChirperError::Configuration(
                "vocabulary entries require spoken text".to_string(),
            ));
        }

        let path = path.as_ref();
        let mut table = read_config_table(path)?;
        let mut vocabulary = remove_vocabulary_table(&mut table)?;
        let removed = vocabulary.remove(&spoken.to_ascii_lowercase()).is_some();

        if vocabulary.is_empty() {
            table.remove("vocabulary");
        } else {
            table.insert("vocabulary".to_string(), toml::Value::Table(vocabulary));
        }

        write_config_table(path, &table)?;
        Ok(removed)
    }

    pub fn save_default_model_selection(
        model: &str,
        model_path: impl AsRef<Path>,
    ) -> ChirperResult<()> {
        Self::save_model_selection(Self::default_path(), model, model_path)
    }

    pub fn save_default_whispercpp_setup(
        model: &str,
        command: &str,
        model_path: impl AsRef<Path>,
        gpu_backend: GpuBackend,
    ) -> ChirperResult<()> {
        Self::save_whispercpp_setup(
            Self::default_path(),
            model,
            command,
            model_path,
            gpu_backend,
        )
    }

    pub fn save_default_audio_target(target: Option<&str>) -> ChirperResult<()> {
        Self::save_audio_target(Self::default_path(), target)
    }

    pub fn save_default_formatter_selection(
        backend: FormatterBackend,
        ollama_model: Option<&str>,
    ) -> ChirperResult<()> {
        Self::save_formatter_selection(Self::default_path(), backend, ollama_model)
    }

    pub fn save_default_language_selection(language: Option<&str>) -> ChirperResult<()> {
        Self::save_language_selection(Self::default_path(), language)
    }

    pub fn save_default_transcription_profile(profile: TranscriptionProfile) -> ChirperResult<()> {
        Self::save_transcription_profile(Self::default_path(), profile)
    }

    pub fn save_default_dictation_mode(mode: DictationMode) -> ChirperResult<()> {
        Self::save_dictation_mode(Self::default_path(), mode)
    }

    pub fn save_default_gui_profile(profile: GuiProfile) -> ChirperResult<()> {
        Self::save_gui_profile(Self::default_path(), profile)
    }

    pub fn save_default_codex_selection(
        model: Option<&str>,
        profile: Option<&str>,
        reasoning_effort: Option<&str>,
        service_tier: Option<&str>,
        config_overrides: &[String],
        enable: bool,
    ) -> ChirperResult<()> {
        Self::save_codex_selection(
            Self::default_path(),
            model,
            profile,
            reasoning_effort,
            service_tier,
            config_overrides,
            enable,
        )
    }

    pub fn save_default_ai_formatting(
        enabled: Option<bool>,
        hardware_tier: Option<AiHardwareTier>,
        log_retention_days: Option<u64>,
        preload_on_recording: Option<bool>,
    ) -> ChirperResult<()> {
        Self::save_ai_formatting(
            Self::default_path(),
            enabled,
            hardware_tier,
            log_retention_days,
            preload_on_recording,
        )
    }

    pub fn save_default_codex_profile(profile: CodexProfileConfig) -> ChirperResult<()> {
        Self::save_codex_profile(Self::default_path(), profile)
    }

    pub fn remove_default_codex_profile(name: &str) -> ChirperResult<bool> {
        Self::remove_codex_profile(Self::default_path(), name)
    }

    pub fn save_default_vocabulary_entry(spoken: &str, written: &str) -> ChirperResult<()> {
        Self::save_vocabulary_entry(Self::default_path(), spoken, written)
    }

    pub fn remove_default_vocabulary_entry(spoken: &str) -> ChirperResult<bool> {
        Self::remove_vocabulary_entry(Self::default_path(), spoken)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyEntry {
    pub spoken: String,
    pub written: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProfileConfig {
    pub name: String,
    pub model: Option<String>,
    pub profile: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub config_overrides: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiProfile {
    Gnome,
    None,
}

impl GuiProfile {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Gnome => "gnome",
            Self::None => "none",
        }
    }
}

impl FromStr for GuiProfile {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "gnome" => Ok(Self::Gnome),
            "none" | "nogui" | "disabled" | "off" => Ok(Self::None),
            _ => Err(unknown_value("gui_profile", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionProfile {
    Balanced,
    Fast,
}

impl TranscriptionProfile {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Balanced => "Current whisper.cpp defaults for better accuracy.",
            Self::Fast => "Lower-latency decoding with fewer retries and less context.",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Balanced, Self::Fast]
    }
}

impl FromStr for TranscriptionProfile {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "balanced" | "default" | "normal" | "quality" | "accurate" => Ok(Self::Balanced),
            "fast" | "quick" | "lowlatency" | "latency" => Ok(Self::Fast),
            _ => Err(unknown_value("transcription_profile", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiHardwareTier {
    Low,
    Medium,
    High,
}

impl AiHardwareTier {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low end hardware",
            Self::Medium => "Medium hardware",
            Self::High => "High hardware",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "Up to 8 GB VRAM",
            Self::Medium => "8 to 12 GB VRAM",
            Self::High => "16+ GB VRAM",
        }
    }

    pub fn ollama_model(self) -> &'static str {
        match self {
            Self::Low => "granite4.1:3b",
            Self::Medium => "granite4.1:8b",
            Self::High => "granite4.1:8b",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Low, Self::Medium, Self::High]
    }
}

impl FromStr for AiHardwareTier {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "low" | "lowend" | "8gb" | "upto8gb" => Ok(Self::Low),
            "medium" | "mid" | "8to12gb" | "12gb" => Ok(Self::Medium),
            "high" | "highend" | "16gb" | "16plusgb" => Ok(Self::High),
            _ => Err(unknown_value("ai_hardware_tier", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    PipeWire,
}

impl FromStr for AudioBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "pipewire" => Ok(Self::PipeWire),
            _ => Err(unknown_value("audio_backend", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackend {
    WhisperCpp,
}

impl FromStr for AsrBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "whispercpp" => Ok(Self::WhisperCpp),
            _ => Err(unknown_value("asr_backend", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    Auto,
    Cpu,
    Vulkan,
    Rocm,
    Cuda,
    OpenVino,
}

impl GpuBackend {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Rocm => "rocm",
            Self::Cuda => "cuda",
            Self::OpenVino => "openvino",
        }
    }
}

impl FromStr for GpuBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "auto" => Ok(Self::Auto),
            "cpu" => Ok(Self::Cpu),
            "vulkan" => Ok(Self::Vulkan),
            "rocm" | "hip" => Ok(Self::Rocm),
            "cuda" => Ok(Self::Cuda),
            "openvino" => Ok(Self::OpenVino),
            _ => Err(unknown_value("gpu_backend", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatterBackend {
    None,
    Rules,
    Ollama,
    Codex,
    LlamaCpp,
}

impl FormatterBackend {
    pub fn as_config_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rules => "rules",
            Self::Ollama => "ollama",
            Self::Codex => "codex",
            Self::LlamaCpp => "llama.cpp",
        }
    }

    pub fn is_ai(self) -> bool {
        matches!(self, Self::Ollama | Self::Codex)
    }
}

impl FromStr for FormatterBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "none" | "disabled" | "off" => Ok(Self::None),
            "rules" | "rulebased" | "localrules" => Ok(Self::Rules),
            "ollama" => Ok(Self::Ollama),
            "codex" | "codexcli" | "openai" => Ok(Self::Codex),
            _ => Err(unknown_value("formatter_backend", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionBackend {
    Clipboard,
    Uinput,
    IBus,
    X11,
}

impl FromStr for InsertionBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "clipboard" => Ok(Self::Clipboard),
            "uinput" => Ok(Self::Uinput),
            "ibus" => Ok(Self::IBus),
            "x11" => Ok(Self::X11),
            _ => Err(unknown_value("insertion_backend", value)),
        }
    }
}

impl FromStr for DictationMode {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "auto" => Ok(Self::Auto),
            "standard" | "text" | "prose" => Ok(Self::Standard),
            "email" => Ok(Self::Email),
            "command" | "shell" | "terminal" => Ok(Self::Command),
            "code" | "programming" => Ok(Self::Code),
            _ => Err(unknown_value("dictation_mode", value)),
        }
    }
}

fn parse_config_value<T>(key: &str, value: &toml::Value) -> ChirperResult<T>
where
    T: FromStr<Err = ChirperError>,
{
    parse_string(key, value)?.parse()
}

fn parse_last_ai_formatter_backend(value: &toml::Value) -> ChirperResult<Option<FormatterBackend>> {
    let value = parse_string("last_ai_formatter_backend", value)?;

    if is_legacy_llama_cpp_formatter(value) {
        return Ok(None);
    }

    let backend = value
        .parse::<FormatterBackend>()
        .map_err(|_| unknown_value("last_ai_formatter_backend", value))?;
    Ok(backend.is_ai().then_some(backend))
}

fn is_legacy_llama_cpp_formatter(value: &str) -> bool {
    normalize(value) == "llamacpp"
}

fn parse_string<'a>(key: &str, value: &'a toml::Value) -> ChirperResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| ChirperError::Configuration(format!("config key `{key}` must be a string")))
}

fn parse_bool(key: &str, value: &toml::Value) -> ChirperResult<bool> {
    value
        .as_bool()
        .ok_or_else(|| ChirperError::Configuration(format!("config key `{key}` must be a boolean")))
}

fn parse_u64(key: &str, value: &toml::Value) -> ChirperResult<u64> {
    let value = value.as_integer().ok_or_else(|| {
        ChirperError::Configuration(format!("config key `{key}` must be an integer"))
    })?;

    u64::try_from(value).map_err(|_| {
        ChirperError::Configuration(format!("config key `{key}` must be zero or greater"))
    })
}

fn parse_optional_string(key: &str, value: &toml::Value) -> ChirperResult<Option<String>> {
    let value = parse_string(key, value)?.trim();

    if value.is_empty() || value.eq_ignore_ascii_case("auto") {
        Ok(None)
    } else {
        Ok(Some(value.to_string()))
    }
}

fn parse_optional_path(key: &str, value: &toml::Value) -> ChirperResult<Option<PathBuf>> {
    Ok(parse_optional_string(key, value)?.map(PathBuf::from))
}

fn parse_vocabulary(value: &toml::Value) -> ChirperResult<Vec<VocabularyEntry>> {
    let table = value.as_table().ok_or_else(|| {
        ChirperError::Configuration("config key `vocabulary` must be a table".to_string())
    })?;
    let mut entries = table
        .iter()
        .map(|(spoken, written)| {
            Ok(VocabularyEntry {
                spoken: spoken.to_string(),
                written: parse_string("vocabulary", written)?.to_string(),
            })
        })
        .collect::<ChirperResult<Vec<_>>>()?;

    entries.sort_by(|left, right| {
        right
            .spoken
            .split_whitespace()
            .count()
            .cmp(&left.spoken.split_whitespace().count())
            .then_with(|| left.spoken.cmp(&right.spoken))
    });

    Ok(entries)
}

fn parse_string_array(key: &str, value: &toml::Value) -> ChirperResult<Vec<String>> {
    let values = value.as_array().ok_or_else(|| {
        ChirperError::Configuration(format!("config key `{key}` must be an array of strings"))
    })?;

    values
        .iter()
        .map(|value| Ok(parse_string(key, value)?.trim().to_string()))
        .filter(|value| {
            value
                .as_ref()
                .map(|value| !value.is_empty())
                .unwrap_or(true)
        })
        .collect()
}

fn parse_codex_profiles(value: &toml::Value) -> ChirperResult<Vec<CodexProfileConfig>> {
    let table = value.as_table().ok_or_else(|| {
        ChirperError::Configuration("config key `codex_profiles` must be a table".to_string())
    })?;
    let mut profiles = Vec::new();

    for (name, value) in table {
        let profile_table = value.as_table().ok_or_else(|| {
            ChirperError::Configuration(format!("codex profile `{name}` must be a table"))
        })?;
        let config_overrides = profile_table
            .get("config_overrides")
            .or_else(|| profile_table.get("config"))
            .map(|value| parse_string_array("codex_profiles.config_overrides", value))
            .transpose()?
            .unwrap_or_default();

        profiles.push(CodexProfileConfig {
            name: name.to_string(),
            model: profile_table
                .get("model")
                .map(|value| parse_optional_string("codex_profiles.model", value))
                .transpose()?
                .flatten(),
            profile: profile_table
                .get("profile")
                .map(|value| parse_optional_string("codex_profiles.profile", value))
                .transpose()?
                .flatten(),
            reasoning_effort: profile_table
                .get("reasoning_effort")
                .or_else(|| profile_table.get("effort"))
                .map(|value| parse_optional_string("codex_profiles.reasoning_effort", value))
                .transpose()?
                .flatten(),
            service_tier: profile_table
                .get("service_tier")
                .or_else(|| profile_table.get("tier"))
                .map(|value| parse_optional_string("codex_profiles.service_tier", value))
                .transpose()?
                .flatten(),
            config_overrides,
        });
    }

    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(profiles)
}

fn set_optional_string(table: &mut toml::Table, key: &str, value: Option<&str>) {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => {
            table.insert(key.to_string(), toml::Value::String(value.to_string()));
        }
        None => {
            table.remove(key);
        }
    }
}

fn read_config_table(path: &Path) -> ChirperResult<toml::Table> {
    if path.exists() {
        let content = fs::read_to_string(path).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to read config file {}: {source}",
                path.display()
            ))
        })?;

        content.parse::<toml::Table>().map_err(|source| {
            ChirperError::Configuration(format!("failed to parse config TOML: {source}"))
        })
    } else {
        Ok(toml::Table::new())
    }
}

fn write_config_table(path: &Path, table: &toml::Table) -> ChirperResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            ChirperError::Configuration(format!(
                "failed to create config directory {}: {source}",
                parent.display()
            ))
        })?;
    }

    let content = toml::to_string_pretty(table).map_err(|source| {
        ChirperError::Configuration(format!("failed to encode config TOML: {source}"))
    })?;
    fs::write(path, content).map_err(|source| {
        ChirperError::Configuration(format!(
            "failed to write config file {}: {source}",
            path.display()
        ))
    })
}

fn remove_vocabulary_table(table: &mut toml::Table) -> ChirperResult<toml::Table> {
    match table.remove("vocabulary") {
        Some(toml::Value::Table(vocabulary)) => Ok(vocabulary),
        Some(_) => Err(ChirperError::Configuration(
            "config key `vocabulary` must be a table".to_string(),
        )),
        None => Ok(toml::Table::new()),
    }
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '_', ' ', '.'], "")
}

fn unknown_value(key: &str, value: &str) -> ChirperError {
    ChirperError::Configuration(format!("unknown value `{value}` for config key `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_values_keep_defaults() {
        let config = ChirperConfig::from_toml_str(r#"gpu_backend = "rocm""#).unwrap();

        assert_eq!(config.audio_backend, AudioBackend::PipeWire);
        assert_eq!(config.pipewire_target, None);
        assert_eq!(config.asr_backend, AsrBackend::WhisperCpp);
        assert_eq!(config.transcription_profile, TranscriptionProfile::Balanced);
        assert_eq!(config.gpu_backend, GpuBackend::Rocm);
        assert_eq!(config.insertion_backend, InsertionBackend::Clipboard);
        assert_eq!(config.dictation_mode, DictationMode::Auto);
        assert_eq!(config.gui_profile, GuiProfile::Gnome);
        assert_eq!(config.whispercpp_command, "whisper-cli");
        assert_eq!(config.whispercpp_model_path, None);
    }

    #[test]
    fn accepts_backend_aliases() {
        let config = ChirperConfig::from_toml_str(
            r#"
            asr_backend = "whisper-cpp"
            transcription_profile = "fast"
            pipewire_target = "alsa_input.usb-example.mic"
            gpu_backend = "HIP"
            formatter_backend = "codex"
            last_ai_formatter_backend = "codex"
            insertion_backend = "i_bus"
            dictation_mode = "code"
            gui_profile = "no-gui"
            whisper_model = "small"
            whispercpp_command = "/opt/whisper.cpp/build/bin/whisper-cli"
            whispercpp_model_path = "/models/ggml-small.bin"
            whisper_language = "en"
            ollama_command = "/usr/bin/ollama"
            ollama_model = "llama3.1:8b"
            ai_hardware_tier = "low"
            format_log_retention_days = 30
            ollama_preload_on_recording = false
            codex_command = "/usr/bin/codex"
            codex_model = "gpt-5.5"
            codex_profile = "work"
            codex_reasoning_effort = "xhigh"
            codex_service_tier = "fast"
            codex_config_overrides = ["model_verbosity=\"low\""]

            [codex_profiles.quick]
            model = "gpt-5.4-mini"
            reasoning_effort = "low"
            service_tier = "fast"
            "#,
        )
        .unwrap();

        assert_eq!(config.asr_backend, AsrBackend::WhisperCpp);
        assert_eq!(config.transcription_profile, TranscriptionProfile::Fast);
        assert_eq!(
            config.pipewire_target,
            Some("alsa_input.usb-example.mic".to_string())
        );
        assert_eq!(config.gpu_backend, GpuBackend::Rocm);
        assert_eq!(config.formatter_backend, FormatterBackend::Codex);
        assert_eq!(
            config.last_ai_formatter_backend,
            Some(FormatterBackend::Codex)
        );
        assert_eq!(config.insertion_backend, InsertionBackend::IBus);
        assert_eq!(config.dictation_mode, DictationMode::Code);
        assert_eq!(config.gui_profile, GuiProfile::None);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(
            config.whispercpp_model_path,
            Some(PathBuf::from("/models/ggml-small.bin"))
        );
        assert_eq!(config.whisper_language, Some("en".to_string()));
        assert_eq!(config.ollama_command, "/usr/bin/ollama");
        assert_eq!(config.ollama_model, "llama3.1:8b");
        assert_eq!(config.ai_hardware_tier, AiHardwareTier::Low);
        assert_eq!(config.format_log_retention_days, 30);
        assert!(!config.ollama_preload_on_recording);
        assert_eq!(config.codex_command, "/usr/bin/codex");
        assert_eq!(config.codex_model, Some("gpt-5.5".to_string()));
        assert_eq!(config.codex_profile, Some("work".to_string()));
        assert_eq!(config.codex_reasoning_effort, Some("xhigh".to_string()));
        assert_eq!(config.codex_service_tier, Some("fast".to_string()));
        assert_eq!(
            config.codex_config_overrides,
            vec!["model_verbosity=\"low\""]
        );
        assert_eq!(config.codex_profiles.len(), 1);
        assert_eq!(config.codex_profiles[0].name, "quick");
    }

    #[test]
    fn parses_vocabulary_entries_longest_first() {
        let config = ChirperConfig::from_toml_str(
            r#"
            [vocabulary]
            "prepped" = "Prepd"
            "silas on linux" = "SilasOnLinux"
            "#,
        )
        .unwrap();

        assert_eq!(
            config.vocabulary,
            vec![
                VocabularyEntry {
                    spoken: "silas on linux".to_string(),
                    written: "SilasOnLinux".to_string(),
                },
                VocabularyEntry {
                    spoken: "prepped".to_string(),
                    written: "Prepd".to_string(),
                },
            ]
        );
    }

    #[test]
    fn accepts_rule_formatter_alias() {
        let config = ChirperConfig::from_toml_str(r#"formatter_backend = "rule-based""#).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
    }

    #[test]
    fn rejects_llama_cpp_as_active_formatter_backend() {
        let result = ChirperConfig::from_toml_str(r#"formatter_backend = "llama.cpp""#);

        assert!(result.is_err());
    }

    #[test]
    fn ignores_legacy_llama_cpp_last_ai_formatter_backend() {
        let config = ChirperConfig::from_toml_str(
            r#"
            formatter_backend = "rules"
            last_ai_formatter_backend = "llama.cpp"
            "#,
        )
        .unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
        assert_eq!(config.last_ai_formatter_backend, None);
    }

    #[test]
    fn derives_model_name_from_standard_ggml_path() {
        let model = ChirperConfig::model_name_from_path("/models/ggml-large-v3-turbo-q5_0.bin");

        assert_eq!(model, Some("large-v3-turbo-q5_0".to_string()));
    }

    #[test]
    fn save_model_selection_updates_model_fields() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "model"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            formatter_backend = "rules"
            "#,
        )
        .unwrap();

        ChirperConfig::save_model_selection(&path, "small", "/models/ggml-small.bin").unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(
            config.whispercpp_model_path,
            Some(PathBuf::from("/models/ggml-small.bin"))
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_whispercpp_setup_updates_existing_config() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "whispercpp"
        ));
        fs::write(
            &path,
            r#"
            gui_profile = "gnome"
            formatter_backend = "none"
            whisper_language = "de"
            "#,
        )
        .unwrap();

        ChirperConfig::save_whispercpp_setup(
            &path,
            "base",
            "/data/chirper/src/whisper.cpp/build-vulkan/bin/whisper-cli",
            "/data/chirper/models/ggml-base.bin",
            GpuBackend::Vulkan,
        )
        .unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gui_profile, GuiProfile::Gnome);
        assert_eq!(config.formatter_backend, FormatterBackend::None);
        assert_eq!(config.asr_backend, AsrBackend::WhisperCpp);
        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "base");
        assert_eq!(
            config.whispercpp_command,
            "/data/chirper/src/whisper.cpp/build-vulkan/bin/whisper-cli"
        );
        assert_eq!(
            config.whispercpp_model_path,
            Some(PathBuf::from("/data/chirper/models/ggml-base.bin"))
        );
        assert_eq!(config.whisper_language, Some("de".to_string()));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_audio_target_updates_only_audio_field() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "audio"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();

        ChirperConfig::save_audio_target(&path, Some("alsa_input.example")).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(
            config.pipewire_target,
            Some("alsa_input.example".to_string())
        );

        ChirperConfig::save_audio_target(&path, None).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.pipewire_target, None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_formatter_selection_updates_backend_and_ollama_model() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "formatter"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            ollama_model = "old-model"
            "#,
        )
        .unwrap();

        ChirperConfig::save_formatter_selection(
            &path,
            FormatterBackend::Ollama,
            Some("llama3.2:latest"),
        )
        .unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.formatter_backend, FormatterBackend::Ollama);
        assert_eq!(
            config.last_ai_formatter_backend,
            Some(FormatterBackend::Ollama)
        );
        assert_eq!(config.ollama_model, "llama3.2:latest");

        ChirperConfig::save_formatter_selection(&path, FormatterBackend::Codex, None).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Codex);
        assert_eq!(
            config.last_ai_formatter_backend,
            Some(FormatterBackend::Codex)
        );
        assert_eq!(config.ollama_model, "llama3.2:latest");

        ChirperConfig::save_formatter_selection(&path, FormatterBackend::Rules, None).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
        assert_eq!(
            config.last_ai_formatter_backend,
            Some(FormatterBackend::Codex)
        );
        assert_eq!(config.ollama_model, "llama3.2:latest");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_formatter_selection_rejects_llama_cpp_backend() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "formatter-llama-cpp"
        ));
        fs::write(&path, r#"formatter_backend = "rules""#).unwrap();

        let result = ChirperConfig::save_formatter_selection(
            &path,
            FormatterBackend::LlamaCpp,
            Some("ignored"),
        );

        assert!(result.is_err());
        let config = ChirperConfig::load_from_path(&path).unwrap();
        assert_eq!(config.formatter_backend, FormatterBackend::Rules);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_language_selection_updates_only_language_field() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "language"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();

        ChirperConfig::save_language_selection(&path, Some("id")).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.whisper_language, Some("id".to_string()));

        ChirperConfig::save_language_selection(&path, None).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.whisper_language, None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_transcription_profile_updates_only_profile_field() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "transcription-profile"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();

        ChirperConfig::save_transcription_profile(&path, TranscriptionProfile::Fast).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.transcription_profile, TranscriptionProfile::Fast);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_dictation_mode_updates_only_mode_field() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "dictation-mode"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();

        ChirperConfig::save_dictation_mode(&path, DictationMode::Code).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.dictation_mode, DictationMode::Code);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_gui_profile_updates_only_profile_field() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "gui-profile"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();

        ChirperConfig::save_gui_profile(&path, GuiProfile::None).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.gui_profile, GuiProfile::None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_codex_selection_updates_only_codex_fields() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "codex"
        ));
        fs::write(
            &path,
            r#"
            gpu_backend = "vulkan"
            whisper_model = "small"
            "#,
        )
        .unwrap();
        let overrides = vec!["model_verbosity=\"low\"".to_string()];

        ChirperConfig::save_codex_selection(
            &path,
            Some("gpt-5.5"),
            None,
            Some("low"),
            Some("fast"),
            &overrides,
            true,
        )
        .unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.gpu_backend, GpuBackend::Vulkan);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(config.formatter_backend, FormatterBackend::Codex);
        assert_eq!(
            config.last_ai_formatter_backend,
            Some(FormatterBackend::Codex)
        );
        assert_eq!(config.codex_model, Some("gpt-5.5".to_string()));
        assert_eq!(config.codex_reasoning_effort, Some("low".to_string()));
        assert_eq!(config.codex_service_tier, Some("fast".to_string()));
        assert_eq!(config.codex_config_overrides, overrides);

        ChirperConfig::save_codex_selection(&path, None, None, None, None, &[], false).unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Codex);
        assert_eq!(config.codex_model, None);
        assert_eq!(config.codex_config_overrides, Vec::<String>::new());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_remove_codex_profiles() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "codex-profile"
        ));
        fs::write(&path, r#"formatter_backend = "ollama""#).unwrap();

        ChirperConfig::save_codex_profile(
            &path,
            CodexProfileConfig {
                name: "fast".to_string(),
                model: Some("gpt-5.5".to_string()),
                profile: None,
                reasoning_effort: Some("low".to_string()),
                service_tier: Some("priority".to_string()),
                config_overrides: vec!["model_verbosity=\"low\"".to_string()],
            },
        )
        .unwrap();

        let config = ChirperConfig::load_from_path(&path).unwrap();
        assert_eq!(config.formatter_backend, FormatterBackend::Ollama);
        assert_eq!(config.codex_profiles.len(), 1);
        assert_eq!(config.codex_profiles[0].name, "fast");
        assert_eq!(config.codex_profiles[0].model, Some("gpt-5.5".to_string()));
        assert_eq!(
            config.codex_profiles[0].service_tier,
            Some("priority".to_string())
        );
        assert_eq!(
            config.codex_profiles[0].config_overrides,
            vec!["model_verbosity=\"low\""]
        );

        assert!(ChirperConfig::remove_codex_profile(&path, "fast").unwrap());
        assert!(!ChirperConfig::remove_codex_profile(&path, "missing").unwrap());
        let config = ChirperConfig::load_from_path(&path).unwrap();
        assert!(config.codex_profiles.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn save_and_remove_vocabulary_entries() {
        let path = env::temp_dir().join(format!(
            "chirper-config-test-{}-{}.toml",
            std::process::id(),
            "vocabulary"
        ));
        fs::write(&path, r#"formatter_backend = "rules""#).unwrap();

        ChirperConfig::save_vocabulary_entry(&path, "Silas on Linux", "SilasOnLinux").unwrap();
        ChirperConfig::save_vocabulary_entry(&path, "prepped", "Prepd").unwrap();
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
        assert_eq!(config.vocabulary.len(), 2);
        assert!(config
            .vocabulary
            .iter()
            .any(|entry| entry.spoken == "silas on linux" && entry.written == "SilasOnLinux"));
        assert!(config
            .vocabulary
            .iter()
            .any(|entry| entry.spoken == "prepped" && entry.written == "Prepd"));

        assert!(ChirperConfig::remove_vocabulary_entry(&path, "prepped").unwrap());
        assert!(!ChirperConfig::remove_vocabulary_entry(&path, "missing").unwrap());
        let config = ChirperConfig::load_from_path(&path).unwrap();

        assert_eq!(config.vocabulary.len(), 1);
        assert_eq!(config.vocabulary[0].written, "SilasOnLinux");

        let _ = fs::remove_file(path);
    }
}

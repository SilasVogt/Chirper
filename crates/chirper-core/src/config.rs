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
    pub gpu_backend: GpuBackend,
    pub formatter_backend: FormatterBackend,
    pub insertion_backend: InsertionBackend,
    pub dictation_mode: DictationMode,
    pub whisper_model: String,
    pub whispercpp_command: String,
    pub whispercpp_model_path: Option<PathBuf>,
    pub whisper_language: Option<String>,
    pub ollama_command: String,
    pub ollama_model: String,
}

impl Default for ChirperConfig {
    fn default() -> Self {
        Self {
            audio_backend: AudioBackend::PipeWire,
            pipewire_target: None,
            asr_backend: AsrBackend::WhisperCpp,
            gpu_backend: GpuBackend::Auto,
            formatter_backend: FormatterBackend::None,
            insertion_backend: InsertionBackend::Clipboard,
            dictation_mode: DictationMode::Auto,
            whisper_model: "base".to_string(),
            whispercpp_command: "whisper-cli".to_string(),
            whispercpp_model_path: None,
            whisper_language: None,
            ollama_command: "ollama".to_string(),
            ollama_model: "llama3.2".to_string(),
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

        if let Some(value) = table.get("gpu_backend") {
            config.gpu_backend = parse_config_value("gpu_backend", value)?;
        }

        if let Some(value) = table.get("formatter_backend") {
            config.formatter_backend = parse_config_value("formatter_backend", value)?;
        }

        if let Some(value) = table.get("insertion_backend") {
            config.insertion_backend = parse_config_value("insertion_backend", value)?;
        }

        if let Some(value) = table.get("dictation_mode") {
            config.dictation_mode = parse_config_value("dictation_mode", value)?;
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

    pub fn save_default_model_selection(
        model: &str,
        model_path: impl AsRef<Path>,
    ) -> ChirperResult<()> {
        Self::save_model_selection(Self::default_path(), model, model_path)
    }

    pub fn save_default_audio_target(target: Option<&str>) -> ChirperResult<()> {
        Self::save_audio_target(Self::default_path(), target)
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
    LlamaCpp,
}

impl FromStr for FormatterBackend {
    type Err = ChirperError;

    fn from_str(value: &str) -> ChirperResult<Self> {
        match normalize(value).as_str() {
            "none" | "disabled" | "off" => Ok(Self::None),
            "rules" | "rulebased" | "localrules" => Ok(Self::Rules),
            "ollama" => Ok(Self::Ollama),
            "llamacpp" => Ok(Self::LlamaCpp),
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

fn parse_string<'a>(key: &str, value: &'a toml::Value) -> ChirperResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| ChirperError::Configuration(format!("config key `{key}` must be a string")))
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
        assert_eq!(config.gpu_backend, GpuBackend::Rocm);
        assert_eq!(config.insertion_backend, InsertionBackend::Clipboard);
        assert_eq!(config.dictation_mode, DictationMode::Auto);
        assert_eq!(config.whispercpp_command, "whisper-cli");
        assert_eq!(config.whispercpp_model_path, None);
    }

    #[test]
    fn accepts_backend_aliases() {
        let config = ChirperConfig::from_toml_str(
            r#"
            asr_backend = "whisper-cpp"
            pipewire_target = "alsa_input.usb-example.mic"
            gpu_backend = "HIP"
            formatter_backend = "llama.cpp"
            insertion_backend = "i_bus"
            dictation_mode = "code"
            whisper_model = "small"
            whispercpp_command = "/opt/whisper.cpp/build/bin/whisper-cli"
            whispercpp_model_path = "/models/ggml-small.bin"
            whisper_language = "en"
            ollama_command = "/usr/bin/ollama"
            ollama_model = "llama3.1:8b"
            "#,
        )
        .unwrap();

        assert_eq!(config.asr_backend, AsrBackend::WhisperCpp);
        assert_eq!(
            config.pipewire_target,
            Some("alsa_input.usb-example.mic".to_string())
        );
        assert_eq!(config.gpu_backend, GpuBackend::Rocm);
        assert_eq!(config.formatter_backend, FormatterBackend::LlamaCpp);
        assert_eq!(config.insertion_backend, InsertionBackend::IBus);
        assert_eq!(config.dictation_mode, DictationMode::Code);
        assert_eq!(config.whisper_model, "small");
        assert_eq!(
            config.whispercpp_model_path,
            Some(PathBuf::from("/models/ggml-small.bin"))
        );
        assert_eq!(config.whisper_language, Some("en".to_string()));
        assert_eq!(config.ollama_command, "/usr/bin/ollama");
        assert_eq!(config.ollama_model, "llama3.1:8b");
    }

    #[test]
    fn accepts_rule_formatter_alias() {
        let config = ChirperConfig::from_toml_str(r#"formatter_backend = "rule-based""#).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
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
}

use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{ChirperError, ChirperResult, DictationMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChirperConfig {
    pub audio_backend: AudioBackend,
    pub asr_backend: AsrBackend,
    pub gpu_backend: GpuBackend,
    pub formatter_backend: FormatterBackend,
    pub insertion_backend: InsertionBackend,
    pub dictation_mode: DictationMode,
    pub whisper_model: String,
    pub whispercpp_command: String,
    pub whispercpp_model_path: Option<PathBuf>,
    pub whisper_language: Option<String>,
}

impl Default for ChirperConfig {
    fn default() -> Self {
        Self {
            audio_backend: AudioBackend::PipeWire,
            asr_backend: AsrBackend::WhisperCpp,
            gpu_backend: GpuBackend::Auto,
            formatter_backend: FormatterBackend::None,
            insertion_backend: InsertionBackend::Clipboard,
            dictation_mode: DictationMode::Auto,
            whisper_model: "base".to_string(),
            whispercpp_command: "whisper-cli".to_string(),
            whispercpp_model_path: None,
            whisper_language: None,
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
            gpu_backend = "HIP"
            formatter_backend = "llama.cpp"
            insertion_backend = "i_bus"
            dictation_mode = "code"
            whisper_model = "small"
            whispercpp_command = "/opt/whisper.cpp/build/bin/whisper-cli"
            whispercpp_model_path = "/models/ggml-small.bin"
            whisper_language = "en"
            "#,
        )
        .unwrap();

        assert_eq!(config.asr_backend, AsrBackend::WhisperCpp);
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
    }

    #[test]
    fn accepts_rule_formatter_alias() {
        let config = ChirperConfig::from_toml_str(r#"formatter_backend = "rule-based""#).unwrap();

        assert_eq!(config.formatter_backend, FormatterBackend::Rules);
    }
}

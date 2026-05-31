use std::{
    path::{Path, PathBuf},
    process::Command,
};

use chirper_core::{
    AsrEngine, CapturedAudio, ChirperConfig, ChirperError, ChirperResult, GpuBackend, Transcript,
    TranscriptionProfile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperCppOptions {
    pub command: String,
    pub model_path: PathBuf,
    pub language: Option<String>,
    pub transcription_profile: TranscriptionProfile,
    pub gpu_backend: GpuBackend,
    pub extra_args: Vec<String>,
}

impl WhisperCppOptions {
    pub fn from_config(config: &ChirperConfig) -> ChirperResult<Self> {
        let model_path = config.whispercpp_model_path.clone().ok_or_else(|| {
            ChirperError::Configuration(
                "whispercpp_model_path must be configured before transcription".to_string(),
            )
        })?;

        Ok(Self {
            command: config.whispercpp_command.clone(),
            model_path,
            language: config.whisper_language.clone(),
            transcription_profile: config.transcription_profile,
            gpu_backend: config.gpu_backend,
            extra_args: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhisperCppAsr {
    options: WhisperCppOptions,
}

impl WhisperCppAsr {
    pub fn new(options: WhisperCppOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &WhisperCppOptions {
        &self.options
    }
}

impl AsrEngine for WhisperCppAsr {
    fn transcribe(&self, audio: &CapturedAudio) -> ChirperResult<Transcript> {
        if !self.options.model_path.exists() {
            return Err(ChirperError::Configuration(format!(
                "whisper.cpp model not found: {}",
                self.options.model_path.display()
            )));
        }

        if !audio.path.exists() {
            return Err(ChirperError::Transcription(format!(
                "audio file not found: {}",
                audio.path.display()
            )));
        }

        let output = Command::new(&self.options.command)
            .args(command_args(&self.options, &audio.path))
            .output()
            .map_err(|source| {
                ChirperError::Transcription(format!(
                    "failed to run `{}`: {source}",
                    self.options.command
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ChirperError::Transcription(format!(
                "`{}` exited with status {}: {}",
                self.options.command,
                output.status,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let text = clean_transcript(&stdout);

        Ok(Transcript {
            text,
            language: self.options.language.clone(),
        })
    }
}

fn command_args(options: &WhisperCppOptions, audio_path: &Path) -> Vec<String> {
    let mut args = vec![
        "-m".to_string(),
        options.model_path.display().to_string(),
        "-f".to_string(),
        audio_path.display().to_string(),
        "-nt".to_string(),
        "-np".to_string(),
    ];

    if options.gpu_backend == GpuBackend::Cpu {
        args.push("-ng".to_string());
    }

    if let Some(language) = &options.language {
        args.push("-l".to_string());
        args.push(language.clone());
    }

    if options.transcription_profile == TranscriptionProfile::Fast {
        args.extend([
            "-nf".to_string(),
            "-bo".to_string(),
            "1".to_string(),
            "-bs".to_string(),
            "1".to_string(),
            "-mc".to_string(),
            "0".to_string(),
        ]);
    }

    args.extend(options.extra_args.clone());
    args
}

fn clean_transcript(stdout: &str) -> String {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "[BLANK_AUDIO]")
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> WhisperCppOptions {
        WhisperCppOptions {
            command: "whisper-cli".to_string(),
            model_path: PathBuf::from("/models/ggml-base.bin"),
            language: Some("en".to_string()),
            transcription_profile: TranscriptionProfile::Balanced,
            gpu_backend: GpuBackend::Auto,
            extra_args: Vec::new(),
        }
    }

    #[test]
    fn builds_default_whisper_cli_args() {
        let args = command_args(&options(), Path::new("/tmp/input.wav"));

        assert_eq!(
            args,
            vec![
                "-m",
                "/models/ggml-base.bin",
                "-f",
                "/tmp/input.wav",
                "-nt",
                "-np",
                "-l",
                "en"
            ]
        );
    }

    #[test]
    fn cpu_backend_disables_gpu() {
        let mut options = options();
        options.gpu_backend = GpuBackend::Cpu;

        let args = command_args(&options, Path::new("/tmp/input.wav"));

        assert!(args.contains(&"-ng".to_string()));
    }

    #[test]
    fn fast_profile_uses_lower_latency_decoder_args() {
        let mut options = options();
        options.transcription_profile = TranscriptionProfile::Fast;

        let args = command_args(&options, Path::new("/tmp/input.wav"));

        assert!(args.contains(&"-nf".to_string()));
        assert_eq!(
            args.windows(2).filter(|pair| *pair == ["-bo", "1"]).count(),
            1
        );
        assert_eq!(
            args.windows(2).filter(|pair| *pair == ["-bs", "1"]).count(),
            1
        );
        assert_eq!(
            args.windows(2).filter(|pair| *pair == ["-mc", "0"]).count(),
            1
        );
    }

    #[test]
    fn cleans_multiline_cli_output() {
        let text = clean_transcript("\n  hello world\nthis is chirper  \n");

        assert_eq!(text, "hello world this is chirper");
    }

    #[test]
    fn filters_blank_audio_sentinel() {
        let text = clean_transcript("[BLANK_AUDIO]\n");

        assert_eq!(text, "");
    }

    #[test]
    fn config_requires_model_path() {
        let config = ChirperConfig::default();

        assert!(WhisperCppOptions::from_config(&config).is_err());
    }
}

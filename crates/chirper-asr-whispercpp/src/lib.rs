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

        // Whisper makes up words such as "you" for silent audio, which is what
        // a muted microphone records. Return no text so callers report that no
        // speech was detected.
        if is_silent_wav(&audio.path) {
            return Ok(Transcript {
                text: String::new(),
                language: self.options.language.clone(),
            });
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

/// Recordings whose loudest sample stays at or below this level (about
/// -60 dBFS) hold no speech. A muted microphone records exact zeros, and a
/// live microphone in a quiet room is well above this.
const SILENCE_PEAK: i32 = 32;

fn is_silent_wav(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| is_silent_wav_bytes(&bytes))
}

/// Whether the bytes are a 16-bit PCM WAV file with no sample louder than
/// `SILENCE_PEAK`. Anything else counts as not silent, so Whisper decides.
fn is_silent_wav_bytes(bytes: &[u8]) -> bool {
    pcm16_data(bytes).is_some_and(|data| {
        let (samples, _) = data.as_chunks::<2>();
        samples
            .iter()
            .all(|&sample| i32::from(i16::from_le_bytes(sample)).abs() <= SILENCE_PEAK)
    })
}

/// The sample bytes of a 16-bit PCM WAV file.
fn pcm16_data(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }

    let mut is_pcm16 = false;
    let mut offset = 12;
    while let Some(header) = bytes.get(offset..offset + 8) {
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        let body = offset + 8;

        if &header[0..4] == b"fmt " {
            let format = bytes.get(body..body.checked_add(usize::try_from(size).ok()?)?)?;
            let tag = u16::from_le_bytes([*format.first()?, *format.get(1)?]);
            let bits = u16::from_le_bytes([*format.get(14)?, *format.get(15)?]);
            // WAVE_FORMAT_EXTENSIBLE keeps the real format code at the start
            // of its sub-format GUID.
            let tag = if tag == 0xFFFE {
                u16::from_le_bytes([*format.get(24)?, *format.get(25)?])
            } else {
                tag
            };
            is_pcm16 = tag == 1 && bits == 16;
        } else if &header[0..4] == b"data" {
            if !is_pcm16 {
                return None;
            }
            // A recorder that was not stopped cleanly can leave the size unset.
            let end = match size {
                0 | u32::MAX => bytes.len(),
                size => body
                    .saturating_add(usize::try_from(size).ok()?)
                    .min(bytes.len()),
            };
            return bytes.get(body..end);
        }

        // Chunks are padded to an even number of bytes.
        let size = usize::try_from(size).ok()?;
        offset = body.checked_add(size)?.checked_add(size % 2)?;
    }

    None
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

    /// A mono 16 kHz WAV file like the ones `pw-record` writes.
    fn wav(format_tag: u16, bits: u16, data_size: Option<u32>, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + data.len() as u32).to_le_bytes());
        bytes.extend(b"WAVE");
        bytes.extend(b"LIST");
        bytes.extend(3u32.to_le_bytes());
        bytes.extend(b"abc\0");
        bytes.extend(b"fmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(format_tag.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(16_000u32.to_le_bytes());
        bytes.extend(32_000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(bits.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_size.unwrap_or(data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    }

    #[test]
    fn detects_silent_recordings() {
        assert!(is_silent_wav_bytes(&wav(1, 16, None, &[0; 1600])));
        assert!(is_silent_wav_bytes(&wav(1, 16, None, &[3, -32, 32, 0])));
        assert!(!is_silent_wav_bytes(&wav(1, 16, None, &[0, 0, -33, 0])));
        assert!(!is_silent_wav_bytes(&wav(1, 16, None, &[0, i16::MIN, 0])));
    }

    #[test]
    fn reads_unset_data_size_to_the_end() {
        assert!(is_silent_wav_bytes(&wav(1, 16, Some(0), &[0, 0, 0])));
        assert!(!is_silent_wav_bytes(&wav(1, 16, Some(0), &[0, 0, 900])));
    }

    #[test]
    fn leaves_other_audio_formats_to_whisper() {
        assert!(!is_silent_wav_bytes(&wav(3, 32, None, &[0, 0])));
        assert!(!is_silent_wav_bytes(&wav(1, 8, None, &[0, 0])));
        assert!(!is_silent_wav_bytes(b"ID3\x04 not a wav file"));
        assert!(!is_silent_wav_bytes(b""));
    }

    #[test]
    fn silent_recording_skips_whisper() {
        let path = std::env::temp_dir().join(format!(
            "chirper-asr-silence-test-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, wav(1, 16, None, &[0; 1600])).unwrap();
        let mut options = options();
        // Running Whisper would fail, so an Ok result means it was skipped.
        options.command = "/nonexistent/whisper-cli".to_string();
        options.model_path = path.clone();
        let audio = CapturedAudio {
            path: path.clone(),
            sample_rate_hz: 16_000,
            channels: 1,
        };

        let transcript = WhisperCppAsr::new(options).transcribe(&audio);
        let _ = std::fs::remove_file(&path);

        assert_eq!(transcript.unwrap().text, "");
    }
}

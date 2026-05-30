use std::{
    env, fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chirper_core::{AudioSource, CapturedAudio, ChirperConfig, ChirperError, ChirperResult};

const DEFAULT_SAMPLE_RATE_HZ: u32 = 16_000;
const DEFAULT_CHANNELS: u16 = 1;
const STOP_WAIT: Duration = Duration::from_millis(50);
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipeWireRecorderOptions {
    pub output_dir: PathBuf,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub command: String,
    pub target: Option<String>,
}

impl Default for PipeWireRecorderOptions {
    fn default() -> Self {
        Self {
            output_dir: default_output_dir(),
            sample_rate_hz: DEFAULT_SAMPLE_RATE_HZ,
            channels: DEFAULT_CHANNELS,
            command: "pw-record".to_string(),
            target: None,
        }
    }
}

impl PipeWireRecorderOptions {
    pub fn from_config(config: &ChirperConfig) -> Self {
        Self {
            target: config.pipewire_target.clone(),
            ..Self::default()
        }
    }
}

#[derive(Debug)]
pub struct PipeWireRecorder {
    options: PipeWireRecorderOptions,
    active: Option<ActiveRecording>,
}

#[derive(Debug)]
struct ActiveRecording {
    child: Child,
    audio: CapturedAudio,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedRecording {
    pub pid: u32,
    pub audio: CapturedAudio,
}

impl PipeWireRecorder {
    pub fn new(options: PipeWireRecorderOptions) -> Self {
        Self {
            options,
            active: None,
        }
    }

    pub fn active_path(&self) -> Option<&Path> {
        self.active
            .as_ref()
            .map(|recording| recording.audio.path.as_path())
    }

    pub fn start_detached(options: PipeWireRecorderOptions) -> ChirperResult<DetachedRecording> {
        fs::create_dir_all(&options.output_dir).map_err(|source| {
            ChirperError::Audio(format!(
                "failed to create recording directory {}: {source}",
                options.output_dir.display()
            ))
        })?;

        let path = next_recording_path(&options.output_dir);
        let child = spawn_pw_record(&options, &path, true)?;
        let pid = child.id();
        drop(child);

        Ok(DetachedRecording {
            pid,
            audio: CapturedAudio {
                path,
                sample_rate_hz: options.sample_rate_hz,
                channels: options.channels,
            },
        })
    }

    pub fn stop_detached(recording: &DetachedRecording) -> ChirperResult<CapturedAudio> {
        stop_process_by_pid(recording.pid)?;

        if !recording.audio.path.exists() {
            return Err(ChirperError::Audio(format!(
                "recording file was not created: {}",
                recording.audio.path.display()
            )));
        }

        Ok(recording.audio.clone())
    }
}

impl Default for PipeWireRecorder {
    fn default() -> Self {
        Self::new(PipeWireRecorderOptions::default())
    }
}

impl AudioSource for PipeWireRecorder {
    fn start_recording(&mut self) -> ChirperResult<()> {
        if self.active.is_some() {
            return Err(ChirperError::Audio(
                "PipeWire recording is already active".to_string(),
            ));
        }

        fs::create_dir_all(&self.options.output_dir).map_err(|source| {
            ChirperError::Audio(format!(
                "failed to create recording directory {}: {source}",
                self.options.output_dir.display()
            ))
        })?;

        let path = next_recording_path(&self.options.output_dir);
        let child = spawn_pw_record(&self.options, &path, false)?;

        self.active = Some(ActiveRecording {
            child,
            audio: CapturedAudio {
                path,
                sample_rate_hz: self.options.sample_rate_hz,
                channels: self.options.channels,
            },
        });

        Ok(())
    }

    fn stop_recording(&mut self) -> ChirperResult<CapturedAudio> {
        let Some(mut recording) = self.active.take() else {
            return Err(ChirperError::Audio(
                "PipeWire recording is not active".to_string(),
            ));
        };

        stop_process(&mut recording.child)?;

        if !recording.audio.path.exists() {
            return Err(ChirperError::Audio(format!(
                "recording file was not created: {}",
                recording.audio.path.display()
            )));
        }

        Ok(recording.audio)
    }
}

impl Drop for PipeWireRecorder {
    fn drop(&mut self) {
        if let Some(mut recording) = self.active.take() {
            let _ = stop_process(&mut recording.child);
        }
    }
}

fn stop_process(child: &mut Child) -> ChirperResult<()> {
    if child.try_wait().map_err(wait_error)?.is_some() {
        return Ok(());
    }

    send_signal(child, libc::SIGINT)?;

    let started = std::time::Instant::now();
    loop {
        if child.try_wait().map_err(wait_error)?.is_some() {
            return Ok(());
        }

        if started.elapsed() >= STOP_TIMEOUT {
            child.kill().map_err(|source| {
                ChirperError::Audio(format!("failed to kill recording process: {source}"))
            })?;
            child.wait().map_err(wait_error)?;
            return Ok(());
        }

        thread::sleep(STOP_WAIT);
    }
}

fn send_signal(child: &Child, signal: libc::c_int) -> ChirperResult<()> {
    send_signal_to_pid(child.id(), signal)
}

fn stop_process_by_pid(pid: u32) -> ChirperResult<()> {
    if !process_is_running(pid) {
        return Ok(());
    }

    send_signal_to_pid(pid, libc::SIGINT)?;

    let started = std::time::Instant::now();
    loop {
        if !process_is_running(pid) {
            return Ok(());
        }

        if started.elapsed() >= STOP_TIMEOUT {
            send_signal_to_pid(pid, libc::SIGKILL)?;
            return Ok(());
        }

        thread::sleep(STOP_WAIT);
    }
}

fn send_signal_to_pid(pid: u32, signal: libc::c_int) -> ChirperResult<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };

    if result == 0 {
        Ok(())
    } else {
        Err(ChirperError::Audio(format!(
            "failed to signal recording process: {}",
            std::io::Error::last_os_error()
        )))
    }
}

fn process_is_running(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    result == 0
}

fn spawn_pw_record(
    options: &PipeWireRecorderOptions,
    path: &Path,
    detached: bool,
) -> ChirperResult<Child> {
    let mut command = Command::new(&options.command);
    command
        .arg("--media-category=Capture")
        .arg("--media-role=Communication")
        .arg(format!("--channels={}", options.channels))
        .arg(format!("--rate={}", options.sample_rate_hz))
        .arg("--format=s16")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Some(target) = options
        .target
        .as_deref()
        .filter(|target| !target.is_empty())
    {
        command.arg(format!("--target={target}"));
    }

    command.arg(path);

    if detached {
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }

    command.spawn().map_err(|source| {
        ChirperError::Audio(format!(
            "failed to start `{}` for PipeWire recording: {source}",
            options.command
        ))
    })
}

fn wait_error(source: std::io::Error) -> ChirperError {
    ChirperError::Audio(format!(
        "failed while waiting for recording process: {source}"
    ))
}

fn default_output_dir() -> PathBuf {
    if let Some(runtime_dir) = env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("chirper");
    }

    env::temp_dir().join("chirper")
}

fn next_recording_path(output_dir: &Path) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    output_dir.join(format!("chirper-{}-{millis}.wav", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_recording_paths_are_wav_files() {
        let path = next_recording_path(Path::new("/tmp/chirper-test"));

        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("wav")
        );
        assert!(path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap()
            .starts_with("chirper-"));
    }

    #[test]
    fn default_recorder_uses_dictation_friendly_audio_format() {
        let recorder = PipeWireRecorder::default();

        assert_eq!(recorder.options.sample_rate_hz, 16_000);
        assert_eq!(recorder.options.channels, 1);
        assert_eq!(recorder.options.target, None);
    }

    #[test]
    fn recorder_options_use_configured_pipewire_target() {
        let config = ChirperConfig {
            pipewire_target: Some("alsa_input.example".to_string()),
            ..ChirperConfig::default()
        };
        let options = PipeWireRecorderOptions::from_config(&config);

        assert_eq!(options.target, Some("alsa_input.example".to_string()));
    }

    #[test]
    fn detached_recording_carries_audio_metadata() {
        let recording = DetachedRecording {
            pid: 123,
            audio: CapturedAudio {
                path: PathBuf::from("/tmp/example.wav"),
                sample_rate_hz: 16_000,
                channels: 1,
            },
        };

        assert_eq!(recording.pid, 123);
        assert_eq!(recording.audio.sample_rate_hz, 16_000);
        assert_eq!(recording.audio.channels, 1);
    }
}

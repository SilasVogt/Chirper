# Development

## Prerequisites

The initial scaffold is a Rust workspace. Install a Rust toolchain before building:

```sh
rustup default stable
```

Then verify the scaffold:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo run -p chirper-cli -- status
cargo run -p chirper-cli -- diagnose
cargo run -p chirper-cli -- copy-test "hello from chirper"
cargo run -p chirper-cli -- format-test "hello comma world period"
cargo run -p chirper-cli -- format-test --mode code "user at host colon path slash file dot rs"
cargo run -p chirper-cli -- dictate-test 3
```

To smoke-test PipeWire microphone capture:

```sh
cargo run -p chirper-cli -- record-test 3
```

If the recording captures desktop audio or a mixer feed instead of your mic,
inspect available sources with:

```sh
wpctl status
cargo run -p chirper-cli -- audio-list
```

Then choose the source you want Chirper to record:

```sh
cargo run -p chirper-cli -- audio-use auto
cargo run -p chirper-cli -- audio-use alsa_input.example
```

This writes `pipewire_target` in `~/.config/chirper/config.toml`. The daemon
reads the selected input when recording starts.

To smoke-test a whisper.cpp transcription once `whisper-cli` and a model are available:

```sh
cargo run -p chirper-cli -- transcribe-file /path/to/audio.wav /path/to/ggml-base.bin
```

To list, download, or switch Whisper models:

```sh
cargo run -p chirper-cli -- model-list
cargo run -p chirper-cli -- model-download small --select
cargo run -p chirper-cli -- model-use base
```

To inspect and select Ollama formatting models:

```sh
cargo run -p chirper-cli -- ollama-list
cargo run -p chirper-cli -- ollama-use llama3.2
cargo run -p chirper-cli -- formatter-use rules
cargo run -p chirper-cli -- formatter-use ollama llama3.2
```

`ollama-use` selects the model and enables the Ollama formatter by default.
Use `--no-enable` to update `ollama_model` while keeping the current formatter
backend.

The Ollama formatter always runs after the rules preprocessor. Its prompt
includes the raw transcript plus the preprocessed draft; the draft is treated as
authoritative so local edit commands, preferred spellings, and deterministic
domain/email cleanup are not undone by the model.

To manage preferred spellings used by the rules preprocessor and the Ollama
prompt:

```sh
cargo run -p chirper-cli -- vocab-list
cargo run -p chirper-cli -- vocab-add "silas on linux" SilasOnLinux
cargo run -p chirper-cli -- vocab-add prepped Prepd
cargo run -p chirper-cli -- vocab-remove prepped
```

The daemon can also learn a spelling during dictation when the transcript
contains an explicit correction such as `prepped spelled p r e p d` or
`silas on linux spelled s i l a s o n l i n u x`.

To smoke-test the first end-to-end local loop:

```sh
cargo run -p chirper-cli -- dictate-test 3
```

This records for three seconds, transcribes through the configured whisper.cpp backend, and copies the transcript to the clipboard.

To test the toggle flow:

```sh
cargo run -p chirper-cli -- toggle
# speak
cargo run -p chirper-cli -- toggle
```

The first invocation starts recording. The second stops recording, transcribes, and copies the transcript to the clipboard.

To run the daemon and exercise the local API:

```sh
cargo run -p chirper-daemon
```

In a second terminal:

```sh
cargo run -p chirper-cli -- daemon-status
cargo run -p chirper-cli -- daemon-toggle
# speak
cargo run -p chirper-cli -- daemon-toggle
cargo run -p chirper-cli -- daemon-shutdown
```

To record desktop/video audio once without changing the saved microphone input:

```sh
cargo run -p chirper-cli -- daemon-start-screen
# play audio
cargo run -p chirper-cli -- daemon-stop
```

To install and start the daemon as a user systemd service:

```sh
scripts/install-systemd-user-service.sh
```

To install the GNOME Shell extension development copy:

```sh
scripts/install-gnome-extension.sh
gnome-extensions enable chirper@local
```

On Wayland, log out and back in after first install or changing extension code.

To build whisper.cpp locally after installing CMake:

```sh
scripts/setup-whispercpp.sh --backend vulkan --model base
```

## First Implementation Target

The first real behavior should be the local loop:

```text
chirper toggle
  start recording through PipeWire

chirper toggle
  stop recording
  transcribe through whisper.cpp
  insert through clipboard or uinput
```

Keep this loop behind the core backend traits so GNOME, KDE, and window-manager frontends can all reuse the same daemon.

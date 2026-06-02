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

To force the transcription language passed to whisper.cpp:

```sh
cargo run -p chirper-cli -- language-list
cargo run -p chirper-cli -- language-use id
cargo run -p chirper-cli -- language-use auto
```

To switch between the default and faster whisper.cpp decoding profiles:

```sh
cargo run -p chirper-cli -- transcription-list
cargo run -p chirper-cli -- transcription-use fast
cargo run -p chirper-cli -- transcribe-file --fast /path/to/audio.wav
```

To inspect and select Ollama formatting models:

```sh
cargo run -p chirper-cli -- ollama-list
cargo run -p chirper-cli -- ai-format-current
cargo run -p chirper-cli -- ollama-use granite4.1:8b
cargo run -p chirper-cli -- ai-format-logs 7
cargo run -p chirper-cli -- ai-format-preload on
cargo run -p chirper-cli -- formatter-use rules
```

`ollama-use` selects an installed local formatter model and enables the Ollama
formatter. `formatter-use ollama` enables the currently selected Ollama model,
and `ai-format-use off` or `formatter-use rules` disables AI formatting and
returns to local rules.
When enabled, the daemon sends the raw transcript to the selected Ollama model,
uses the model output as the copied/pasted text, writes prompt/input/output logs
under `~/.config/chirper/prompt-logs`, and prunes those logs according to
`format_log_retention_days`.

To inspect and select Codex CLI formatting:

```sh
cargo run -p chirper-cli -- codex-current
cargo run -p chirper-cli -- codex-list
cargo run -p chirper-cli -- codex-use gpt-5.5 --effort low --fast
cargo run -p chirper-cli -- codex-profile-add fast --model gpt-5.5 --effort low --fast
cargo run -p chirper-cli -- formatter-use codex
```

Codex formatting calls `codex exec` non-interactively with a read-only sandbox,
no approvals, an ephemeral session, and `--output-last-message`. It uses the
installed Codex auth/config. `codex-use` writes Chirper's top-level Codex
settings. `codex-profile-add` saves named comparison profiles under
`[codex_profiles.<name>]`. `--fast` stores Codex's `priority` service tier,
which the Codex UI labels as Fast.

The daemon's Ollama formatter uses the current raw-transcript prompt directly.
The compare tools can still test raw-only, rules-preprocessed, and custom prompt
variants without changing daemon configuration.

To compare formatter output across all installed Ollama models without changing
the saved daemon configuration:

```sh
cargo run -p chirper-cli -- format-compare "hello comma world"
cargo run -p chirper-cli -- format-compare --models gemma3:4b,gemma4:latest "hello comma world"
cargo run -p chirper-cli -- format-compare --prompt-input raw "hello comma world"
cargo run -p chirper-cli -- format-compare --prompt-note "Prefer exact casing instructions." "hello comma world"
cargo run -p chirper-cli -- format-compare --custom-prompt "strict=Return only corrected final text." "hello comma world"
cargo run -p chirper-cli -- format-compare --custom-prompt-file strict=./prompt.txt --transcript "case-a=hello comma world" --transcript-file case-b=./transcript.txt
cargo run -p chirper-cli -- format-compare --no-preprocessor "hello comma world"
cargo run -p chirper-cli -- format-compare --no-ollama --codex "hello comma world"
cargo run -p chirper-cli -- format-compare --no-ollama --codex-profile fast "hello comma world"
cargo run -p chirper-cli -- format-compare --report-dir ./reports "hello comma world"
cargo run -p chirper-cli -- format-compare --json "hello comma world"
scripts/run-onboarding.sh
scripts/run-model-compare.sh
scripts/run-report-viewer.sh
scripts/run-test-workflow-builder.sh
```

The command runs models sequentially so timing stays meaningful and large models
do not compete for VRAM. Compared models are unloaded after each run by default;
pass `--keep-loaded` only when intentionally testing warm repeated generations.
By default the Ollama prompt receives both the raw transcript and the rules
preprocessed draft. Use `--prompt-input raw` to test a model without that draft,
or `--no-preprocessor` to send only the raw transcript.
Use `--prompt-note` or `--prompt-file` to add comparison-only prompt
instructions without changing the daemon formatter prompt.
Use `--custom-prompt` or `--custom-prompt-file` to test alternate prompt
templates. If custom prompts are provided, Chirper runs only those prompts unless
`--include-default-prompt` is passed. Prompt templates may contain
`{transcript}`, `{preprocessed}`, `{mode}`, and `{vocabulary}` placeholders; if
no transcript placeholder is present, Chirper appends the transcript block.
Use `--transcript` and `--transcript-file` to add named transcript cases. The
compare matrix is every selected model or Codex profile times every selected
prompt times every transcript case.
`--report-dir` writes durable text reports with raw transcript cases, model
outputs, timings, hardware snapshot, and best-effort average
CPU/RAM/GPU/VRAM/power telemetry from `/proc` and `/sys`. The report writer
creates one file per prompt variant and intentionally omits preprocessed/rules
baseline dumps so prompt results are easier to scan.

`scripts/run-model-compare.sh` opens the GTK/libadwaita compare app. It exposes
the same main options as checkboxes, model tickboxes, Codex profile management,
prompt notes, custom prompt variants, transcript cases, report folder selection,
live current-model progress, elapsed runtime, and hardware summary.

`scripts/run-report-viewer.sh` opens the GTK/libadwaita report viewer. It reads
saved `chirper-format-compare-*.txt` files, parses prompt/model/transcript
sections, and lets you filter outputs by prompt, transcript, and model. It can
show the raw output as regular text or apply a lightweight Markdown preview for
models that emit Markdown.

`scripts/run-test-workflow-builder.sh` opens the GTK/libadwaita test workflow
builder. It is for prompt and pipeline experiments rather than live dictation:
paste a transcript, add ordered Ollama stages, choose a model and prompt for
each stage, then run the stages sequentially. Stage prompts can use
`{input}`, `{transcript}`, `{previous}`, `{outputs}`, and
`{stage:Stage Name}` placeholders. Runs are written to
`~/Documents/Chirper Workflow Runs` by default, including `workflow.json`, the
source transcript, every prompt file, every output file, and `summary.json`.

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
scripts/setup-whispercpp.sh --backend vulkan --model base --write-config
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

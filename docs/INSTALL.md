# Install

Chirper is currently an early Linux/GNOME-focused project. The install flow is
intended for people comfortable installing system dependencies and running local
AI tooling.

The installer does not install distro packages. It checks for required commands,
builds Chirper, installs the user service and GNOME extension, and can build
whisper.cpp plus download an initial model.

## Dependencies

Required for the normal local dictation loop:

- Rust/Cargo
- Git
- PipeWire tools: `pw-record` and `pw-dump`
- Clipboard tool: `wl-copy` from `wl-clipboard` on Wayland, or `xclip` on X11
- CMake and a C/C++ toolchain for whisper.cpp
- systemd user services for the daemon install

Required for the GNOME extension and settings window:

- GNOME Shell 50
- `gnome-extensions`
- `glib-compile-schemas`
- `gjs`
- GTK 4 and libadwaita GJS introspection packages

Required for whisper.cpp Vulkan builds:

- Vulkan loader/headers
- `shaderc` / `glslc`
- `glslangValidator`
- `SPIRV-Headers`

Optional:

- Ollama, for local AI formatting with Granite presets
- Codex CLI, for OpenAI/Codex proofreading after the rules formatter
- ROCm/HIP tooling, only if your distro/GPU stack supports the ROCm backend

Example Arch/CachyOS package set:

```sh
sudo pacman -S rust git pipewire pipewire-audio pipewire-pulse wireplumber \
  wl-clipboard cmake base-devel vulkan-headers shaderc glslang spirv-headers \
  gnome-shell gnome-browser-connector gjs gtk4 libadwaita
```

Package names vary by distro. Install equivalents for the commands listed
above, then run `chirper diagnose` after installation.

## One-Line Install

Default install:

```sh
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | bash
```

What it does:

- clones or updates the repo at `~/.local/share/chirper/source`
- builds `chirper` and `chirper-daemon` in release mode
- symlinks `chirper`, `chirper-daemon`, `chirper-onboarding`,
  `chirper-model-compare`, `chirper-report-viewer`, and
  `chirper-test-workflow-builder` into
  `~/.local/bin`
- builds whisper.cpp with `--backend auto --model base`
- writes a starter config only if `~/.config/chirper/config.toml` does not exist
- installs and starts `chirper-daemon.service` as a user systemd service
- installs the GNOME Shell extension into the user extension directory

Useful variants:

```sh
# Choose the whisper.cpp backend and initial model.
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | \
  bash -s -- --whisper-backend vulkan --whisper-model small

# Install the app/service but skip whisper.cpp setup.
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | \
  bash -s -- --no-whispercpp

# Skip the GNOME extension for non-GNOME systems.
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | \
  bash -s -- --no-gnome-extension
```

If `~/.local/bin` is not on your `PATH`, add:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## Manual Install From A Clone

```sh
git clone https://github.com/SilasVogt/Chirper.git
cd Chirper
cargo build --release -p chirper-cli -p chirper-daemon
scripts/setup-whispercpp.sh --backend vulkan --model base --write-config
CHIRPER_BUILD_PROFILE=release scripts/install-systemd-user-service.sh
CHIRPER_BUILD_PROFILE=release scripts/install-gnome-extension.sh
mkdir -p ~/.local/bin
ln -sf "$PWD/scripts/run-model-compare.sh" ~/.local/bin/chirper-model-compare
ln -sf "$PWD/scripts/run-onboarding.sh" ~/.local/bin/chirper-onboarding
ln -sf "$PWD/scripts/run-report-viewer.sh" ~/.local/bin/chirper-report-viewer
ln -sf "$PWD/scripts/run-test-workflow-builder.sh" ~/.local/bin/chirper-test-workflow-builder
```

Enable the extension:

```sh
gnome-extensions enable chirper@local
```

On Wayland, GNOME Shell may not discover a newly installed local extension until
the next login. If enabling says the extension does not exist, log out and back
in, then retry the enable command.

## First Checks

```sh
chirper diagnose
chirper daemon-status
chirper audio-list
chirper language-list
chirper record-test 3
```

If recording works, test the full daemon path:

```sh
chirper daemon-toggle
# speak
chirper daemon-toggle
```

The final text should be copied to the clipboard. From the GNOME extension,
`Ctrl+Alt+Space` toggles recording and can paste after stop when that setting is
enabled.

## Ollama Formatting

Rules-based formatting works without Ollama. To enable local AI formatting with
the current recommended prompt and hardware preset:

```sh
ollama pull granite4.1:8b
chirper ollama-list
chirper ai-format-use high
```

The settings window exposes the same controls as an `AI Formatting` toggle plus
low/medium/high hardware presets. Low uses `granite4.1:3b`; medium and high use
`granite4.1:8b` for now. The daemon starts loading the selected Ollama model
when recording starts, sends the raw transcript to the model after transcription,
copies/pastes the model output, and asks Ollama to unload after formatting.
Prompt logs are written under `~/.config/chirper/prompt-logs` by default and can
be retained for off, 1 day, 1 week, or 30 days:

```sh
chirper ai-format-current
chirper ai-format-logs 7
chirper ai-format-preload on
```

To compare all installed Ollama models on the same transcript:

```sh
chirper format-compare "hello comma world period"
chirper format-compare --report-dir ./chirper-reports "hello comma world period"
chirper format-compare --custom-prompt "strict=Return only corrected final text." \
  --transcript "case-1=hello comma world period" \
  --report-dir ./chirper-reports
chirper-onboarding
chirper-model-compare
chirper-report-viewer
chirper-test-workflow-builder
```

The compare command runs models sequentially and unloads each model afterward by
default. Reports include model output, timing, a hardware snapshot, and
best-effort average CPU/RAM/GPU/VRAM/power telemetry when the kernel exposes it.
`chirper-model-compare` opens a simple GTK app with tickboxes for installed
Ollama models, saved Codex configs, prompt-input options, report storage, live
current-model progress, elapsed runtime, hardware summary, custom prompt
variants, and named transcript cases. Custom prompt comparisons write one report
file per prompt variant.
`chirper-onboarding` opens a guided GTK setup flow. It checks required commands
and recommended Whisper/Ollama models, records one test passage, compares
`medium`, `large-v3-turbo`, and `large-v3` transcription, compares the selected
transcript through the local Ollama presets and optional Codex, then saves the
selected config.
`chirper-report-viewer` opens the saved reports and lets you filter by prompt,
transcript, and model, with regular text and Markdown preview modes.
`chirper-test-workflow-builder` opens a GTK app for chaining local Ollama stages
such as transcript cleanup, vocabulary lookup planning, and final formatting.
Each stage receives the previous stage output, can use placeholders like
`{input}`, `{transcript}`, and `{outputs}`, and writes prompt/output files under
`~/Documents/Chirper Workflow Runs` by default.

## Updates

For source-based installs, Chirper can check whether the installed checkout is
behind its upstream branch:

```sh
chirper update-check
chirper update-check --json
```

Apply an update with:

```sh
chirper update
```

That reruns the installed checkout's `scripts/install.sh`, pulls the configured
branch, rebuilds Chirper, reinstalls the user systemd service and GNOME
extension, and restarts the daemon. It skips whisper.cpp by default so normal
app updates do not rebuild the inference backend or redownload models. To
explicitly include whisper.cpp setup:

```sh
chirper update --with-whispercpp --whisper-backend vulkan --whisper-model base
```

Updates refuse to pull over local source changes. Commit or stash changes in the
configured source checkout before running `chirper update`.

The GNOME extension periodically runs the same update check when automatic
update checks are enabled. When an update is available, it shows a Shell
notification and enables `Update Chirper` in the panel menu. The settings window
also has `Check` and `Update` buttons.

On Wayland, updating extension files does not always reload the already-running
GNOME Shell extension code. If the update changed extension UI or behavior, log
out and back in after updating.

## Codex CLI Formatting

If the OpenAI Codex CLI is installed and logged in, Chirper can use `codex exec`
as the proofreader instead of Ollama:

```sh
chirper codex-current
chirper codex-list
chirper codex-use gpt-5.5 --effort low --fast
```

For comparison runs, save named Codex configs and then run:

```sh
chirper codex-profile-add fast --model gpt-5.5 --effort low --fast
chirper format-compare --no-ollama --codex-profile fast --report-dir ./chirper-reports "hello comma world period"
```

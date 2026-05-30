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

- Ollama, for local LLM proofreading after the rules formatter
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
- symlinks both binaries into `~/.local/bin`
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

Rules-based formatting works without Ollama. To enable local LLM proofreading:

```sh
ollama pull gemma4
chirper ollama-list
chirper formatter-use ollama gemma4:latest
```

To compare all installed Ollama models on the same transcript:

```sh
chirper format-compare "hello comma world period"
```

The compare command runs models sequentially and unloads each model afterward by
default.

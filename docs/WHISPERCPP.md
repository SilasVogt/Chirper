# whisper.cpp Setup

Chirper talks to whisper.cpp through its `whisper-cli` binary. This keeps the first ASR backend simple and lets each machine choose the backend that fits its GPU stack.

## Current Machine

On the current machine, `chirper diagnose` reports the configured runtime paths separately from PATH tools:

```text
whisper-cli: missing from PATH
whispercpp_command: /home/silas/.local/share/chirper/src/whisper.cpp/build-vulkan/bin/whisper-cli (true)
whispercpp_model_path: /home/silas/.local/share/chirper/models/ggml-base.bin (true)
vulkan_radeon_detected: true
rocm_tool_detected: false
suggested_gpu_backend: Vulkan
```

That means this machine should use a Vulkan-oriented whisper.cpp build. ROCm is optional for other machines, but on this setup we are treating Vulkan as the first-class AMD backend because ROCm userspace is not available.

On Arch, the Vulkan build dependencies are typically:

```sh
sudo pacman -S cmake vulkan-headers shaderc glslang spirv-headers
```

## Build Helper

After installing CMake, run:

```sh
scripts/setup-whispercpp.sh --backend vulkan --model base --write-config
```

To download and switch to a larger model later:

```sh
cargo run -p chirper-cli -- model-download small --select
cargo run -p chirper-cli -- model-use small
```

The daemon reloads config when a recording is stopped, so model changes apply to
the next transcription without restarting the service.

For ROCm/HIP later:

```sh
scripts/setup-whispercpp.sh --backend rocm --model base --write-config
```

The script builds whisper.cpp under:

```text
~/.local/share/chirper/src/whisper.cpp
```

and downloads models under:

```text
~/.local/share/chirper/models
```

It prints the config snippet to add to `~/.config/chirper/config.toml`.
With `--write-config`, it writes a starter config only when the config file does
not already exist.

## Model Selection

List installed models:

```sh
cargo run -p chirper-cli -- model-list
```

Select an installed model by name:

```sh
cargo run -p chirper-cli -- model-use small
```

Select an arbitrary ggml model path:

```sh
cargo run -p chirper-cli -- model-use /path/to/ggml-large-v3-turbo.bin
```

The GNOME extension panel menu and settings window use the same CLI commands
for graphical model switching and downloads.

## Language Selection

Auto language detection can be unreliable for multilingual recordings. Force a
language with:

```sh
cargo run -p chirper-cli -- language-use id
```

Use `auto` to return to whisper.cpp detection:

```sh
cargo run -p chirper-cli -- language-use auto
```

The GNOME settings window exposes the same language list.

## Notes

If CMake fails with:

```text
Could not find a package configuration file provided by "SPIRV-Headers"
```

install `spirv-headers` and rerun the setup script.

The upstream whisper.cpp CLI supports `-nt`/`--no-timestamps`, `-np`/`--no-prints`, `-m`/`--model`, `-f`/`--file`, and `-ng`/`--no-gpu`. The wrapper uses those flags for clean dictation output.

Upstream build flags used by the helper:

```text
Vulkan: -DGGML_VULKAN=1
ROCm:   -DGGML_HIP=1 -DAMDGPU_TARGETS=<gfx target>
```

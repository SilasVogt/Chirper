# Troubleshooting

## `chirper-daemon` Is Not Running

Check the user service:

```sh
systemctl --user status chirper-daemon.service
journalctl --user -u chirper-daemon.service -n 100 --no-pager
```

Restart it:

```sh
systemctl --user restart chirper-daemon.service
chirper daemon-status
```

If you built from source without using the installer, reinstall the service with
the profile you built:

```sh
CHIRPER_BUILD_PROFILE=release scripts/install-systemd-user-service.sh
```

## GNOME Extension Does Not Exist

If this fails:

```sh
gnome-extensions enable chirper@local
```

with:

```text
Extension "chirper@local" does not exist
```

the files may be installed but the running Shell has not discovered them. On
Wayland, log out and back in, then run the enable command again.

## GNOME Extension Shows Daemon Unavailable

Make sure the service is installed and running:

```sh
systemctl --user status chirper-daemon.service
chirper daemon-status
```

The extension can restart the service only after
`chirper-daemon.service` exists.

## Wrong Microphone Or Desktop Audio Captured

List inputs:

```sh
chirper audio-list
```

Select the default microphone:

```sh
chirper audio-use auto
```

Or select a specific PipeWire node by id, serial, name, or description:

```sh
chirper audio-use alsa_input.example
```

For one recording of screen/video audio, use the GNOME extension `Input` menu or:

```sh
chirper daemon-start-screen
# play audio
chirper daemon-stop
```

## Clipboard Copy Fails

Install `wl-clipboard` on Wayland or `xclip` on X11, then test:

```sh
chirper copy-test "hello from chirper"
```

## whisper.cpp Vulkan Build Fails On SPIR-V Headers

If CMake reports:

```text
Could not find a package configuration file provided by "SPIRV-Headers"
```

install your distro's SPIR-V headers package and rerun:

```sh
scripts/setup-whispercpp.sh --backend vulkan --model base --write-config
```

On Arch-like systems the missing package is usually `spirv-headers`.

## Ollama Keeps GPU Memory After Tests

Ollama keeps models loaded for a while after generation. Inspect loaded models:

```sh
ollama ps
```

Unload one:

```sh
ollama stop gemma4:latest
```

`chirper format-compare` unloads compared models after each run unless
`--keep-loaded` is passed.

## Output Quality Is Poor

If Whisper is detecting the wrong language, force the language first:

```sh
chirper language-list
chirper language-use id
```

First compare rules-only output against the selected Ollama model:

```sh
chirper formatter-use rules
chirper format-test "your transcript"
chirper format-compare "your transcript"
```

Add preferred spellings for recurring names:

```sh
chirper vocab-add "silas on linux" SilasOnLinux
chirper vocab-add prepped Prepd
chirper vocab-list
```

Use a larger Whisper model when ASR mistakes are the root problem:

```sh
chirper model-download medium --select
```

Use `chirper format-compare` to test local Ollama models before making one the
default formatter. For side-by-side prompt testing:

```sh
chirper format-compare --prompt-input raw --report-dir ./reports "your transcript"
chirper format-compare --prompt-input both --report-dir ./reports "your transcript"
```

To compare Codex CLI settings instead of local Ollama models, first check that
Codex is available:

```sh
chirper codex-current
chirper codex-list
chirper format-compare --no-ollama --codex --report-dir ./reports "your transcript"
```

If Codex returns an auth or connectivity error, run `codex doctor` and fix the
Codex CLI login before enabling `formatter_backend = "codex"`.

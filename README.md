# Chirper

Local-first voice dictation for Linux, designed around a small daemon and modular desktop frontends.

The first target is a GNOME-friendly workflow on AMD GPUs, with the architecture kept open for KDE, wlroots window managers, and alternative inference/input backends.

## Current Status

This repo is early but usable for local testing on Linux. The current path is:

- PipeWire audio capture
- whisper.cpp transcription with CPU, Vulkan, or ROCm builds
- selectable transcription language for whisper.cpp
- selectable balanced or fast whisper.cpp decoding profiles
- local rules formatting with preferred spellings
- optional Ollama AI formatting with model selection, model preload, and
  prompt/input/output logs
- optional Codex CLI proofreading
- clipboard insertion
- daemon API for frontends
- GNOME Shell 50 extension with recording controls, overlay, audio input
  selection, and settings
- GTK/libadwaita onboarding app for checking dependencies, testing Whisper and
  formatter models, saving a recommended config, and cleaning up unused setup
  models
- GTK/libadwaita model compare app for testing Ollama and Codex formatter
  configurations across prompt variants and transcript cases
- GTK/libadwaita report viewer for comparing saved benchmark outputs
- GTK/libadwaita test workflow builder for chaining local model stages and
  inspecting each prompt/output file

## Install

The stable installer checks dependencies but does not install distro packages.
`--gui gnome` makes the desktop scope explicit: it installs the GNOME Shell
extension and GTK/libadwaita helper apps.

```sh
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/v0.1.0/scripts/install.sh | \
  bash -s -- --ref v0.1.0 --gui gnome
```

See [Install](docs/INSTALL.md) for dependency lists, install options, and first
checks.

## First Use

After installing:

```sh
chirper diagnose
chirper daemon-status
chirper-onboarding
chirper settings
```

`chirper-onboarding` is the first-run setup flow. It lets you pick the Whisper
transcription model and formatter model before the GNOME panel item starts
recording.

Check for updates. By default, release installs compare against numeric release
tags:

```sh
chirper update-check
chirper update
```

Canary mode is available for testers who want to follow `main`:

```sh
chirper update-check --mode canary
chirper update --mode canary
```

Uninstall user-local install artifacts while keeping config and models:

```sh
chirper uninstall
```

Test the full daemon path:

```sh
chirper daemon-toggle
# speak
chirper daemon-toggle
```

The final text should be copied to your clipboard. On GNOME, enable or reload
the extension after onboarding:

```sh
gnome-extensions enable chirper@local
```

On Wayland, you may need to log out and back in before GNOME discovers a newly
installed local extension.

## Documentation

The durable docs are:

- [Install](docs/INSTALL.md)
- [Releases](docs/RELEASES.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Local API](docs/API.md)
- [Example Config](docs/config.example.toml)
- [Development](docs/DEVELOPMENT.md)
- [GNOME Extension](docs/GNOME_EXTENSION.md)
- [Frontend Integration](docs/ARCHITECTURE.md#frontend-integration)
- [Roadmap](docs/ROADMAP.md)
- [Troubleshooting](docs/TROUBLESHOOTING.md)
- [whisper.cpp Setup](docs/WHISPERCPP.md)

Contributions should go through pull requests. See
[Contributing](CONTRIBUTING.md) for the branch, review, and CI workflow.

## Initial Shape

```text
crates/chirper-core     shared state, config, errors, backend contracts
crates/chirper-api      shared local API request/response types
crates/chirper-daemon   background service and local API server
crates/chirper-cli      command-line control/debug client
crates/chirper-formatter-*  formatter backends
apps/onboarding        GTK/libadwaita guided setup utility
apps/model-compare      GTK/libadwaita model comparison utility
apps/report-viewer      GTK/libadwaita report comparison utility
apps/workflow-builder   GTK/libadwaita test workflow builder
extensions/gnome        GNOME Shell extension frontend
```

## License

Chirper is licensed under the [MIT License](LICENSE).

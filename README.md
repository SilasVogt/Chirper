# Chirper

Local-first voice dictation for Linux, designed around a small daemon and modular desktop frontends.

The first target is a GNOME-friendly workflow on AMD GPUs, with the architecture kept open for KDE, wlroots window managers, and alternative inference/input backends.

## Current Status

This repo is an early scaffold. The durable planning docs are:

- [Architecture](docs/ARCHITECTURE.md)
- [Local API](docs/API.md)
- [Example Config](docs/config.example.toml)
- [Development](docs/DEVELOPMENT.md)
- [GNOME Extension](docs/GNOME_EXTENSION.md)
- [Roadmap](docs/ROADMAP.md)
- [whisper.cpp Setup](docs/WHISPERCPP.md)

## Initial Shape

```text
crates/chirper-core     shared state, config, errors, backend contracts
crates/chirper-api      shared local API request/response types
crates/chirper-daemon   background service and local API server
crates/chirper-cli      command-line control/debug client
extensions/gnome        GNOME Shell extension frontend
```

## License

Chirper is licensed under the [MIT License](LICENSE).

# Roadmap

This file is the project checklist. Keep it updated as implementation choices become real.

## Milestone 0: Scaffold

- [x] Create repo layout.
- [x] Record architecture goals.
- [x] Add core workflow types.
- [x] Add daemon and CLI placeholders.
- [x] Add first local daemon API crate and Unix socket protocol.

## Milestone 1: Local Dictation Loop

- [x] Add config loading.
- [x] Add PipeWire recording backend.
- [x] Add whisper.cpp backend.
- [x] Add backend selection for CPU, Vulkan, and ROCm.
- [x] Add clipboard insertion backend.
- [ ] Add uinput insertion backend.
- [x] Add reusable dictation workflow orchestration.
- [x] Implement `chirper toggle`.
- [x] Add user-selectable Whisper model commands.
- [x] Verify record -> transcribe -> insert on GNOME.

## Milestone 2: GNOME Integration

- [x] Add user systemd service.
- [x] Add daemon-owned recording state.
- [ ] Add D-Bus API.
- [x] Add GNOME Shell extension.
- [x] Add recording overlay.
- [x] Add extension menu with status and settings action.
- [x] Add extension recording hotkey.
- [x] Add extension Whisper model menu.
- [ ] Add extension mode switch.
- [ ] Add GTK/libadwaita settings app.
- [ ] Add diagnostics for audio, GPU backend, and insertion backend.

## Milestone 3: Text Quality

- [x] Add formatter contract implementation.
- [x] Add deterministic spoken punctuation/symbol formatter.
- [x] Add Ollama formatter backend.
- [x] Add Ollama model menu once formatter backend exists.
- [x] Add modes: auto, standard, email, command, code.
- [x] Add personal dictionary.
- [x] Pass raw transcript plus preprocessed draft into Ollama formatting.
- [x] Add deterministic domain/email cleanup before LLM formatting.
- [x] Add CLI formatter comparison across installed Ollama models.
- [ ] Add snippets.
- [ ] Add app/context profiles.

## Milestone 4: Better Input Integration

- [ ] Investigate IBus engine/backend.
- [ ] Add X11 insertion backend.
- [ ] Add wlroots-specific insertion notes or backend.
- [ ] Add automated insertion backend diagnostics.

## Milestone 5: Other Frontends

- [x] Document frontend API.
- [ ] Add KDE/Qt frontend notes.
- [ ] Add status/widget examples for window managers.
- [ ] Add packaging guide.

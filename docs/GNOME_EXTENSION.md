# GNOME Extension

The GNOME Shell extension is intentionally thin. It does not record audio,
transcribe, format, or insert text directly. It only calls the daemon API
documented in [Local API](API.md).

## Target

- GNOME Shell 50
- GJS 1.88 or newer
- `chirper-daemon` running in the user session

The extension uses the GNOME 45+ ES module extension style, which remains the
current pattern for GNOME Shell 50.

## Install

From the repository root:

```sh
scripts/install-gnome-extension.sh
gnome-extensions enable chirper@local
```

If `gnome-extensions` does not see `chirper@local` immediately, log out and
back in before enabling it. On Wayland this is also the least surprising reload
path after changing extension code.

The symptom looks like this:

```text
Extension "chirper@local" does not exist
```

That means the files are installed, but the running Shell has not rescanned the
user extension directory yet.

## Run

Start the daemon:

```sh
cargo run -p chirper-daemon
```

Or install it as a user service:

```sh
scripts/install-systemd-user-service.sh
```

Use the panel menu or `Ctrl+Alt+Space` to toggle recording, or keep using the CLI:

```sh
cargo run -p chirper-cli -- daemon-toggle
```

The extension polls `status` once per second. This is a temporary bridge until
the daemon has an event stream or D-Bus signals.

The extension can ask systemd to start `chirper-daemon.service` when a recording
command is clicked, but the service must be installed first.

## UI

The extension menu has one primary action. It reads `Start Recording` while idle
and changes to `Stop Recording` while recording. The configured shortcut is shown
on the right side of that action.

The panel icon uses separate idle, recording, processing, and disconnected
states. The overlay is shown while recording or processing and uses a subtle
pulse animation.

The panel menu keeps quick controls at the top level: paste behavior, Whisper
model selection/downloads, an Ollama placeholder, status refresh, daemon restart,
and config folder actions. `Open Settings Window` launches a small Adwaita
preferences window from the installed extension directory.

## Paste Behavior

The daemon copies the transcript to the clipboard. When `Paste After Stop` is
enabled in the panel menu or preferences window, the extension remembers the
previously focused window, restores it after transcription, then sends `Ctrl+V`
through a GNOME Shell virtual keyboard device.

This is why paste should be triggered from the extension or its hotkey rather
than by manually moving focus before stopping recording.

## Shortcut

The default extension shortcut is:

```text
Ctrl+Alt+Space
```

It starts recording from idle and stops/pastes while recording.

## Model Settings

The `Whisper Model` submenu and settings window list installed local models
and common downloadable models. Selecting a model updates
`~/.config/chirper/config.toml`; the daemon uses the updated model on the next
transcription.

The `Ollama Model` submenu and preferences section are present as placeholders
for the local LLM formatter work. The config already has `ollama_command` and
`ollama_model` fields, but the formatter backend itself is still on the roadmap.

## Design Boundary

The extension may:

- show daemon state
- call `toggle`, `start_recording`, and `stop_recording`
- show a shell-native recording overlay
- restore the previous window and send paste after the daemon copies text
- open the config folder

The extension should not:

- spawn `pw-record`
- run `whisper-cli`
- manipulate the clipboard directly
- duplicate daemon workflow state

# Local API

Chirper frontends should talk to `chirper-daemon` instead of owning recording,
transcription, formatting, or insertion. The first API is a newline-delimited
JSON protocol over a Unix stream socket.

## Transport

Default socket path:

```text
$XDG_RUNTIME_DIR/chirper/daemon.sock
```

If `XDG_RUNTIME_DIR` is unset, clients fall back to:

```text
/tmp/chirper/daemon.sock
```

Each request opens one Unix socket connection, writes one JSON object plus a
newline, closes the write side, then reads one JSON response. This is simple
enough for CLI clients, shell extensions, and lightweight widgets while the
daemon state model is still settling.

## Requests

Requests use a tagged JSON object:

```json
{"command":"status"}
```

Supported commands:

| Command | Meaning |
| --- | --- |
| `status` | Return daemon state and active recording path if present. |
| `toggle` | Start recording from idle, or stop/process/insert while recording. |
| `start_recording` | Start recording only. |
| `stop_recording` | Stop recording, transcribe, format, and insert. |
| `shutdown` | Ask the daemon to exit. Intended for development. |

`toggle` and `start_recording` may include a one-shot audio target override:

```json
{
  "command": "start_recording",
  "audio": {
    "kind": "screen_audio",
    "target": "alsa_output.example",
    "label": "Screen audio: Speakers"
  }
}
```

`kind` is `input` or `screen_audio`. If omitted, the daemon uses
`pipewire_target` from config, or the PipeWire default input when that config key
is unset.

## Response

Every command returns this shape:

```json
{
  "ok": true,
  "state": "idle",
  "message": "transcript copied to clipboard",
  "audio_target": "alsa_input.example",
  "audio_label": "Example Microphone",
  "recording_path": "/run/user/1000/chirper/chirper-123.wav",
  "transcript": "comma open quote hello close quote",
  "formatted": ", \"hello\"",
  "copied": true
}
```

Fields:

| Field | Meaning |
| --- | --- |
| `ok` | Whether the command completed successfully. |
| `state` | Final daemon state after the command response. |
| `message` | Human-readable status or error text. |
| `audio_target` | PipeWire node name used for the active or completed recording, when known. |
| `audio_label` | Human-readable input/output label for UI overlays. |
| `recording_path` | WAV path when a recording was started or processed. |
| `transcript` | Raw ASR transcript when available. |
| `formatted` | Final text after formatting when available. |
| `copied` | Whether final text was copied to the clipboard. |

State names are lowercase strings: `idle`, `recording`, `transcribing`,
`formatting`, `inserting`, and `error`.

## Current CLI Commands

These commands exercise the daemon API:

```sh
cargo run -p chirper-cli -- daemon-status
cargo run -p chirper-cli -- daemon-toggle
cargo run -p chirper-cli -- daemon-start
cargo run -p chirper-cli -- daemon-start-screen
cargo run -p chirper-cli -- daemon-stop
cargo run -p chirper-cli -- daemon-shutdown
```

The existing standalone `chirper toggle` command is still separate. It is useful
from a terminal because it does not require a daemon process.

## Frontend Contract

A frontend may:

- call `status` to paint its current UI
- call `toggle`, `start_recording`, or `stop_recording` from hotkeys or buttons
- show `state` and `message` directly in development UI
- show `recording_path`, `transcript`, and `formatted` only in diagnostics UI

A frontend should not:

- spawn its own recorder
- run ASR directly
- maintain independent workflow state
- assume clipboard insertion will remain the only insertion backend

## Planned D-Bus Adapter

GNOME Shell integration should eventually use a D-Bus service such as
`org.chirper.Chirper1`. The D-Bus methods should map one-to-one to the request
commands above, and the daemon should also emit state-change signals for
recording overlays and status widgets.

The Unix socket API remains useful for CLI tools, window-manager integrations,
tests, and development diagnostics even after D-Bus is added.

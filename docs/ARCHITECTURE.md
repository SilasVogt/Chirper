# Architecture

Chirper should feel native on GNOME first, but the core product is the daemon. Frontends call the daemon; they do not own audio capture, transcription, formatting, or text insertion.

## Goals

- Run primarily locally.
- Prefer AMD-friendly inference paths first.
- Keep desktop integration replaceable.
- Make new frontends easy to add without touching the dictation pipeline.
- Make backend support incremental and testable on one machine at a time.

## Process Model

```text
chirper-daemon
  Owns workflow state.
  Captures audio.
  Runs ASR and optional formatting.
  Inserts text through the selected backend.
  Exposes the local frontend API.

chirper-cli
  Debug and scripting client.
  Can use either standalone debug paths or the daemon API.

GNOME Shell extension
  Optional shell integration.
  Provides recording overlay, menu, shortcut, and settings launcher.
  Talks to the daemon over the Unix socket API today; D-Bus can wrap the same
  commands later.

GTK/libadwaita settings app
  Optional settings frontend.
  Edits config, models, dictionaries, snippets, and diagnostics.
  Talks to the CLI/config helpers today; D-Bus or config service methods can
  replace that once the control surface settles.

GTK/libadwaita test tools
  Optional local experimentation frontends.
  Compare formatter models, review benchmark reports, and chain staged model
  prompts without changing the live daemon workflow.
```

The first implemented daemon API is newline-delimited JSON over a Unix socket at
`$XDG_RUNTIME_DIR/chirper/daemon.sock`. See [Local API](API.md). D-Bus remains
the intended GNOME-facing adapter, but it should map to the same daemon commands
instead of creating a parallel control surface.

## Workflow State

```text
Idle
  -> Recording
  -> Transcribing
  -> Formatting
  -> Inserting
  -> Idle
```

Errors should return to `Idle` after being surfaced to the active frontend.

## Backend Contracts

Backends should be swappable behind narrow interfaces:

- `AudioSource`: start and stop capture, returning audio data or a recording path.
- `AsrEngine`: turn captured audio into a transcript.
- `Formatter`: optionally clean, rewrite, or style the transcript. The local
  rules preprocessor stays first in the pipeline. It handles spoken punctuation,
  edit commands, configured vocabulary, learned spelling corrections, common
  domain/email phrases, and conservative context-specific symbol cleanup.
  The daemon's Ollama AI formatting path currently sends the raw transcript to
  the selected model and treats the model output as the final pasted text.
  Compare/test tools can still run raw-only, rules-preprocessed, and custom
  prompt variants for experimentation.
- `InsertionBackend`: insert final text into the focused application.
- `HotkeyBackend`: optional frontend-side trigger source.

The first implementation can be direct and in-process. A later external plugin API can wrap these same concepts through subprocesses or D-Bus once the contracts settle.

## Frontend Integration

Chirper is designed so AI agents and contributors can add another GUI without
copying the dictation pipeline. A new GUI should treat `chirper-daemon` and the
documented [Local API](API.md) as the product boundary:

- call daemon commands for recording state
- use CLI/config commands for model, audio, language, and formatter settings
- keep toolkit-specific code under a frontend directory such as `apps/` or
  `extensions/`
- do not spawn `pw-record`, run `whisper-cli`, call Ollama/Codex, or write to
  the clipboard directly from the GUI

That lets GNOME, KDE, wlroots widgets, and experiment apps share the same audio,
ASR, formatting, insertion, and update behavior. If a GUI needs a daemon feature
that is not in the API yet, add the daemon/API command first, then build the GUI
on top of that command.

## Initial Backend Choices

| Area | First backend | Later backends |
| --- | --- | --- |
| Audio | PipeWire | file input for tests |
| ASR | whisper.cpp | faster-whisper, remote API adapters |
| GPU | ROCm/Vulkan/CPU selection | CUDA, OpenVINO |
| Formatting | none, rules, vocabulary, Ollama, Codex CLI | llama.cpp |
| Insertion | clipboard, uinput | IBus, X11, wlroots-specific |
| GNOME UI | CLI first, extension second | GTK settings app |

## GNOME Strategy

The GNOME Shell extension is intentionally thin. It should not run transcription or maintain independent workflow state. It should subscribe to daemon events and render shell-native UI:

- hidden mode or top-bar indicator mode
- recording overlay
- processing state
- quick mode switch
- settings launcher

The GTK/libadwaita app exists for settings and diagnostics, not as a permanently running app.

## Contributor Boundaries

Future contributors should be able to add a backend by implementing one contract and registering it in the daemon. A new frontend should only need the public daemon API and, once added, the event stream.

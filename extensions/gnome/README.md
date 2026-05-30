# GNOME Shell Extension

This extension targets GNOME Shell 50 and talks directly to the local
`chirper-daemon` Unix socket.

It provides:

- a top-panel indicator
- one primary Start/Stop Recording action
- default `Ctrl+Alt+Space` toggle hotkey
- optional paste-after-stop behavior
- Whisper model selection and common model downloads
- an animated recording/processing overlay
- a settings submenu with daemon and config actions

Install the development copy with:

```sh
scripts/install-gnome-extension.sh
```

Then reload GNOME Shell if needed and enable the extension:

```sh
gnome-extensions enable chirper@local
```

On Wayland, logging out and back in is the reliable way to make a newly
installed extension visible and to reload Shell extension code.

If `gnome-extensions enable chirper@local` says the extension does not exist,
log out and back in once, then run the enable command again.

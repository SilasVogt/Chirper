# Releases

Chirper is currently installed from source. The release system should keep that
path working for contributors while adding packaged builds for normal users.

## Current Install Path

The installer clones or updates the repository, builds release binaries locally,
installs the user systemd service, installs the GNOME Shell extension, and can
build whisper.cpp plus download an initial model:

```sh
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | bash
```

For testing a development branch, use the branch copy of the script and pass the
same branch to the checkout:

```sh
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/codex/release-install-workflow/scripts/install.sh | \
  bash -s -- --branch codex/release-install-workflow
```

The source installer is useful while Chirper is moving quickly, but it requires
Rust and a local toolchain. Stable releases should not require Rust for ordinary
users.

## Channels

Chirper should use three update channels:

- `stable`: tagged GitHub releases such as `v0.1.0`.
- `nightly`: the latest successful build from `main`.
- `source`: a named branch, useful for contributors and testers.

The default public install should eventually use `stable`. Early testers can keep
using `main` until the first tagged release exists.

## Stable Releases

A stable release should be cut from `main` after CI passes and any release notes
are written.

Recommended flow:

1. Merge feature PRs into `main`.
2. Update the version and release notes.
3. Create a tag such as `v0.1.0`.
4. GitHub Actions builds release artifacts.
5. GitHub Actions publishes a GitHub Release with checksums.
6. The stable installer downloads the latest stable artifact.

The release artifact should include:

- `chirper`
- `chirper-daemon`
- GNOME extension files from `extensions/gnome/chirper@local`
- GJS app launchers from `scripts/run-*.sh`
- install scripts for the user service and extension
- docs and example config
- a checksum file

The release artifact should not include whisper.cpp builds, Whisper models,
Ollama models, or distro packages. Those stay machine-specific.

## Nightly Releases

Nightly releases should be generated from successful `main` builds. They are for
testers who want the newest merged work without building from source.

Recommended flow:

1. CI passes on `main`.
2. A nightly workflow builds the same artifact shape as stable.
3. The workflow publishes or replaces a prerelease named `nightly`.
4. The nightly installer downloads that prerelease.

A moving `nightly` prerelease is simpler than keeping one release per day. If
dated nightlies are added later, they should have retention cleanup so the repo
does not accumulate old test builds forever.

## Updating

The current update path is source-checkout based and supports two modes:

- `canary`: checks whether the installed checkout is behind `origin/main`.
- `releases`: checks numeric tags such as `v0.1.0` and targets the highest
  versioned tag.

The default remains `canary` while early testers install from `main`:

```sh
chirper update-check
chirper update
```

Release mode is active once tags exist:

```sh
chirper update-check --mode releases
chirper update --mode releases
```

`chirper update` reruns the checkout's `scripts/install.sh`, checks out the
target branch or release tag, rebuilds release binaries, reinstalls the user
service, reinstalls the GNOME extension, and restarts the daemon. It skips
whisper.cpp by default. It refuses to update over local source changes; commit
or stash changes in the configured source checkout before updating.

Updates should remain user-local by default:

- binaries in `~/.local/bin` or a Chirper data directory
- service file in `~/.config/systemd/user`
- extension files in `~/.local/share/gnome-shell/extensions/chirper@local`
- config in `~/.config/chirper`
- models in `~/.local/share/chirper/models`

The updater should restart `chirper-daemon.service` after replacing binaries.
GNOME Shell extension updates are different: on Wayland, GNOME may keep old
extension code loaded until the user logs out and back in. The updater can copy
new files immediately, but it should tell the user when a relog is required.

The CLI accepts channel aliases for the source updater:

```sh
chirper update --channel stable
chirper update --channel canary
```

`stable` maps to release tags. `canary` maps to `main`. Artifact downloads for
stable and nightly builds are still future work; current updates rebuild from a
local source checkout.

## Auto Updating

Silent auto-update should not be the default. Chirper controls microphone input,
clipboard output, and local model execution, so users should know when code
changes.

Recommended first version:

- `chirper update-check` reports whether the configured update mode is behind.
- when automatic checks are enabled, the GNOME extension periodically runs
  `chirper update-check --json --mode <mode>`.
- the GNOME extension shows a notification when an update is available.
- the GNOME panel menu and settings window expose an explicit update button.

Optional later version:

- a user systemd timer checks daily.
- config chooses `notify`, `download`, or `install` behavior.
- the default remains `notify`.

Nightly auto-updates should be opt-in, because nightly builds may change
extension UI, settings shape, and formatter behavior more often than stable
releases.

## New-Machine Smoke Test

Before the first stable release, test a clean machine or VM with:

```sh
curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | \
  bash -s -- --whisper-backend vulkan --whisper-model base
chirper diagnose
chirper daemon-status
chirper record-test 3
chirper daemon-toggle
```

CI should also grow an installer smoke test that runs the installer with
`--no-whispercpp` in a clean Linux runner. That catches shell syntax, path, and
service/extension packaging errors without requiring GPU access in CI.

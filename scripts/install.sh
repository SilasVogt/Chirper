#!/usr/bin/env bash
set -euo pipefail

repo_url="${CHIRPER_REPO_URL:-https://github.com/SilasVogt/Chirper.git}"
branch="${CHIRPER_BRANCH:-main}"
source_dir="${CHIRPER_SOURCE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/chirper/source}"
bin_dir="${CHIRPER_BIN_DIR:-$HOME/.local/bin}"
build_profile="${CHIRPER_BUILD_PROFILE:-release}"
with_whispercpp=true
with_service=true
with_gnome_extension=true
whisper_backend="${CHIRPER_WHISPER_BACKEND:-auto}"
whisper_model="${CHIRPER_WHISPER_MODEL:-base}"

usage() {
  cat <<'USAGE'
Usage: scripts/install.sh [options]

Clones or updates Chirper, builds release binaries, installs the user service,
optionally installs the GNOME extension, and optionally prepares whisper.cpp.
The script checks dependencies but does not install distro packages.

One-line install:
  curl -fsSL https://raw.githubusercontent.com/SilasVogt/Chirper/main/scripts/install.sh | bash

Options:
  --repo URL                  Git repository URL.
  --branch NAME              Git branch to install. Default: main.
  --source-dir PATH          Source checkout path. Default: ~/.local/share/chirper/source.
  --bin-dir PATH             Symlink destination for chirper binaries. Default: ~/.local/bin.
  --profile debug|release    Cargo build profile. Default: release.
  --whisper-backend NAME     auto, cpu, vulkan, or rocm. Default: auto.
  --whisper-model NAME       whisper.cpp model to download. Default: base.
  --no-whispercpp            Skip whisper.cpp build/model download.
  --no-service               Skip user systemd service install.
  --no-gnome-extension       Skip GNOME Shell extension install.
  -h, --help                 Show this help.

Environment overrides:
  CHIRPER_REPO_URL, CHIRPER_BRANCH, CHIRPER_SOURCE_DIR, CHIRPER_BIN_DIR,
  CHIRPER_BUILD_PROFILE, CHIRPER_WHISPER_BACKEND, CHIRPER_WHISPER_MODEL.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --repo)
      repo_url="${2:?missing value for --repo}"
      shift 2
      ;;
    --branch)
      branch="${2:?missing value for --branch}"
      shift 2
      ;;
    --source-dir)
      source_dir="${2:?missing value for --source-dir}"
      shift 2
      ;;
    --bin-dir)
      bin_dir="${2:?missing value for --bin-dir}"
      shift 2
      ;;
    --profile)
      build_profile="${2:?missing value for --profile}"
      shift 2
      ;;
    --whisper-backend)
      whisper_backend="${2:?missing value for --whisper-backend}"
      shift 2
      ;;
    --whisper-model)
      whisper_model="${2:?missing value for --whisper-model}"
      shift 2
      ;;
    --no-whispercpp)
      with_whispercpp=false
      shift
      ;;
    --no-service)
      with_service=false
      shift
      ;;
    --no-gnome-extension)
      with_gnome_extension=false
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$build_profile" in
  debug|release) ;;
  *)
    echo "unsupported build profile: $build_profile" >&2
    exit 2
    ;;
esac

case "$whisper_backend" in
  auto|cpu|vulkan|rocm) ;;
  *)
    echo "unsupported whisper backend: $whisper_backend" >&2
    exit 2
    ;;
esac

missing=()

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    missing+=("$1")
  fi
}

need_command git
need_command cargo
need_command pw-record
need_command pw-dump

if ! command -v wl-copy >/dev/null 2>&1 && ! command -v xclip >/dev/null 2>&1; then
  missing+=("wl-copy or xclip")
fi

if [ "$with_service" = true ]; then
  need_command systemctl
fi

if [ "$with_gnome_extension" = true ]; then
  need_command gnome-extensions
  need_command glib-compile-schemas
  need_command gjs
fi

if [ "$with_whispercpp" = true ]; then
  need_command cmake
fi

if [ "${#missing[@]}" -gt 0 ]; then
  echo "Missing required commands:" >&2
  for command in "${missing[@]}"; do
    echo "  - $command" >&2
  done
  echo >&2
  echo "Install the missing distro packages, then rerun this script." >&2
  echo "See docs/INSTALL.md for package examples." >&2
  exit 1
fi

if [ -f "Cargo.toml" ] && [ -d "crates/chirper-cli" ] && [ -d "scripts" ]; then
  repo_root="$(pwd)"
  echo "Using existing checkout: $repo_root"
else
  if [ -d "$source_dir/.git" ]; then
    echo "Updating existing checkout: $source_dir"
    git -C "$source_dir" fetch --prune origin
    git -C "$source_dir" checkout "$branch"
    git -C "$source_dir" pull --ff-only origin "$branch"
  else
    if [ -e "$source_dir" ]; then
      echo "source directory exists but is not a git checkout: $source_dir" >&2
      exit 1
    fi

    echo "Cloning $repo_url to $source_dir"
    mkdir -p "$(dirname "$source_dir")"
    git clone --branch "$branch" "$repo_url" "$source_dir"
  fi

  repo_root="$source_dir"
fi

case "$build_profile" in
  debug)
    cargo_args=(build -p chirper-cli -p chirper-daemon)
    target_dir="debug"
    ;;
  release)
    cargo_args=(build --release -p chirper-cli -p chirper-daemon)
    target_dir="release"
    ;;
esac

echo "Building Chirper ($build_profile)"
cargo "${cargo_args[@]}" --manifest-path "$repo_root/Cargo.toml"

mkdir -p "$bin_dir"
ln -sf "$repo_root/target/$target_dir/chirper" "$bin_dir/chirper"
ln -sf "$repo_root/target/$target_dir/chirper-daemon" "$bin_dir/chirper-daemon"
ln -sf "$repo_root/scripts/run-model-compare.sh" "$bin_dir/chirper-model-compare"
ln -sf "$repo_root/scripts/run-report-viewer.sh" "$bin_dir/chirper-report-viewer"
ln -sf "$repo_root/scripts/run-test-workflow-builder.sh" "$bin_dir/chirper-test-workflow-builder"

echo "Linked binaries:"
echo "  $bin_dir/chirper"
echo "  $bin_dir/chirper-daemon"
echo "  $bin_dir/chirper-model-compare"
echo "  $bin_dir/chirper-report-viewer"
echo "  $bin_dir/chirper-test-workflow-builder"

if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
  echo
  echo "Add this to your shell profile if needed:"
  echo "  export PATH=\"$bin_dir:\$PATH\""
fi

if [ "$with_whispercpp" = true ]; then
  "$repo_root/scripts/setup-whispercpp.sh" \
    --backend "$whisper_backend" \
    --model "$whisper_model" \
    --write-config
fi

if [ "$with_service" = true ]; then
  CHIRPER_BUILD_PROFILE="$build_profile" CHIRPER_SKIP_BUILD=1 \
    "$repo_root/scripts/install-systemd-user-service.sh"
fi

if [ "$with_gnome_extension" = true ]; then
  CHIRPER_BUILD_PROFILE="$build_profile" "$repo_root/scripts/install-gnome-extension.sh"
fi

cat <<DONE

Chirper install finished.

Next checks:
  $bin_dir/chirper diagnose
DONE

if [ "$with_service" = true ]; then
  cat <<DONE
  $bin_dir/chirper daemon-status
DONE
else
  cat <<DONE
  $bin_dir/chirper-daemon
  $bin_dir/chirper daemon-status
DONE
fi

if [ "$with_gnome_extension" = true ]; then
  cat <<DONE
If the GNOME extension is not visible yet, log out and back in, then run:
  gnome-extensions enable chirper@local
DONE
fi

#!/usr/bin/env bash
set -euo pipefail

uuid="chirper@local"
bin_dir="${CHIRPER_BIN_DIR:-$HOME/.local/bin}"
source_dir="${CHIRPER_SOURCE_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/chirper/source}"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
runtime_dir="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
service_name="chirper-daemon.service"
service_path="$config_home/systemd/user/$service_name"
extension_dir="$data_home/gnome-shell/extensions/$uuid"
config_dir="$config_home/chirper"
models_dir="$data_home/chirper/models"
whispercpp_dir="$data_home/chirper/src/whisper.cpp"
chirper_runtime_dir="$runtime_dir/chirper"
purge_config=false
purge_models=false
purge_source=false
purge_whispercpp=false
reset_gnome_settings=false
dry_run=false

usage() {
  cat <<'USAGE'
Usage: scripts/uninstall.sh [options]

Removes the user-local Chirper install artifacts:
  - ~/.local/bin/chirper*
  - ~/.config/systemd/user/chirper-daemon.service
  - ~/.local/share/gnome-shell/extensions/chirper@local
  - $XDG_RUNTIME_DIR/chirper

By default, configuration, downloaded models, whisper.cpp, and source checkouts
are preserved.

Options:
  --bin-dir PATH             Chirper binary link directory. Default: ~/.local/bin.
  --source-dir PATH          Source checkout to remove with --purge-source.
                             Default: ~/.local/share/chirper/source.
  --purge-config            Remove ~/.config/chirper.
  --purge-models            Remove downloaded Whisper models.
  --purge-whispercpp        Remove the managed whisper.cpp checkout/build.
  --purge-source            Remove the source checkout passed by --source-dir.
  --purge-data              Equivalent to --purge-config --purge-models
                             --purge-whispercpp --purge-source.
  --reset-gnome-settings    Reset the extension dconf settings.
  --dry-run                 Print what would be removed.
  -h, --help                Show this help.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bin-dir)
      bin_dir="${2:?missing value for --bin-dir}"
      shift 2
      ;;
    --source-dir)
      source_dir="${2:?missing value for --source-dir}"
      shift 2
      ;;
    --purge-config)
      purge_config=true
      shift
      ;;
    --purge-models)
      purge_models=true
      shift
      ;;
    --purge-whispercpp)
      purge_whispercpp=true
      shift
      ;;
    --purge-source)
      purge_source=true
      shift
      ;;
    --purge-data)
      purge_config=true
      purge_models=true
      purge_whispercpp=true
      purge_source=true
      shift
      ;;
    --reset-gnome-settings)
      reset_gnome_settings=true
      shift
      ;;
    --dry-run)
      dry_run=true
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

run() {
  if [ "$dry_run" = true ]; then
    printf 'would run:'
    printf ' %q' "$@"
    printf '\n'
  else
    "$@"
  fi
}

remove_path() {
  local path="$1"

  if [ -e "$path" ] || [ -L "$path" ]; then
    run rm -rf "$path"
  elif [ "$dry_run" = true ]; then
    printf 'not present: %s\n' "$path"
  fi
}

if command -v gnome-extensions >/dev/null 2>&1; then
  if [ "$dry_run" = true ]; then
    printf 'would disable GNOME extension: %s\n' "$uuid"
  else
    gnome-extensions disable "$uuid" >/dev/null 2>&1 || true
  fi
fi

if command -v systemctl >/dev/null 2>&1; then
  if [ "$dry_run" = true ]; then
    printf 'would stop and disable user service: %s\n' "$service_name"
  else
    systemctl --user disable --now "$service_name" >/dev/null 2>&1 || true
  fi
fi

remove_path "$service_path"

if command -v systemctl >/dev/null 2>&1; then
  if [ "$dry_run" = true ]; then
    printf 'would reload user systemd manager\n'
  else
    systemctl --user daemon-reload >/dev/null 2>&1 || true
    systemctl --user reset-failed "$service_name" >/dev/null 2>&1 || true
  fi
fi

for name in \
  chirper \
  chirper-daemon \
  chirper-settings \
  chirper-onboarding \
  chirper-model-compare \
  chirper-report-viewer \
  chirper-test-workflow-builder
do
  remove_path "$bin_dir/$name"
done

remove_path "$extension_dir"
remove_path "$chirper_runtime_dir"

if [ "$reset_gnome_settings" = true ] && command -v dconf >/dev/null 2>&1; then
  if [ "$dry_run" = true ]; then
    printf 'would reset dconf path: /org/gnome/shell/extensions/chirper/\n'
  else
    dconf reset -f /org/gnome/shell/extensions/chirper/ || true
  fi
fi

if [ "$purge_config" = true ]; then
  remove_path "$config_dir"
fi

if [ "$purge_models" = true ]; then
  remove_path "$models_dir"
fi

if [ "$purge_whispercpp" = true ]; then
  remove_path "$whispercpp_dir"
fi

if [ "$purge_source" = true ]; then
  remove_path "$source_dir"
fi

echo "Chirper uninstall finished."

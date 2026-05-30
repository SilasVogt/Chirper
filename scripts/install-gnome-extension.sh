#!/usr/bin/env bash
set -euo pipefail

uuid="chirper@local"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$repo_root/extensions/gnome/$uuid"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
target_dir="$data_home/gnome-shell/extensions/$uuid"
build_profile="${CHIRPER_BUILD_PROFILE:-debug}"

case "$build_profile" in
  debug|release) ;;
  *)
    echo "unsupported CHIRPER_BUILD_PROFILE: $build_profile" >&2
    exit 2
    ;;
esac

if [[ ! -d "$source_dir" ]]; then
  echo "extension source not found: $source_dir" >&2
  exit 1
fi

mkdir -p "$(dirname "$target_dir")"
rm -rf "$target_dir"
cp -R "$source_dir" "$target_dir"

if [[ -d "$target_dir/schemas" ]]; then
  glib-compile-schemas "$target_dir/schemas"
fi

cat > "$target_dir/runtime.json" <<JSON
{
  "repoPath": "$repo_root",
  "cliPath": "$repo_root/target/$build_profile/chirper"
}
JSON

echo "Installed $uuid to $target_dir"

if ! command -v gnome-extensions >/dev/null 2>&1; then
  echo "Install gnome-extensions, then enable it with: gnome-extensions enable $uuid"
elif gnome-extensions info "$uuid" >/dev/null 2>&1; then
  echo "Enable it with: gnome-extensions enable $uuid"
else
  echo "GNOME Shell has not discovered this extension in the current session yet."
  echo "Log out and back in, then run: gnome-extensions enable $uuid"
fi

echo "On Wayland, log out and back in after first install or code changes."

#!/usr/bin/env bash
set -euo pipefail

uuid="chirper@local"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$repo_root/extensions/gnome/$uuid"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
target_dir="$data_home/gnome-shell/extensions/$uuid"

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

echo "Installed $uuid to $target_dir"

if gnome-extensions info "$uuid" >/dev/null 2>&1; then
  echo "Enable it with: gnome-extensions enable $uuid"
else
  echo "GNOME Shell has not discovered this extension in the current session yet."
  echo "Log out and back in, then run: gnome-extensions enable $uuid"
fi

echo "On Wayland, log out and back in after first install or code changes."

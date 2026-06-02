#!/usr/bin/env bash
set -euo pipefail

script_path="${BASH_SOURCE[0]}"
while [[ -L "$script_path" ]]; do
  script_dir="$(cd "$(dirname "$script_path")" && pwd)"
  target="$(readlink "$script_path")"
  if [[ "$target" == /* ]]; then
    script_path="$target"
  else
    script_path="$script_dir/$target"
  fi
done

repo_root="$(cd "$(dirname "$script_path")/.." && pwd)"

if [[ -x "$repo_root/target/release/chirper" ]]; then
  export CHIRPER_CLI="$repo_root/target/release/chirper"
elif [[ -x "$repo_root/target/debug/chirper" ]]; then
  export CHIRPER_CLI="$repo_root/target/debug/chirper"
else
  export CHIRPER_CLI="${CHIRPER_CLI:-chirper}"
fi

exec gjs -m "$repo_root/apps/onboarding/main.js"

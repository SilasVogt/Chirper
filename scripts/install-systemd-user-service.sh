#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
service_name="chirper-daemon.service"
service_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
service_path="$service_dir/$service_name"
daemon_bin="$repo_root/target/debug/chirper-daemon"

cargo build -p chirper-daemon

mkdir -p "$service_dir"

systemctl --user disable "$service_name" >/dev/null 2>&1 || true

cat > "$service_path" <<SERVICE
[Unit]
Description=Chirper local dictation daemon
Documentation=file://$repo_root/docs/API.md
After=graphical-session.target pipewire.service wireplumber.service
PartOf=graphical-session.target

[Service]
Type=simple
WorkingDirectory=$repo_root
ExecStart=$daemon_bin
Restart=on-failure
RestartSec=2
Environment=RUST_BACKTRACE=1

[Install]
WantedBy=default.target graphical-session.target
SERVICE

systemctl --user daemon-reload
systemctl --user enable "$service_name"

if ! systemctl --user is-active --quiet "$service_name"; then
  systemctl --user start "$service_name"
fi

echo "Installed and started $service_name"
systemctl --user --no-pager --full status "$service_name"

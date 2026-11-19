#!/usr/bin/env bash
# Installs quest-wireless-adb as a systemd user service:  bash ~/utils/quest-wireless-adb/install.sh
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
[[ $EUID -ne 0 ]] || { echo "run it as your normal user, not root" >&2; exit 1; }
command -v adb >/dev/null || { echo "needs adb (android-tools)" >&2; exit 1; }
install -d "$HOME/.local/bin" "$HOME/.config/systemd/user"
install -m 755 "$HERE/quest-wireless-adb" "$HOME/.local/bin/quest-wireless-adb"
install -m 644 "$HERE/quest-wireless-adb.service" "$HOME/.config/systemd/user/"
systemctl --user daemon-reload
systemctl --user enable --now quest-wireless-adb.service
systemctl --user restart quest-wireless-adb.service
echo "quest-wireless-adb is running. Plug the Quest in over USB once and wireless adb comes back on."
echo "Stop it with: systemctl --user disable --now quest-wireless-adb"

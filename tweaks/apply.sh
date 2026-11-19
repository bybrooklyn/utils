#!/usr/bin/env bash
# Puts my system tweaks back on a fresh install:  bash ~/utils/tweaks/apply.sh
# Shows what's different first and asks before changing anything. Safe to run again.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
[[ $EUID -ne 0 ]] || { echo "run it as your normal user, it asks for sudo" >&2; exit 1; }

ask() { local r; read -r -p "  $1 [y/N] " r </dev/tty || r=""; [[ $r == [yY]* ]]; }

# source file -> where it goes
declare -A SYSTEM=(
    [disable-usb-device-wakeup.service]=/etc/systemd/system/disable-usb-device-wakeup.service
    [99-zz-swappiness.conf]=/etc/sysctl.d/99-zz-swappiness.conf
    [80-uinput.rules]=/etc/udev/rules.d/80-uinput.rules
)

changed=()
for f in "${!SYSTEM[@]}"; do
    dest=${SYSTEM[$f]}
    if cmp -s "$HERE/system/$f" "$dest"; then
        echo "  ✓ $dest"
    else
        echo "  ~ $dest"
        diff -u --label "now" --label "tweaks" "$dest" "$HERE/system/$f" 2>/dev/null | sed 's/^/      /' || true
        changed+=("$f")
    fi
done

if ((${#changed[@]})) && ask "Install the ${#changed[@]} system file(s) above? (sudo)"; then
    for f in "${changed[@]}"; do
        sudo install -D -m 644 "$HERE/system/$f" "${SYSTEM[$f]}"
    done
    sudo systemctl daemon-reload
    sudo systemctl enable --now disable-usb-device-wakeup.service
    sudo sysctl -q --system
    sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=misc --sysname-match=uinput
    echo "  done"
fi

dest=$HOME/.config/systemd/user/proton-drive.service
if ! cmp -s "$HERE/user/proton-drive.service" "$dest" && ask "Install the Proton Drive mount unit (~/Proton, needs rclone set up)?"; then
    install -D -m 644 "$HERE/user/proton-drive.service" "$dest"
    systemctl --user daemon-reload
    echo "  installed. Start it with: systemctl --user enable --now proton-drive"
fi

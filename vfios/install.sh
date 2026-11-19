#!/usr/bin/env bash
# Installs vfios (cf-ddns) for the current user on this machine: the machine whose public IP your
# DNS should follow, usually a home server. Run it there:  bash ~/utils/vfios/install.sh
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
CONF=$HOME/.config/cf-ddns

say() { printf '%s\n' "$*"; }
ok()  { printf '  \e[32m✓\e[0m %s\n' "$*"; }
die() { printf '  \e[31m✗\e[0m %s\n' "$*" >&2; exit 1; }

[[ $EUID -ne 0 ]] || die "run it as your normal user, not root"
command -v python3 >/dev/null || die "needs python3"
command -v systemctl >/dev/null || die "needs systemd"

say "vfios: keeps every Cloudflare A record that points at home on home's current public IP"
say ""

install -d -m 755 "$HOME/cf-ddns" "$HOME/.local/bin" "$HOME/.config/systemd/user"
install -d -m 700 "$CONF"
install -m 755 "$HERE/cf-ddns.py" "$HOME/cf-ddns/cf-ddns.py"
install -m 755 "$HERE/vfios" "$HOME/.local/bin/vfios"
install -m 644 "$HERE/cf-ddns.service" "$HERE/cf-ddns.timer" "$HOME/.config/systemd/user/"
ok "installed the script, the vfios command and the user timer"

if [[ ! -s $CONF/token ]]; then
    say ""
    say "  It needs a Cloudflare API token. Make one at dash.cloudflare.com > My Profile > API Tokens >"
    say "  Create Token > Custom token, with Zone > Zone > Read and Zone > DNS > Edit, on All zones."
    read -r -s -p "  Paste the token (it won't show) and press Enter: " tok </dev/tty; echo
    [[ -n $tok ]] || die "no token given"
    (umask 077; printf '%s\n' "$tok" > "$CONF/token")
    unset tok
fi
python3 "$HOME/cf-ddns/cf-ddns.py" --status | grep '^token' || die "Cloudflare didn't accept the token"

if ! python3 -c "import json,sys; sys.exit(0 if json.load(open('$CONF/state.json')).get('home_ip') else 1)" 2>/dev/null; then
    now=$(curl -fs4 https://api.ipify.org || true)
    say ""
    say "  Which IP do your home records point at right now? Every A record holding it will follow"
    say "  your public IP from now on. Press Enter for this machine's public IP (${now:-unknown})."
    read -r -p "  IP: " ip </dev/tty
    python3 "$HOME/cf-ddns/cf-ddns.py" --seed "${ip:-$now}"
fi

systemctl --user daemon-reload
systemctl --user enable --now cf-ddns.timer
ok "timer on: checks every 5 minutes"
if [[ $(loginctl show-user "$USER" -p Linger --value 2>/dev/null) != yes ]]; then
    loginctl enable-linger "$USER" 2>/dev/null && ok "linger on, so it runs while you're logged out" ||
        say "  Run 'sudo loginctl enable-linger $USER' so it keeps running while you're logged out."
fi
say ""
"$HOME/.local/bin/vfios" records | tail -1
say "  Run 'vfios' any time to see where things stand. Make sure ~/.local/bin is on your PATH."

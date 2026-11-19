#!/usr/bin/env bash
# Installs pirate and walks you through setting it up.
#
#   curl -fsSL https://raw.githubusercontent.com/bybrooklyn/utils/main/pirate/install.sh | bash
#
# or, from a clone:  bash pirate/install.sh
#
# What it does: checks your system, installs git and zsh if they're missing, clones
# bybrooklyn/utils to ~/utils (or updates it), puts `pirate` on your PATH, helps you get
# a Proton WireGuard config, then runs `pirate` to set everything up. Nothing here needs
# root except the package install and `pirate` itself, which both ask for sudo.
set -euo pipefail

REPO=https://github.com/bybrooklyn/utils.git
DEST=${UTILS_DIR:-$HOME/utils}
BIN_DIR=$HOME/.local/bin
PROTON_URL=https://account.protonvpn.com/downloads

if [[ -t 1 ]]; then
    B=$'\e[1m' G=$'\e[32m' Y=$'\e[33m' R=$'\e[31m' D=$'\e[2m' N=$'\e[0m'
else
    B="" G="" Y="" R="" D="" N=""
fi
say()  { printf '%s\n' "$*"; }
title() { printf '\n%s%s%s\n' "$B" "$*" "$N"; }
ok()   { printf '  %s✓%s %s\n' "$G" "$N" "$*"; }
warn() { printf '  %s!%s %s\n' "$Y" "$N" "$*"; }
die()  { printf '  %s✗%s %s\n' "$R" "$N" "$*" >&2; exit 1; }

# stdin is the script itself under `curl | bash`, so questions read from the terminal
(exec </dev/tty) 2>/dev/null || die "run this in a terminal, it asks you questions"
ask() {  # prompt default(y|n) -> 0 for yes
    local reply
    read -r -p "  $1 [$( [[ $2 == y ]] && echo Y/n || echo y/N )] " reply </dev/tty || reply=""
    reply=${reply:-$2}
    [[ $reply == [yY]* ]]
}
pause() { read -r -p "  $1" _ </dev/tty || true; }

title "pirate installer"
say "  qBittorrent inside a Proton VPN tunnel. Everything else on your computer stays off the VPN."

# ---------- checks ----------

title "Checking your system"
[[ $EUID -ne 0 ]] || die "don't run this as root or with sudo, it asks for sudo when it needs it"
[[ $(uname -s) == Linux ]] || die "pirate only works on Linux"
command -v systemctl >/dev/null || die "pirate needs systemd"
ok "Linux with systemd"

if command -v pacman >/dev/null; then
    PM=(sudo pacman -S --needed --noconfirm); ok "pacman (Arch, CachyOS, EndeavourOS, Manjaro...)"
elif command -v apt-get >/dev/null; then
    PM=(sudo apt-get install -y); ok "apt (Debian, Ubuntu, Mint, Pop!_OS...)"
elif command -v dnf >/dev/null; then
    PM=(sudo dnf install -y); ok "dnf (Fedora, Nobara...)"
else
    PM=()
    warn "no pacman, apt or dnf, so pirate can't install its packages for you."
    say  "    It still works if you install whatever provides these commands first:"
    say  "    git zsh wg natpmpc jq curl qbittorrent python3 bwrap notify-send update-desktop-database"
    ask "Keep going anyway?" n || exit 1
fi

if [[ -n ${WAYLAND_DISPLAY:-} || -n ${DISPLAY:-} ]] && systemctl --user show-environment >/dev/null 2>&1; then
    ok "desktop session: ${XDG_CURRENT_DESKTOP:-unknown desktop} on ${XDG_SESSION_TYPE:-${WAYLAND_DISPLAY:+wayland}}"
else
    warn "this doesn't look like a desktop session (no Wayland or X11 display, or no systemd user session)"
    say  "    pirate runs the qBittorrent app, so set it up from a terminal inside your desktop."
    ask "Keep going anyway?" n || exit 1
fi

if [[ $(readlink -f /etc/resolv.conf) == /run/systemd/resolve/* ]]; then
    ok "systemd-resolved handles DNS"
else
    warn "/etc/resolv.conf doesn't point at systemd-resolved"
    say  "    pirate keeps qBittorrent's DNS inside the VPN through systemd-resolved, so it won't set up without it."
    say  "    Ubuntu and Fedora have it on already. On Debian: sudo apt install systemd-resolved, reboot, run this again."
    exit 1
fi

if [[ -e /sys/class/net/proton0 ]]; then
    warn "the Proton VPN app is connected right now"
    say  "    Disconnect it. Running both at once can break pirate's tunnel."
    pause "Press Enter once it's disconnected..."
fi

if ! modinfo wireguard >/dev/null 2>&1 && [[ ! -d /sys/module/wireguard ]]; then
    die "your kernel has no WireGuard module (any normal kernel from the last few years does)"
fi
ok "WireGuard kernel module"

# ---------- git, zsh, the repo ----------

title "Getting pirate"
missing=()
command -v git >/dev/null || missing+=(git)
command -v zsh >/dev/null || missing+=(zsh)
if ((${#missing[@]})); then
    ((${#PM[@]})) || die "install ${missing[*]} and run this again"
    say "  Installing ${missing[*]} (sudo)"
    "${PM[@]}" "${missing[@]}" </dev/tty
fi
ok "git and zsh"

if [[ -d $DEST/.git ]]; then
    git -C "$DEST" pull --ff-only --quiet 2>/dev/null && ok "updated $DEST" || warn "couldn't update $DEST, using it as it is"
elif [[ -e $DEST ]]; then
    die "$DEST already exists and isn't a clone of bybrooklyn/utils. Move it, or set UTILS_DIR=/somewhere/else"
else
    git clone --quiet "$REPO" "$DEST"
    ok "cloned to $DEST"
fi

mkdir -p "$BIN_DIR"
ln -sfn "$DEST/pirate/bin/pirate" "$BIN_DIR/pirate"
ok "linked $BIN_DIR/pirate"
PIRATE=$BIN_DIR/pirate
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR isn't on your PATH, so typing 'pirate' won't work yet."
       say  "    Add this line to ~/.bashrc or ~/.zshrc, then open a new terminal:"
       say  "    export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac

# ---------- the Proton config ----------

find_conf() {  # newest Proton WireGuard config in ~/Downloads
    local f
    while IFS= read -r -d '' f; do
        f=${f#* }
        grep -q '^\[Peer\]' "$f" && grep -q '^PrivateKey' "$f" && grep -q '10\.2\.0\.2' "$f" && { echo "$f"; return 0; }
    done < <(find "$HOME/Downloads" -maxdepth 1 -type f -name '*.conf' -printf '%T@ %p\0' 2>/dev/null | sort -z -rn)
    return 1
}

title "Your Proton WireGuard config"
if [[ -r /etc/pirate/wg.meta ]]; then
    ok "pirate already has a config, skipping this part"
    CONF=""
else
    CONF=$(find_conf || true)
    if [[ -n $CONF ]]; then
        ok "found ${CONF##*/} in Downloads"
        ask "Use it?" y || CONF=""
    fi
    if [[ -z $CONF ]]; then
        say "  pirate needs a WireGuard config file from your Proton account. This needs a paid plan."
        say ""
        say "    1. Go to ${B}$PROTON_URL${N} and log in"
        say "    2. Scroll to ${B}WireGuard configuration${N}"
        say "    3. Name it anything (like \"pirate\") and pick platform ${B}GNU/Linux${N}"
        say "    4. Turn ${B}NAT-PMP (Port Forwarding)${N} ON. This is the important one."
        say "       Leave NetShield and Moderate NAT off."
        say "    5. Under servers, pick one with the ${B}P2P${N} icon (two arrows). Close to you is best."
        say "    6. Click ${B}Create${N}, then ${B}Download${N}. It lands in ~/Downloads."
        say ""
        ask "Open that page in your browser now?" y && { xdg-open "$PROTON_URL" >/dev/null 2>&1 & }
        say ""
        say "  Waiting for the .conf to show up in ~/Downloads. ${D}(or type a path to one and press Enter)${N}"
        while [[ -z $CONF ]]; do
            if read -r -t 3 typed </dev/tty && [[ -n $typed ]]; then
                typed=${typed/#\~/$HOME}
                [[ -r $typed ]] && CONF=$typed || warn "can't read $typed"
            else
                CONF=$(find_conf || true)
            fi
        done
        ok "got ${CONF##*/}"
    fi
    if grep -qiE '^#\s*NAT-PMP \(Port Forwarding\)\s*=\s*off' "$CONF"; then
        warn "this config has NAT-PMP (port forwarding) OFF"
        say  "    Without it you'll download slowly and barely seed. Make a new one with it ON."
        ask "Use it anyway?" n || die "make a config with NAT-PMP on and run this again"
        FORCE=--force
    fi
fi

# ---------- run pirate ----------

title "Setting up"
say "  Now pirate takes over. It asks for your sudo password once, then:"
say "    - installs whatever packages are missing"
say "    - stores the config where only root can read it (and offers to delete the download,"
say "      since it has your private key in it)"
say "    - builds the tunnel and runs leak tests ${B}before${N} it touches qBittorrent"
say "    - closes qBittorrent if it's open, points it at the tunnel, and starts it again"
say "    - makes the app menu, magnet links and login all open the VPN copy"
say ""
pause "Press Enter to start..."
say ""
args=()
[[ -n ${CONF:-} ]] && args+=(setup "$CONF")
[[ -n ${FORCE:-} ]] && args+=("$FORCE")
"$PIRATE" "${args[@]}" </dev/tty || die "pirate setup didn't finish. Fix what it says above and run 'pirate' again, it picks up where it stopped."

title "Done"
say "  Open qBittorrent from the app menu like normal. It's inside the VPN now."
say ""
say "    ${B}pirate status${N}         is everything OK"
say "    ${B}pirate doctor${N}         status plus the full leak tests"
say "    ${B}pirate server NL${N}      prefer a country (or ${B}pirate server list${N})"
say "    ${B}pirate logs${N}           watch what it's doing"
say "    ${B}pirate help${N}           everything else"
say ""
if [[ ! -r $HOME/.cache/Proton/VPN/serverlist.json ]]; then
    say "  ${D}Optional: install the Proton VPN app, log in once and disconnect. pirate reads its server"
    say "  list to switch servers on its own when one dies. Then run 'pirate' again.${N}"
fi

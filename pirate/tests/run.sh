#!/usr/bin/env bash
# pirate's checks. No root, no network, nothing touched outside a temp folder.
#   bash tests/run.sh
set -u
cd "$(dirname "$0")/.." || exit 1
ROOT=$PWD
FIX=$ROOT/tests/fixtures
T=$(mktemp -d)
FAKE=""
trap 'rm -rf "$T"; [[ -n $FAKE ]] && kill -9 "$FAKE" 2>/dev/null' EXIT
mkdir -p "$T/home"
pass=0 fail=0

check() {  # name command...
    local name=$1 out
    shift
    if out=$("$@" 2>&1); then
        pass=$((pass + 1))
        printf '  ok    %s\n' "$name"
    else
        fail=$((fail + 1))
        printf '  FAIL  %s\n' "$name"
        [[ -n $out ]] && printf '%s\n' "$out" | head -8 | sed 's/^/        /'
    fi
}

# run zsh code with bin/pirate's functions loaded and HOME inside the temp folder
pz() { HOME=$T/home XDG_CONFIG_HOME="" XDG_STATE_HOME="" PIRATE_SOURCED=1 zsh -c "source '$ROOT/bin/pirate'; $1"; }

# qBittorrent settings check on a copy of a fixture; prints what it says needs changing
qbt_check() {  # fixture [sed-expression to break it]
    cp "$FIX/$1" "$T/q.conf"
    [[ -n ${2:-} ]] && sed -i -E "$2" "$T/q.conf"
    pz "QBT_CONF=$T/q.conf CONFIG_DIR=$FIX; qbt_conf check"
}

echo "syntax"
check "bin/pirate parses (zsh)" zsh -n bin/pirate
for f in share/pirate-netns share/pirate-vpnd install.sh tests/run.sh; do
    check "$f parses (bash)" bash -n "$f"
done
if command -v shellcheck >/dev/null; then
    check "shellcheck finds no warnings" shellcheck -S warning share/pirate-netns share/pirate-vpnd install.sh tests/run.sh
fi

echo "qBittorrent settings"
check "5.2 layout with everything set passes" qbt_check qbt-5.2.conf
reports() {  # fixture sed-expression word: the check must fail and name word
    local out
    out=$(qbt_check "$1" "$2") && { echo "check passed but shouldn't have"; return 1; }
    [[ $out == *"$3"* ]] || { echo "said: $out"; return 1; }
}
check "missing VPN binding is reported" reports qbt-5.2.conf '/^Session.Interface=/d' Interface
check "5.3 layout with everything set passes" qbt_check qbt-5.3.conf
check "5.3: missing VPN binding is reported" reports qbt-5.3.conf '/^Session.Interface=/d' Interface
check "5.3: WebUI turned off is reported" reports qbt-5.3.conf 's/^Enabled=true/Enabled=false/' Enabled
check "5.3: a wrong WebUI password is reported" reports qbt-5.3.conf 's/^Password_PBKDF2=.*/Password_PBKDF2="@ByteArray(AAAA:BBBB)"/' password

qbt_apply() {  # fixture sed-expression: break it, apply, print the result
    cp "$FIX/$1" "$T/q.conf"
    sed -i -E "$2" "$T/q.conf"
    pz "QBT_CONF=$T/q.conf CONFIG_DIR=$FIX; qbt_conf apply" && cat "$T/q.conf"
}
apply_keeps_layout() {  # fixture sed-expression: after apply, the check passes and the file kept its layout
    local out
    out=$(qbt_apply "$1" "$2") || { echo "$out"; return 1; }
    if [[ $1 == qbt-5.3.conf ]]; then
        grep -q '^WebUI\\' <<<"$out" && { echo "wrote old-style WebUI\\ keys into a 5.3 file"; return 1; }
        awk '/^\[WebUI\]/{s=1;next} /^\[/{s=0} s && /^Port=8080$/{f=1} END{exit !f}' <<<"$out" || { echo "no Port=8080 under [WebUI]"; return 1; }
    else
        grep -q '^\[WebUI\]' <<<"$out" && { echo "added a [WebUI] section to a 5.2 file"; return 1; }
        grep -q '^WebUI\\Port=8080$' <<<"$out" || { echo "no WebUI\\Port=8080"; return 1; }
    fi
    grep -qE '^Session\\DefaultSavePath=/somewhere-else$' <<<"$out" || { echo "touched the save path (that's the API's job now)"; return 1; }
    pz "QBT_CONF=$T/q.conf CONFIG_DIR=$FIX; qbt_conf check" >/dev/null || { echo "check still unhappy after apply"; return 1; }
}
check "5.3: fixing a setting writes [WebUI], never WebUI\\ keys" apply_keeps_layout qbt-5.3.conf 's/^Port=8080/Port=9999/; s|^(Session.DefaultSavePath=).*|\1/somewhere-else|'
check "5.2: fixing a setting keeps the old layout" apply_keeps_layout qbt-5.2.conf 's/^WebUI.Port=8080/WebUI\\Port=9999/; s|^(Session.DefaultSavePath=).*|\1/somewhere-else|'

echo "WireGuard configs"
check "a good Proton config parses with NAT-PMP on" bash -c "bash share/pirate-netns parse $FIX/wg-good.conf | grep -qx NATPMP=on"
check "NAT-PMP off is detected" bash -c "bash share/pirate-netns parse $FIX/wg-natpmp-off.conf | grep -qx NATPMP=off"
check "a hostname endpoint is rejected" bash -c "! bash share/pirate-netns parse $FIX/wg-bad-endpoint.conf"

echo "server list"
servers_built() {
    local got
    got=$(pz "PROTON_CACHE=$FIX/serverlist.json; build_servers" | jq -r '[.servers[].n] | join(",")')
    [[ $got == 'NL#1,US-NY#1' ]] || { echo "got: $got"; return 1; }
}
check "only P2P servers: no Secure Core, Tor, duplicates, bad keys or bad names" servers_built

# `pirate server refresh` with a stand-in Proton app that just updates a stand-in cache,
# and a recorder instead of the root install
refresh_with() {  # snapshot-date-of-installed-list extra-args
    mkdir -p "$T/r/bin" "$T/r/etc"
    cp "$FIX/serverlist.json" "$T/r/cache.json"; touch -d 2026-01-01 "$T/r/cache.json"
    printf '#!/bin/sh\nsleep 1; touch "%s"\n' "$T/r/cache.json" > "$T/r/bin/protonvpn-app"; chmod +x "$T/r/bin/protonvpn-app"
    printf '{"snapshot_date":"%s","servers":[{"n":"US-NY#1"}]}' "$1" > "$T/r/etc/servers.json"
    rm -f "$T/r/installed"
    PATH=$T/r/bin:$PATH WAYLAND_DISPLAY=test pz "
        ETC=$T/r/etc PROTON_CACHE=$T/r/cache.json
        pgrep() { return 1 }; pkill() { : }; systemctl() { return 1 }; desktop_notify() { : }
        run_sudo() { print -r -- \"\$*\" > $T/r/installed }
        cmd_server_refresh $2"
}
refresh_installs() {
    refresh_with 2026-01-01 "" >/dev/null || { echo "refresh failed"; return 1; }
    grep -q "servers-install $T/r/cache.json" "$T/r/installed" 2>/dev/null || { echo "never installed the refreshed list"; return 1; }
}
refresh_auto_waits() {
    refresh_with "$(date +%F)" --auto >/dev/null
    [[ ! -e $T/r/installed ]] || { echo "refreshed a list that was only a day old"; return 1; }
}
check "server refresh opens the Proton app, waits for its cache, installs the list" refresh_installs
check "the weekly timer leaves a fresh list alone" refresh_auto_waits

echo "packages"
check "apt, dnf and pacman tables cover the same commands" pz '
    [[ ${(ok)PKG_PACMAN} == ${(ok)PKG_APT} && ${(ok)PKG_APT} == ${(ok)PKG_DNF} ]]'
missing_on() {  # pm word: with nothing installed, the package list for pm includes word
    local got
    got=$(pz "path=(); missing_packages $1")
    [[ " $got " == *" $2 "* ]] || { echo "got: $got"; return 1; }
}
check "Debian/Ubuntu get the natpmpc package" missing_on apt natpmpc
check "Debian/Ubuntu get libnotify-bin" missing_on apt libnotify-bin
check "Fedora gets natpmpc from libnatpmp" missing_on dnf libnatpmp
check "nothing to install when every command is there" pz '[[ -z $(missing_packages pacman) ]]'
check "DNS through systemd-resolved is accepted" pz "ln -sf /run/systemd/resolve/stub-resolv.conf $T/rc; RESOLV_CONF=$T/rc resolved_dns"
check "VPN DNS: Proton first, then 1.1.1.1, with a short timeout" pz '[[ $VPN_RESOLV == $'"'"'nameserver 10.2.0.1\nnameserver 1.1.1.1\noptions timeout:2 attempts:1'"'"' ]]'
check "a plain /etc/resolv.conf is refused" pz "echo 'nameserver 1.1.1.1' > $T/rc2; ! RESOLV_CONF=$T/rc2 resolved_dns"

echo "desktop session"
session_env() {  # fake `systemctl --user show-environment` output → what qBittorrent gets
    pz "systemctl() { print -r -- '$1' }; SESSION_ENV=$T/session.env; write_session_env; cat $T/session.env"
}
wayland_session() {
    local got
    got=$(session_env $'WAYLAND_DISPLAY=wayland-1\nDISPLAY=:0\nXAUTHORITY=/x\nXDG_CURRENT_DESKTOP=GNOME\nQT_QPA_PLATFORM=xcb\nSESSION_MANAGER=x')
    [[ $got == *WAYLAND_DISPLAY=wayland-1* && $got == *XDG_CURRENT_DESKTOP=GNOME* && $got == *QT_QPA_PLATFORM=wayland* ]] || { echo "$got"; return 1; }
    [[ $got != *DISPLAY=:0* && $got != *XAUTHORITY* && $got != *QT_QPA_PLATFORM=xcb* && $got != *SESSION_MANAGER* ]] || { echo "$got"; return 1; }
}
x11_session() {
    local got
    got=$(session_env $'DISPLAY=:1\nXAUTHORITY=/home/x/.Xauthority\nXDG_CURRENT_DESKTOP=XFCE')
    [[ $got == *DISPLAY=:1* && $got == *XAUTHORITY=/home/x/.Xauthority* && $got == *QT_QPA_PLATFORM=xcb* ]] || { echo "$got"; return 1; }
}
check "Wayland session: Wayland only, no DISPLAY, any desktop" wayland_session
check "X11 session: DISPLAY, XAUTHORITY and the xcb platform" x11_session
check "no desktop at all is refused by the startup check" bash -c \
    "source <(sed -n '/^have_display()/,/^}/p' share/pirate-netns); ! WAYLAND_DISPLAY= DISPLAY= have_display"
check "an X11 display with its socket file is accepted" bash -c \
    "source <(sed -n '/^have_display()/,/^}/p' share/pirate-netns); n=\$(ls /tmp/.X11-unix 2>/dev/null | head -1); [[ -z \$n ]] || WAYLAND_DISPLAY= DISPLAY=:\${n#X}.0 have_display"

echo "settings file"
check "a setting saves and reads back" pz 'ensure_config; set_config COUNTRY NL; [[ $(get_config COUNTRY) == NL ]]'
check "FORWARD=plain is kept" pz 'ensure_config; set_config FORWARD plain; ensure_forward; [[ $(get_config FORWARD) == plain ]]'
check "a broken FORWARD gets fixed" pz 'ensure_config; set_config FORWARD junk; ensure_forward; [[ $(get_config FORWARD) == (bwrap|plain) ]]'

echo "daemon"
# run bash code with pirate-vpnd's functions loaded (USER_HOME is the temp home)
pv() { PIRATE_SOURCED=1 PIRATE_USER=$USER bash -c "source share/pirate-vpnd; USER_HOME=$T/home; warn() { :; }; $1"; }
check "DOWNLOAD_DIR: ~ is expanded and a trailing slash dropped" pv '[[ $(download_dir_from "~/Movies/") == '"$T/home/Movies"' ]]'
check "DOWNLOAD_DIR: empty means ~/Torrents" pv '[[ $(download_dir_from "") == '"$T/home/Torrents"' ]]'
check "DOWNLOAD_DIR: a relative path is refused" pv '[[ $(download_dir_from "Movies") == '"$T/home/Torrents"' ]]'
check "DOWNLOAD_DIR is read from the settings file" pv '
    mkdir -p "$USER_HOME/.config/pirate"; USER_CONFIG=$USER_HOME/.config/pirate/config
    echo "DOWNLOAD_DIR=\"/data/films\"  # where downloads go" > "$USER_CONFIG"
    load_user_config; [[ $DOWNLOAD_DIR == /data/films ]]'
PREFS='{"upnp":false,"current_network_interface":"wgpirate","listen_port":55101,"save_path":"/data/films/"}'
check "qBittorrent already right: nothing to send" pv "[[ \$(qbt_changes '$PREFS' 55101 /data/films) == '{}' ]]"
check "a new DOWNLOAD_DIR is sent as save_path" pv "[[ \$(qbt_changes '$PREFS' 55101 /data/other) == '{\"save_path\":\"/data/other\"}' ]]"
check "a new port and UPnP back on are both sent" pv "[[ \$(qbt_changes '${PREFS/\"upnp\":false/\"upnp\":true}' 40000 /data/films) == '{\"upnp\":false,\"listen_port\":40000}' ]]"

check "messages land in the event log with a time and a level" pv "
    EVENTS=$T/events.log; log 'switched to TEST-NL#1'; err 'something broke'
    grep -qE '^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9:]{8} info  switched to TEST-NL#1$' \$EVENTS &&
    grep -qE ' error something broke$' \$EVENTS"
check "a big event log is trimmed to the last 2000 lines" pv "
    EVENTS=$T/big.log; seq 1 200000 > \$EVENTS; trim_events
    [[ \$(wc -l < \$EVENTS) == 2000 && \$(tail -1 \$EVENTS) == 200000 ]]"
check "a small event log is left alone" pv "
    EVENTS=$T/small.log; seq 1 10 > \$EVENTS; trim_events; [[ \$(wc -l < \$EVENTS) == 10 ]]"

echo "guard"
guard_test() {
    cp /usr/bin/sleep "$T/qbittorrent-nox"
    bash -c "trap '' TERM; exec '$T/qbittorrent-nox' 60" &
    FAKE=$!
    sleep 6   # the guard ignores processes younger than 5 s
    PIRATE_SOURCED=1 PIRATE_USER=$USER FAKE=$FAKE bash -c '
        source share/pirate-vpnd
        pgrep() { echo "$FAKE"; }   # only ever look at the fake, never a real qBittorrent
        err() { :; }; notify() { :; }
        HOST_NS=$(stat -Lc %i /proc/self/ns/net) KILL_HOST_QBT=yes KILL_AFTER=2
        declare -A warned_at=()
        leak_guard; sleep 1
        kill -0 "$FAKE" 2>/dev/null || { echo "gone after SIGTERM, it should have ignored it"; exit 1; }
        sleep 2; leak_guard; sleep 1
        ! kill -0 "$FAKE" 2>/dev/null || { echo "still alive after the kill deadline"; exit 1; }'
}
check "a qBittorrent outside the VPN that ignores SIGTERM gets killed" guard_test

echo "MyAnonamouse"
TOK=abcdefghijklmnopqrstuvwxyz0123456789_-AB
check "a pasted mam_id= prefix and spaces are stripped" pz "[[ \$(mam_clean_token ' mam_id=$TOK ') == $TOK ]]"
check "a token that isn't a mam_id is refused" pz "! mam_clean_token 'short' && ! mam_clean_token \"abc'def\$(id)ghijklmnopqrstuvwxyz\""
mam_set_test() {
    mkdir -p "$T/r"
    echo "mam_id=$TOK" > "$T/r/pending"
    local out
    out=$(pz "
        run_sudo() { [[ \$2 == -d ]] && return 0; install -m600 \"\${@: -2:1}\" '$T/r/installed'; }
        mam_request_update() { :; }; mam_wait() { :; }
        mam_set --from '$T/r/pending'" 2>&1) || { echo "$out"; return 1; }
    [[ $(cat "$T/r/installed") == "mam_id=$TOK" ]] || { echo "installed: $(cat "$T/r/installed")"; return 1; }
    [[ $(stat -c %a "$T/r/installed") == 600 ]] || { echo "mode $(stat -c %a "$T/r/installed")"; return 1; }
    [[ ! -e $T/r/pending ]] || { echo "the --from file was left behind"; return 1; }
    [[ $out != *"$TOK"* ]] || { echo "the token was printed"; return 1; }
}
check "mam set --from installs the token root-only, removes the file, never prints it" mam_set_test

# the daemon side, with MAM faked: mam_curl prints a canned reply and counts calls
pm() {  # reply-json bash-code
    mkdir -p "$T/m/etc" "$T/m/run" "$T/m/st"
    PIRATE_SOURCED=1 PIRATE_USER=$USER REPLY_JSON=$1 bash -c "
        source share/pirate-vpnd
        ETC=$T/m/etc RUN=$T/m/run STATE=$T/m/st EVENTS=$T/m/events
        MAM_TOKEN=\$ETC/mam MAM_STATE=\$STATE/mam.state
        log() { :; }; warn() { :; }; notify() { echo n >> $T/m/notified; }
        mam_curl() { echo c >> $T/m/calls; [[ -n \${NEW_COOKIE:-} ]] && printf '.myanonamouse.net\tTRUE\t/\tTRUE\t0\tmam_id\t%s\n' \"\$NEW_COOKIE\" > \"\$1\"; printf '%s' \"\$REPLY_JSON\"; }
        state=connected want_mam=0 mam_tok_mtime='' mam_rejected=0 mam_fails=0 mam_next_try=0 mam_notified=0
        mam_state_load
        echo 'mam_id=$TOK' > \$MAM_TOKEN; chmod 600 \$MAM_TOKEN
        put public_ip 146.70.202.190
        $2"
}
reset_m() { rm -rf "$T/m"; }
calls() { [[ -f $T/m/calls ]] && wc -l < "$T/m/calls" || echo 0; }

mam_ok_test() {
    reset_m
    pm '{"Success":true,"msg":"Completed"}' 'mam_sync; mam_sync' || return 1
    grep -q '^state=ok$' "$T/m/run/mam" && grep -q '^ip=146.70.202.190$' "$T/m/run/mam" || { cat "$T/m/run/mam"; return 1; }
    [[ $(cut -d' ' -f1 "$T/m/st/mam.state") == 146.70.202.190 ]] || return 1
    [[ $(calls) == 1 ]] || { echo "calls: $(calls) (the unchanged IP should not be sent twice)"; return 1; }
}
check "MAM accepts the IP: status ok, remembered, not resent while unchanged" mam_ok_test

mam_ipchange_test() {
    reset_m
    pm '{"Success":true,"msg":"Completed"}' 'mam_sync; put public_ip 89.187.1.2; mam_sync' || return 1
    [[ $(calls) == 2 ]] && grep -q '^ip=89.187.1.2$' "$T/m/run/mam"
}
check "a new VPN IP is sent to MAM" mam_ipchange_test

mam_cooldown_test() {
    reset_m
    pm '{"Success":false,"msg":"Last change too recent"}' 'mam_sync; mam_sync; ((mam_next_try >= EPOCHSECONDS + 3500))' || return 1
    grep -q '^state=cooldown$' "$T/m/run/mam" && [[ $(calls) == 1 ]]
}
check "MAM's one-change-per-hour limit waits an hour" mam_cooldown_test

mam_reject_test() {
    reset_m
    pm '{"Success":false,"msg":"Invalid session"}' 'mam_sync; put public_ip 89.187.1.2; mam_sync' || return 1
    grep -q '^state=rejected$' "$T/m/run/mam" || return 1
    [[ $(calls) == 1 ]] || { echo "kept calling after a rejection"; return 1; }
    [[ $(wc -l < "$T/m/notified") == 1 ]] || { echo "notified $(wc -l < "$T/m/notified") times"; return 1; }
}
check "a rejected session notifies once and stops until the token changes" mam_reject_test

mam_refresh_test() {
    reset_m
    NEW_COOKIE=zyxwvutsrqponmlkjihgfedcba9876543210 pm '{"Success":true,"msg":"No change"}' 'mam_sync' || return 1
    [[ $(cat "$T/m/etc/mam") == mam_id=zyxwvutsrqponmlkjihgfedcba9876543210 ]] || { echo "token file: $(cat "$T/m/etc/mam")"; return 1; }
    [[ $(stat -c %a "$T/m/etc/mam") == 600 ]] || return 1
    ! grep -rq "$TOK\|zyxwvutsrqponmlkjihgfedcba" "$T/m/run" "$T/m/st" || { echo "the cookie leaked into status or state"; return 1; }
}
check "a refreshed cookie from MAM replaces the stored one (600) and never reaches status files" mam_refresh_test

mam_noanswer_test() {
    reset_m
    pm 'curl: (6) Could not resolve host' 'mam_sync; mam_sync; ((mam_next_try >= EPOCHSECONDS + 250))' || return 1
    grep -q '^state=error$' "$T/m/run/mam" && [[ $(calls) == 1 ]]
}
check "no answer from MAM retries later instead of every tick" mam_noanswer_test

echo "installer"
eval "$(sed -n '/^find_conf()/,/^}/p' install.sh)"
finds_spaced_conf() {
    mkdir -p "$T/dl/Downloads"
    cp "$FIX/wg-good.conf" "$T/dl/Downloads/pirate (1).conf"
    echo junk > "$T/dl/Downloads/newer junk.conf"
    local got
    got=$(HOME=$T/dl find_conf)
    [[ $got == "$T/dl/Downloads/pirate (1).conf" ]] || { echo "got: $got"; return 1; }
}
check "finds a downloaded config with a space in its name" finds_spaced_conf

echo
echo "$pass passed, $fail failed"
((fail == 0))

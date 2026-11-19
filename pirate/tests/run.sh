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

echo "WireGuard configs"
check "a good Proton config parses with NAT-PMP on" bash -c "bash share/pirate-netns parse $FIX/wg-good.conf | grep -qx NATPMP=on"
check "NAT-PMP off is detected" bash -c "bash share/pirate-netns parse $FIX/wg-natpmp-off.conf | grep -qx NATPMP=off"
check "a hostname endpoint is rejected" bash -c "! bash share/pirate-netns parse $FIX/wg-bad-endpoint.conf"

echo "server list"
servers_built() {
    local got
    got=$(pz "PROTON_CACHE=$FIX/serverlist.json; build_servers" | jq -r '[.servers[].n] | join(",")')
    [[ $got == 'TEST-NL#1,TEST-NY#1' ]] || { echo "got: $got"; return 1; }
}
check "only P2P servers, no Secure Core, Tor or duplicates" servers_built

echo "settings file"
check "a setting saves and reads back" pz 'ensure_config; set_config COUNTRY NL; [[ $(get_config COUNTRY) == NL ]]'
check "FORWARD=plain is kept" pz 'ensure_config; set_config FORWARD plain; ensure_forward; [[ $(get_config FORWARD) == plain ]]'
check "a broken FORWARD gets fixed" pz 'ensure_config; set_config FORWARD junk; ensure_forward; [[ $(get_config FORWARD) == (bwrap|plain) ]]'

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

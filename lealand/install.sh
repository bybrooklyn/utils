#!/usr/bin/env bash
# Builds lealand (the ll command) into ~/.cargo/bin and checks the zsh side:  bash ~/utils/lealand/install.sh
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)

ok()  { printf '  \e[32m✓\e[0m %s\n' "$*"; }
die() { printf '  \e[31m✗\e[0m %s\n' "$*" >&2; exit 1; }

[[ $EUID -ne 0 ]] || die "run it as your normal user, not root"
command -v cargo >/dev/null || die "needs cargo (rustup.rs)"

cargo install --path "$HERE" --locked --quiet --force
ok "built ll into ~/.cargo/bin"
# it was called ll before it was lealand
for d in "${XDG_CONFIG_HOME:-$HOME/.config}" "${XDG_DATA_HOME:-$HOME/.local/share}" "${XDG_CACHE_HOME:-$HOME/.cache}"; do
    [[ -d $d/ll && ! -e $d/lealand ]] && mv "$d/ll" "$d/lealand"
done
mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}/lealand"

if grep -q _ll_visit "$HOME/.zshrc" 2>/dev/null; then
    ok "the ll function is in ~/.zshrc"
else
    printf '\n  ~/.zshrc needs the ll function, or ll can'\''t cd your shell. Either run\n'
    printf '  bash ~/utils/dotfiles/install.sh, or paste this block at the end of ~/.zshrc:\n\n'
    sed -n '/^# lealand (~/,$p' "$HERE/../dotfiles/home/zshrc" | sed 's/^/    /'
    printf '\n'
fi
echo "Done. Open a new terminal and type ll."

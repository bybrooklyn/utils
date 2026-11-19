#!/usr/bin/env bash
# Links my dotfiles into place:  bash ~/utils/dotfiles/install.sh
# Each file becomes a symlink into this folder, so editing it edits the repo copy.
# Anything already there is moved aside to <name>.bak-<date> first, never deleted.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
STAMP=$(date +%Y%m%d-%H%M%S)

# repo path -> where it goes
LINKS=(
    "home/zshrc                               $HOME/.zshrc"
    "home/p10k.zsh                            $HOME/.p10k.zsh"
    "config/fish/config.fish                  $HOME/.config/fish/config.fish"
    "config/alacritty/alacritty.toml          $HOME/.config/alacritty/alacritty.toml"
    "config/btop/btop.conf                    $HOME/.config/btop/btop.conf"
    "config/fastfetch/config.jsonc            $HOME/.config/fastfetch/config.jsonc"
    "config/micro/settings.json               $HOME/.config/micro/settings.json"
    "config/micro/colorschemes/catppuccin-macchiato.micro  $HOME/.config/micro/colorschemes/catppuccin-macchiato.micro"
)

for entry in "${LINKS[@]}"; do
    read -r src dest <<<"$entry"
    src=$HERE/$src
    if [[ -L $dest && $(readlink -f "$dest") == "$(readlink -f "$src")" ]]; then
        echo "  ✓ $dest"
        continue
    fi
    mkdir -p "$(dirname "$dest")"
    if [[ -e $dest || -L $dest ]]; then
        mv "$dest" "$dest.bak-$STAMP"
        echo "  ~ $dest (old one kept as ${dest##*/}.bak-$STAMP)"
    else
        echo "  + $dest"
    fi
    ln -s "$src" "$dest"
done
echo "Done. Open a new terminal to see the shell changes."

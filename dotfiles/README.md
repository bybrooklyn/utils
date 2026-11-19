# dotfiles

My zsh setup and a few app configs. The zsh part is built on top of CachyOS's own zsh config, so on another distro the first `source` line in `zshrc` won't find anything.

| File | For |
|---|---|
| `home/zshrc` | zsh: CachyOS config, Powerlevel10k, bun, PATH, the Android SDK and NDK |
| `home/zprofile` | zsh login shells: puts `~/.local/bin` on PATH |
| `home/p10k.zsh` | my Powerlevel10k prompt |
| `config/btop/btop.conf` | btop |
| `config/fastfetch/config.jsonc` | fastfetch with the CachyOS logo in purple and teal |
| `config/micro/` | micro with Catppuccin Macchiato |

## Install

```sh
git clone https://github.com/bybrooklyn/utils ~/utils
bash ~/utils/dotfiles/install.sh
```

It replaces each file with a symlink into this folder, so changing a setting changes the repo copy and `git diff` shows it. Whatever was there before gets moved to `<name>.bak-<date>`, nothing is deleted.

You need the programs themselves: `cachyos-zsh-config` (which brings Powerlevel10k), `btop`, `fastfetch`, `micro`.

General stuff I use every day on my PC and my home server. Each folder is its own thing with its own README and its own installer, so take whatever you want and ignore the rest.

| Folder | What it is |
|---|---|
| [pirate](pirate/) | qBittorrent inside a Proton VPN tunnel, and nothing else on the VPN. Port forwarding, server failover, no leaks. |
| [vfios](vfios/) | Keeps every Cloudflare DNS record that points at home on your home's current IP, across all your domains. |
| [quest-wireless-adb](quest-wireless-adb/) | Turns wireless adb back on every time a Meta Quest reboots. |
| [tweaks](tweaks/) | Small system fixes for a fresh install: USB wakeup off, swappiness, uinput, a Proton Drive mount. |
| [dotfiles](dotfiles/) | My zsh, fish, prompt, terminal and editor configs. |

## How to use it

Everything here is built for my setup: CachyOS (Arch) with KDE Plasma on Wayland for the PC, Fedora for the server. Most of it works on other distros, and each README says when something won't.

1. Clone it. Most installers expect it at `~/utils`.

   ```sh
   git clone https://github.com/bybrooklyn/utils ~/utils
   ```

2. Read the README in the folder you want. They say what it does, what it needs and how to undo it.

3. Run that folder's installer as your normal user, not root. They ask for sudo themselves when they need it, and none of them delete anything without asking.

   ```sh
   bash ~/utils/pirate/install.sh
   bash ~/utils/vfios/install.sh               # on the machine your DNS should follow
   bash ~/utils/quest-wireless-adb/install.sh
   bash ~/utils/tweaks/apply.sh
   bash ~/utils/dotfiles/install.sh
   ```

4. To update later, `git -C ~/utils pull` and run the installer again. They're all safe to run twice.

pirate also has a one-line install that does step 1 for you:

```sh
curl -fsSL https://raw.githubusercontent.com/bybrooklyn/utils/main/pirate/install.sh | bash
```

Claude Opus wrote most of the code in here. I decide what it does and test it on my own machines.

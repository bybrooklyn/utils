# pirate

i pirate and im proud of it.

pirate runs qBittorrent inside a Proton VPN tunnel on Linux. Only qBittorrent. Your browser, games, Discord, Tailscale and Docker keep using your normal connection.

I made it because Proton's Linux app couldn't do this. Its "split tunneling" still pushed all my traffic through the VPN. Its settings didn't stick, and it won't run without someone logged in to click it. So pirate skips the app entirely and uses a plain WireGuard config.

Claude Opus wrote the code.

## What you get

- **Only qBittorrent is on the VPN.** It lives in its own network namespace, a little box with exactly two network interfaces: loopback and the tunnel. Everything else on your computer never sees the VPN.
- **It can't leak.** That box has no route out except the tunnel. If WireGuard dies, qBittorrent has no internet at all.
- **No DNS leaks either.** qBittorrent asks Proton's DNS (10.2.0.1) and can't see your system's resolver.
- **Port forwarding that stays up.** pirate asks Proton for a forwarded port every 45 seconds and tells qBittorrent which one it got. That's the difference between downloading at full speed and sitting there "firewalled" with three peers.
- **It switches servers on its own** when one stops answering or refuses port forwarding.
- **It's always on.** The tunnel comes up at boot. qBittorrent starts when you log in, from the app menu, and from magnet links, always inside the tunnel.
- **It kills qBittorrent if you start it wrong.** Launch `/usr/bin/qbittorrent` from a terminal and it runs outside the VPN, so pirate closes it within about 20 seconds and tells you why. If it hasn't closed a minute later, pirate kills it. `qbittorrent-nox` gets the same treatment.
- **It sends desktop notifications** when something is actually wrong, like an expired config.
- **It keeps its own log.** Every port change, server switch and killed qBittorrent goes in `/var/lib/pirate/events.log`, and `pirate logs` shows it.
- **No Proton app needed to connect, no login.** One WireGuard key works on every Proton server.

## What you need

- **Linux with systemd and systemd-resolved.** Ubuntu and Fedora have systemd-resolved on already. Plain Debian doesn't: `sudo apt install systemd-resolved`, then reboot.
- **A desktop, Wayland or X11.** I run KDE Plasma 6 on Wayland. GNOME and the rest should work too, but nobody has tried them yet.
- **pacman, apt or dnf**, so pirate can install what it needs. I run it on CachyOS with qBittorrent 5.3. The package names are checked on Debian 13, Ubuntu 24.04 and Fedora 42.
- **A paid Proton VPN plan.** The free plan has no P2P servers and no port forwarding.

## Install

Paste this in a terminal:

```sh
curl -fsSL https://raw.githubusercontent.com/bybrooklyn/utils/main/pirate/install.sh | bash
```

Don't trust random curl-pipe-bash commands? Good. Read [install.sh](install.sh) first, or clone it and run it yourself:

```sh
git clone https://github.com/bybrooklyn/utils ~/utils
bash ~/utils/pirate/install.sh
```

The installer:

1. checks you're on Linux with systemd, systemd-resolved, a desktop session and a WireGuard-capable kernel, and tells you plainly if you're not
2. installs git and zsh with pacman, apt or dnf if you don't have them
3. clones this repo to `~/utils` (or updates it) and links `pirate` into `~/.local/bin`
4. walks you through getting a WireGuard config from Proton, opens the page for you, and waits for the file to land in Downloads
5. runs `pirate`, which does the actual setup

It asks for your sudo password when it needs it. Nothing else runs as root.

### Getting the Proton config

The installer walks you through this, but here it is if you want to do it yourself:

1. Go to [account.protonvpn.com/downloads](https://account.protonvpn.com/downloads) and scroll to **WireGuard configuration**.
2. Give it a name and pick platform **GNU/Linux**.
3. Turn **NAT-PMP (Port Forwarding) ON.** This is the one that matters. Without it you'll download like shit and barely seed. pirate refuses a config with it off unless you pass `--force`.
4. Leave NetShield and Moderate NAT off.
5. Pick a server with the **P2P** icon (two arrows). Somewhere close to you is fastest.
6. Click **Create**, then **Download**. It goes to `~/Downloads`.

Then run `pirate`. It finds the newest Proton config in Downloads by itself.

### What setup does

`pirate` with no arguments sets up or repairs everything. It's safe to run as many times as you want.

1. installs missing packages with pacman, apt or dnf
2. copies the config to `/etc/pirate/wg.conf`, readable by root only, and offers to delete the copy in Downloads since your private key is in it
3. installs its helpers and systemd units, then starts the tunnel
4. **runs leak tests before it touches qBittorrent.** If any test fails, it stops there and qBittorrent is left alone.
5. closes qBittorrent if it's open (it saves your torrents first) and changes two things in its settings file: it's bound to the tunnel interface, and it gets a Web UI on 127.0.0.1 inside the tunnel with a random password. Everything else (the save folder, the forwarded port, its own UPnP off) pirate sets through that Web UI while it runs, so a qBittorrent update that moves its settings around doesn't break anything. If nothing needs changing, qBittorrent is left running.
6. makes the app menu entry, magnet links and login all go through pirate. On KDE it also stops Plasma from restoring a qBittorrent outside the VPN when you log in.
7. turns on a weekly check that keeps the server list fresh
8. starts qBittorrent and shows `pirate status`

Your old qBittorrent settings are backed up to `~/.config/qBittorrent/qBittorrent.conf.bak-pirate-v3`.

## Using it

Open qBittorrent from the app menu like you always did. Magnet links work too. That's it.

| Command | What it does |
|---|---|
| `pirate` | set up or repair everything |
| `pirate status` | is the VPN up, is the port forwarded, is qBittorrent inside |
| `pirate doctor` | status, plus every leak test (sudo) |
| `pirate open [magnet]` | open qBittorrent inside the VPN (what the app menu runs) |
| `pirate server NL` | prefer a country |
| `pirate server US-NY#486` | pin one server |
| `pirate server list [CC]` | the best P2P servers, or the best in one country |
| `pirate server auto` | back to automatic |
| `pirate server refresh` | get a fresh server list from the Proton app |
| `pirate reconnect` | fresh handshake with the server you're on |
| `pirate config FILE` | install a new or renewed WireGuard config |
| `pirate start`, `stop`, `restart [--hard]` | control the services, no password needed |
| `pirate logs` | the last 50 things it did, then live. `--journal` for the full system log. |
| `pirate uninstall` | remove all of it |

A healthy `pirate status` looks like this:

```
pirate status
  ✓ VPN namespace up (starts at boot)
  ✓ VPN: US-NY#177 · 146.70.202.190 · port 55101
  ✓ last WireGuard handshake 15s ago
  ✓ qBittorrent running inside the VPN (its only interfaces: lo, wgpirate)
  ✓ qBittorrent listening on forwarded port 55101, incoming peers OK
  ✓ WireGuard config from US-NJ#163, imported 2026-09-14
```

### Settings

`~/.config/pirate/config`:

| Setting | Default | |
|---|---|---|
| `DOWNLOAD_DIR` | `~/Torrents` | where downloads go. Change it whenever, qBittorrent picks it up within 15 seconds. |
| `COUNTRY` | the config's country | preferred server country, set by `pirate server CC` |
| `SERVER` | automatic | a pinned server, set by `pirate server NAME` |
| `KILL_HOST_QBT` | `yes` | close any qBittorrent started outside the VPN. `no` only warns. |
| `FORWARD` | `bwrap` | how magnet links get handed to qBittorrent. `bwrap` does it from a process with no network at all. Set it to `plain` and it stays that way. |

## Automatic server switching

pirate switches servers when the one you're on stops answering for a minute, or refuses port forwarding four times in a row. A server that failed gets skipped for 30 minutes.

To know which servers exist, it reads the server list the Proton VPN app keeps at `~/.cache/Proton/VPN/serverlist.json`. If you've never had the app, there's no list and pirate just stays on your config's server. To fix that, install the Proton app, sign in once, and run `pirate server refresh`. Don't leave the app connected while pirate runs, they fight.

The app only updates that list when it's open, and Proton's API only talks to Proton's own apps. So `pirate server refresh` opens the Proton app for a few seconds, waits for it to update its list, closes it again and installs the new list. A weekly timer does the same by itself once the list is a month old, so you'll see the app flash up about once a month. `pirate status` warns if the list ever gets past 60 days.

## Renewing the config

Proton WireGuard configs expire after about a year. `pirate status` starts warning 30 days before. When it runs out you get a notification saying no server answers your key.

Hit **Extend** on the config in Proton's dashboard, or make a new one the same way as before, then:

```sh
pirate config ~/Downloads/your-new-config.conf
```

## When something's wrong

- **Run `pirate` first.** It repairs almost everything.
- **`pirate doctor`** runs every leak test and every check and tells you which one failed.
- **"VPN config expired?"** Your config ran out, or the network you're on blocks UDP port 51820 (some school and work Wi-Fi does). pirate can't tell those two apart.
- **"port forwarding refused"** on three servers means your config was made with NAT-PMP off. Make a new one with it on.
- **"stopped unprotected qBittorrent"** means you started qBittorrent some way other than the app menu or `pirate open`. Use those.
- **qBittorrent didn't open at login.** Check `pirate status`. If the tunnel was down when you logged in, pirate won't start qBittorrent outside it. `pirate start` fixes it.
- **"server list not refreshed"** means the Proton app opened but didn't update its list, usually because it's signed out. Open it, sign in, run `pirate server refresh`.
- **`pirate logs`** shows what it did recently and what it's doing now.

## Uninstall

```sh
pirate uninstall
```

It removes the services, the namespace, the config in `/etc/pirate` and the app menu override, and offers to put your old qBittorrent settings back. If you say no, it unbinds qBittorrent from the tunnel so it works on your normal connection again. Then delete `~/utils` if you want.

## How it works

```
             host network: browser, games, Tailscale, Docker, DNS → your ISP
┌──────────────────────────────────────────────────────────────────────────┐
│  network namespace "pirate":  lo + wgpirate, nothing else                │
│      qBittorrent  ──►  wgpirate  ──►  Proton P2P server (NAT-PMP)        │
└──────────────────────────────────────────────────────────────────────────┘
       WireGuard's encrypted UDP leaves through your normal connection
```

The WireGuard interface is created in your normal network first and then moved into the namespace. Its encrypted packets still go out your real connection, while everything that goes *into* it only exists inside the namespace. IPv6 is turned off in there. The tunnel's packets carry Tailscale's bypass mark, so it still works if you use a Tailscale exit node.

Four services do the work:

- `pirate-netns.service` builds the namespace and the tunnel at boot.
- `pirate-vpnd.service` checks everything every 15 seconds. It watches the handshake, renews the forwarded port, keeps qBittorrent's port, save folder and UPnP setting right through its Web UI, switches servers, notices suspend and network changes, and closes any qBittorrent running outside. If the namespace ever looks wrong it repairs it, or stops qBittorrent if it can't.
- `pirate-qbittorrent.service` runs qBittorrent as you, inside the namespace, on your desktop session (Wayland or X11). It checks the namespace and its own DNS before starting and refuses to run if anything is off.
- `pirate-servers.service` rebuilds the server list from the Proton app's cache, and checks every key, IP and name in it before root hands any of them to WireGuard.

Two user units run as you: `pirate-session.service` starts and stops qBittorrent with your desktop, and `pirate-servers-refresh.timer` is the weekly server list check. Desktops that don't use systemd's session target get an autostart entry instead. A polkit rule lets you start, stop and restart the services without a password and start the server list rebuild, and nothing else. Installing or changing them still needs sudo.

Root only ever runs copies in `/usr/local/lib/pirate` owned by root, never files in your home folder. The private key is stored root-only and only ever reaches `wg` through stdin, so it never shows up in a process list. qBittorrent's Web UI listens on 127.0.0.1 inside the namespace, where nothing else on your computer can reach it.

### Files

```
pirate/bin/pirate                    the command (zsh)
pirate/install.sh                    the installer
pirate/share/pirate-netns            namespace and WireGuard helper, leak tests   → /usr/local/lib/pirate/
pirate/share/pirate-vpnd             health, failover, port forwarding daemon     → /usr/local/lib/pirate/
pirate/share/pirate-*.service        system units                                 → /etc/systemd/system/
pirate/share/50-pirate.rules         polkit rule                                  → /etc/polkit-1/rules.d/
pirate/share/pirate-session.service  user unit, qBittorrent with your desktop     → ~/.config/systemd/user/
pirate/share/pirate-servers-refresh.*  user units, the weekly server list check   → ~/.config/systemd/user/
pirate/tests/run.sh                  the checks
```

### Packages

pirate installs what's missing itself with pacman, apt or dnf. Anywhere else, install whatever gives you these commands first: `wg`, `natpmpc`, `jq`, `curl`, `nsenter`, `setpriv`, `notify-send`, `bwrap`, `qbittorrent`, `python3`, `update-desktop-database` and `modinfo`, plus `git` and `zsh`.

## Tests

```sh
bash tests/run.sh
```

45 checks, about 15 seconds, no root and no network. They cover qBittorrent's settings in both the 5.2 and 5.3 layouts, reading Proton configs, building the server list, the settings file, what gets sent to qBittorrent, the event log, packages on each distro, Wayland and X11 sessions, the server refresh, and the guard killing a fake qBittorrent outside the VPN.

## Limits

- Only tested on KDE Plasma on Wayland. Other desktops and X11 are written for but untried.
- Without the Proton app there's no server list, so no automatic switching.
- An expired config and a network that blocks UDP 51820 look exactly the same from here.

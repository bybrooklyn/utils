# pirate

Run the **qBittorrent desktop app inside a Proton VPN WireGuard tunnel** on Linux — and only qBittorrent. It comes up at boot, keeps a NAT-PMP port forward alive, switches servers when one dies, and can't leak.

```
            host network (browser, games, Tailscale, Docker, DNS…) → your ISP
┌──────────────────────────────────────────────────────────────────────────┐
│  network namespace "pirate":  lo + wgpirate  (nothing else)              │
│      qBittorrent GUI  ──►  wgpirate  ──►  Proton P2P server (NAT-PMP)    │
└──────────────────────────────────────────────────────────────────────────┘
      WireGuard's encrypted UDP leaves through the host's normal connection
```

## Why a namespace?

- **Real split tunnel.** Only processes inside the namespace use the VPN; the rest of the system is untouched.
- **Built-in kill switch.** The namespace has no route except the tunnel. If WireGuard is down, qBittorrent has no network at all.
- **No DNS leaks.** qBittorrent resolves through Proton's DNS (10.2.0.1); systemd-resolved's socket is hidden from it.
- **No Proton app or login.** A static WireGuard key does everything; one key works on every Proton server.

The official Proton Linux CLI couldn't do this: its "split tunneling" still routes all system traffic through the VPN, its settings are volatile, and it isn't supported headless.

## What it does

- `pirate-netns.service` (boot): creates the `pirate` namespace and the `wgpirate` WireGuard interface from your Proton config.
- `pirate-vpnd.service` (boot): watches the handshake, renews the NAT-PMP port every 45 s, pushes the port into qBittorrent, fails over to other P2P servers (deduplicated by physical server), detects suspend/resume and network changes, stops any qBittorrent started *outside* the VPN, and sends desktop notifications for real problems (e.g. an expired config).
- `pirate-qbittorrent.service`: the qBittorrent GUI as your user, inside the namespace, on your Wayland session. Started at login, from the app menu, and by magnet links.
- `pirate`: one command to set up, repair and check everything.

## Requirements

- Linux with systemd, NetworkManager + systemd-resolved, **KDE Plasma (Wayland)** — tested on CachyOS/Arch, Plasma 6, qBittorrent 5.2
- A **paid Proton VPN plan**
- Packages: `wireguard-tools libnatpmp jq curl qbittorrent python bubblewrap libnotify kconfig desktop-file-utils` (`pirate` offers to install missing ones with pacman)
- Optional: the Proton VPN app, only for its cached server list (`~/.cache/Proton/VPN/serverlist.json`), which enables automatic server failover. Don't have it connected while pirate runs.

## Setup

1. In [account.protonvpn.com → Downloads → WireGuard configuration](https://account.protonvpn.com/downloads) create a config: platform **Linux**, **NAT-PMP (Port Forwarding) on**, Moderate NAT off, and pick a **P2P** server. Download the `.conf` (it lands in `~/Downloads`).
2. Clone and run:
   ```sh
   git clone https://github.com/bybrooklyn/pirate ~/pirate
   ln -s ~/pirate/bin/pirate ~/.local/bin/pirate   # somewhere on your PATH
   pirate
   ```
   `pirate` asks for sudo once. It imports the config into `/etc/pirate/wg.conf` (root-only, offers to delete the download), starts the tunnel, **runs leak tests before touching qBittorrent**, reconfigures qBittorrent (bound to `wgpirate`, WebUI on 127.0.0.1 inside the namespace), and hooks up the app menu, magnet links and login.

Proton WireGuard configs expire after about a year: use **Extend** in the dashboard (or download a new one) and run `pirate config <file>`.

## Commands

| Command | |
|---|---|
| `pirate` | set up or repair everything (safe to re-run) |
| `pirate status` | VPN, port forward and qBittorrent health |
| `pirate doctor` | status + full leak tests (sudo) |
| `pirate open [magnet]` | open qBittorrent inside the VPN (what the app menu uses) |
| `pirate server [CC\|NAME\|auto\|list [CC]]` | prefer a country or pin a server |
| `pirate reconnect` | fresh handshake with the current server |
| `pirate config FILE` | install a new/renewed WireGuard config |
| `pirate start \| stop \| restart [--hard]` | control the services (no sudo, via polkit) |
| `pirate logs` | follow the logs |
| `pirate uninstall` | remove everything |

Settings live in `~/.config/pirate/config`: `DOWNLOAD_DIR`, `COUNTRY`, `SERVER`, `KILL_HOST_QBT`, `FORWARD`.

## Layout

```
bin/pirate                     the CLI (zsh)
share/pirate-netns             namespace + WireGuard helper, leak tests      → /usr/local/lib/pirate/
share/pirate-vpnd              health / failover / NAT-PMP / qBittorrent daemon → /usr/local/lib/pirate/
share/pirate-*.service         system units (templated)                     → /etc/systemd/system/
share/50-pirate.rules          polkit: your user may start/stop these units → /etc/polkit-1/rules.d/
share/pirate-session.service   user unit: start/stop qBittorrent with the desktop session
```

Root runs only root-owned copies in `/usr/local/lib/pirate`, never files in your home directory.

## Security notes

- The private key is stored root-only and is only ever passed to `wg` through stdin.
- The polkit rule lets your user start/stop/restart the pirate units and nothing else; installing or changing them needs sudo.
- qBittorrent's WebUI listens on 127.0.0.1 *inside* the namespace, so nothing on the host can reach it.
- If you launch `/usr/bin/qbittorrent` directly from a terminal it runs outside the VPN; `pirate-vpnd` stops it within seconds (`KILL_HOST_QBT=yes`). Use the app menu or `pirate open`.

## Limitations

- KDE Plasma on Wayland only (session environment, autostart and session-restore handling).
- Without Proton's server cache there's no automatic failover; the config's own server is used.
- Expired config vs. a network blocking UDP 51820 can't be told apart — the notification mentions both.

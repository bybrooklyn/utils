# vfios

My server lives at home on Verizon Fios, and home IPs change whenever they feel like it. vfios keeps every Cloudflare DNS record that points at my house pointed at my house, across every domain I own, without me touching anything.

The name is just Verizon Fios. Claude Opus wrote it the day I switched over from Optimum, and it moved 31 records to the new IP before I'd even set up the router.

## What it does

Every 5 minutes it asks a few different websites what your public IP is, and only believes an answer when two of them agree. If the IP changed, it goes through **every zone** your Cloudflare token can see and moves every A record that held the old IP to the new one.

Records that point anywhere else are left alone: GitHub Pages, other servers, Workers routes, all of it. It matches on the old IP, not on names, so a new subdomain you point at home in the dashboard gets kept up to date without changing anything here.

When nothing changed it stays quiet, so its log only shows real moves.

## Install

On the machine whose IP your DNS should follow (my home server):

```sh
git clone https://github.com/bybrooklyn/utils ~/utils
bash ~/utils/vfios/install.sh
```

It asks for two things:

1. **A Cloudflare API token.** Make it at dash.cloudflare.com, My Profile, API Tokens, Create Token, Custom token. Give it **Zone > Zone > Read** and **Zone > DNS > Edit** on **All zones**. Set the expiry as far out as you like. It's stored at `~/.config/cf-ddns/token`, readable only by you.
2. **The IP your home records hold right now.** Usually that's your current public IP, which it offers as the default.

Then it turns on a systemd user timer and turns on linger, so it keeps running when nobody's logged in.

## Using it

| Command | What it does |
|---|---|
| `vfios` | last known IP, current public IP, token expiry, timer state |
| `vfios records` | every A record pointing at home, in every zone |
| `vfios now` | check and update right now instead of waiting |
| `vfios dry [IP]` | show what would change, optionally pretending your IP is IP |
| `vfios set IP` | move every home record to IP right now |
| `vfios seed IP` | tell it which IP the records hold, without changing DNS |
| `vfios log [N]` | the last N lines of its log |
| `vfios on`, `vfios off` | turn the 5-minute timer on or off |
| `vfios token` | swap in a new token. Paste it when asked, never on the command line. |

Run `vfios` from any other computer and it SSHes to the server and runs there. Set `VFIOS_HOST` if yours isn't `brook@server`.

## Files

```
~/cf-ddns/cf-ddns.py                  the script (stdlib Python, no packages)
~/.local/bin/vfios                    the command
~/.config/cf-ddns/token               your Cloudflare token (mode 600)
~/.config/cf-ddns/state.json          the last known home IP
~/.config/systemd/user/cf-ddns.*      the service and the 5-minute timer
```

To remove it: `vfios off`, delete those files, and revoke the token in Cloudflare.

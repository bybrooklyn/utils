#!/usr/bin/env python3
"""Keep every Cloudflare DNS record that points at home in step with home's public IPv4.

Runs on the server every 5 minutes (user timer `cf-ddns`). Each run:
  1. asks several independent services for the public IPv4 and needs two of them to agree;
  2. walks every zone the token can see and every A record in it;
  3. any A record whose address is the *last known home address* (state file) is moved to the
     new one. Records that point anywhere else (GitHub Pages, other servers, Workers' 100::)
     are never touched, so this is safe across all zones.

Records are matched by the old home address, not by name, so a new subdomain pointed at home
in the dashboard is picked up with no change here.

Usage:
  cf-ddns.py              update if the address changed (quiet when nothing to do)
  cf-ddns.py --dry-run    show what would change, change nothing
  cf-ddns.py --force      say so when the address is unchanged (instead of staying quiet)
  cf-ddns.py --seed IP    set the last known home address (first run, or after a manual fix)
  cf-ddns.py --ip IP      use IP as the new address instead of looking it up
  cf-ddns.py --list       list the A records that point at home
  cf-ddns.py --status     last known address, public address now, token expiry
(Day to day, use the `vfios` command, which wraps all of this.)

Token: ~/.config/cf-ddns/token (mode 600), needs Zone Read + DNS Edit on all zones.
State: ~/.config/cf-ddns/state.json
Hooks: ~/.config/cf-ddns/hooks.d/* (executables), run after the records move, with
       CF_DDNS_OLD_IP and CF_DDNS_NEW_IP set (e.g. restart a service that advertises the address).
"""
import argparse
import ipaddress
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import Counter

CONF = os.path.expanduser(os.environ.get("CF_DDNS_DIR", "~/.config/cf-ddns"))
TOKEN_FILE = os.path.join(CONF, "token")
STATE_FILE = os.path.join(CONF, "state.json")
HOOKS_DIR = os.path.join(CONF, "hooks.d")
API = "https://api.cloudflare.com/client/v4"
IP_SOURCES = [
    "https://api.ipify.org",
    "https://ipv4.icanhazip.com",
    "https://ifconfig.me/ip",
    "https://checkip.amazonaws.com",
    "https://1.1.1.1/cdn-cgi/trace",  # "ip=..." line
]


def log(msg):
    print(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}", flush=True)


def fetch(url, timeout=8):
    req = urllib.request.Request(url, headers={"User-Agent": "cf-ddns/1"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read().decode().strip()


def public_ipv4():
    seen = []
    for url in IP_SOURCES:
        try:
            body = fetch(url)
            if "ip=" in body:
                body = next(l[3:] for l in body.splitlines() if l.startswith("ip="))
            ip = ipaddress.ip_address(body)
            if ip.version == 4 and ip.is_global:
                seen.append(str(ip))
        except Exception:
            continue
        top = Counter(seen).most_common(1)
        if top and top[0][1] >= 2:
            return top[0][0]
    raise SystemExit(f"could not agree on the public IPv4 (answers: {seen or 'none'})")


def cf(method, path, token, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(API + path, data=data, method=method, headers={
        "Authorization": "Bearer " + token, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            out = json.load(r)
    except urllib.error.HTTPError as e:
        out = json.load(e)
    if not out.get("success"):
        raise RuntimeError(f"{method} {path}: {out.get('errors')}")
    return out


def paged(path, token):
    page = 1
    while True:
        sep = "&" if "?" in path else "?"
        out = cf("GET", f"{path}{sep}per_page=100&page={page}", token)
        yield from out["result"]
        info = out.get("result_info") or {}
        if page >= info.get("total_pages", 1):
            return
        page += 1


def load_state():
    try:
        with open(STATE_FILE) as f:
            return json.load(f)
    except FileNotFoundError:
        return {}


def save_state(state):
    os.makedirs(CONF, mode=0o700, exist_ok=True)
    tmp = STATE_FILE + ".tmp"
    with open(tmp, "w") as f:
        json.dump(state, f, indent=2)
    os.replace(tmp, STATE_FILE)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--seed", metavar="IP")
    ap.add_argument("--ip", metavar="IP", help="use this as the new address instead of looking it up")
    ap.add_argument("--list", action="store_true", help="list the A records that point at home")
    ap.add_argument("--status", action="store_true", help="last known address, public address, token expiry")
    a = ap.parse_args()

    state = load_state()
    if a.list or a.status:
        with open(TOKEN_FILE) as f:
            token = f.read().strip()
        old = state.get("home_ip", "?")
        if a.status:
            tok = cf("GET", "/user/tokens/verify", token)["result"]
            print(f"last known home address  {old}"
                  + (f"  (changed {state['changed_at']}, was {state['previous']})" if "changed_at" in state else ""))
            try:
                print(f"public address now       {public_ipv4()}")
            except SystemExit as e:
                print(f"public address now       unknown: {e}")
            print(f"token                    {tok['status']}, expires {tok.get('expires_on', 'never')}")
        if a.list:
            n = 0
            for zone in paged("/zones?status=active", token):
                for rec in paged(f"/zones/{zone['id']}/dns_records?type=A", token):
                    if rec["content"] == old:
                        n += 1
                        print(f"  {rec['name']:<50} {'proxied' if rec['proxied'] else 'dns-only'}")
            print(f"{n} records point at {old}")
        return
    if a.seed:
        ipaddress.IPv4Address(a.seed)
        state["home_ip"] = a.seed
        save_state(state)
        log(f"last known home address set to {a.seed}")
        return
    old = state.get("home_ip")
    if not old:
        raise SystemExit("no last known home address: run once with --seed <current A record address>")

    new = str(ipaddress.IPv4Address(a.ip)) if a.ip else public_ipv4()
    if new == old:
        if a.force or a.dry_run:
            print(f"home address unchanged ({new}); nothing to do")
        return  # stay quiet on timer runs so the journal only shows changes

    with open(TOKEN_FILE) as f:
        token = f.read().strip()

    log(f"home address {old} -> {new}{' (dry run)' if a.dry_run else ''}")
    moved, failed = 0, 0
    for zone in paged("/zones?status=active", token):
        for rec in paged(f"/zones/{zone['id']}/dns_records?type=A", token):
            if rec["content"] != old:
                continue
            log(f"  {rec['name']}: {rec['content']} -> {new}")
            if a.dry_run:
                continue
            try:
                cf("PATCH", f"/zones/{zone['id']}/dns_records/{rec['id']}", token, {"content": new})
                moved += 1
            except Exception as e:
                failed += 1
                log(f"  FAILED {rec['name']}: {e}")

    if a.dry_run:
        return
    if failed:
        # keep the old address so the next run retries the records that failed
        log(f"{moved} moved, {failed} failed; will retry")
        sys.exit(1)
    state.update(home_ip=new, changed_at=time.strftime("%Y-%m-%dT%H:%M:%S%z"), previous=old)
    save_state(state)
    log(f"{moved} records moved to {new}")
    run_hooks(old, new)


def run_hooks(old, new):
    """Run every executable in hooks.d (sorted) once the records have moved, with the old and new
    address in CF_DDNS_OLD_IP and CF_DDNS_NEW_IP. A failing hook is logged and does not stop the rest."""
    if not os.path.isdir(HOOKS_DIR):
        return
    env = dict(os.environ, CF_DDNS_OLD_IP=old or "", CF_DDNS_NEW_IP=new)
    for name in sorted(os.listdir(HOOKS_DIR)):
        path = os.path.join(HOOKS_DIR, name)
        if name.startswith(".") or not os.path.isfile(path) or not os.access(path, os.X_OK):
            continue
        try:
            r = subprocess.run([path], env=env, capture_output=True, text=True, timeout=300)
            out = (r.stdout + r.stderr).strip().replace("\n", " | ")
            log(f"hook {name}: exit {r.returncode}{': ' + out[:300] if out else ''}")
        except Exception as e:
            log(f"hook {name}: FAILED {e}")


if __name__ == "__main__":
    main()

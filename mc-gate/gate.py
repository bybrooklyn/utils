#!/usr/bin/env python3
"""mc-gate: one front door for all Minecraft servers run by Crafty.

Players connect to <name>.mc.bybrooklyn.dev:25565. mc-gate reads the hostname from the Minecraft
handshake and:
  - running server  -> pipes the connection to it inside the Docker network (crafty:<port>),
  - stopped server  -> answers the server list with "asleep, join to wake it"; a join asks Crafty to start
                       it and tells the player to rejoin in a minute,
  - empty for IDLE_MINUTES (except ALWAYS_ON) -> asks Crafty to stop it.
It also keeps every server on its own internal port (fixing server.properties while a server is stopped)
and notices new servers in Crafty, so a new server gets <name>.mc.bybrooklyn.dev with no setup.

Routes change live; nobody is disconnected when another server wakes or sleeps. Standard library only.
Config comes from environment variables (see CONFIG below).
"""
import asyncio, json, logging, os, re, ssl, struct, time, urllib.request

CONFIG = {
    "LISTEN": os.environ.get("LISTEN", "0.0.0.0:25565"),
    "DOMAIN": os.environ.get("DOMAIN", "mc.bybrooklyn.dev"),            # <name>.<DOMAIN>
    "DEFAULT_SERVER": os.environ.get("DEFAULT_SERVER", "Bwingle"),       # plain DOMAIN, IPs, unknown names
    "ALWAYS_ON": [s.strip() for s in os.environ.get("ALWAYS_ON", "Bwingle").split(",") if s.strip()],
    "IDLE_MINUTES": float(os.environ.get("IDLE_MINUTES", "30")),
    "CRAFTY": os.environ.get("CRAFTY", "https://crafty:8443"),
    "CRAFTY_TOKEN_FILE": os.environ.get("CRAFTY_TOKEN_FILE", "/secrets/crafty-token"),
    "BACKEND_HOST": os.environ.get("BACKEND_HOST", "crafty"),
    "SERVERS_DIR": os.environ.get("SERVERS_DIR", "/servers"),             # Crafty's servers folder, for ports
    "PORT_RANGE": os.environ.get("PORT_RANGE", "25565-25599"),
    "GOTIFY_URL": os.environ.get("GOTIFY_URL", ""),
    "GOTIFY_TOKEN_FILE": os.environ.get("GOTIFY_TOKEN_FILE", "/secrets/gotify-token"),
    "STATE_FILE": os.environ.get("STATE_FILE", "/data/state.json"),
    "POLL_SECONDS": float(os.environ.get("POLL_SECONDS", "10")),
}
log = logging.getLogger("mc-gate")
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

def slug(name):
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")

# ---------------------------------------------------------------- Crafty API

_ctx = ssl.create_default_context()
_ctx.check_hostname = False
_ctx.verify_mode = ssl.CERT_NONE   # Crafty's own self-signed certificate, inside the Docker network only

def crafty(method, path, body=None):
    token = open(CONFIG["CRAFTY_TOKEN_FILE"]).read().strip()
    req = urllib.request.Request(CONFIG["CRAFTY"] + "/api/v2" + path, method=method,
                                 data=json.dumps(body).encode() if body is not None else None,
                                 headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=15, context=_ctx) as r:
        return json.loads(r.read() or b"{}")

def notify(title, message):
    url, tf = CONFIG["GOTIFY_URL"], CONFIG["GOTIFY_TOKEN_FILE"]
    if not url or not os.path.exists(tf):
        return
    try:
        token = open(tf).read().strip()
        req = urllib.request.Request(url.rstrip("/") + "/message", method="POST",
                                     data=json.dumps({"title": title, "message": message, "priority": 5}).encode(),
                                     headers={"X-Gotify-Key": token, "Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=10).read()
    except Exception as e:
        log.warning("gotify: %s", e)

# ---------------------------------------------------------------- server table

class Server:
    def __init__(self, sid, name):
        self.id, self.name, self.slug = sid, name, slug(name)
        self.port, self.running, self.online, self.ready = None, False, 0, False
        self.starting_since = 0.0     # we asked Crafty to start it
        self.last_active = time.time()

servers = {}            # id -> Server
state = {"known": []}   # ids seen before (for "new server" messages)

def load_state():
    global state
    try:
        state = json.load(open(CONFIG["STATE_FILE"]))
    except Exception:
        pass

def save_state():
    os.makedirs(os.path.dirname(CONFIG["STATE_FILE"]), exist_ok=True)
    tmp = CONFIG["STATE_FILE"] + ".tmp"
    json.dump(state, open(tmp, "w"))
    os.replace(tmp, CONFIG["STATE_FILE"])

def find(host):
    """Server for a handshake hostname, or the default server."""
    host = host.split("\0")[0].rstrip(".").lower()      # Forge clients append \0FML...\0
    dom = CONFIG["DOMAIN"].lower()
    if host.endswith("." + dom):
        want = host[: -len(dom) - 1]
        for s in servers.values():
            if s.slug == want:
                return s
    for s in servers.values():
        if s.name == CONFIG["DEFAULT_SERVER"]:
            return s
    return None

def props_path(s):
    return os.path.join(CONFIG["SERVERS_DIR"], s.id, "server.properties")

def file_port(s):
    """The port the server really listens on: server-port in its server.properties (the source of truth)."""
    try:
        m = re.search(r"(?m)^server-port=(\d+)", open(props_path(s)).read())
        return int(m.group(1)) if m else 25565
    except OSError:
        return None

def set_port(s, port):
    """Give a stopped server its own internal port by editing its server.properties.

    Crafty's own "server port" field is only its display record and a limited API key may not change it,
    so mc-gate never relies on it: routing and player counts use the port from server.properties.
    """
    p = props_path(s)
    text = open(p).read()
    text = re.sub(r"(?m)^server-port=.*$", f"server-port={port}", text) if re.search(r"(?m)^server-port=", text) \
        else text + f"\nserver-port={port}\n"
    tmp = p + ".mc-gate"
    open(tmp, "w").write(text)
    os.replace(tmp, p)
    log.info("%s now uses internal port %d (set Crafty's 'Server Port' to match for its own stats)", s.name, port)
    s.port = port

def players_online(port):
    """Server-list ping to the backend; returns the online count, or None if it does not answer yet."""
    import socket
    try:
        with socket.create_connection((CONFIG["BACKEND_HOST"], port), timeout=2) as c:
            h = b"localhost"
            hs = varint(0) + varint(767) + varint(len(h)) + h + struct.pack(">H", port) + varint(1)
            c.sendall(varint(len(hs)) + hs + varint(1) + varint(0))
            c.settimeout(3)
            data = b""
            while True:
                chunk = c.recv(65536)
                if not chunk:
                    break
                data += chunk
                start = data.find(b"{")
                if start >= 0:
                    try:
                        return int(json.loads(data[start:].decode(errors="replace"))["players"]["online"])
                    except (ValueError, KeyError):
                        continue
    except Exception:
        return None
    return None

def poll_once():
    data = crafty("GET", "/servers").get("data", [])
    seen = set()
    for d in data:
        sid = d["server_id"]; seen.add(sid)
        s = servers.get(sid) or Server(sid, d["server_name"])
        s.name, s.slug = d["server_name"], slug(d["server_name"])
        servers[sid] = s
        try:
            st = crafty("GET", f"/servers/{sid}/stats").get("data", {})
        except Exception as e:
            log.warning("stats %s: %s", s.name, e)
            continue
        was = s.running
        s.running = bool(st.get("running"))
        s.port = file_port(s) or s.port
        if s.running and not was:
            s.last_active = time.time()
            log.info("%s is running", s.name)
        if s.running:
            count = players_online(s.port) if s.port else None
            # not answering yet (still booting) counts as active, so a booting server is never stopped
            s.online = 1 if count is None else count
            s.ready = count is not None
            if s.online:
                s.last_active = time.time()
            if s.ready:
                s.starting_since = 0.0
        else:
            s.online, s.ready = 0, False
            if s.starting_since and time.time() - s.starting_since > 300:
                # asked Crafty to start it 5 minutes ago and it is still not running: the start failed
                log.warning("%s did not start within 5 minutes; check it in Crafty", s.name)
                notify("Minecraft server did not start", f"{s.name} was woken up but did not start. Check it in Crafty.")
                s.starting_since = 0.0
        if sid not in state["known"]:
            state["known"].append(sid); save_state()
            addr = f"{s.slug}.{CONFIG['DOMAIN']}"
            log.info("new server %s -> %s", s.name, addr)
            notify("New Minecraft server", f"{s.name} is reachable at {addr}")
    for sid in list(servers):
        if sid not in seen:
            log.info("%s was removed from Crafty", servers.pop(sid).name)
    # every server its own internal port; only stopped servers get moved
    lo, hi = (int(x) for x in CONFIG["PORT_RANGE"].split("-"))
    used = {}
    for s in sorted(servers.values(), key=lambda x: (not x.running, x.name)):
        if s.port in used or not (lo <= (s.port or 0) <= hi):
            if s.running:
                continue
            taken = set(used) | {x.port for x in servers.values() if x is not s}
            free = next((p for p in range(lo, hi + 1) if p not in taken), None)
            if free is None:
                log.warning("no free internal port for %s", s.name)
                continue
            try:
                set_port(s, free)
            except Exception as e:
                log.warning("could not move %s to port %d: %s", s.name, free, e)
                continue
        used[s.port] = s
    # sleep servers that have been empty too long
    idle = CONFIG["IDLE_MINUTES"] * 60
    for s in servers.values():
        if s.running and s.name not in CONFIG["ALWAYS_ON"] and not s.online and time.time() - s.last_active > idle:
            log.info("%s empty for %d min, stopping", s.name, CONFIG["IDLE_MINUTES"])
            try:
                crafty("POST", f"/servers/{s.id}/action/stop_server")
                s.last_active = time.time()
            except Exception as e:
                log.warning("stop %s: %s", s.name, e)

async def poller():
    while True:
        try:
            await asyncio.to_thread(poll_once)
        except Exception as e:
            log.warning("poll: %s", e)
        await asyncio.sleep(CONFIG["POLL_SECONDS"])

# ---------------------------------------------------------------- Minecraft protocol bits

def varint(n):
    out = b""
    while True:
        b = n & 0x7F; n >>= 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out

async def read_varint(r):
    n = shift = 0
    for _ in range(5):
        b = (await r.readexactly(1))[0]
        n |= (b & 0x7F) << shift; shift += 7
        if not b & 0x80:
            return n
    raise ValueError("bad varint")

def parse_varint(buf, i):
    n = shift = 0
    while True:
        b = buf[i]; i += 1
        n |= (b & 0x7F) << shift; shift += 7
        if not b & 0x80:
            return n, i

def packet(pid, payload):
    body = varint(pid) + payload
    return varint(len(body)) + body

def mcstr(s):
    b = s.encode()
    return varint(len(b)) + b

async def read_packet(r):
    ln = await read_varint(r)
    if not 0 < ln < 1 << 21:
        raise ValueError("bad length")
    return await r.readexactly(ln)

# ---------------------------------------------------------------- connections

async def pipe(src, dst):
    try:
        while data := await src.read(65536):
            dst.write(data)
            await dst.drain()
    except Exception:
        pass
    finally:
        try:
            dst.close()
        except Exception:
            pass

async def handle(reader, writer):
    peer = writer.get_extra_info("peername")
    try:
        first = await asyncio.wait_for(reader.readexactly(1), 10)
        if first == b"\xfe":                      # pre-1.7 legacy ping: not supported
            return
        ln, shift, raw = first[0] & 0x7F, 7, first   # handshake length varint, kept raw to forward
        b = first[0]
        while b & 0x80:
            nb = await reader.readexactly(1); raw += nb; b = nb[0]
            ln |= (b & 0x7F) << shift; shift += 7
        payload = await asyncio.wait_for(reader.readexactly(ln), 10)
        handshake = raw + payload
        pid, i = parse_varint(payload, 0)
        if pid != 0:
            return
        proto, i = parse_varint(payload, i)
        hlen, i = parse_varint(payload, i)
        host = payload[i:i + hlen].decode(errors="replace"); i += hlen + 2
        next_state, i = parse_varint(payload, i)
        s = find(host)
        if not s:
            return
        if s.running and s.port:
            try:
                br, bw = await asyncio.wait_for(asyncio.open_connection(CONFIG["BACKEND_HOST"], s.port), 3)
            except Exception:
                br = None
            if br:
                bw.write(handshake)
                await bw.drain()
                await asyncio.gather(pipe(reader, bw), pipe(br, writer))
                return
        # asleep (or still booting)
        booting = s.running or s.starting_since
        if next_state == 1:
            await read_packet(reader)            # status request
            text = (f"{s.name} is starting, give it a minute." if booting
                    else f"{s.name} is asleep. Join to wake it up.")
            status = {"version": {"name": "asleep" if not booting else "starting", "protocol": proto},
                      "players": {"max": 0, "online": 0}, "description": {"text": text, "color": "gray"}}
            writer.write(packet(0, mcstr(json.dumps(status))))
            await writer.drain()
            try:
                ping = await asyncio.wait_for(read_packet(reader), 5)
                writer.write(varint(len(ping)) + ping)  # pong: same id 0x01 and payload
                await writer.drain()
            except Exception:
                pass
        else:
            if not booting:
                log.info("%s asleep; waking it for %s", s.name, peer[0] if peer else "?")
                s.starting_since = time.time()
                try:
                    await asyncio.to_thread(crafty, "POST", f"/servers/{s.id}/action/start_server")
                except Exception as e:
                    log.warning("start %s: %s", s.name, e)
            msg = {"text": f"{s.name} is waking up. Rejoin in about a minute.", "color": "yellow"}
            writer.write(packet(0, mcstr(json.dumps(msg))))   # login disconnect
            await writer.drain()
    except (asyncio.IncompleteReadError, asyncio.TimeoutError, ConnectionError, ValueError, IndexError):
        pass
    except Exception as e:
        log.warning("connection %s: %s", peer, e)
    finally:
        try:
            writer.close()
        except Exception:
            pass

async def main():
    load_state()
    host, port = CONFIG["LISTEN"].rsplit(":", 1)
    srv = await asyncio.start_server(handle, host, int(port))
    log.info("mc-gate listening on %s, domain %s, default %s, always on %s, idle %s min",
             CONFIG["LISTEN"], CONFIG["DOMAIN"], CONFIG["DEFAULT_SERVER"], CONFIG["ALWAYS_ON"], CONFIG["IDLE_MINUTES"])
    await asyncio.gather(srv.serve_forever(), poller())

if __name__ == "__main__":
    asyncio.run(main())

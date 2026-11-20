# mc-gate

One front door for all my Minecraft servers. They run in Crafty, and every one of them is reachable at its own name on port 25565, like `bwingle.mc.bybrooklyn.dev`. Nobody types a port.

Servers I don't need running all the time go to sleep when nobody's on, and wake up when someone tries to join. Before this I had infrared, plus a script with full access to Docker that rewrote infrared's config and restarted it, which kicked everyone off every server whenever anything changed. This replaces both.

## What it does

- **Routes by hostname.** It reads the server name from the Minecraft handshake and passes the connection straight to that server inside the Docker network. The plain domain, a bare IP, or any unknown name goes to the default server.
- **Sleeping servers answer anyway.** In the server list a stopped server shows "asleep. Join to wake it up." Joining asks Crafty to start it and tells the player to rejoin in about a minute. While it boots, the list says it's starting.
- **Empty servers go to sleep.** After 30 minutes with nobody on, it asks Crafty to stop the server. Servers in `ALWAYS_ON` are never stopped. It checks the player count itself with a server-list ping, so a server it can't reach yet is never treated as empty.
- **Every server gets its own internal port.** If two servers share a port it moves the stopped one to a free port in its `server.properties`. New servers made in Crafty get picked up automatically, and with Gotify set up it sends a message with the new address.
- **Routes change live.** Waking or sleeping one server never touches anyone on another.

It's one Python file using only the standard library.

## Setup

It runs as a container next to Crafty on the same Docker network. It needs:

- a Crafty API key with COMMANDS, LOGS, CONFIG and PLAYERS. Crafty only lets a key do what the user's roles allow, so give the user a role with those four on every server.
- Crafty's servers folder mounted at `/servers`, for the port fixes
- a wildcard DNS record `*.mc.yourdomain` pointing at the machine, and TCP 25565 forwarded to it

```yaml
  mc-gate:
    image: python:3.12-alpine
    command: ["python", "-u", "/app/gate.py"]
    ports: ["25565:25565/tcp"]
    environment:
      DOMAIN: "mc.bybrooklyn.dev"
      DEFAULT_SERVER: "Bwingle"
      ALWAYS_ON: "Bwingle"
      IDLE_MINUTES: "30"
      GOTIFY_URL: "http://192.168.1.2:8085"   # optional, with /secrets/gotify-token
    volumes:
      - ./mc-gate:/app:ro
      - ./mc-gate/data:/data
      - ./crafty-token:/secrets/crafty-token:ro
      - /opt/crafty/servers:/servers
    networks: [mc-routing]   # the same network as Crafty
```

Other settings: `CRAFTY` (default `https://crafty:8443`), `BACKEND_HOST` (default `crafty`), `PORT_RANGE` (default `25565-25599`), `POLL_SECONDS` (default 10).

Crafty keeps its own "Server Port" field for its stats page. A limited API key can't change it, so when mc-gate moves a server, set that field by hand if you want Crafty's player count to match. mc-gate doesn't rely on it.

# ups-guard

Acts on a UPS that [NUT](https://networkupstools.org/) watches. Runs on the machine the UPS is plugged into
(the "server") and on any PC on the same UPS.

## What happens

| when | server | PC |
|---|---|---|
| power goes out | nothing yet | lowest power (power-saver profile, GPU at its minimum power limit), a desktop notice, an alert for Claude Code sessions |
| battery at `SHED_AT` (90%) | stops the `SHED` containers, gives the `LOWPRIO` ones the lowest CPU weight | |
| battery at `OFF_AT` + 10% | | "powers off soon" notice |
| battery at `OFF_AT` (50%) | | 60 s countdown with a **Keep PC on** button, then power off |
| UPS low battery (10% or 10 min, set in NUT) | NUT's upsmon shuts it down cleanly | notice |
| power back | after 60 s of steady mains, starts what it stopped and puts CPU weights back | power settings back, "power is back" notice |

`ups-guard keep-on` keeps the PC on for this boot (`ups-guard allow-off` undoes it). `FORCE_OFF=no` in
`/etc/ups-guard/pc.conf` turns the power off off for good. `ups-guard status` shows what the UPS says.

## Install

On the server (Fedora, UPS on USB):

```sh
sudo ./nut-setup.sh                       # NUT: driver, upsd on port 3493, clean shutdown at 10% / 600 s
install -m755 ups-guard ~/.local/bin/
install -Dm644 ups-guard-server.service ~/.config/systemd/user/ups-guard.service
$EDITOR ~/.config/ups-guard/server.conf   # SHED=..., LOWPRIO=..., SHED_AT=90
systemctl --user enable --now ups-guard   # needs linger for running without a login: loginctl enable-linger
```

On a PC:

```sh
sudo install -Dm755 ups-guard /usr/local/bin/ups-guard
sudo install -Dm644 ups-guard-pc.service /etc/systemd/system/ups-guard.service
printf 'UPS=ups@server\nOFF_AT=50\n' | sudo install -Dm644 /dev/stdin /etc/ups-guard/pc.conf
sudo systemctl enable --now ups-guard
```

## Claude Code

While the UPS is on battery the PC service writes `/run/ups-guard/alert`. `ups-guard claude-hook`, set as a
`UserPromptSubmit` and `PostToolUse` hook in `~/.claude/settings.json`, tells each session about it once per
change (power out, services shed, PC powering off, power back), as context for the model and a line for you:

```json
"hooks": {
  "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "\"$HOME/.local/bin/ups-guard\" claude-hook 2>/dev/null || true", "timeout": 5 }] }],
  "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "\"$HOME/.local/bin/ups-guard\" claude-hook 2>/dev/null || true", "timeout": 5 }] }]
}
```

## Config keys

Server (`~/.config/ups-guard/server.conf`): `UPS` (ups@localhost), `SHED_AT` (90), `SHED`, `LOWPRIO`,
`RESTORE_AFTER` (60 s), `POLL` (10 s), `USABLE_WH` (0), `STATE` (~/.local/state/ups-guard/shed.json).

PC (`/etc/ups-guard/pc.conf`): `UPS` (ups@server), `USER` (owner of uid 1000), `OFF_AT` (50), `FORCE_OFF` (yes),
`COUNTDOWN` (60 s), `POWER_SAVE` (yes), `POLL` (10 s), `USABLE_WH` (0).

`USABLE_WH`: many small UPSes report no load over USB. Set it to the battery energy the UPS can actually
deliver (roughly its rated watts times its rated runtime at that load) and ups-guard shows an estimated load
worked out from the runtime the UPS reports. Calibrate it against the UPS's own display.

It speaks upsd's text protocol itself, so a PC needs nothing installed but Python.

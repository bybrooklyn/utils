# tweaks

Little system fixes I made by hand on my PC (CachyOS, KDE Plasma). They're here so a fresh install gets them back in one go instead of me remembering them one at a time.

| File | What it does |
|---|---|
| `system/disable-usb-device-wakeup.service` | Stops USB devices from waking the computer up. Without it, bumping the mouse wakes it from sleep. |
| `system/99-zz-swappiness.conf` | `vm.swappiness = 10`, so it only swaps when memory is actually tight. |
| `system/80-uinput.rules` | Lets the `input` group use `/dev/uinput`, which tools like ydotool need to fake key presses. |
| `user/proton-drive.service` | Mounts Proton Drive at `~/Proton` with rclone, with a 20 GB local cache. Needs a remote called `proton` set up with `rclone config` first. Off unless you start it. |

## Apply

```sh
git clone https://github.com/bybrooklyn/utils ~/utils
bash ~/utils/tweaks/apply.sh
```

It shows what's different from what's on your system and asks before changing anything. Run it again whenever, it skips what's already in place.

For the uinput rule to matter, you need to be in the `input` group: `sudo usermod -aG input $USER`, then log out and back in.

# quest-wireless-adb

The Quest forgets wireless adb every time it reboots, which means digging out a USB cable and typing `adb tcpip 5555` again. This does that for you.

Plug the headset in over USB and within a minute it turns wireless adb back on and connects to it over Wi-Fi. You can unplug it after that.

While the headset is charging it also keeps it awake, so it doesn't fall asleep in the middle of a build. Unplugged, it's left alone to sleep and save its battery.

It only touches devices whose model starts with `Quest`, so your phone is safe.

## Install

Needs `adb` (on Arch: `android-tools`).

```sh
git clone https://github.com/bybrooklyn/utils ~/utils
bash ~/utils/quest-wireless-adb/install.sh
```

It runs as a systemd user service and checks once a minute. To stop it:

```sh
systemctl --user disable --now quest-wireless-adb
```

#!/bin/bash
# Set up NUT on a Fedora machine with a USB HID UPS: driver, upsd for the LAN/tailnet, and upsmon to shut
# this machine down cleanly at low battery. Run with sudo. Safe to run again (rewrites the same files).
#
#   sudo ./nut-setup.sh                 defaults: shut down at 10% or 600 s of runtime left
#   sudo LOW_CHARGE=20 LOW_RUNTIME=300 ./nut-setup.sh
set -euo pipefail
[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }

UPS_NAME=${UPS_NAME:-ups}
LOW_CHARGE=${LOW_CHARGE:-10}
LOW_RUNTIME=${LOW_RUNTIME:-600}
FIREWALL_ZONE=${FIREWALL_ZONE:-$(firewall-cmd --get-default-zone 2>/dev/null || true)}

dnf install -y nut nut-client

# Ask the UPS which vendor/product it is so the driver binds to it and nothing else.
read -r VID PID < <(lsusb | awk '/Phoenixtec|UPS|Smart-Battery|CyberPower|American Power|Eaton|Tripp/ && !f {f=1; split($6,a,":"); print a[1], a[2]}')
[ -n "${VID:-}" ] || { echo "no USB UPS found in lsusb"; exit 1; }
echo "UPS on USB: $VID:$PID"

MONPASS=$(grep -oP '^\s*password\s*=\s*\K\S+' /etc/ups/upsd.users 2>/dev/null | head -1 || true)
[ -n "$MONPASS" ] || MONPASS=$(head -c 32 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | cut -c1-24)  # no SIGPIPE under pipefail

cat > /etc/ups/nut.conf <<EOF
MODE=netserver
EOF

cat > /etc/ups/ups.conf <<EOF
maxretry = 3

[$UPS_NAME]
  driver = usbhid-ups
  port = auto
  vendorid = $VID
  productid = $PID
  desc = "UPS"
  pollinterval = 5
  # Decide low battery here instead of trusting the UPS: LB at ${LOW_CHARGE}% or ${LOW_RUNTIME}s left.
  ignorelb
  override.battery.charge.low = $LOW_CHARGE
  override.battery.runtime.low = $LOW_RUNTIME
EOF

cat > /etc/ups/upsd.conf <<EOF
# Read-only status for every machine the firewall lets in (LAN and tailnet); changes need a login.
LISTEN 0.0.0.0 3493
LISTEN :: 3493
EOF

cat > /etc/ups/upsd.users <<EOF
[upsmon]
  password = $MONPASS
  upsmon primary
EOF

cat > /etc/ups/upsmon.conf <<EOF
MONITOR $UPS_NAME@localhost 1 upsmon $MONPASS primary
MINSUPPLIES 1
SHUTDOWNCMD "/sbin/shutdown -h +0"
POLLFREQ 5
POLLFREQALERT 5
HOSTSYNC 15
DEADTIME 15
FINALDELAY 5
POWERDOWNFLAG /etc/killpower
# Some UPSes flash LB with a near-zero runtime for a few seconds at the switch to battery; only a low battery
# that lasts 30 s counts, so that glitch cannot shut this machine down at full charge.
OBLBDURATION 30
EOF

chown root:nut /etc/ups/upsd.users /etc/ups/upsmon.conf /etc/ups/ups.conf /etc/ups/upsd.conf
chmod 640 /etc/ups/upsd.users /etc/ups/upsmon.conf

# Let the nut user open the device even if the packaged udev rules do not list it.
cat > /etc/udev/rules.d/90-ups-nut.rules <<EOF
SUBSYSTEM=="usb", ATTR{idVendor}=="$VID", ATTR{idProduct}=="$PID", MODE="0664", GROUP="nut"
EOF
udevadm control --reload-rules
udevadm trigger --subsystem-match=usb --attr-match=idVendor="$VID"

if [ -n "$FIREWALL_ZONE" ]; then
  firewall-cmd --permanent --zone="$FIREWALL_ZONE" --add-port=3493/tcp
  firewall-cmd --reload
fi

upsdrvsvcctl resync || true
systemctl enable --now nut-driver-enumerator.service nut-driver.target nut-server.service nut-monitor.service nut.target
systemctl restart nut-driver.target nut-server.service nut-monitor.service
sleep 5
upsc "$UPS_NAME@localhost" | grep -E '^(device|ups\.(status|load)|battery\.(charge|runtime))' || { echo "driver not answering yet; see: journalctl -u 'nut-driver@*'"; exit 1; }
echo "NUT is up: shutdown at ${LOW_CHARGE}% or ${LOW_RUNTIME}s left."

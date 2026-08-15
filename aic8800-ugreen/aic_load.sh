#!/bin/bash
K=/lib/modules/5.10.0+/kernel
AIC=$K/drivers/net/wireless/aic8800

[ $EUID -ne 0 ] && { echo "sudo bash $0"; exit 1; }

echo "[1/4] cfg80211"
lsmod | grep -q cfg80211 || insmod $K/net/wireless/cfg80211.ko || { echo FAIL-cfg80211; exit 1; }

echo "[2/4] aic_load_fw"
lsmod | grep -q aic_load_fw || insmod $AIC/aic_load_fw.ko || { echo FAIL-load_fw; exit 1; }
sleep 3

echo "[3/4] aic8800_fdrv"
lsmod | grep -q aic8800_fdrv || insmod $AIC/aic8800_fdrv.ko || { echo FAIL-fdrv; exit 1; }

echo "[4/4] bind if needed"
IFACE=$(ls /sys/bus/usb/devices/ | grep "^3-1.3:" | head -1)
if [ -n "$IFACE" ] && [ ! -e "/sys/bus/usb/devices/$IFACE/driver" ]; then
    echo "$IFACE" > /sys/bus/usb/drivers/aic8800_fdrv/bind 2>/dev/null && echo "bound $IFACE" || echo "bind failed"
fi

sleep 8
echo "=== result ==="
ls /sys/class/net/ | grep -iE "wlan|wlx" && echo WLAN_OK || echo no_wlan
dmesg | grep -iE "chipmatch|USE AIC|interface add|New interface" | tail -5

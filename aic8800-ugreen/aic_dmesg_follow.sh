#!/bin/bash
# AIC8800 崩溃现场捕获: 增量追加 dmesg, panic 重启后历史不丢
LOG=/home/lwboy/aic8800/dmesg_follow.log
LAST_LEN=0
while true; do
    CUR=$(dmesg 2>/dev/null | wc -c)
    if [ "$CUR" -lt "$LAST_LEN" ]; then
        # dmesg buffer 被清(重启), 重新从头记录
        echo "=== DMESG BUFFER RESET (reboot?) $(date) ===" >> "$LOG"
        LAST_LEN=0
    fi
    if [ "$CUR" -gt "$LAST_LEN" ]; then
        dmesg 2>/dev/null | tail -c +$((LAST_LEN + 1)) >> "$LOG"
        LAST_LEN=$CUR
    fi
    sleep 0.2
done
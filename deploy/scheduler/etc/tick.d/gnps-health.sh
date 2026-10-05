#!/usr/bin/env bash
# gnps-health.sh — tick.d 组件 (服务端): gnps 服务非 active 则拉起
# 约定: 失败必须非零退出 (tick.sh 落 WARN); 缺席=静默。
# 客户端机器没有这个组件 (etc/tick.d/ 空) —— 设计行为, 不告警。
set -u

if [ "$(systemctl is-active gnps 2>/dev/null)" = "active" ]; then
    echo "OK   gnps active"
    exit 0
fi

echo "WARN gnps 非 active ($(systemctl is-active gnps 2>&1 || true)), 尝试启动"
systemctl start gnps >/dev/null 2>&1
sleep 2
if [ "$(systemctl is-active gnps 2>/dev/null)" = "active" ]; then
    echo "OK   gnps 已拉起"
    exit 0
fi
echo "WARN gnps 启动失败"
exit 1

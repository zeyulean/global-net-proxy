#!/usr/bin/env bash
# tick.sh — gnp 统一调度入口。全部机器同一份; cron/launchd 只调它。
# 约定: 组件缺席=静默跳过(设计行为); 组件失败=落 WARN(不允许无声)。
# 兼容 Mac bash 3.2: 不用关联数组/${var,,}; 不用 jq。
BASE="${GNP_HOME:-$HOME/.local/gnp}"          # 服务端部署时导出 GNP_HOME=/opt/gnp
LOG="$BASE/var/tick.log"
mkdir -p "$BASE/var"
touch "$LOG"

# 日志上限 ~1MB: 截断保后半
if [ "$(wc -c < "$LOG")" -gt 1048576 ]; then
    tail -c 524288 "$LOG" > "$LOG.tmp" && mv "$LOG.tmp" "$LOG"
fi

echo "== tick $(date '+%F %T') =="

# 内核组件 1: 客户端看门狗 (gnpc 在才跑; 服务端机器自然跳过)
# 必须 if/else 判缺席: `[ -x ] && cmd || WARN` 分不清缺席与失败 —— 服务端无 gnpc
# 会每分钟刷一条 WARN (违反 §7.1 验收 3 无持续 WARN)。
if [ -x "$BASE/bin/gnpc" ]; then
    "$BASE/bin/gnpc" guard >>"$LOG" 2>&1 \
        || echo "WARN gnpc guard 非零退出" >>"$LOG"
fi

# 内核组件 2: 规则集日更 (04 点窗口 + 当日标记防重)
if [ -x "$BASE/bin/gnpc" ] && [ "$(date +%H)" = "04" ] \
   && [ ! -e "$BASE/var/.rules-updated-$(date +%F)" ]; then
    "$BASE/bin/gnpc" rules-update >>"$LOG" 2>&1 \
        && touch "$BASE/var/.rules-updated-$(date +%F)" \
        || echo "WARN rules-update 失败" >>"$LOG"
fi

# 插件组件: 存在即参与; 缺失=静默
for t in "$BASE"/etc/tick.d/*.sh; do
    [ -x "$t" ] || continue
    bash "$t" >>"$LOG" 2>&1 || echo "WARN tick.d 失败: $t" >>"$LOG"
done

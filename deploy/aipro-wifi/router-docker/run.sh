#!/usr/bin/env bash
# run.sh — 在 aipro 上运行 aipro-wifi-router 容器
# 用法（aipro 上，本目录内）：
#   sudo bash run.sh            # 镜像不存在才构建; 否则直接重建容器 (plan §6: 镜像不重建)
#   sudo bash run.sh --build    # 强制重建镜像
# 前提：wlan1 驱动已加载（bash/aic_load.sh）
#
# 唯一事实源 = 宿主机 gnp 的 config.toml (plan D2/D4 新布局 ~/.local/gnp/)
# 密码用 grep/sed 提取单行, **不依赖 python tomllib**: toml 是 gnpc 自己生成的
# (一键一行 key = "value"), grep 足够且不引入 python 依赖 (plan §6)
set -euo pipefail
cd "$(dirname "$0")"

# aipro 的 /home/lwboy 是 /mnt/disk/lwboy 的软链 —— 配置内嵌路径一律写 /home/lwboy
# (plan §7.3 #6)
BASE=/home/lwboy/.local/gnp
LEGACY=/home/lwboy/.local/share/sing-box

if [ ! -f "$BASE/config.toml" ]; then
  echo "找不到唯一事实源 $BASE/config.toml —— 先部署 gnpc (scp deploy/hosts/aipro.toml + gnpc migrate)" >&2
  exit 1
fi

# 密码提取: 单行 key = "value" (sed 去掉前缀/引号/行尾空白)
extract() {
  sed -n "s/^$1[[:space:]]*=[[:space:]]*\"\(.*\)\"[[:space:]]*\$/\1/p" "$BASE/config.toml" | head -1
}
HY2_PASS=$(extract hy2_password)
HY2_OBFS=$(extract obfs_password)
if [ -z "$HY2_PASS" ]; then
  echo "在 $BASE/config.toml 里提取不到 hy2_password" >&2
  exit 1
fi
[ -n "$HY2_OBFS" ] || echo "⚠️  config.toml 里没有 obfs_password（服务端 inbound 强制 salamander, 客户端不带 obfs 会握手失败）" >&2

# sing-box 二进制进构建上下文：
#   优先 submodule aipro-wifi/resources（分支 aipro-resources，离线可用）
#   再取宿主机 gnp 新布局, 最后回退旧布局 (未迁移的机器还能用)
if [ -x ./resources/sing-box/sing-box ]; then
  cp -f ./resources/sing-box/sing-box ./sing-box
elif [ -x "$BASE/bin/sing-box" ]; then
  cp -f "$BASE/bin/sing-box" ./sing-box
elif [ -x "$LEGACY/sing-box" ]; then
  echo "⚠️  回退到旧布局的 sing-box: $LEGACY/sing-box" >&2
  cp -f "$LEGACY/sing-box" ./sing-box
else
  echo "缺少 sing-box 二进制：请先 git submodule update --init ../resources" >&2
  exit 1
fi

# 规则集进构建上下文（模板引用 local .srs；取宿主机 gnp 的规则缓存）
mkdir -p ./rules
if compgen -G "$BASE/rules/*.srs" > /dev/null; then
  cp -f "$BASE"/rules/*.srs ./rules/
elif compgen -G "$LEGACY/rules/*.srs" > /dev/null; then
  echo "⚠️  回退到旧布局的规则集: $LEGACY/rules" >&2
  cp -f "$LEGACY"/rules/*.srs ./rules/
else
  echo "缺少规则集 (.srs): 先在宿主机跑 gnpc install" >&2
  exit 1
fi

echo "=== 1/4 NM 交接 wlan1（持久 unmanaged）==="
nmcli device set wlan1 managed no 2>/dev/null || true
mkdir -p /etc/NetworkManager/conf.d
cat > /etc/NetworkManager/conf.d/unmanage-wlan1.conf <<'EOF'
[keyfile]
unmanaged-devices=interface-name:wlan1
EOF

echo "=== 2/4 镜像（已存在则不重建, plan §6）==="
if [ "${1:-}" = "--build" ] || ! docker image inspect aipro-wifi-router:latest > /dev/null 2>&1; then
  docker build -t aipro-wifi-router:latest .
else
  echo "镜像已存在, 跳过构建 (要重建: sudo bash run.sh --build)"
fi

# hy2/obfs 密码 → root-only secret 文件（容器经 /run/secrets 只读挂载，不经命令行/环境）
# 新布局: $BASE/secrets/ (gnpc install 也写同一份, 内容一致)
# 带换行结尾: read 无换行时返回非零, 会让 set -e 的 entrypoint 死循环重启 (plan §7.3 #3)
SECRET_FILE=$BASE/secrets/hy2-password
OBFS_FILE=$BASE/secrets/hy2-obfs
umask 077
mkdir -p "$BASE/secrets"
printf '%s\n' "$HY2_PASS" > "$SECRET_FILE"
printf '%s\n' "$HY2_OBFS" > "$OBFS_FILE"
chmod 600 "$SECRET_FILE" "$OBFS_FILE"
echo "secret: $SECRET_FILE / $OBFS_FILE (0600, 带换行)"

echo "=== 3/4 运行（host 网络 + privileged）==="
docker rm -f aipro-wifi-router 2>/dev/null || true
docker run -d \
  --name aipro-wifi-router \
  --network host \
  --privileged \
  --restart unless-stopped \
  -v "$SECRET_FILE":/run/secrets/hy2_password:ro \
  -v "$OBFS_FILE":/run/secrets/hy2_obfs_password:ro \
  -e WIFI_IFACE=wlan1 \
  aipro-wifi-router:latest

echo "=== 4/4 状态 ==="
sleep 6
docker ps --filter name=aipro-wifi-router --format "{{.Names}} {{.Status}}"
docker logs aipro-wifi-router 2>&1 | tail -15

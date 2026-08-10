# global-net-proxy 安装指南

## 架构回顾

- **server**（lwtop / 任意 Ubuntu 海外节点）：原生 WireGuard，`wg-quick` 管理
- **client**（Mac / Ubuntu / Windows）：sing-box 二进制，tun + wg endpoint + DNS 分流

---

## Step 1: 部署 Server（lwtop）

在 lwtop（8.209.203.17, Ubuntu 24.04）上：

```bash
# 上传脚本
scp bash/server/server.sh lw:~/

# 安装 (交互式)
sudo bash server.sh --install
```

安装完成后会生成：
- `/etc/wireguard/wg0.conf` — server 配置
- `/etc/wireguard/server.key` / `server.pub` — 密钥
- 内核转发已开启，NAT 已配置

**添加客户端**（每台要接入的机器）：

```bash
sudo bash server.sh --add-peer macbook    # 生成一个客户端
sudo bash server.sh --add-peer linux-01
sudo bash server.sh --add-peer win-01
```

每次 `--add-peer` 会输出该客户端的：
- 私钥 `private_key`
- 地址 `address`（如 10.99.0.2/32）
- server 公钥 `server_public_key`
- server 地址 `8.209.203.17:51820`

> ⚠️ 这些参数要安全传给对应客户端，私钥不外泄。

---

## Step 2: 部署 Client

### 通用（交互式）

```bash
# 上传到目标机器
scp -r bash/client root@client-ip:/opt/global-net-proxy/

# 交互式安装 (会提示输入 server/密钥/IP)
bash /opt/global-net-proxy/client.sh --install

# 启动
bash /opt/global-net-proxy/client.sh --start

# 查看状态
bash /opt/global-net-proxy/client.sh --status
```

### 全自动（脚本化，适合多机批量）

```bash
SERVER=8.209.203.17 \
WG_PUBKEY=<server公钥> \
CLIENT_PRIVKEY=<本机私钥> \
CLIENT_IP=10.99.0.2/32 \
bash client.sh --install-auto
```

### 规则集自动更新 + 常驻

```bash
# 安装 cron: 每天04:00 检查 sing-box 常驻 + 更新规则集
bash update-rules.sh --install-cron
```

sing-box 的 remote rule-set 本身也会每 24h 自动更新，cron 是双保险。

---

## 平台差异说明

### Linux (Ubuntu/Debian)
- 需要 root 建 tun：脚本检测到非 root 会自动 `sudo`
- 依赖：`curl`, `tar`, `wg`（server 端需要）

### macOS
- 需要 root 建 tun：脚本会 `sudo`
- 首次允许 tun 设备
- sing-box 下载 darwin 版本

### Windows
- 用 Git Bash / WSL 运行脚本
- Windows 下 sing-box 后台用任务计划程序（脚本会给提示）
- 或 WSL 直接跑 Linux 版本

---

## 验证

```bash
# 确认 wg 隧道建立
wg show

# 确认国外流量走 wg (应该显示 lwtop 公网出口 IP)
curl -s ifconfig.me   # 期望 8.209.203.17

# 确认国内流量直连
curl -s -o /dev/null -w "%{http_code}\n" https://www.baidu.com   # 200
```

---

## 常见问题

**Q: 国外流量没走 wg？**
检查 route 规则优先级：国外 geosite 规则必须在 geoip-cn/geosite-cn 之前（脚本已保证顺序）。确认 sing-box 日志无报错。

**Q: DNS 解析延迟高？**
国外域名走 `https://1.1.1.1/dns-query`（经 wg），首次解析较慢但会缓存。若频繁，可考虑本地缓存。

**Q: 某域名漏了，走了直连？**
geosite 分类覆盖大部分，但小众域名可能没有。可临时在 route 规则加一条精确 `domain` 匹配指到 wg-out。

**Q: Windows 无法常驻？**
用任务计划程序，触发条件"登录时"，操作 `sing-box.exe run -c config.json`。

---

## 卸载

```bash
# server
sudo bash server.sh --uninstall

# client
bash client.sh --uninstall
```
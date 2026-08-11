# global-net-proxy 安装指南

> ⚠️ **安全警告**: 客户端使用 **mixed 代理模式**，绝不使用 tun 模式。
> tun 模式（`strict_route` + `auto_route`）会接管系统路由表，在无带外访问的机器上会导致**完全断网**。
> 详见 [incident-2026-08-10.md](incident-2026-08-10.md)。

## 架构回顾

- **server**（lwtop / 任意 Ubuntu 海外节点）：原生 WireGuard，`wg-quick` 管理
- **client**（Mac / Ubuntu / Windows）：sing-box 二进制，**mixed 代理** + wg endpoint + DNS 分流

> 推荐使用 **Rust CLI** (`gnp-client` / `gnp-server`) 管理，bash 脚本作为参考/应急保留。

---

## Step 0: 安装 gnp CLI（推荐）

在任意机器构建并安装到系统 PATH：

```bash
# 克隆项目 (含 submodule)
git clone --recurse-submodules <repo-url>
cd global-net-proxy

# 构建 + 安装
bash bash/install.sh

# 验证
gnp-client --version   # gnp-client 0.1.0
gnp-server --version   # gnp-server 0.1.0

# 卸载
bash bash/uninstall.sh
```

---

## Step 1: 部署 Server（lwtop）

### 方式 A：Rust CLI（推荐）

```bash
sudo gnp-server install          # 安装 wireguard + 生成密钥 + 配置 NAT + 开机自启
sudo gnp-server status           # 查看状态
sudo gnp-server add-peer macbook # 添加客户端
sudo gnp-server pregen 20        # 预生成 peer 池
sudo gnp-server activate <id>    # 激活预生成的 peer
sudo gnp-server uninstall        # 卸载
```

安装完成后会生成：
- `/etc/wireguard/wg0.conf` — server 配置
- `/etc/wireguard/server.key` / `server.pub` — 密钥
- 内核转发已开启，NAT 已配置

**添加客户端**（每台要接入的机器）：

```bash
sudo gnp-server add-peer macbook    # 生成一个客户端
sudo gnp-server add-peer aipro
sudo gnp-server add-peer win-01
```

每次 `add-peer` 会输出该客户端的：
- 私钥 `private_key`
- 地址 `address`（如 10.0.0.5/32）
- server 公钥 `server_public_key`
- server 地址 `8.209.203.17:51820`

> ⚠️ 这些参数要安全传给对应客户端，私钥不外泄。

---

## Step 2: 部署 Client

### 方式 A：Rust CLI（推荐）

```bash
# 安装 sing-box (需先下载二进制到 ~/.local/share/sing-box/sing-box)
# 生成 config.json (参考 config/safe-template.json, 填入 server 公钥+本机私钥+IP)

gnp-client start       # 启动 (开机自启)
gnp-client status      # 查看状态
gnp-client wg          # wg 隧道诊断
gnp-client test        # 测试连通性
gnp-client config --check  # 校验配置安全
```

### 自动注册（推荐，多机批量）

新机器用 `gnp-client register <client_id>` 从 gitee peer 池一键注册：

```bash
export GITEE_TOKEN=xxxx
gnp-client register ningsure
# 自动: 拉取 peer → 标记 used → 生成 config → 安装 sing-box → 提示激活
```

> 详见 `docs/auto-registration.md`。

### systemd 常驻（Linux）

`gnp-client start` 会自动生成 `~/.config/systemd/user/sing-box-gnp.service`：
- **User**: 当前用户（非 root）
- **Restart**: `on-failure`（非 always）

```bash
systemctl --user daemon-reload
systemctl --user enable --now sing-box-gnp
systemctl --user status sing-box-gnp
journalctl --user -u sing-box-gnp -f
```

---

## Step 3: 使用代理

sing-box 启动后，代理端口为 **1080**（同时支持 socks5 和 http）。

### 临时使用（当前 shell）

```bash
export http_proxy=http://127.0.0.1:1080
export https_proxy=http://127.0.0.1:1080
export all_proxy=socks5://127.0.0.1:1080
```

### 永久生效（写入 ~/.bashrc 或 ~/.zshrc）

```bash
# 按需代理函数（只在需要时用）
proxy_on() {
    export http_proxy=http://127.0.0.1:1080
    export https_proxy=http://127.0.0.1:1080
    export all_proxy=socks5://127.0.0.1:1080
    echo "代理已开启"
}
proxy_off() {
    unset http_proxy https_proxy all_proxy
    echo "代理已关闭"
}
```

### 按程序配置

| 程序 | 配置方式 |
|------|---------|
| curl/wget | `http_proxy`/`https_proxy` 环境变量 |
| git | `git config --global http.proxy http://127.0.0.1:1080` |
| npm | `npm config set proxy http://127.0.0.1:1080` |
| pip | `pip install --proxy http://127.0.0.1:1080 ...` |
| Docker | `/etc/systemd/system/docker.service.d/proxy.conf` |
| SSH | `ProxyCommand nc -X 5 -x 127.0.0.1:1080 %h %p` |

---

## 平台差异说明

### Linux (Ubuntu/Debian)
- **不需要 root**（mixed 模式不建 tun）
- 依赖：`curl`, `tar`
- systemd --user 服务常驻

### macOS
- **不需要 root**
- sing-box 下载 darwin 版本
- 用 launchd 或 nohup 常驻

### Windows
- 用 Git Bash / WSL 运行脚本
- WSL 下直接跑 Linux 版本
- 或下载 Windows 版 sing-box.exe，用任务计划常驻

---

## 验证

```bash
# 确认代理能通 (通过代理访问 Google)
curl -x http://127.0.0.1:1080 -s https://www.google.com -o /dev/null -w "%{http_code}\n"  # 期望 200

# 确认代理出口是 lwtop (期望 8.209.203.17)
curl -x socks5://127.0.0.1:1080 -s ifconfig.me

# 确认国内直连不走代理
curl -s -o /dev/null -w "%{http_code}\n" https://www.baidu.com   # 200

# 确认 sing-box 运行中
gnp-client status
```

---

## 常见问题

**Q: curl 报 Connection refused?**
sing-box 未启动或端口不对。确认 `gnp-client status` 显示运行中，端口是 1080。

**Q: DNS 解析延迟高？**
国外域名走 `https://1.1.1.1`（经 wg），首次解析较慢但会缓存。

**Q: 某域名没走代理？**
geosite 分类覆盖大部分，但小众域名可能没有。可临时在 route 规则加精确 `domain` 匹配。或直接用 `export https_proxy=http://127.0.0.1:1080` 强制走代理。

**Q: 想让所有流量默认走代理？**
当前 `final: "wg-ep"` 已经是默认走代理。未命中任何规则的域名会走 wg。

---

## 卸载

```bash
# client (会同时清理 sing-box + systemd service)
gnp-client stop
bash uninstall.sh --clean

# server
sudo gnp-server uninstall
```

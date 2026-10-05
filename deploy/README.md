# deploy/ — 个人部署资产（与 gnp 产品核心分离）

> 2026-08-15 起仓库分家：**根目录 = gnp 产品（server & client 源码 + 产品文档）**，
> 本目录 = 与个人环境/节点相关的部署资产。gnp 保持独立通用，个人拓扑都在这。

## 内容

| 目录 | 内容 |
|---|---|
| `hosts/` | ★ 每台机器的 `config.toml` canonical 版本（v2 唯一事实源的 repo 侧基准，与实机 md5 必须一致；密码内联，私有 repo） |
| `scheduler/` | tick.sh + launchd plist / systemd unit / cron 资产（`include_str!` 编进 gnpc/gnps，改后需重编） |
| `aipro-wifi/` | OrangePi AIpro WiFi 修复全程（三层根因文档/驱动模块归档/无线路由 docker/资源 submodule） |
| `ns-hub/` | ningsure 枢纽 WireGuard 星型虚拟网（10.99.0.0/24） |
| `peers/` | server 密码校验 + 用户池（配套 gitee 私有仓库使用） |
| `quic-test/` | hy2/QUIC 实验 + 服务端模板与排障手册（端口/obfs/ufw 坑） |

（v2 重构时 `config-mac/` 已删除，被 `hosts/mac.toml` 取代。）

## 当前个人拓扑速查（2026-10-05 v2 后）

- **节点**：mac / aipro(192.168.1.2, AP ssid=aipro, wg 10.99.0.2) / lwmate(192.168.0.110) / cozepc(火山引擎, wg 10.99.0.4, 仅出站) / ningsure(47.103.71.171, wg hub :9100) / lwtop(8.209.203.17, gnps server, UDP 5766) / vmwin+lwwin(Win, 离线待 `gnpc migrate`)
- **布局**：客户端 `~/.local/gnp/`（gnpc + tick.sh），服务端 `/opt/gnp/`（gnps）；hy2=UDP 5766 + salamander obfs，ssh 22 兜底
- **接入新节点**：lwtop `sudo gnps gen-user --name <名>` → 追加 `deploy/hosts/<host>.toml` → 目标机 `gnpc migrate`（或 `gnpc register` 自动注册流）

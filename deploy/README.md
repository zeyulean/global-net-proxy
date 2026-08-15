# deploy/ — 个人部署资产（与 gnp 产品核心分离）

> 2026-08-15 起仓库分家：**根目录 = gnp 产品（server & client 源码 + 产品文档）**，
> 本目录 = 与个人环境/节点相关的部署资产。gnp 保持独立通用，个人拓扑都在这。

## 内容

| 目录 | 内容 |
|---|---|
| `aipro-wifi/` | OrangePi AIpro WiFi 修复全程（三层根因文档/驱动模块归档/无线路由 docker/资源 submodule） |
| `ns-hub/` | ningsure 枢纽 WireGuard 星型虚拟网（10.99.0.0/24） |
| `config-mac/` | Mac 节点的手工精调 sing-box 配置（hosts 映射等，勿被 `peer`/`install` 覆盖） |
| `peers/` | server 密码校验 + 用户池（配套 gitee 私有仓库使用） |
| `quic-test/` | hy2/QUIC 早期实验 |

## 当前个人拓扑速查

- **节点**：mac / aipro(192.168.1.2, AP ssid=aipro, wg 10.99.0.2) / ningsure(47.103.71.171, wg hub :9100) / coze-pc(wg 10.99.0.3, 仅出站) / lwtop(8.209.203.17, gnp server) / vmwin(Win11 ARM64 client)
- **接入新节点**：`sudo gnp-server gen-user --name <名>`（lwtop）→ 把 gnp.cfg 送到目标机 → `gnp-client peer gnp.cfg`

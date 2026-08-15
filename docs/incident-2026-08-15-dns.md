# 2026-08-15 分流故障全档案（DNS 三连坑 + 修复范式）

> 从 "AP 下 baidu 半天打不开" 一路挖出的四层根因，全部已修复并固化进
> `install.rs` 生成器与路由容器模板。本文是排障范式，遇"分流不稳"先按此走。

## 故障时序与四层根因

| # | 症状 | 根因 | 修复 |
|---|---|---|---|
| ① | google 解析出 Facebook IP（污染） | AP 网关 192.168.88.1 自身在 192.168/16 内——DNS 不先于私网 RETURN 劫持，就被 dnsmasq 代答→宿主 resolv.conf→GFW 污染 | mangle 里 **dport53 的 TPROXY 提到私网 RETURN 之前**；dnsmasq `port=0` |
| ② | sing-box 起了但端口迟迟不监听/全网黑洞 | rule-set 用 **remote**（raw.githubusercontent 直连被墙），初始化阻塞；容器重建即丢 cache 再踩 | **local .srs 烧进镜像/装机**（generator 与容器均 local path），不依赖网络 |
| ③ | CN 域名 DNS 400-1000ms、baidu 1.7-7.8s | sing-box **hijack-dns 直答客户端迟滞**（实测模块级，偶发丢包） | **fakeip**：应答即时假 IP（22-54ms），连接进来按域名路由：CN→direct 内部真解析 ~20ms，海外→hy2 携域名出海 |
| ④ | systemd 服务 crash-loop 111 次 | `cache_file` 无绝对路径 → CWD=/ 不可写 → `cache.db permission denied` 启动即死 | cache_file **必须绝对路径**（generator 已写入 `~/.local/share/sing-box/cache.db`） |

## 修复后基线（Mac 连 AP 实测）

```
DNS 22-54ms · baidu 0.067s · taobao 0.20s · google 0.16s
CN 出口=本地宽带直连 · 海外出口=hy2 服务器 · 内网 192.168/16 RETURN 不走代理
```

## 已固化到源码的位置

- `crates/gnp-core/src/install.rs::generate_config`：fakeip 块 + cache 绝对路径 +
  local rule-set + 新版 DNS 格式（1.13 兼容三连：type/server 字段、
  default_domain_resolver、下载失败 tmp+rename 不污染）
- `deploy/aipro-wifi/router-docker/`：local .srs 烧镜像（Dockerfile COPY）+ fakeip 模板
- 宿主 gnp-proxy：local rule-set + cache 绝对路径（2026-08-15 已修）

## 排障速查（"分流不稳定"的标准流程）

1. `dig @<网关> 域名` 看耗时/答案真伪 → 定位 DNS 层（①③）
2. `docker logs` / journalctl 找 `rule-set`/`take too much` → 定位 ②
3. 服务反复重启看 `cache.db permission` → 定位 ④
4. 应急通道：`aipro-wifi gnpoff` 走原通道 NAT，恢复后 `gnpon` 切回

## 附：fakeip 副作用与边界

- 客户端 `ping 域名` 显示 198.18.x.x 且不通（ICMP 不走 tproxy）——纯观感
- cache_file 持久化假 IP↔域名映射，重启不漂移（independent_cache）

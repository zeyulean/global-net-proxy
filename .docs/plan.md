# gnp 跨境链路 MTU 黑洞修复计划（2026-10-04/05 事件）

> 10-4 13:23 lwtop 重启后，跨境路径 UDP MTU 掉到 ~1240B，QUIC 强制 ≥1200B 初始包落入死区，
> 全部 hy2 客户端（Mac/aipro/lwmate/cozepc）同时断网两天。本文是根因、临时处置现状、
> 与把"单协议无降级"这个结构性缺陷修掉的分期计划。排障范式见附录。

## 1. 根因（一句话 + 证据链）

**跨境路径（国内家宽 → lwtop 8.209.203.17）UDP 载荷 MTU ≈1240B，QUIC 必需的 ≥1200B
初始/握手包（线上 1228B+）部分被丢，握手无声失败。** 不是 GFW 特征识别（obfs 无效佐证），
不是服务端问题（127.0.0.1 自环全通）。

| # | 证据 | 结论 |
|---|---|---|
| ① | `/proc/net/snmp` Udp InDatagrams 差分：载荷 1200B（线上 1228）全达，1240/1280/1332/1400 **+0** | 尺寸截止点在 1228~1268 之间 |
| ② | 客户端日志错误形态全部是 `timeout: no recent network activity`，起点 10-4 13:23:24（服务端重启 +2s） | 握手包发出无回应 |
| ③ | obfs/salamander 两端加上并重启后依旧失败 | 排除特征识别，坐实尺寸天花板 |
| ④ | lwtop 本机自环 hy2 全通；TCP 443/SSH/ping 全通；lwtop 出境 UDP（DNS）正常 | 服务端软硬件无辜 |
| ⑤ | aipro（同家宽）同样失败 | 与 Mac 单机无关，是路径问题 |

## 2. 临时处置现状（10-5 已生效，四台全通）

方案：sing-box 加 `ssh-out` 出站（TCP 底座免疫 UDP MTU 黑洞，复用 lwtop sshd，
密钥 `~/.ssh/id_ed25519`，user `lw`），`route.final → ssh-out`。hy2-out 保留。

| 主机 | 服务管理 | 验证（github / 出口 IP） | 配置备份 |
|---|---|---|---|
| Mac | gnp-client stop/start | 200 / 8.209.203.17 | `~/.local/share/sing-box/config.json.bak-20261005` |
| aipro | `systemctl --user restart sing-box` | 200 / 8.209.203.17 | 同名 .bak（home=/mnt/disk/lwboy） |
| lwmate | `sudo systemctl restart gnp-proxy` | 200 / 8.209.203.17 | 同名 .bak |
| cozepc | `systemctl restart gnp-proxy`（root） | 200 / 8.209.203.17 | 同名 .bak（/root/.local/...） |

附带变更：lwtop `/opt/gnp-quic/config.json` 入站已加 salamander obfs（对本次无效但保留，
备份 `.bak-*`）；cozepc 公钥已装入 lwtop `authorized_keys`（标记 `cozepc-gnp`）。
**回滚 = 各机 route.final 改回 `hy2-out`（运营商路径恢复后）。**

## 3. 目标

- **G1 韧性**：任一通道（协议/路径级）故障，gnp 自动降级到 TCP 兜底，用户无感；恢复自动回切
- **G2 可观测**：通道状态/延迟可见，故障主动告警（autoworker notify）
- **G3 运维简单**：四台机器一套生成逻辑，无手工配置漂移（本次手工修复就是漂移，需收敛进生成器）

## 4. 方案设计

### 4.1 核心机制：urltest 出站自动选路（P0，纯配置）

sing-box 原生 `urltest` 出站组，把 hy2 与 ssh 兜底放进一组，`route.final` 指向组：

```json
{ "type": "urltest", "tag": "auto-out",
  "outbounds": ["hy2-out", "ssh-out"],
  "url": "https://www.gstatic.com/generate_204",
  "interval": "3m", "tolerance": 300 }
```

- hy2 健康时低延迟胜出；hy2 死时其探测失败，组内自动落到 ssh；hy2 恢复自动回切——G1/G3 免代码
- `tolerance=300ms` 防抖：ssh 偶发快过 hy2 也不会来回跳
- 取舍：urltest 按"最快"选，不保证优先 hy2；tolerance 大致等价"优先当前者"。若要严格
  hy2 优先，P1 里用 selector + probe 脚本切换（见 4.3）

### 4.2 服务端配套

- **P0 不动服务端**：ssh 兜底复用现有 sshd（密钥认证已四台打通）
- **P2 换正式 TCP 入站**：ssh 通道的吞吐/连接管理不是最优（单流 TCP、无多路复用）。
  在 lwtop 加 vless+reality 或 trojan over TCP（443 端口经 nginx SNI 分流复用，或独立高位
  端口 + ufw 放行），gnp 侧把 ssh-out 替换为该出站。QUIC/hy2 保留为主通道 + salamander
  obfs（已部署）
- 注意 lwtop 有 ufw（enabled）：新增任何端口要同步放行，install 脚本里固化

### 4.3 gnp-client 功能（P1）

- `gnp-client probe`：主动诊断——hy2 握手测试 + **载荷尺寸扫描**（1200/1240/1280B 三档，
  内核计数器差分法，见附录）直接输出"路径 MTU 是否容得下 QUIC"，把本次两天的排查变成一条命令
- `gnp-client switch <hy2|ssh|auto>`：manual override（selector 实现），修 urltest 取舍的兜底
- `gnp-client status`：显示当前生效通道、各通道最近探测延迟
- **重连退避**：hy2 失败后指数退避 + 抖动（如 1s→2s→…→cap 60s）。10-4 断网疑似与"服务端
  重启 → 全员重连风暴"有关（hysteria2 社区知名 QoS 触发模式），退避能把这类二次伤害掐掉
- 告警接入：探测异常时走 autoworker `notify_send`（Mac/aipro 已有 autoworker 常驻）

### 4.4 MTU 自适应（P2，增强）

- `gnp-client probe --mtu` 定期（如每小时）跑尺寸扫描，结果写 cache；MTU < 1250 时自动
  通知 urltest 侧权重/直接切 ssh，并在 status 里亮黄灯
- 该探测同样能提前发现"路径恢复"，缩短回切延迟

## 5. 实施阶段

| 阶段 | 内容 | 落点 | 工作量 |
|---|---|---|---|
| **P0** | 生成器出双通道 + urltest，四台重刷配置收敛漂移 | `crates/gnp-core/src/install.rs::generate_config`（同 8-15 事件的 fakeip 修法） | 半天 |
| **P1** | probe/switch/status 子命令 + 重连退避 + 告警 | gnp-client CLI | 1-2 天 |
| **P2** | 服务端 TCP 正式入站替换 ssh 兜底；MTU 自适应 | lwtop install 脚本 + generator | 2-3 天 |
| **P3** | 多出口（ningsure 中转/家宽双线）、协议矩阵（tuic/hy2/ssh/trojan）可插拔 | gnp-core 架构扩展 | 远期 |

## 6. 验收标准

1. 断 hy2（服务端停 gnp-hy2）→ 60 秒内四台 github 仍 200，出口自动变为 ssh 通道（TCP 特征）
2. 恢复 hy2 → 一个探测周期（3m）内自动回切
3. `gnp-client probe` 在当前 MTU 黑洞路径上输出明确结论（"QUIC 不可用，建议 ssh"）
4. 重启/断网场景无重连风暴（日志里退避间隔可见）
5. 四台机器配置由生成器统一产出，手工 diff 为零

## 7. 风险与回滚

- urltest 探测流量（每 3m 一次 generate_204）极小，可忽略
- ssh 兜底安全面 = sshd 暴露面，密钥已限；P2 换 vless/reality 后收敛
- 全部变更可回滚：配置层回 `final: hy2-out`，代码层 revert generator

## 附录：本次排障范式（"代理时通时不通/无声超时"标准流程）

1. **客户端日志定性**：`/tmp/sing-box-gnp.err` 看错误形态与起始时间，对照事件时间轴
2. **服务端自环**：127.0.0.1 起临时 hy2 client 配置，隔离网络因素
3. **内核计数器差分**（无需 root）：`/proc/net/snmp` `Udp: InDatagrams`，发 N 个包对差值
   ——判"到达 vs 被丢"；注意 `awk /^Udp:/` 会同时匹配表头行
4. **载荷尺寸扫描**：1200/1240/1280/1332/1400 五档 → MTU 截止点
5. **多观测点**：同宽带第二台（aipro）判单机/路径；云内网判 GFW/安全组
6. 坑：/tmp 测试文件先 rm 后读会扑空；非交互 ssh 不加载 ~/.cargo/bin 等 PATH

（事件全档案与四台机器状态快照存 ZCode 记忆 `gnp-hy2-mtu-blackhole`，2026-10-05）

## 8. 实施记录（2026-10-05 当日完成）

### 8.1 根因修正（重要）

**"UDP MTU 黑洞"结论是误判。真根因：lwtop ufw 缺 `443/udp` 放行规则。**

- ufw 规则只有 `443/tcp`；默认 deny incoming 把 hy2 的 QUIC 全部丢弃
- 一直没暴露的原因：hy2 是长活 UDP 流，conntrack ESTABLISHED 条目让后续入包
  绕过默认 deny；**10-4 13:24 lwtop 重启清空 conntrack**，新握手（NEW 流）从此
  全部无声被丢 —— 与故障起点（13:23:24 重启 +2s）严丝合缝
- 铁证：`[UFW BLOCK] ... PROTO=UDP DPT=443`（家宽源 + ningsure 源双确认）；
  9999/udp 可达、443/udp 双源 0 到达、127.0.0.1 自环全通
- **10-4 的"尺寸截止"扫描（1200 达/1240+丢）是测量噪声伪影**：lwtop InDatagrams
  统计全机 UDP（含隧道 DNS 回包突发），松窗口差分会把噪声读成"截止点"。
  10-5 复测时同样的伪影复现（1332"到达"而 1280"被丢"，物理上不可能）
- 修复 = `ufw allow 443/udp`，hy2 当场恢复（握手 390ms）；1240B 10/10 到达
- 教训：**排查"无声超时"先查服务端防火墙对协议+端口的双栈放行**，尺寸扫描
  结论必须过单调性校验（probe 已内置）

### 8.2 已落地

| 项 | 状态 | 实测 |
|---|---|---|
| P0 双通道生成器（selector+urltest+ssh+clash_api） | ✅ `install.rs::build_config` + 3 单元测试 | 四台重刷收敛 |
| 四台配置重刷（Mac/aipro/lwmate/cozepc） | ✅ 生成器产出, 备份 `.bak-p0-20261005` | 全部 github 200 |
| urltest 自动降级 | ✅ | hy2 死→ssh-out；恢复→hy2-out（135ms） |
| `gnp-client probe`（握手+MTU 扫描+单调性校验） | ✅ | 断网时/恢复后双态实测 |
| `gnp-client switch`（热切换+持久化） | ✅ | ssh/auto 回合实测 |
| `gnp-client guard`（退避+冻结+重启+告警） | ✅ Mac launchd `com.gnp.guard` 每 60s | 停 hy2 → 2 次失败冻结+告警 → 恢复回切 全程实测 |
| 服务端 ufw 修复 | ✅ `ufw allow 443/udp`（comment gnp hy2 QUIC） | 恢复即时 |

- P0 期间实测发现并处理：长跑 sing-box 实例 ssh-out 出站可能僵死（直接 ssh 正常
  但出站探测挂）→ guard 冻结时附带重启服务确保兜底干净
- guard 退避上限从 1h 收到 180s（对齐 urltest 3m 周期）：1h 上限实测让恢复回切
  滞后 26min
- Mac 调度用 launchd `com.gnp.guard`（StartInterval=60）而非 cron —— 沙箱/CLI 环境下
  macOS `crontab -` 会挂起；Linux 机器仍用 `guard --install-cron`
- 单实例审计：Mac launchd 仅 `com.gnp.sing-box`（代理本体）+ `com.gnp.guard`（看门狗
  tick，跑完即退），crontab 空，sing-box run 进程数 = 1，无重复后台

### 8.3 端口迁移 443 → 5766（2026-10-05 晚，用户拍板）

- **动因**：443 的 tcp/udp 双栈歧义是本次事故土壤；5766 专用端口 ufw 一条规则说清；
  >1024 端口未来 gnp-hy2 可降权（未做，可选）；给 P2 的 TCP 443 让路
- **云安全组收敛**（用户操作）：18 条 → 6 条（22/tcp、80/tcp、443/tcp、1194/udp、5766/udp、5767/udp 备用）；
  删除的僵尸项：Redis 6379 / MySQL 3306 / RDP 3389 / SMTP 587+465 / frps 7000+7500 /
  14568 tcp+udp / 3000 / 8080 / wg 51820 —— 全部无监听（对账 ss -tulnp + docker ps）；
  UDP 443 同步关闭。docker 发布端口绕过 ufw，安全组是最后屏障，僵尸规则=未来手滑的雷
- **服务端**：/opt/gnp-quic/config.json listen_port → 5766（备份 .bak-20261005-port443）；
  ufw `allow 5766/udp` + `delete allow 443/udp`（收敛后 4 条：22/tcp、443/tcp、1194/udp、5766/udp）
- **客户端**：四台全部重刷 `--server-port 5766`（备份 .bak-20261005-port5766），全部 urltest → hy2-out
- **aipro 路由容器**：修复两个潜伏问题——还指 443 + **无 obfs**（10-5 服务端强制 obfs 后
  AP 出海实际已断）。仓库模板改 5766+obfs 占位符、entrypoint 加 obfs secret 渲染、
  run.sh 补 rules/ staging（原构建上下文缺）+ obfs secret 挂载 + secret 带换行
  （`read` 无换行返回非零 × `set -e` = 死循环重启，旧 secret 带换行掩盖了此坑）。
  重建镜像后 AP 恢复，日志实测 AP 客户端 DNS 经 hy2 5766 出海（180ms）
- **未跟进项**：vmwin/lwwin 离线机旧配置指 443，下次开机需重刷（`--server-port 5766`）；
  lwtop gnp-server gen-user 仍发 443 端口（新 peer 接入前需更新 gnp-server 或手改 cfg）；
  gnp-hy2 降权 User=root → lw（可选加固）

### 8.4 未做（按 plan 分期）

- **P2**（vless/reality 正式 TCP 入站替换 ssh 兜底；MTU 定期自适应）：ssh 兜底经本次
  实测可靠，替换涉及 lwtop 新端口/证书/nginx 决策，留待后续单独评估；MTU 自适应的
  探测能力已由 `probe` 覆盖，guard 的冻结/回切已覆盖"自动切 ssh"语义
- **P3**（多出口/协议矩阵）：远期不变
- 回滚：各机 config 备份 `.bak-p0-20261005`；配置层回 `final: hy2-out` 即单通道旧行为

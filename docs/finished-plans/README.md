# finished-plans — 已完成的实施计划归档

计划完成、验收后归档于此。代码注释中引用的 "plan §x.x" 指这些文件。

| 归档文件 | 内容 | 状态 |
|---|---|---|
| [2026-10-04-hy2-outage-repair.md](2026-10-04-hy2-outage-repair.md) | 跨境 hy2 断网两天修复: 真根因 (ufw 缺 443/udp + conntrack)、双通道自动降级、probe/switch/guard、443→5766 迁移 | P0/P1 完成, P2/P3 未立项 |
| [2026-10-05-v2-refactor.md](2026-10-05-v2-refactor.md) | v2 重构: gnpc/gnps 改名、config.toml 唯一事实源、tick.sh 调度、路径收敛 (~/.local/gnp + /opt/gnp)、gnpc migrate | 全部完成并验收 (2026-10-05) |

未完成、仍在计划中的工作**不放这里**（P2 vless/reality、端口跳跃、降权等见 v2 计划 §9 "明确不做"）。

相关目录：[../archive/](../archive/) 放被取代的旧文档（如 pre-v2 安装指南）。

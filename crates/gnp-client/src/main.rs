//! gnp-client — global-net-proxy client CLI
//!
//! 管理本机 sing-box mixed 代理 (hysteria2/QUIC 隧道)。
//! 安全原则: 只用 mixed 代理模式 (socks5+http on 1080), 不碰路由表, 零断网风险。

use std::io::IsTerminal;

mod api;
mod cleanup;
mod guard;
mod probe;
mod recover;
mod register;
mod switch;
mod update_rules;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use gnp_core::{config, install, platform, proxy, service, tunnel};

/// global-net-proxy client — 管理 sing-box mixed 代理 (hysteria2/QUIC 隧道)
///
/// 安全原则: 本工具只使用 mixed 代理模式 (socks5+http on 127.0.0.1:1080),
/// 不修改系统路由表, 零断网风险。绝不用 tun 模式。
///
/// 快速开始:
///   gnp-client start     # 启动代理 (开机自启)
///   gnp-client status    # 查看状态
///   gnp-client tunnel    # 隧道诊断 (兼容旧名 wg)
#[derive(Parser)]
#[command(
    name = "gnp-client",
    version,
    about = "global-net-proxy client (sing-box mixed 代理)",
    long_about = "管理本机 sing-box mixed 代理 (hysteria2/QUIC 隧道)。\n\
        \n\
        安全原则: 只用 mixed 代理模式 (socks5+http on 127.0.0.1:1080),\n\
        不碰路由表, 零断网风险。绝不用 tun 模式。\n\
        \n\
        数据目录: ~/.local/share/sing-box/\n\
        - 二进制:  sing-box\n\
        - 配置:    config.json\n\
        - 规则集:  rules/*.srs\n\
        \n\
        常用命令:\n\
        \n  \
        gnp-client start           启动代理 (开机自启)\n  \
        gnp-client stop            停止代理\n  \
        gnp-client status          查看状态 (进程/端口/隧道/通道/出口IP)\n  \
        gnp-client probe           诊断: hy2 握手 + MTU 扫描\n  \
        gnp-client switch ssh      强制 TCP 兜底 (auto 恢复自动)\n  \
        gnp-client guard --install-cron  通道看门狗 (每分钟)\n  \
        gnp-client tunnel          隧道诊断 (兼容旧名 wg)\n  \
        gnp-client test            测试代理连通性\n  \
        gnp-client config --check  校验配置安全\n  \
        gnp-client proxy --on      开启系统代理 (macOS)\n\
        \n\
        详见: docs/usage.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动 sing-box 代理 (开机自启)
    #[command(long_about = "启动 sing-box 代理服务并注册为开机自启。\n\n\
        平台行为:\n  \
        macOS  → launchctl load (KeepAlive=true, 崩溃自动重启)\n  \
        Linux → systemctl start gnp-proxy (自动创建 unit 如不存在)\n\n\
        启动后代理监听 0.0.0.0:1080 (socks5+http)。\n\n\
        示例:\n  \
        gnp-client start")]
    Start,
    /// 停止 sing-box 代理
    #[command(long_about = "停止 sing-box 代理服务, 卸载开机自启并杀掉残留进程。\n\n\
        平台行为:\n  \
        macOS  → launchctl unload + pkill sing-box run\n  \
        Linux → systemctl stop gnp-proxy\n\n\
        示例:\n  \
        gnp-client stop")]
    Stop,
    /// 查看状态 (进程/端口/隧道/出口IP)
    #[command(long_about = "显示完整的代理运行状态。\n\n\
        输出内容:\n  \
        1. 安装状态 (sing-box 二进制 + config.json)\n  \
        2. 进程状态 (是否运行中)\n  \
        3. 端口 1080 是否在监听\n  \
        4. 配置安全检查 (无 tun/strict_route, 有 mixed+hysteria2)\n  \
        5. 隧道出口 IP 和延迟 (如果运行中)\n\n\
        示例:\n  \
        gnp-client status")]
    Status,
    /// 查看/校验配置
    #[command(long_about = "查看 sing-box 配置文件内容, 或校验配置是否安全。\n\n\
        校验项:\n  \
        - 无 tun/strict_route/auto_route (危险配置检测)\n  \
        - 有 mixed inbound (socks5+http)\n  \
        - 有 hysteria2 outbound\n\n\
        示例:\n  \
        gnp-client config --check   # 校验配置安全性\n  \
        gnp-client config --show    # 显示完整 JSON 配置")]
    Config {
        /// 显示完整配置内容
        #[arg(long)]
        show: bool,
        /// 校验配置是否正确 (无 tun/strict_route, 有 mixed+hysteria2)
        #[arg(long)]
        check: bool,
    },
    /// 隧道诊断 (兼容旧名 wg)
    #[command(alias = "wg", long_about = "显示 hysteria2/QUIC 隧道配置详情并测试连通性。\n\n\
        输出内容:\n  \
        1. 隧道配置 (远端 server:port / 密码状态)\n  \
        2. 隧道连通性 (通过 socks5h 检测出口 IP 和延迟)\n  \
        3. 代理测试 (github + google HTTP 状态码)\n\n\
        示例:\n  \
        gnp-client tunnel   # 或旧名 gnp-client wg")]
    Tunnel,
    /// 测试代理连通性
    #[command(long_about = "快速测试代理是否可用。\n\n\
        测试内容:\n  \
        1. 通过 socks5h://127.0.0.1:1080 检测出口 IP\n  \
        2. 测试 github (api.github.com/zen)\n  \
        3. 测试 google (www.google.com)\n\n\
        使用 socks5h (带 h) 表示 DNS 在代理端远程解析, 避免本地 DNS 污染。\n\n\
        示例:\n  \
        gnp-client test")]
    Test,
    /// 主动诊断: hy2 握手测试 + UDP 载荷尺寸扫描 (MTU 黑洞定位)
    #[command(long_about = "主动诊断跨境链路, 把\"代理无声超时\"排查一条命令化。\n\n\
        两项检测 (2026-10-05 MTU 黑洞事件范式):\n  \
        1. hy2 握手测试 — 临时 sing-box 实例经真实路径 curl generate_204\n  \
        2. UDP 载荷尺寸扫描 — 向服务端发 1200/1240/1280/1332/1400B 各 N 包,\n  \
           ssh 读服务端 /proc/net/snmp Udp InDatagrams 差分 → MTU 截止点\n\n\
        输出明确结论: 路径是否容得下 QUIC (≥1200B 初始包)。\n\
        MTU 扫描需本机 ssh 密钥可免密登录服务端。\n\n\
        示例:\n  \
        gnp-client probe                     # 全套诊断\n  \
        gnp-client probe --skip-mtu          # 只测 hy2 握手\n  \
        gnp-client probe --sizes 1200,1240,1280 --count 20")]
    Probe {
        /// 尺寸档 (逗号分隔, B)
        #[arg(long, default_value = "1200,1240,1280,1332,1400")]
        sizes: String,
        /// 每档 UDP 包数
        #[arg(long, default_value_t = 10)]
        count: u32,
        /// 读服务端计数器的 ssh 用户
        #[arg(long, default_value = "lw")]
        ssh_user: String,
        /// 服务端 ssh 端口
        #[arg(long, default_value_t = 22)]
        ssh_port: u16,
        /// 跳过 hy2 握手测试
        #[arg(long)]
        skip_handshake: bool,
        /// 跳过 MTU 扫描
        #[arg(long)]
        skip_mtu: bool,
    },
    /// 手动通道切换 (manual override: auto/hy2/ssh)
    #[command(long_about = "手动切换出站通道 (urltest 的 manual override)。\n\n\
        通道:\n  \
        auto → 交还 urltest 自动选路 (默认, hy2 快则 hy2, 死则自动落 ssh)\n  \
        hy2  → 强制 hy2-out (QUIC 主通道)\n  \
        ssh  → 强制 ssh-out (TCP 兜底, 免疫 UDP MTU 黑洞)\n\n\
        实现: clash_api 热切换 (立即生效) + config selector default 写回 (重启仍生效)。\n\n\
        示例:\n  \
        gnp-client switch            # 查看当前通道\n  \
        gnp-client switch ssh        # 强制 TCP 兜底\n  \
        gnp-client switch auto       # 恢复自动选路")]
    Switch {
        /// 目标通道: auto | hy2 | ssh (缺省显示当前)
        target: Option<String>,
    },
    /// 通道看门狗 (退避探测 + 故障冻结 + 告警; cron 每分钟)
    #[command(long_about = "通道看门狗: 探测 hy2 健康, 故障自动冻结到 ssh 兜底, 恢复自动回切。\n\n\
        设计为 cron 每分钟调用一次 (单 tick, 无常驻进程):\n  \
        1. hy2 探测失败 → 指数退避 (1m→2m→4m→…→cap 1h, ±20% 抖动) 掐掉重连风暴\n  \
        2. 连续 2 次失败 → selector 冻结到 ssh-out (urltest 3m 周期太久)\n  \
        3. 恢复探测通过 → 自动解冻交还 urltest\n  \
        4. 状态翻转告警 (GNP_ALERT_CMD 钩子 / macOS 通知), 全程落 guard.log\n\n\
        示例:\n  \
        gnp-client guard                 # 手动跑一次 tick\n  \
        gnp-client guard --install-cron  # 安装每分钟 cron")]
    Guard {
        /// 安装每分钟 cron 任务
        #[arg(long)]
        install_cron: bool,
    },
    /// 从 gnp.cfg 接入（peer）
    #[command(long_about = "读取 gnp-server gen-user 生成的 gnp.cfg，一键完成安装。\n\n\
        cfg 字段: user-name / server-ip / server-port / peer-key\n\n\
        流程 = install: 下载 sing-box → 规则集 → 生成 config.json → 装服务\n\n\
        示例:\n          gnp-client peer gnp.cfg\n          gnp-client peer ~/Downloads/gnp.cfg --name 自定义本机名(仅显示用)")]
    Peer {
        /// gnp.cfg 路径
        cfg: String,
    },
    /// 安装 sing-box + 规则集 + 生成配置
        #[command(long_about = "下载 sing-box 二进制 + 规则集, 生成双通道配置。\n\n\
            行为步骤:\n  \
            1. 下载 sing-box (with_quic, 支持 hysteria2)\n  \
            2. 下载规则集 (geosite-cn, geoip-cn, google, github, openai 等)\n  \
            3. 生成 config.json (2026-10-05 P0 双通道形态):\n     \
               route.final → proxy-out (selector)\n       \
               → auto-out (urltest: hy2-out + ssh-out 自动降级/回切)\n       \
               → hy2-out (QUIC, 可带 salamander obfs) + ssh-out (TCP 兜底)\n  \
            4. Linux 自动安装 systemd 系统级服务\n\n\
            ssh 兜底默认开 (复用服务端 sshd, 密钥 ~/.ssh/id_ed25519, user=lw):\n  \
            密钥不存在时 urltest 自动忽略 ssh-out, 仅日志噪音, 无需关闭。\n\n\
            示例:\n  \
            gnp-client install --server 8.209.203.17 --password <密码> --server-port 443 --obfs-password <obfs密码>")]
        Install {
            /// 远端 hysteria2 server 地址 (IP 或域名)
            #[arg(long)]
            server: String,
            /// hysteria2 密码
            #[arg(long)]
            password: String,
            /// server 端口 (默认 443, hysteria2/QUIC UDP)
            #[arg(long, default_value_t = 443)]
            server_port: u16,
            /// salamander obfs 密码 (须与服务端 inbound 一致)
            #[arg(long)]
            obfs_password: Option<String>,
            /// mixed 监听地址
            #[arg(long, default_value = "0.0.0.0")]
            listen: String,
            /// 本地域名预定义解析 (aipro.host=192.168.1.2,lwtop.host=8.209.203.17)
            #[arg(long)]
            hosts: Option<String>,
            /// 关闭 ssh 兜底 (单通道, 旧行为)
            #[arg(long, default_value_t = false)]
            no_ssh_fallback: bool,
            /// ssh 兜底端口
            #[arg(long, default_value_t = 22)]
            ssh_port: u16,
            /// ssh 兜底用户
            #[arg(long, default_value = "lw")]
            ssh_user: String,
            /// ssh 私钥路径 (默认 ~/.ssh/id_ed25519)
            #[arg(long)]
            ssh_key: Option<String>,
            /// 关闭 clash_api (switch/status/guard 依赖它)
            #[arg(long, default_value_t = false)]
            no_clash_api: bool,
            /// 只生成配置文件到指定路径 (跳过规则下载/服务安装; 跨机部署用:
            /// HOME=<目标机home> gnp-client install ... --out /tmp/x.json)
            #[arg(long)]
            out: Option<String>,
            /// 只下载 sing-box (不生成配置)
            #[arg(long)]
            bin_only: bool,
        },
    /// 自动注册新机器 (从 gitee peer 池取配置)
    #[command(long_about = "从 gitee 私有仓库的 peer 池自动取配置, 一键完成安装。\n\n\
        工作流程:\n  \
        1. 克隆 gitee 私有仓库 (需要 GITEE_TOKEN)\n  \
        2. 读取 peers/ 目录下的 peer JSON\n  \
        3. 选择一个 status=available 的 peer\n  \
        4. 标记为 used 并 push 回 gitee\n  \
        5. 生成 config.json + 下载 sing-box + 规则集\n  \
        6. 安装 systemd 服务 (Linux)\n\n\
        重要: register 完成后, 需要在 server 上执行:\n  \
        sudo gnp-server activate <client_id>\n\n\
        示例:\n  \
        export GITEE_TOKEN=xxxx\n  \
        gnp-client register --client-id macbook\n  \
        gnp-client register --list       # 查看 peer 池\n  \
        gnp-client register --dry-run    # 试运行")]
    Register {
        /// client_id (可选, 默认用 hostname)
        #[arg(long)]
        client_id: Option<String>,
        /// 列出 peer 池状态
        #[arg(long)]
        list: bool,
        /// 只看会选中哪个, 不实际修改
        #[arg(long)]
        dry_run: bool,
    },
    /// 规则集更新 + sing-box 守护
    #[command(long_about = "更新 sing-box 规则集 (geosite/geoip), 或检查守护进程。\n\n\
        三种模式 (互斥, 按顺序匹配):\n  \
        --install-cron  安装 crontab (每天 04:00 检查)\n  \
        --update        强制重启 sing-box (触发 remote rule-set 重新拉取)\n  \
        --check / 默认   检查 sing-box 是否运行, 挂了就重启\n\n\
        示例:\n  \
        gnp-client update-rules                  # 检查并守护\n  \
        gnp-client update-rules --update         # 强制更新规则集\n  \
        gnp-client update-rules --install-cron   # 安装每日 cron")]
    UpdateRules {
        /// 强制更新规则集并重启
        #[arg(long)]
        update: bool,
        /// 检查 sing-box 状态, 挂了就重启
        #[arg(long)]
        check: bool,
        /// 安装 cron (每天 04:00 检查)
        #[arg(long)]
        install_cron: bool,
    },
    /// 应急清理 (参考 aipro 事故)
    #[command(long_about = "彻底清理 sing-box 所有残留。\n\n\
        参考: aipro 2026-08-10 sing-box tun 破坏路由导致完全断网事故。\n\n\
        清理步骤 (6 步):\n  \
        1. 停止 sing-box 服务 + 杀进程\n  \
        2. 禁用开机自启\n  \
        3. 清理残留 tun 接口 (gnp0, tun0)\n  \
        4. 清理策略路由 (ip rule priority 9000-9010, table 2022)\n  \
        5. 恢复主网卡默认路由\n  \
        6. 备份并禁用 sing-box 数据目录\n\n\
        注意: 此命令会彻底清除 sing-box, 之后需重新安装。\n\n\
        示例:\n  \
        gnp-client cleanup")]
    Cleanup,
    /// 断网恢复 (参考 aipro 事故)
    #[command(long_about = "sing-box tun 模式破坏路由表后的网络恢复工具。\n\n\
        恢复步骤 (5 步):\n  \
        1. 停止 sing-box 服务 (破坏路由的元凶)\n  \
        2. 清理策略路由 (ip rule flush)\n  \
        3. 清理独立路由表 (table 2022/100/200)\n  \
        4. 恢复主网卡默认路由 (尝试常见网关)\n  \
        5. 清理残留 tun 接口 (gnp0, tun0) + 恢复 DNS\n\n\
        如果 recover 后仍不通, 直接 reboot 重启机器。\n\n\
        示例:\n  \
        gnp-client recover")]
    Recover,
    /// CLI 环境变量代理开关 (curl/pip/npm/git 等)
    #[command(long_about = "为 CLI 工具 (curl/pip/uv/npm/git/docker 等) 输出代理环境变量。\n\n\
        这些工具不读系统代理设置, 只认环境变量:\n  \
        http_proxy / https_proxy / all_proxy → 127.0.0.1:1080\n\n\
        用法:\n          eval \"$(gnp-client env --on)\"    # 当前 shell 生效\n          eval \"$(gnp-client env --off)\"   # 取消\n          eval \"$(gnp-client env --hook)\"  # 装入 gnp-on/gnp-off 快捷函数(放 .zshrc/.bashrc)\n\n\
        示例 (.zshrc):\n          eval \"$(~/.local/bin/gnp-client env --hook)\"\n\n\
        注: 国内流量无需加 no_proxy —— sing-box 已按 geosite-cn 分流直连。")]
    Env {
        /// 输出 export 环境变量 (eval 用)
        #[arg(long)]
        on: bool,
        /// 输出 unset 环境变量 (eval 用)
        #[arg(long)]
        off: bool,
        /// 输出 gnp-on/gnp-off shell 函数 (eval 用)
        #[arg(long)]
        hook: bool,
    },
    /// 系统代理开关 (macOS 系统代理 / Linux GNOME 代理)
    #[command(long_about = "设置或取消操作系统层面的代理, 让浏览器等 GUI 程序走 sing-box。\n\n\
        平台差异:\n  \
        macOS  → networksetup + osascript 弹管理员授权 (不存密码)\n  \
                 设置 HTTP/HTTPS/SOCKS 代理为 127.0.0.1:1080\n  \
        Linux  → gsettings 设置 GNOME 系统代理 (manual 模式)\n  \
                 无 GNOME 时提示 export 环境变量\n\n\
        示例:\n  \
        gnp-client proxy --on      # 开启系统代理\n  \
        gnp-client proxy --off     # 关闭系统代理\n  \
        gnp-client proxy --status  # 查看当前状态")]
    Proxy {
        /// 开启系统代理
        #[arg(long)]
        on: bool,
        /// 关闭系统代理
        #[arg(long)]
        off: bool,
        /// 查看代理状态
        #[arg(long)]
        status: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Start => cmd_start(),
        Commands::Stop => cmd_stop(),
        Commands::Status => cmd_status(),
        Commands::Config { show, check } => cmd_config(show, check),
        Commands::Tunnel => cmd_tunnel(),
        Commands::Test => cmd_test(),
        Commands::Probe {
            sizes,
            count,
            ssh_user,
            ssh_port,
            skip_handshake,
            skip_mtu,
        } => probe::run(probe::ProbeArgs {
            sizes: sizes
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect(),
            count,
            ssh_user,
            ssh_port,
            skip_handshake,
            skip_mtu,
        }),
        Commands::Switch { target } => switch::run(target),
        Commands::Guard { install_cron } => guard::run(install_cron),
        Commands::Peer { cfg } => cmd_peer(&cfg),
        Commands::Install {
            server,
            password,
            server_port,
            obfs_password,
            listen,
            hosts,
            no_ssh_fallback,
            ssh_port,
            ssh_user,
            ssh_key,
            no_clash_api,
            out,
            bin_only,
        } => cmd_install(install::ClientConfigParams {
            server: server.clone(),
            password,
            server_port,
            obfs_password,
            listen,
            hosts: hosts.map(|h| parse_hosts(&h)).unwrap_or_default(),
            ssh_fallback: if no_ssh_fallback {
                None
            } else {
                Some(install::SshFallback {
                    server: server.clone(),
                    server_port: ssh_port,
                    user: ssh_user,
                    private_key_path: ssh_key
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(default_ssh_key),
                })
            },
            clash_api_port: if no_clash_api { None } else { Some(9090) },
            output: out.map(std::path::PathBuf::from),
        }, bin_only),
        Commands::Register {
            client_id,
            list,
            dry_run,
        } => register::run(&register::RegisterArgs {
            client_id,
            list,
            dry_run,
        }),
        Commands::UpdateRules {
            update,
            check: _,
            install_cron,
        } => {
            if install_cron {
                update_rules::cmd_install_cron()
            } else if update {
                update_rules::cmd_update()
            } else {
                update_rules::cmd_check()
            }
        }
        Commands::Cleanup => {
            if cfg!(windows) {
                println!("ℹ️  cleanup 针对 macOS/Linux 的 tun 路由事故; Windows 上 sing-box 只监听端口,");
                println!("   无路由劫持风险。如需卸载: gnp-client stop 后删除 %USERPROFILE%\\.local\\share\\sing-box");
                Ok(())
            } else {
                cleanup::run()
            }
        }
        Commands::Recover => {
            if cfg!(windows) {
                println!("ℹ️  recover 针对 macOS/Linux 的 tun 路由事故恢复; Windows 无此风险, 无需使用。");
                Ok(())
            } else {
                recover::run()
            }
        }
        Commands::Env { on, off, hook } => cmd_env(on, off, hook),
        Commands::Proxy { on, off, status } => cmd_proxy(on, off, status),
    }
}

/// 默认 ssh 私钥路径 (~/.ssh/id_ed25519)
fn default_ssh_key() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".ssh/id_ed25519")
}

/// 解析 --hosts "a.host=1.2.3.4,b.host=5.6.7.8"
fn parse_hosts(s: &str) -> Vec<(String, String)> {
    s.split(',')
        .filter_map(|pair| {
            let (k, v) = pair.trim().split_once('=')?;
            Some((k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

/// 从 gnp.cfg 接入
fn cmd_peer(cfg_path: &str) -> Result<()> {
    let content = std::fs::read_to_string(cfg_path)
        .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", cfg_path, e))?;

    // 极简 key = value 解析（# 注释行忽略）
    let mut kv = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let server = kv.get("server-ip").context("cfg 缺少 server-ip")?.clone();
    let password = kv.get("peer-key").context("cfg 缺少 peer-key")?.clone();
    let port: u16 = kv
        .get("server-port")
        .map(|s| s.parse().unwrap_or(443))
        .unwrap_or(443);
    let user = kv.get("user-name").cloned().unwrap_or_else(|| "peer".into());

    println!("== gnp peer 接入 ({} → {}:{}) ==", user, server, port);

    // 复用 install 流水线
    let platform = platform::ensure_supported()?;
    if !platform::sb_exists() {
        install::install_singbox(None)?;
    } else {
        println!("✅ sing-box 已存在: {}", platform::sb_bin().display());
    }
    install::install_rules()?;
    let mut params = install::ClientConfigParams::new(&server, &password, port);
    // gnp.cfg 可选携带 obfs-pass (服务端 inbound 开 salamander 时必填)
    params.obfs_password = kv.get("obfs-pass").cloned();
    install::generate_config(&params)?;
    if platform == platform::Platform::Linux {
        service::install_linux()?;
    }
    println!("\n✅ peer 接入完成! 运行 `gnp-client start` 启动");
    println!("   验证: gnp-client test");
    Ok(())
}

/// 安装
fn cmd_install(params: install::ClientConfigParams, bin_only: bool) -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== gnp-client 安装 ({}) ==", platform.as_str());

    // 1. 下载 sing-box (--out 只生成配置模式跳过, 供跨机部署)
    if params.output.is_none() {
        if !platform::sb_exists() {
            install::install_singbox(None)?;
        } else {
            println!(
                "✅ sing-box 已存在: {}",
                platform::sb_bin().display()
            );
        }
    }

    // 2. 下载规则集 (--out 只生成配置模式跳过, 供跨机部署)
    if params.output.is_none() {
        install::install_rules()?;
    }

    // 3. 生成配置 (除非 bin_only)
    if !bin_only {
        install::generate_config(&params)?;
        if params.output.is_some() {
            return Ok(());
        }
        println!("✅ 配置已生成, 运行 `gnp-client start` 启动");
    } else {
        println!("✅ 只安装了二进制 (--bin-only), 未生成配置");
    }

    // 4. Linux: 安装 systemd 系统服务 (开机自启, 无需 root)
    if platform == platform::Platform::Linux {
        service::install_linux()?;
    }
    Ok(())
}

/// 启动
fn cmd_start() -> Result<()> {
    let platform = platform::ensure_supported()?;
    platform::ensure_installed()?;
    println!("♻️  启动 sing-box ({})...", platform.as_str());
    service::start(platform)?;
    println!("✅ sing-box 已启动 (socks5+http on 127.0.0.1:1080)");
    Ok(())
}

/// 停止
fn cmd_stop() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("⏹️  停止 sing-box ({})...", platform.as_str());
    service::stop(platform)?;
    println!("✅ sing-box 已停止");
    Ok(())
}

/// 状态
fn cmd_status() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== gnp-client 状态 ({}) ==", platform.as_str());

    // 1. 二进制/配置
    println!("\n📦 安装:");
    println!(
        "  sing-box 二进制: {}",
        if platform::sb_exists() { "已安装 ✅" } else { "未安装 ❌" }
    );
    println!(
        "  配置文件: {}",
        if platform::config_exists() { "存在 ✅" } else { "缺失 ❌" }
    );

    // 2. 进程状态
    let running = service::is_running(platform)?;
    println!("\n🔄 进程:");
    println!(
        "  运行状态: {}",
        if running { "运行中 ✅" } else { "已停止" }
    );

    // 3. 端口
    let port_open = check_port(1080);
    println!(
        "  端口 1080: {}",
        if port_open { "监听中 ✅" } else { "未监听" }
    );

    // 4. 配置安全检查
    if platform::config_exists() {
        let cfg_path = platform::sb_config();
        if let Ok(v) = config::load(&cfg_path) {
            let safe = config::is_safe(&v);
            let has_mixed = config::has_mixed_inbound(&v);
            let has_hy2 = config::has_hy2_endpoint(&v);
            println!("\n🔒 配置安全:");
            println!(
                "  无 tun/strict_route: {}",
                if safe { "✅" } else { "❌ 危险!" }
            );
            println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
            println!("  hysteria2 outbound: {}", if has_hy2 { "✅" } else { "❌" });
        }
    }

    // 5. 隧道状态 (如果运行中)
    if running {
        println!("\n🌐 隧道:");
        match test_proxy_simple() {
            Ok((ip, ms)) => println!("  出口 IP: {} ({}ms)", ip, ms),
            Err(e) => println!("  出口检测失败: {}", e),
        }
    }

    // 6. 通道状态 (双通道配置: selector/urltest + 各通道实时延迟)
    if running {
        if let Ok(v) = config::load(&platform::sb_config()) {
            if let Some(sel) = config::find_outbound(&v, "selector") {
                println!("\n🔀 通道:");
                println!("  route.final: {}", config::final_outbound(&v).unwrap_or_default());
                println!(
                    "  selector 默认: {}",
                    sel.get("default").and_then(|d| d.as_str()).unwrap_or("?")
                );
                if let Some(addr) = api::controller_addr() {
                    if let Ok(proxies) = api::api_get_json(&addr, "/proxies", 2) {
                        if let Some(now) = proxies.get("proxy-out").and_then(|p| p.get("now")).and_then(|n| n.as_str()) {
                            println!("  当前生效 (热): {}", now);
                        }
                        if let Some(auto) = proxies.get("auto-out").and_then(|p| p.get("now")).and_then(|n| n.as_str()) {
                            println!("  urltest 选中: {}", auto);
                        }
                        for tag in ["hy2-out", "ssh-out"] {
                            let delay = proxies
                                .get(tag)
                                .and_then(|p| p.get("history"))
                                .and_then(|h| h.as_array())
                                .and_then(|arr| arr.last())
                                .and_then(|e| e.get("delay"))
                                .and_then(|d| d.as_u64());
                            match delay {
                                Some(ms) => println!("  {}: {}ms ✅", tag, ms),
                                None => println!("  {}: 最近探测超时/失败 ❌", tag),
                            }
                        }
                    }
                } else {
                    println!("  (旧配置无 clash_api — 重刷配置后可见热状态)");
                }
            }
        }
    }

    Ok(())
}

/// 配置
fn cmd_config(show: bool, check: bool) -> Result<()> {
    let cfg_path = platform::sb_config();
    if !cfg_path.exists() {
        anyhow::bail!("配置文件不存在: {}", cfg_path.display());
    }
    let v = config::load(&cfg_path)?;

    if check {
        let safe = config::is_safe(&v);
        let has_mixed = config::has_mixed_inbound(&v);
        let has_hy2 = config::has_hy2_endpoint(&v);
        println!("== 配置校验 ==");
        println!(
            "  无 tun/strict_route: {}",
            if safe { "✅" } else { "❌ 危险!" }
        );
        println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
        println!(
            "  hysteria2 outbound: {}",
            if has_hy2 { "✅" } else { "❌" }
        );
        if !safe {
            anyhow::bail!("检测到危险配置 (tun/strict_route)! 请立即修复。");
        }
        println!("✅ 配置安全且完整");
        return Ok(());
    }

    if show {
        println!("== 当前配置 ({}) ==", cfg_path.display());
        let pretty = serde_json::to_string_pretty(&v)?;
        println!("{}", pretty);
    }
    Ok(())
}

/// 隧道诊断 (Hysteria2/QUIC; 旧名 wg)
fn cmd_tunnel() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== 隧道诊断 ({}) ==", platform.as_str());

    // 读配置
    if platform::config_exists() {
        let v = config::load(&platform::sb_config())?;
        if let Some(hy2) = config::extract_hy2_endpoint(&v) {
            println!("\n📋 隧道配置:");
            println!("  远端 server: {}:{}", hy2.server, hy2.server_port);
            println!(
                "  密码: 已配置 ({} 字符)",
                hy2.password.len()
            );
        }
    }

    // 检测出口
    println!("\n🌐 隧道连通性:");
    match test_proxy_simple() {
        Ok((ip, ms)) => println!("  ✅ 出口 IP: {} ({}ms)", ip, ms),
        Err(e) => println!("  ❌ {}", e),
    }

    // 测试几个网站
    println!("\n🔍 代理测试:");
    let proxy = "socks5h://127.0.0.1:1080";
    for (name, url) in [
        ("github", "https://api.github.com/zen"),
        ("google", "https://www.google.com"),
    ] {
        match tunnel::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }

    Ok(())
}

/// 测试代理
fn cmd_test() -> Result<()> {
    println!(
        "== 代理连通性测试 (socks5://127.0.0.1:1080) =="
    );
    let proxy = "socks5h://127.0.0.1:1080";
    match test_proxy_simple() {
        Ok((ip, ms)) => println!("✅ 出口 IP: {} ({}ms)", ip, ms),
        Err(e) => {
            println!("❌ 代理不可用: {}", e);
            return Ok(());
        }
    }
    for (name, url) in [
        ("github", "https://api.github.com/zen"),
        ("google", "https://www.google.com"),
    ] {
        match tunnel::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }
    Ok(())
}

// --- 辅助函数 ---

/// CLI 环境变量代理开关
fn cmd_env(on: bool, off: bool, hook: bool) -> Result<()> {
    let url = "http://127.0.0.1:1080".to_string();
    let no_proxy = "localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,169.254.0.0/16";

    // 直接运行（输出到终端）时给引导而非裸 export —— 子进程改不了父 shell 环境
    if (on || off || hook)
        && std::io::stdout().is_terminal()
        && std::env::var("GNP_ENV_RAW").is_err()
    {
        println!("⚠️  env 的输出需要 eval 装载 —— 直接运行不会改变当前 shell：");
        if on {
            println!(r#"  eval "$(gnp-client env --on)"    # 或用快捷函数: gnp-on"#);
        }
        if off {
            println!(r#"  eval "$(gnp-client env --off)"   # 或用快捷函数: gnp-off"#);
        }
        if hook {
            println!(r#"  eval "$(gnp-client env --hook)"  # 放入 ~/.zshrc 得到 gnp-on/gnp-off"#);
        }
        println!("   (新终端已自动加载快捷函数；当前旧会话可 source ~/.zshrc)");
        return Ok(());
    }
    if on {
        println!("export http_proxy={}", url);
        println!("export https_proxy={}", url);
        println!("export all_proxy=socks5://127.0.0.1:1080");
        println!("export HTTP_PROXY=${{http_proxy}} HTTPS_PROXY=${{https_proxy}} ALL_PROXY=${{all_proxy}}");
        println!("export no_proxy={} NO_PROXY=${{no_proxy}}", no_proxy);
        return Ok(());
    }
    if off {
        for k in ["http_proxy","https_proxy","all_proxy","HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","no_proxy","NO_PROXY"] {
            println!("unset {}", k);
        }
        return Ok(());
    }
    if hook {
        println!("# gnp CLI 代理快捷函数 (由 gnp-client env --hook 生成, eval 装载)");
        println!("gnp-on()  {{ eval \"$(gnp-client env --on)\"; echo \"🌐 CLI 代理已开 (127.0.0.1:1080) – curl/pip/npm/git 生效\"; }}");
        println!("gnp-off() {{ eval \"$(gnp-client env --off)\"; echo \"⏹️  CLI 代理已关\"; }}");
        println!("# 提示: gnp-on 只对当前 shell 及其子进程生效");
        return Ok(());
    }
    // 无参数: 显示当前状态
    println!("== gnp CLI 代理环境 ==");
    let mut any = false;
    for k in ["http_proxy","https_proxy","all_proxy"] {
        match std::env::var(k) {
            Ok(v) => { println!("  {} = {} ✅", k, v); any = true; }
            Err(_) => println!("  {} = (未设置)", k),
        }
    }
    if any {
        println!("\n当前 shell 已走代理; 取消: eval \"$(gnp-client env --off)\"");
    } else {
        println!("\n当前 shell 未走代理。开启: eval \"$(gnp-client env --on)\"");
        println!("永久快捷: 把 eval \"$(gnp-client env --hook)\" 加入 ~/.zshrc (得到 gnp-on/gnp-off)");
    }
    Ok(())
}

/// 系统代理开关
fn cmd_proxy(on: bool, off: bool, status: bool) -> Result<()> {
    if on {
        proxy::enable()?;
    } else if off {
        proxy::disable()?;
    } else if status {
        proxy::status()?;
    } else {
        // 无参数: 显示状态和用法
        println!("== 系统代理管理 ==\n");
        let _ = proxy::status();
        println!("\n用法:");
        println!("  gnp-client proxy --on      开启系统代理 (浏览器走 sing-box)");
        println!("  gnp-client proxy --off     关闭系统代理");
        println!("  gnp-client proxy --status  查看当前代理状态");
    }
    Ok(())
}

/// 检查端口是否监听 (跨平台: 直接 TCP 连接探测, 不依赖 lsof/netstat)
fn check_port(port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let addr = format!("127.0.0.1:{}", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next());
    match addr {
        Some(a) => TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok(),
        None => false,
    }
}

/// 简单出口检测
fn test_proxy_simple() -> Result<(String, u64)> {
    tunnel::detect_exit_ip("socks5h://127.0.0.1:1080", 8)
}
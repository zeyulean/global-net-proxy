//! gnp-client — global-net-proxy client CLI
//!
//! 管理本机 sing-box mixed 代理 (wg 隧道)。
//! 安全原则: 只用 mixed 代理模式 (socks5+http on 1080), 不碰路由表, 零断网风险。

mod cleanup;
mod recover;
mod register;
mod update_rules;

use anyhow::Result;
use clap::{Parser, Subcommand};

use gnp_core::{config, install, platform, proxy, service, wg};

/// global-net-proxy client — 管理 sing-box mixed 代理 (wg 隧道)
///
/// 安全原则: 本工具只使用 mixed 代理模式 (socks5+http on 127.0.0.1:1080),
/// 不修改系统路由表, 零断网风险。绝不用 tun 模式。
///
/// 快速开始:
///   gnp-client start     # 启动代理 (开机自启)
///   gnp-client status    # 查看状态
///   gnp-client wg        # wg 隧道诊断
#[derive(Parser)]
#[command(
    name = "gnp-client",
    version,
    about = "global-net-proxy client (sing-box mixed 代理)",
    long_about = "管理本机 sing-box mixed 代理 (wg 隧道)。\n\
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
        gnp-client start           启动代理 (注册开机自启)\n  \
        gnp-client stop            停止代理\n  \
        gnp-client status          查看状态 (进程/端口/隧道/出口IP)\n  \
        gnp-client wg              WireGuard 隧道诊断\n  \
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
        4. 配置安全检查 (无 tun/strict_route, 有 mixed+wg)\n  \
        5. 隧道出口 IP 和延迟 (如果运行中)\n\n\
        示例:\n  \
        gnp-client status")]
    Status,
    /// 查看/校验配置
    #[command(long_about = "查看 sing-box 配置文件内容, 或校验配置是否安全。\n\n\
        校验项:\n  \
        - 无 tun/strict_route/auto_route (危险配置检测)\n  \
        - 有 mixed inbound (socks5+http)\n  \
        - 有 wg endpoint (WireGuard outbound)\n\n\
        示例:\n  \
        gnp-client config --check   # 校验配置安全性\n  \
        gnp-client config --show    # 显示完整 JSON 配置")]
    Config {
        /// 显示完整配置内容
        #[arg(long)]
        show: bool,
        /// 校验配置是否正确 (无 tun/strict_route, 有 mixed+wg)
        #[arg(long)]
        check: bool,
    },
    /// wg 隧道诊断
    #[command(long_about = "显示 WireGuard 隧道配置详情并测试连通性。\n\n\
        输出内容:\n  \
        1. 隧道配置 (本机 wg IP / 远端 server:port / MTU / 密钥状态)\n  \
        2. 隧道连通性 (通过 socks5h 检测出口 IP 和延迟)\n  \
        3. 代理测试 (github + google HTTP 状态码)\n\n\
        示例:\n  \
        gnp-client wg")]
    Wg,
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
    /// 安装 sing-box + 规则集 + 生成配置
    #[command(long_about = "下载 sing-box 二进制 + 规则集, 生成 mixed+wg 配置。\n\n\
        行为步骤:\n  \
        1. 下载 sing-box v1.12.3 (非 1.13, endpoint wg 有 bug)\n  \
        2. 下载规则集 (geosite-cn, geoip-cn, google, github, openai 等)\n  \
        3. 生成 config.json (mixed + wg outbound 格式)\n  \
        4. Linux 自动安装 systemd 系统级服务\n\n\
        需要 server 地址/公钥/本机私钥/IP。\n\n\
        示例:\n  \
        gnp-client install \\\n    \
        --server 8.209.203.17 \\\n    \
        --server-pubkey M/t3YYw... \\\n    \
        --client-privkey <你的私钥> \\\n    \
        --client-ip 10.0.0.5/32\n\n\
        注意: 私钥需要安全传输, 不要泄露。")]
    Install {
        /// 远端 wg server 地址 (IP 或域名)
        #[arg(long)]
        server: String,
        /// 远端 server 公钥
        #[arg(long)]
        server_pubkey: String,
        /// 本机私钥
        #[arg(long)]
        client_privkey: String,
        /// 本机 wg IP (如 10.0.0.5/32)
        #[arg(long)]
        client_ip: String,
        /// wg 端口 (默认 1194)
        #[arg(long, default_value_t = 1194)]
        wg_port: u16,
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
        Commands::Wg => cmd_wg(),
        Commands::Test => cmd_test(),
        Commands::Install {
            server,
            server_pubkey,
            client_privkey,
            client_ip,
            wg_port,
            bin_only,
        } => cmd_install(
            &server,
            &server_pubkey,
            &client_privkey,
            &client_ip,
            wg_port,
            bin_only,
        ),
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
            check,
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
        Commands::Cleanup => cleanup::run(),
        Commands::Recover => recover::run(),
        Commands::Proxy { on, off, status } => cmd_proxy(on, off, status),
    }
}

/// 安装
fn cmd_install(
    server: &str,
    server_pubkey: &str,
    client_privkey: &str,
    client_ip: &str,
    wg_port: u16,
    bin_only: bool,
) -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== gnp-client 安装 ({}) ==", platform.as_str());

    // 1. 下载 sing-box
    if !platform::sb_exists() {
        install::install_singbox(None)?;
    } else {
        println!(
            "✅ sing-box 已存在: {}",
            platform::sb_bin().display()
        );
    }

    // 2. 下载规则集
    install::install_rules()?;

    // 3. 生成配置 (除非 bin_only)
    if !bin_only {
        install::generate_config(server, server_pubkey, client_privkey, client_ip, wg_port)?;
        println!("✅ 配置已生成, 运行 `gnp-client start` 启动");
    } else {
        println!("✅ 只安装了二进制 (--bin-only), 未生成配置");
    }

    // 4. Linux: 安装 systemd 用户服务 (开机自启, 无需 root)
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
            let has_wg = config::has_wg_endpoint(&v);
            println!("\n🔒 配置安全:");
            println!(
                "  无 tun/strict_route: {}",
                if safe { "✅" } else { "❌ 危险!" }
            );
            println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
            println!("  wg endpoint: {}", if has_wg { "✅" } else { "❌" });
        }
    }

    // 5. 隧道状态 (如果运行中)
    if running {
        println!("\n🌐 wg 隧道:");
        match test_proxy_simple() {
            Ok((ip, ms)) => println!("  出口 IP: {} ({}ms)", ip, ms),
            Err(e) => println!("  出口检测失败: {}", e),
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
        let has_wg = config::has_wg_endpoint(&v);
        println!("== 配置校验 ==");
        println!(
            "  无 tun/strict_route: {}",
            if safe { "✅" } else { "❌ 危险!" }
        );
        println!("  mixed inbound: {}", if has_mixed { "✅" } else { "❌" });
        println!("  wg endpoint: {}", if has_wg { "✅" } else { "❌" });
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

/// wg 诊断
fn cmd_wg() -> Result<()> {
    let platform = platform::ensure_supported()?;
    println!("== wg 隧道诊断 ({}) ==", platform.as_str());

    // 读配置
    if platform::config_exists() {
        let v = config::load(&platform::sb_config())?;
        if let Some(wg) = config::extract_wg_endpoint(&v) {
            println!("\n📋 隧道配置:");
            println!("  本机 wg IP: {}", wg.address);
            println!("  远端 server: {}:{}", wg.peer_address, wg.peer_port);
            println!("  MTU: {}", wg.mtu);
            println!(
                "  密钥: 已配置 ({} 字符)",
                wg.private_key.len()
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
        match wg::test_proxy(proxy, url, 8) {
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
        match wg::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }
    Ok(())
}

// --- 辅助函数 ---

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

/// 检查端口是否监听
fn check_port(port: u16) -> bool {
    let out = std::process::Command::new("lsof")
        .args(["-i", &format!(":{}", port)])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// 简单出口检测
fn test_proxy_simple() -> Result<(String, u64)> {
    wg::detect_exit_ip("socks5h://127.0.0.1:1080", 8)
}
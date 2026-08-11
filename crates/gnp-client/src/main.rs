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

use gnp_core::{config, install, platform, service, wg};

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
    long_about = "管理本机 sing-box mixed 代理 (wg 隧道)。\n\n安全原则: 只用 mixed 代理模式 (socks5+http on 127.0.0.1:1080), 不碰路由表, 零断网风险。绝不用 tun 模式。\n\n数据目录: ~/.local/share/sing-box/\n- 二进制:  sing-box\n- 配置:    config.json\n- 规则集:  rules/*.srs"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动 sing-box 代理 (开机自启)
    ///
    /// macOS 用 launchctl, Linux 用 systemd 注册为开机自启服务。
    Start,
    /// 停止 sing-box 代理
    ///
    /// 卸载 launchctl/systemd 服务并杀掉残留进程。
    Stop,
    /// 查看状态 (进程/端口/隧道/出口IP)
    ///
    /// 显示: 安装状态, 进程状态, 端口 1080, 配置安全检查, wg 隧道出口。
    Status,
    /// 查看/校验配置
    Config {
        /// 显示完整配置内容
        #[arg(long)]
        show: bool,
        /// 校验配置是否正确 (无 tun/strict_route, 有 mixed+wg)
        #[arg(long)]
        check: bool,
    },
    /// wg 隧道诊断
    ///
    /// 显示隧道配置 (本机IP/远端server/MTU), 检测出口 IP, 测试 github/google。
    Wg,
    /// 测试代理连通性
    ///
    /// 检测出口 IP + 测试 github/google 是否可访问。
    Test,
    /// 安装 sing-box + 规则集 + 生成配置
    ///
    /// 下载 sing-box 二进制, 下载规则集, 生成 mixed+wg 配置。
    /// 需要提供 server 地址/公钥/本机私钥/IP。
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
    ///
    /// 从 gitee 私有仓库拉取预生成的 peer 配置池, 挑一个未使用的,
    /// 标记为 used 并 push, 然后自动安装 sing-box + 生成配置。
    ///
    /// 需要 GITEE_TOKEN 环境变量。
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
    ///
    /// 彻底清理 sing-box: 停服务、杀进程、清 tun/策略路由、
    /// 备份配置。用于 tun 模式破坏路由后的恢复。
    Cleanup,
    /// 断网恢复 (参考 aipro 事故)
    ///
    /// sing-box tun 破坏路由后恢复网络: 停服务、清策略路由、
    /// 恢复默认路由、清 tun 接口、恢复 DNS。
    Recover,
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
    let proxy = "socks5://127.0.0.1:1080";
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
    let proxy = "socks5://127.0.0.1:1080";
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
    wg::detect_exit_ip("socks5://127.0.0.1:1080", 8)
}
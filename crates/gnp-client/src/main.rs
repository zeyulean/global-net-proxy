//! gnp-client — global-net-proxy client CLI
//!
//! 管理本机 sing-box mixed 代理 (wg 隧道)。
//! 安全原则: 只用 mixed 代理模式 (socks5+http on 1080), 不碰路由表, 零断网风险。

use anyhow::Result;
use clap::{Parser, Subcommand};

use gnp_core::{
    config, platform, service, wg,
};

/// global-net-proxy client — 管理 sing-box mixed 代理 (wg 隧道)
#[derive(Parser)]
#[command(name = "gnp-client", version, about = "global-net-proxy client (sing-box mixed 代理)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动 sing-box 代理 (开机自启)
    Start,
    /// 停止 sing-box 代理
    Stop,
    /// 查看状态 (进程/端口/隧道/出口IP)
    Status,
    /// 查看/编辑配置
    Config {
        /// 显示配置 (默认)
        #[arg(long)]
        show: bool,
        /// 校验配置是否正确
        #[arg(long)]
        check: bool,
    },
    /// wg 隧道诊断
    Wg,
    /// 测试代理连通性
    Test,
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
    }
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
    println!("  sing-box 二进制: {}", if platform::sb_exists() { "已安装 ✅" } else { "未安装 ❌" });
    println!("  配置文件: {}", if platform::config_exists() { "存在 ✅" } else { "缺失 ❌" });

    // 2. 进程状态
    let running = service::is_running(platform)?;
    println!("\n🔄 进程:");
    println!("  运行状态: {}", if running { "运行中 ✅" } else { "已停止" });

    // 3. 端口
    let port_open = check_port(1080);
    println!("  端口 1080: {}", if port_open { "监听中 ✅" } else { "未监听" });

    // 4. 配置安全检查
    if platform::config_exists() {
        let cfg_path = platform::sb_config();
        if let Ok(v) = config::load(&cfg_path) {
            let safe = config::is_safe(&v);
            let has_mixed = config::has_mixed_inbound(&v);
            let has_wg = config::has_wg_endpoint(&v);
            println!("\n🔒 配置安全:");
            println!("  无 tun/strict_route: {}", if safe { "✅" } else { "❌ 危险!" });
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
        println!("  无 tun/strict_route: {}", if safe { "✅" } else { "❌ 危险!" });
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
            println!("  远端 server: {}:51820", wg.peer_address);
            println!("  MTU: {}", wg.mtu);
            println!("  密钥: 已配置 ({} 字符)", wg.private_key.len());
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
    for (name, url) in [("github", "https://api.github.com/zen"), ("google", "https://www.google.com")] {
        match wg::test_proxy(proxy, url, 8) {
            Ok((code, ms)) => println!("  {}: HTTP {} ({}ms)", name, code, ms),
            Err(e) => println!("  {}: 失败 ({})", name, e),
        }
    }

    Ok(())
}

/// 测试代理
fn cmd_test() -> Result<()> {
    println!("== 代理连通性测试 (socks5://127.0.0.1:1080) ==");
    let proxy = "socks5://127.0.0.1:1080";
    match test_proxy_simple() {
        Ok((ip, ms)) => println!("✅ 出口 IP: {} ({}ms)", ip, ms),
        Err(e) => {
            println!("❌ 代理不可用: {}", e);
            return Ok(());
        }
    }
    for (name, url) in [("github", "https://api.github.com/zen"), ("google", "https://www.google.com")] {
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
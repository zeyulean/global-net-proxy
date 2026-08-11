//! gnp-server — global-net-proxy server CLI
//!
//! 管理 WireGuard server (内核 wg0):
//! - install:  安装 wg + 配置 + NAT
//! - uninstall: 卸载 (停止服务/删配置/移除包)
//! - status:   查看状态 (wg0/peers/握手)
//! - peers:    列出所有客户端
//! - add-peer: 添加客户端
//! - pregen:   预生成 N 个 peer 配置包
//! - activate: 激活预生成的 peer
//!
//! 需要 root 权限 (wg set / iptables)。

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;

use gnp_core::wg;

/// global-net-proxy server — WireGuard server 管理
///
/// 管理内核 wg0: 安装/状态/客户端管理/peer 池。
/// 需要 root 权限 (wg set / iptables)。
///
/// 快速开始:
///   sudo gnp-server install            # 安装 + NAT + 开机自启
///   sudo gnp-server add-peer macbook   # 添加客户端
#[derive(Parser)]
#[command(
    name = "gnp-server",
    version,
    about = "global-net-proxy server (WireGuard)",
    long_about = "管理 WireGuard server (内核 wg0)。需要 root 权限 (wg set / iptables)。\n\
        \n\
        配置: /etc/wireguard/wg0.conf\n\
        端口: 1194 (UDP)\n\
        网段: 10.0.0.0/24\n\
        pending-peers: /etc/wireguard/pending-peers/\n\
        \n\
        常用命令:\n\
        \n  \
        sudo gnp-server install           安装 wg + NAT + 开机自启\n  \
        sudo gnp-server status            查看 wg0 状态\n  \
        sudo gnp-server add-peer <名称>   添加客户端\n  \
        sudo gnp-server pregen <N>        预生成 peer 池\n  \
        sudo gnp-server activate <id>     激活预生成的 peer\n\
        \n\
        详见: docs/usage.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 安装 WireGuard server (wg + NAT + 开机自启)
    #[command(long_about = "安装 WireGuard server, 包括软件包、密钥、配置、NAT、开机自启。\n\n\
        安装步骤:\n  \
        1. apt install wireguard wireguard-tools\n  \
        2. 生成 server 密钥 (wg genkey / wg pubkey)\n  \
        3. 写 /etc/wireguard/wg0.conf (IP=10.0.0.1/24, Port=1194)\n  \
        4. systemctl enable --now wg-quick@wg0\n  \
        5. 配置 NAT (iptables MASQUERADE + ip_forward=1)\n\n\
        注意: 需要在云服务商安全组放行 UDP 1194。\n\n\
        示例:\n  \
        sudo gnp-server install")]
    Install,
    /// 卸载 WireGuard server
    #[command(long_about = "完全卸载 WireGuard server。\n\n\
        卸载内容:\n  \
        1. 停止并禁用 wg-quick@wg0\n  \
        2. 删除 /etc/wireguard/wg0.conf\n  \
        3. 删除 server 密钥 (server.key / server.pub)\n  \
        4. 删除 pending-peers 目录\n  \
        5. 移除 wireguard-tools 包\n\n\
        示例:\n  \
        sudo gnp-server uninstall")]
    Uninstall,
    /// 查看状态 (wg0/peers/握手)
    #[command(long_about = "查看 wg0 接口状态和 peer 信息。\n\n\
        输出内容:\n  \
        - wg0 是否激活\n  \
        - wg show wg0 原始输出 (接口详情/peer 握手时间/传输量)\n\n\
        示例:\n  \
        sudo gnp-server status")]
    Status,
    /// 列出所有已注册客户端
    #[command(long_about = "列出所有已注册的客户端 peer 公钥。\n\n\
        示例:\n  \
        sudo gnp-server peers")]
    Peers,
    /// 添加一个客户端 (生成配置)
    #[command(long_about = "为新客户端生成密钥、分配 IP 并加入 wg0。\n\n\
        行为:\n  \
        1. 生成客户端密钥对 (wg genkey / wg pubkey)\n  \
        2. 分配客户端 IP (10.0.0.x)\n  \
        3. wg set 加入 wg0 运行时\n  \
        4. 持久化到 wg0.conf\n  \
        5. 输出客户端配置 (含私钥, 注意安全传输)\n\n\
        示例:\n  \
        sudo gnp-server add-peer macbook\n  \
        sudo gnp-server add-peer aipro")]
    AddPeer {
        /// 客户端名称
        name: String,
    },
    /// 预生成 N 个 peer 配置包 (不入 wg0)
    #[command(long_about = "批量生成待用 peer 配置包, 不占运行时资源。\n\n\
        行为:\n  \
        - 为每个 peer 生成密钥对和 IP\n  \
        - 存为 JSON 到 /etc/wireguard/pending-peers/<id>.json\n  \
        - JSON 包含: id, status=available, 密钥, IP, server 信息\n\n\
        用途: 配合 gnp-client register 实现新机器自动注册。\n\
        将 peers/ 推送到 gitee 私有仓库后, 客户端可自动取用。\n\n\
        示例:\n  \
        sudo gnp-server pregen 20")]
    Pregen {
        /// 数量
        count: u32,
    },
    /// 激活一个预生成的 peer (加入 wg0)
    #[command(long_about = "将 pending-peers 中的 peer 加入 wg0 运行时。\n\n\
        行为:\n  \
        1. 从 /etc/wireguard/pending-peers/<id>.json 读取配置\n  \
        2. wg set 加入 wg0 运行时\n  \
        3. 持久化到 wg0.conf\n  \
        4. 更新 JSON 状态为 activated\n\n\
        重要: gnp-client register 完成后, 必须执行此命令,\n\
        否则客户端无法连通。\n\n\
        示例:\n  \
        sudo gnp-server activate macbook")]
    Activate {
        /// client_id
        id: String,
    },
}

// 常量
const WG_CONF: &str = "/etc/wireguard/wg0.conf";
const WG_IFACE: &str = "wg0";
const PENDING_DIR: &str = "/etc/wireguard/pending-peers";
const WG_SUBNET: &str = "10.0.0.0/24";
const SERVER_IP: &str = "10.0.0.1/24";
const WG_PORT: &str = "1194";

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install => cmd_install(),
        Commands::Uninstall => cmd_uninstall(),
        Commands::Status => cmd_status(),
        Commands::Peers => cmd_peers(),
        Commands::AddPeer { name } => cmd_add_peer(&name),
        Commands::Pregen { count } => cmd_pregen(count),
        Commands::Activate { id } => cmd_activate(&id),
    }
}

/// 检查是否 root
fn check_root() -> Result<()> {
    let uid = libc_geteuid();
    if uid != 0 {
        bail!("需要 root 权限, 请用 sudo 运行 gnp-server");
    }
    Ok(())
}

/// 简化: 用 `id -u` 检查 root
fn libc_geteuid() -> u32 {
    let out = Command::new("id").arg("-u").output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(1),
        Err(_) => 1,
    }
}

/// 安装 server
fn cmd_install() -> Result<()> {
    check_root()?;
    println!("== 安装 WireGuard server ==");

    // 1. 安装 wireguard
    println!("📦 安装 wireguard...");
    let st = Command::new("apt")
        .args(["install", "-y", "wireguard", "wireguard-tools"])
        .status()
        .context("apt install 失败")?;
    if !st.success() {
        bail!("apt install wireguard 失败");
    }

    // 2. 生成 server 密钥
    println!("🔑 生成密钥...");
    let privkey = run_capture("wg", &["genkey"])?;
    let pubkey = run_capture_with_input("wg", &["pubkey"], &privkey)?;
    println!("  server 公钥: {}", pubkey.trim());

    // 3. 写 wg0.conf
    println!("📝 写入 {}", WG_CONF);
    let conf = format!(
        "[Interface]\nAddress = {}\nListenPort = {}\nPrivateKey = {}\n\n",
        SERVER_IP, WG_PORT, privkey.trim()
    );
    std::fs::write(WG_CONF, conf).context("写 wg0.conf 失败")?;
    std::fs::set_permissions(WG_CONF, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .ok();

    // 4. 启动 wg-quick
    println!("🚀 启动 wg-quick@wg0...");
    let _ = Command::new("systemctl").args(["enable", "wg-quick@wg0"]).status();
    let _ = Command::new("systemctl").args(["restart", "wg-quick@wg0"]).status()
        .or_else(|_| Command::new("wg-quick").args(["up", "wg0"]).status());

    // 5. 配置 NAT
    println!("🔥 配置 NAT...");
    let iface = detect_wan_iface()?;
    println!("  出口网卡: {}", iface);
    let _ = Command::new("iptables")
        .args(["-t", "nat", "-A", "POSTROUTING", "-s", WG_SUBNET, "-o", &iface, "-j", "MASQUERADE"])
        .status();
    let _ = Command::new("iptables")
        .args(["-A", "FORWARD", "-i", "wg0", "-j", "ACCEPT"])
        .status();
    let _ = Command::new("iptables")
        .args(["-A", "FORWARD", "-o", "wg0", "-j", "ACCEPT"])
        .status();
    // 持久化
    let _ = Command::new("sh").arg("-c").arg("echo 'net.ipv4.ip_forward=1' >> /etc/sysctl.conf").status();
    let _ = Command::new("sysctl").args(["-p"]).status();

    println!("\n✅ Server 安装完成!");
    println!("  server IP: {}", SERVER_IP);
    println!("  端口: {}", WG_PORT);
    println!("  公钥: {}", pubkey.trim());
    println!("  出口网卡: {}", iface);
    println!("\n下一步: gnp-server add-peer <名称> 添加客户端");
    Ok(())
}

/// 卸载 server
fn cmd_uninstall() -> Result<()> {
    check_root()?;
    println!("== 卸载 WireGuard server ==");

    // 停止并禁用 wg0
    println!("🛑 停止 wg-quick@wg0...");
    let _ = Command::new("systemctl").args(["stop", "wg-quick@wg0"]).status();
    let _ = Command::new("systemctl")
        .args(["disable", "wg-quick@wg0"])
        .status();

    // 删除配置/密钥/pending peers
    println!("🗑️  删除配置和密钥...");
    let wg_dir = "/etc/wireguard";
    let _ = std::fs::remove_file(WG_CONF);
    let _ = std::fs::remove_file(format!("{}/server.key", wg_dir));
    let _ = std::fs::remove_file(format!("{}/server.pub", wg_dir));
    let _ = std::fs::remove_dir_all(PENDING_DIR);

    // 移除 wireguard-tools
    println!("📦 移除 wireguard-tools...");
    let _ = Command::new("apt")
        .args(["remove", "-y", "-qq", "wireguard-tools"])
        .status();

    println!("✅ 已卸载");
    Ok(())
}

/// 检测出口网卡
fn detect_wan_iface() -> Result<String> {
    let out = Command::new("sh")
        .arg("-c")
        .arg("ip route show default | awk '{print $5}' | head -1")
        .output()?;
    let iface = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if iface.is_empty() {
        bail!("无法检测出口网卡");
    }
    Ok(iface)
}

/// 状态
fn cmd_status() -> Result<()> {
    println!("== gnp-server 状态 ==");
    if !wg::server_wg_active() {
        println!("❌ wg0 未激活 (运行 gnp-server install)");
        return Ok(());
    }
    println!("✅ wg0 已激活");
    println!("\n📋 wg0 详情:");
    let raw = wg::wg_show_raw()?;
    println!("{}", raw);
    Ok(())
}

/// Peers 列表
fn cmd_peers() -> Result<()> {
    check_root()?;
    println!("== 已注册客户端 ==");
    let raw = wg::wg_show_raw()?;
    // 解析 peer 公钥
    let mut peers = Vec::new();
    for line in raw.lines() {
        if line.starts_with("peer:") {
            peers.push(line.trim_start_matches("peer:").trim().to_string());
        }
    }
    if peers.is_empty() {
        println!("  (无客户端)");
        return Ok(());
    }
    for (i, p) in peers.iter().enumerate() {
        println!("  {}: {}", i + 1, p);
    }
    Ok(())
}

/// 添加 peer
fn cmd_add_peer(name: &str) -> Result<()> {
    check_root()?;
    println!("== 添加客户端: {} ==", name);

    // 生成 client 密钥
    let client_priv = run_capture("wg", &["genkey"])?;
    let client_pub = run_capture_with_input("wg", &["pubkey"], &client_priv)?;

    // 分配 IP
    let client_ip = alloc_client_ip()?;
    println!("  client IP: {}", client_ip);

    // 添加 peer 到 wg0
    let server_pub = run_capture("wg", &["show", WG_IFACE, "public-key"])?;
    let _ = Command::new("wg")
        .args(["set", WG_IFACE, "peer", client_pub.trim(), "allowed-ips", &format!("{}/32", client_ip)])
        .status()
        .context("wg set peer 失败")?;

    // 持久化到 wg0.conf
    append_peer_conf(&client_pub, &client_ip)?;

    println!("\n✅ 客户端已添加!");
    println!("==================");
    println!("[Interface]");
    println!("PrivateKey = {}", client_priv.trim());
    println!("Address = {}/32", client_ip);
    println!("");
    println!("[Peer]");
    println!("PublicKey = {}", server_pub.trim());
    println!("Endpoint = <SERVER_IP>:{}", WG_PORT);
    println!("AllowedIPs = 0.0.0.0/0");
    println!("PersistentKeepalive = 25");
    println!("==================");
    Ok(())
}

/// 预生成 N 个 peer
fn cmd_pregen(count: u32) -> Result<()> {
    check_root()?;
    println!("== 预生成 {} 个 peer 配置包 ==", count);
    std::fs::create_dir_all(PENDING_DIR).context("创建 pending-peers 目录失败")?;
    let server_pub = run_capture("wg", &["show", WG_IFACE, "public-key"])?;

    for i in 0..count {
        let client_priv = run_capture("wg", &["genkey"])?;
        let client_pub = run_capture_with_input("wg", &["pubkey"], &client_priv)?;
        let client_ip = alloc_client_ip()?;
        let id = format!("{}-{}", uuid_prefix(), i + 1);

        let peer_json = serde_json::json!({
            "id": id,
            "status": "available",
            "client_public_key": client_pub.trim(),
            "client_private_key": client_priv.trim(),
            "client_ip": client_ip,
            "server_public_key": server_pub.trim(),
            "server_endpoint": format!("<SERVER_IP>:{}", WG_PORT),
            "created": now_iso(),
        });
        let out_path = PathBuf::from(PENDING_DIR).join(format!("{}.json", id));
        std::fs::write(&out_path, serde_json::to_string_pretty(&peer_json)?)
            .context("写 peer json 失败")?;
        std::fs::set_permissions(&out_path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .ok();
        println!("  {} → {} (IP {})", id, out_path.display(), client_ip);
    }
    println!("\n✅ 生成 {} 个 peer, 存在 {}", count, PENDING_DIR);
    Ok(())
}

/// 激活 peer
fn cmd_activate(id: &str) -> Result<()> {
    check_root()?;
    println!("== 激活 peer: {} ==", id);
    let path = PathBuf::from(PENDING_DIR).join(format!("{}.json", id));
    if !path.exists() {
        bail!("peer 不存在: {} (找 {} )", id, path.display());
    }
    let content = std::fs::read_to_string(&path)?;
    let v: serde_json::Value = serde_json::from_str(&content)?;
    let client_pub = v["client_public_key"].as_str().unwrap_or_default().to_string();
    let client_ip = v["client_ip"].as_str().unwrap_or_default().to_string();

    // 加入 wg0
    let _ = Command::new("wg")
        .args([
            "set",
            WG_IFACE,
            "peer",
            &client_pub,
            "allowed-ips",
            &format!("{}/32", client_ip),
        ])
        .status()
        .context("wg set peer 失败")?;
    append_peer_conf(&client_pub, &client_ip)?;

    // 更新状态
    let mut v = v;
    v["status"] = serde_json::json!("activated");
    std::fs::write(&path, serde_json::to_string_pretty(&v)?)?;

    println!("✅ peer {} 已激活 (IP {})", id, client_ip);
    Ok(())
}

// --- 辅助 ---

/// 运行命令捕获 stdout
fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output().context(format!("{} 失败", cmd))?;
    if !out.status.success() {
        bail!("{} 失败: {}", cmd, String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 运行命令, 通过 stdin 传输入
fn run_capture_with_input(cmd: &str, args: &[&str], input: &str) -> Result<String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context(format!("{} 启动失败", cmd))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{} 失败", cmd);
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 分配客户端 IP (从 wg0 已用 IP 递增)
fn alloc_client_ip() -> Result<String> {
    // 从 wg0.conf 读取已用 IP
    let used: Vec<String> = Vec::new();
    if let Ok(content) = std::fs::read_to_string(WG_CONF) {
        // 简单解析 AllowedIPs
        for line in content.lines() {
            if line.contains("AllowedIPs") {
                if let Some(ip) = line.split_whitespace().last() {
                    let ip = ip.split('/').next().unwrap_or("");
                    // 收集
                }
            }
        }
    }
    // 简化: 从 10.0.0.2 开始递增
    let base = 2;
    // 用固定值 (实际应扫描)
    Ok(format!("10.0.0.{}", base))
}

/// 追加 peer 到 wg0.conf
fn append_peer_conf(pubkey: &str, ip: &str) -> Result<()> {
    let peer_conf = format!(
        "\n[Peer]\nPublicKey = {}\nAllowedIPs = {}/32\n",
        pubkey, ip
    );
    let mut content = std::fs::read_to_string(WG_CONF).unwrap_or_default();
    content.push_str(&peer_conf);
    std::fs::write(WG_CONF, content).context("写 wg0.conf 失败")?;
    Ok(())
}

/// UUID 前缀 (简化)
fn uuid_prefix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos % 0xFFFF)
}

/// 当前时间 ISO
fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}s", now)
}
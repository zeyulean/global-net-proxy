//! gnp-server — global-net-proxy server CLI
//!
//! 管理 sing-box hysteria2 (QUIC) server:
//! - install:   部署 sing-box + hysteria2 inbound + 自签证书 + systemd 服务
//! - uninstall: 卸载 (停止服务/删配置/删证书)
//! - status:    查看状态 (hy2 服务/443 端口)
//! - users:     列出所有 hysteria2 用户
//! - add-user:  添加用户 (生成密码)
//! - pregen:    预生成 N 个用户密码包
//! - activate:  激活预生成的用户
//!
//! 需要 root 权限 (写 /opt/gnp-quic, 监听 443 需 root)。

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;

use gnp_core::wg;

/// global-net-proxy server — Hysteria2 (QUIC) server 管理
///
/// 管理 sing-box hysteria2 inbound: 安装/状态/用户管理/密码池。
/// 需要 root 权限 (写 /opt/gnp-quic, 监听 443 UDP 需 root)。
///
/// 快速开始:
///   sudo gnp-server install            # 部署 sing-box + hysteria2 + systemd
///   sudo gnp-server add-user  macbook  # 添加用户
#[derive(Parser)]
#[command(
    name = "gnp-server",
    version,
    about = "global-net-proxy server (Hysteria2/QUIC)",
    long_about = "管理 sing-box hysteria2 (QUIC) server。需要 root 权限。\n\
        \n\
        配置: /opt/gnp-quic/config.json\n\
        证书: /opt/gnp-quic/certs/ (自签)\n\
        端口: 443 (UDP, QUIC)\n\
        pending-users: /opt/gnp-quic/pending-users/\n\
        \n\
        常用命令:\n\
        \n  \
        sudo gnp-server install           部署 sing-box + hysteria2 + 开机自启\n  \
        sudo gnp-server status            查看 hy2 服务状态\n  \
        sudo gnp-server add-user <名称>   添加用户\n  \
        sudo gnp-server pregen <N>        预生成用户密码池\n  \
        sudo gnp-server activate <id>     激活预生成的用户\n\
        \n\
        详见: docs/usage.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 部署 Hysteria2 server (sing-box + 证书 + systemd)
    #[command(long_about = "部署 sing-box hysteria2 (QUIC) server。\n\n\
        部署步骤:\n  \
        1. 下载 sing-box (with_quic) 到 /opt/gnp-quic/sing-box\n  \
        2. 生成自签证书 (openssl req -x509)\n  \
        3. 写 /opt/gnp-quic/config.json (hysteria2 inbound on UDP 443)\n  \
        4. 写 systemd 服务 (gnp-hy2) + enable --now\n  \
        5. 放行 UDP 443 (iptables)\n\n\
        注意: 需要在云服务商安全组放行 UDP 443。\n\n\
        示例:\n  \
        sudo gnp-server install")]
    Install,
    /// 卸载 Hysteria2 server
    #[command(long_about = "完全卸载 hysteria2 server。\n\n\
        卸载内容:\n  \
        1. 停止并禁用 gnp-hy2 服务\n  \
        2. 删除 /opt/gnp-quic 目录 (配置/证书/用户)\n\n\
        示例:\n  \
        sudo gnp-server uninstall")]
    Uninstall,
    /// 查看状态 (hy2 服务/443 端口)
    #[command(long_about = "查看 hysteria2 server 状态。\n\n\
        输出内容:\n  \
        - gnp-hy2 systemd 服务是否激活\n  \
        - UDP 443 是否监听\n  \
        - 服务详情 (进程/状态)\n\n\
        示例:\n  \
        sudo gnp-server status")]
    Status,
    /// 列出所有已注册用户
    #[command(long_about = "列出所有已注册的 hysteria2 用户密码。\n\n\
        示例:\n  \
        sudo gnp-server users")]
    Users,
    /// 添加一个用户 (生成密码)
    #[command(long_about = "为新用户生成 hysteria2 密码并加入 server 配置。\n\n\
        行为:\n  \
        1. 生成随机密码\n  \
        2. 加入 /opt/gnp-quic/config.json 的 users 列表\n  \
        3. 重启 gnp-hy2 服务\n  \
        4. 输出用户配置 (含密码, 注意安全传输)\n\n\
        示例:\n  \
        sudo gnp-server add-user macbook\n  \
        sudo gnp-server add-user aipro")]
    AddUser {
        /// 用户名称
        name: String,
    },
    /// 预生成 N 个用户密码包 (不加入 server)
    #[command(long_about = "批量生成待用用户密码包, 不占运行时资源。\n\n\
        行为:\n  \
        - 为每个用户生成随机密码\n  \
        - 存为 JSON 到 /opt/gnp-quic/pending-users/<id>.json\n  \
        - JSON 包含: id, status=available, password, server 信息\n\n\
        用途: 配合 gnp-client register 实现新机器自动注册。\n\
        将 peers/ 推送到 gitee 私有仓库后, 客户端可自动取用。\n\n\
        示例:\n  \
        sudo gnp-server pregen 20")]
    Pregen {
        /// 数量
        count: u32,
    },
    /// 激活一个预生成的用户 (加入 server)
    #[command(long_about = "将 pending-users 中的用户密码加入 server 配置。\n\n\
        行为:\n  \
        1. 从 /opt/gnp-quic/pending-users/<id>.json 读取配置\n  \
        2. 加入 /opt/gnp-quic/config.json 的 users 列表\n  \
        3. 重启 gnp-hy2 服务\n  \
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
const HY2_DIR: &str = "/opt/gnp-quic";
const HY2_CONF: &str = "/opt/gnp-quic/config.json";
const PENDING_DIR: &str = "/opt/gnp-quic/pending-users";
const CERTS_DIR: &str = "/opt/gnp-quic/certs";
const CERT_CRT: &str = "/opt/gnp-quic/certs/server.crt";
const CERT_KEY: &str = "/opt/gnp-quic/certs/server.key";
const SYSTEMD_UNIT: &str = "/etc/systemd/system/gnp-hy2.service";
const HY2_PORT: &str = "443";
const SERVER_IP: &str = "8.209.203.17";
const SB_BIN: &str = "/opt/gnp-quic/sing-box";

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install => cmd_install(),
        Commands::Uninstall => cmd_uninstall(),
        Commands::Status => cmd_status(),
        Commands::Users => cmd_users(),
        Commands::AddUser { name } => cmd_add_user(&name),
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

/// 下载 sing-box 到 /opt/gnp-quic/
fn install_singbox() -> Result<()> {
    if std::path::Path::new(SB_BIN).exists() {
        println!("✅ sing-box 已存在: {}", SB_BIN);
        return Ok(());
    }
    println!("📦 下载 sing-box (with_quic)...");
    std::fs::create_dir_all(HY2_DIR).context("创建 /opt/gnp-quic 失败")?;
    let tmp = format!("{}/sb.tar.gz", HY2_DIR);
    let url = "https://github.com/SagerNet/sing-box/releases/download/v1.13.16/sing-box-1.13.16-linux-amd64.tar.gz";
    let st = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .context("curl 下载 sing-box 失败")?;
    if !st.success() {
        bail!("下载 sing-box 失败");
    }
    let st = Command::new("tar")
        .args(["-xzf", &tmp, "-C", HY2_DIR])
        .status()
        .context("解压 sing-box 失败")?;
    if !st.success() {
        bail!("解压 sing-box 失败");
    }
    // 找到二进制并改名
    let found = find_sb_bin(std::path::Path::new(HY2_DIR))?;
    std::fs::copy(&found, SB_BIN).context("复制 sing-box 失败")?;
    let _ = std::fs::set_permissions(
        SB_BIN,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    );
    let _ = std::fs::remove_file(&tmp);
    println!("✅ sing-box 安装完成: {}", SB_BIN);
    Ok(())
}

/// 递归找 sing-box 二进制
fn find_sb_bin(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if let Ok(found) = find_sb_bin(&path) {
                return Ok(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("sing-box") {
            return Ok(path);
        }
    }
    bail!("解压目录中找不到 sing-box 二进制")
}

/// 生成自签证书
fn gen_cert() -> Result<()> {
    std::fs::create_dir_all(CERTS_DIR).context("创建 certs 目录失败")?;
    if std::path::Path::new(CERT_CRT).exists() && std::path::Path::new(CERT_KEY).exists() {
        println!("✅ 证书已存在");
        return Ok(());
    }
    println!("🔐 生成自签证书...");
    let st = Command::new("openssl")
        .args([
            "req", "-x509", "-nodes", "-newkey", "rsa:2048",
            "-keyout", CERT_KEY, "-out", CERT_CRT,
            "-days", "3650", "-subj", "/CN=gnp-quic",
        ])
        .status()
        .context("openssl 生成证书失败")?;
    if !st.success() {
        bail!("openssl 生成证书失败");
    }
    println!("✅ 证书已生成: {} / {}", CERT_CRT, CERT_KEY);
    Ok(())
}

/// 生成 hysteria2 server config
fn gen_server_config() -> Result<()> {
    let config = serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        "inbounds": [
            {
                "type": "hysteria2",
                "tag": "hy2-in",
                "listen": "::",
                "listen_port": 443,
                "users": [],
                "tls": {
                    "enabled": true,
                    "certificate_path": CERT_CRT,
                    "key_path": CERT_KEY
                }
            }
        ],
        "outbounds": [ { "type": "direct", "tag": "direct" } ]
    });
    // 若已有配置, 保留现有 users
    if let Ok(content) = std::fs::read_to_string(HY2_CONF) {
        if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(users) = existing.get("inbounds").and_then(|i| i.as_array())
                .and_then(|arr| arr.first().and_then(|inb| inb.get("users")))
            {
                let mut config = config;
                config["inbounds"][0]["users"] = users.clone();
                let out = serde_json::to_string_pretty(&config)?;
                std::fs::write(HY2_CONF, out)?;
                println!("✅ 配置已更新: {}", HY2_CONF);
                return Ok(());
            }
        }
    }
    let out = serde_json::to_string_pretty(&config)?;
    std::fs::write(HY2_CONF, out).context("写 config.json 失败")?;
    println!("✅ 配置已生成: {}", HY2_CONF);
    Ok(())
}

/// 写 systemd 服务
fn write_systemd_unit() -> Result<()> {
    let unit = format!(
        r#"[Unit]
Description=GNP Hysteria2 QUIC Server
After=network.target

[Service]
Type=simple
ExecStart={} run -c {}
Restart=always
RestartSec=3
User=root
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
        SB_BIN, HY2_CONF
    );
    std::fs::write(SYSTEMD_UNIT, unit).context("写 systemd 单元失败")?;
    println!("✅ systemd 单元已写入: {}", SYSTEMD_UNIT);
    Ok(())
}

/// 安装 server
fn cmd_install() -> Result<()> {
    check_root()?;
    println!("== 部署 Hysteria2 (QUIC) server ==");

    // 1. 下载 sing-box
    install_singbox()?;
    // 2. 生成证书
    gen_cert()?;
    // 3. 写配置
    gen_server_config()?;
    // 4. 写 systemd
    write_systemd_unit()?;
    // 5. 启动服务
    println!("🚀 启动 gnp-hy2 服务...");
    let _ = Command::new("systemctl").args(["daemon-reload"]).status();
    let _ = Command::new("systemctl").args(["enable", "gnp-hy2"]).status();
    let _ = Command::new("systemctl").args(["restart", "gnp-hy2"]).status();
    // 6. 放行 UDP 443
    println!("🔥 放行 UDP 443...");
    let _ = Command::new("iptables")
        .args(["-I", "INPUT", "-p", "udp", "--dport", HY2_PORT, "-j", "ACCEPT"])
        .status();
    let _ = Command::new("sh")
        .arg("-c")
        .arg("iptables-save > /etc/iptables/rules.v4 2>/dev/null")
        .status();

    println!("\n✅ Server 部署完成!");
    println!("  server: {}:{} (UDP/QUIC)", SERVER_IP, HY2_PORT);
    println!("  证书: {} / {}", CERT_CRT, CERT_KEY);
    println!("  配置: {}", HY2_CONF);
    println!("  服务: gnp-hy2 (systemd)");
    println!("\n下一步: gnp-server add-user <名称> 添加用户");
    Ok(())
}

/// 卸载 server
fn cmd_uninstall() -> Result<()> {
    check_root()?;
    println!("== 卸载 Hysteria2 server ==");

    // 停止并禁用服务
    println!("🛑 停止 gnp-hy2...");
    let _ = Command::new("systemctl").args(["stop", "gnp-hy2"]).status();
    let _ = Command::new("systemctl").args(["disable", "gnp-hy2"]).status();

    // 删除配置/证书/用户
    println!("🗑️  删除 {} ...", HY2_DIR);
    let _ = std::fs::remove_dir_all(HY2_DIR);
    // 删除 systemd 单元
    let _ = std::fs::remove_file(SYSTEMD_UNIT);
    let _ = Command::new("systemctl").args(["daemon-reload"]).status();

    println!("✅ 已卸载");
    Ok(())
}

/// 检测出口网卡
#[allow(dead_code)]
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
    println!("== gnp-server 状态 (Hysteria2/QUIC) ==");
    if !wg::hy2_server_active() {
        println!("❌ gnp-hy2 服务未激活 (运行 gnp-server install)");
        return Ok(());
    }
    println!("✅ gnp-hy2 服务已激活");
    if wg::port_443_listening() {
        println!("✅ UDP 443 正在监听");
    } else {
        println!("⚠️  UDP 443 未检测到监听!");
    }
    println!("\n📋 服务详情:");
    match wg::hy2_status_raw() {
        Ok(raw) => println!("{}", raw),
        Err(e) => println!("  (获取失败: {})", e),
    }
    Ok(())
}

/// 用户列表
fn cmd_users() -> Result<()> {
    check_root()?;
    println!("== 已注册用户 ==");
    let content = std::fs::read_to_string(HY2_CONF).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    let users = v
        .get("inbounds")
        .and_then(|i| i.as_array())
        .and_then(|arr| arr.first())
        .and_then(|inb| inb.get("users"))
        .and_then(|u| u.as_array())
        .cloned()
        .unwrap_or_default();
    if users.is_empty() {
        println!("  (无用户)");
        return Ok(());
    }
    for (i, u) in users.iter().enumerate() {
        let pwd = u.get("password").and_then(|p| p.as_str()).unwrap_or("?");
        println!("  {}: {}", i + 1, pwd);
    }
    Ok(())
}

/// 读取当前 users 列表
fn read_users() -> Result<Vec<serde_json::Value>> {
    let content = std::fs::read_to_string(HY2_CONF).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    Ok(v.get("inbounds")
        .and_then(|i| i.as_array())
        .and_then(|arr| arr.first())
        .and_then(|inb| inb.get("users"))
        .and_then(|u| u.as_array())
        .cloned()
        .unwrap_or_default())
}

/// 写入 users 列表并重启服务
fn write_users(users: Vec<serde_json::Value>) -> Result<()> {
    let content = std::fs::read_to_string(HY2_CONF).unwrap_or_default();
    let mut v: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    v["inbounds"][0]["users"] = serde_json::Value::Array(users);
    std::fs::write(HY2_CONF, serde_json::to_string_pretty(&v)?).context("写 config.json 失败")?;
    let _ = Command::new("systemctl").args(["restart", "gnp-hy2"]).status();
    Ok(())
}

/// 生成随机密码
fn gen_password() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // 简单随机: 时间戳 + 进程号 hash
    let pid = std::process::id();
    let h = |mut x: u128| -> u64 {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        x as u64
    };
    let a = h(nanos);
    let b = h(nanos.wrapping_add(pid as u128));
    format!("gnp-{:x}{:x}", a, b)
}

/// 添加用户
fn cmd_add_user(name: &str) -> Result<()> {
    check_root()?;
    println!("== 添加用户: {} ==", name);

    let password = gen_password();
    println!("  password: {}", password);

    // 加入 server 配置
    let mut users = read_users()?;
    if users
        .iter()
        .any(|u| u.get("password").and_then(|p| p.as_str()) == Some(password.as_str()))
    {
        bail!("密码冲突, 重新生成");
    }
    users.push(serde_json::json!({ "password": password }));
    write_users(users)?;

    println!("\n✅ 用户已添加!");
    println!("==================");
    println!("server: {}:{}", SERVER_IP, HY2_PORT);
    println!("password: {}", password);
    println!("==================");
    Ok(())
}

/// 预生成 N 个用户密码包
fn cmd_pregen(count: u32) -> Result<()> {
    check_root()?;
    println!("== 预生成 {} 个用户密码包 ==", count);
    std::fs::create_dir_all(PENDING_DIR).context("创建 pending-users 目录失败")?;

    for i in 0..count {
        let password = gen_password();
        let id = format!("{}-{}", uuid_prefix(), i + 1);

        let user_json = serde_json::json!({
            "id": id,
            "status": "available",
            "client_id": "",
            "password": password,
            "server_endpoint": format!("{}:{}", SERVER_IP, HY2_PORT),
            "created": now_iso(),
        });
        let out_path = PathBuf::from(PENDING_DIR).join(format!("{}.json", id));
        std::fs::write(&out_path, serde_json::to_string_pretty(&user_json)?)
            .context("写用户 json 失败")?;
        std::fs::set_permissions(&out_path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .ok();
        println!("  {} → {}", id, out_path.display());
    }
    println!("\n✅ 生成 {} 个用户, 存在 {}", count, PENDING_DIR);
    println!("  将 pending-users/ 复制为 peers/ 并推送到 gitee 供客户端使用");
    Ok(())
}

/// 激活用户
fn cmd_activate(id: &str) -> Result<()> {
    check_root()?;
    println!("== 激活用户: {} ==", id);
    let path = PathBuf::from(PENDING_DIR).join(format!("{}.json", id));
    if !path.exists() {
        bail!("用户不存在: {} (找 {} )", id, path.display());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut v: serde_json::Value = serde_json::from_str(&content)?;
    let password = v["password"].as_str().unwrap_or_default().to_string();

    // 加入 server
    let mut users = read_users()?;
    if users
        .iter()
        .any(|u| u.get("password").and_then(|p| p.as_str()) == Some(password.as_str()))
    {
        println!("⚠️  密码已存在于 server, 跳过加入");
    } else {
        users.push(serde_json::json!({ "password": password }));
        write_users(users)?;
    }

    // 更新状态
    v["status"] = serde_json::json!("activated");
    std::fs::write(&path, serde_json::to_string_pretty(&v)?)?;

    println!("✅ 用户 {} 已激活", id);
    Ok(())
}

// --- 辅助 ---

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
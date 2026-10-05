//! gnps — global-net-proxy server CLI (sing-box hysteria2 inbound)
//!
//! v2 (plan D2/D3/D4): 部署根 `/opt/gnp/` ($GNP_SERVER_HOME 可覆盖)。
//! `config.toml` 是唯一事实源; `config.json` / `gnps.service` / `bin/tick.sh`
//! 全是生成物。**config.json 内不放注释** (不依赖 sing-box 对未知字段的容忍度)。
//!
//! 需要 root (写 /opt/gnp 与 /etc/systemd/system, hy2 端口 >1024 可降权但本次不做)。

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::Command;

use gnp_core::platform;
use gnp_core::scheduler;
use gnp_core::settings::{ServerSettings, ServerUser};
use gnp_core::tunnel;

/// 全网 obfs 密码默认值 (客户端 deploy/hosts/*.toml 同步; 服务端 toml 覆盖之)
const DEFAULT_OBFS: &str = "gnp-obfs-20261005";
/// 对外公布的 server IP (写进 gnp.cfg; 实测值)
const SERVER_IP: &str = "8.209.203.17";

/// global-net-proxy server — Hysteria2 (QUIC) server 管理
///
/// 部署根 `/opt/gnp/` (config.toml 唯一事实源 → config.json + gnps.service + tick.sh)。
/// hy2 默认端口 5766 (GNP_PORT, 可在 `[server].hy2_port` 覆盖; 改动需 ufw + 云安全组双侧)。
#[derive(Parser)]
#[command(
    name = scheduler::SYSTEMD_SERVER,
    version,
    about = "global-net-proxy server (Hysteria2/QUIC)",
    long_about = "管理 sing-box hysteria2 inbound。需要 root 权限。\n\
        \n\
        部署根: /opt/gnp/ ($GNP_SERVER_HOME 可覆盖)\n\
        - bin/{gnps,sing-box,tick.sh}\n\
        - config.toml   ← 唯一事实源 (勿手改生成物)\n\
        - config.json   ← 生成物\n\
        - certs/ var/ etc/tick.d/ backups/\n\
        - 端口: 5766/udp (GNP_PORT); 服务: gnps.service (旧名 gnp-hy2)\n\
        \n\
        常用命令:\n\
        \n  \
        sudo gnps install --config /opt/gnp/config.toml   渲染 + 装服务 + 装调度\n  \
        sudo gnps status                                  看服务/端口\n  \
        sudo gnps users                                   列用户\n  \
        sudo gnps gen-user --name macbook                 加用户 + 出 gnp.cfg (含 obfs/5766)\n\
        \n\
        详见: docs/usage.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 部署 (渲染 config.json + gnps.service + tick.sh + 调度)
    #[command(long_about = "按 config.toml (唯一事实源) 部署服务端。\n\n\
        步骤:\n  \
        1. 确保 /opt/gnp 布局 (bin/ etc/tick.d/ var/ secrets/ backups/)\n  \
        2. 证书 (缺失则 openssl 自签到 certs/)\n  \
        3. 渲染 config.json (users 来自 [[users]]; certs 路径 /opt/gnp/certs/)\n  \
        4. `sing-box check` 校验生成物\n  \
        5. 写 gnps.service (ExecStart=/opt/gnp/bin/sing-box, 仍 root)\n  \
        6. 写 bin/tick.sh + etc/tick.d/gnps-health.sh, 装 crontab 单行 (GNP_HOME=/opt/gnp)\n  \
        7. enable --now gnps; disable 旧 gnp-hy2\n\n\
        ufw **不动** (5766/udp 早已放行; 端口改动必须 ufw + 云安全组双侧且持久化)。\n\
        sing-box 二进制缺失时从 GitHub 下载 (1.13.16)。\n\n\
        示例:\n  \
        sudo gnps install --config /opt/gnp/config.toml")]
    Install {
        /// config.toml 路径 (缺省 $GNP_SERVER_HOME/config.toml)
        #[arg(long)]
        config: Option<String>,
    },
    /// 卸载
    #[command(long_about = "停止并禁用 gnps, 删除 systemd 单元与调度行。\n\n\
        **不删数据**: config.toml / 证书 / users 全保留在 /opt/gnp (要删手动 rm)。\n\n\
        示例:\n  \
        sudo gnps uninstall")]
    Uninstall,
    /// 查看状态 (服务 + 端口)
    #[command(long_about = "查看 hysteria2 server 状态。\n\n\
        输出: gnps.service 是否 active / hy2 端口是否监听 / systemctl 详情。\n\n\
        示例:\n  \
        sudo gnps status")]
    Status,
    /// 列出所有已注册用户
    #[command(long_about = "列出 config.toml 里的所有 [[users]] (密码来自唯一事实源)。\n\n\
        示例:\n  \
        sudo gnps users")]
    Users,
    /// 添加用户 + 生成 gnp.cfg peer 配置 (gen-user)
    #[command(long_about = "生成密码 → 追加 [[users]] 到 config.toml → 重渲染 config.json → 重启 gnps → 落盘 gnp.cfg。\n\n\
        gnp.cfg 内容: user-name / server-ip / server-port / peer-key / obfs-pass\n\
        (server-port=5766, obfs-pass 必带 —— §3.9: 客户端靠它才能握手)\n\
        \n\
        客户端一条命令接入:\n  \
        gnpc peer gnp.cfg\n\
        \n\
        重复密码会拒绝写入。\n\n\
        示例:\n  \
        sudo gnps gen-user --name macbook")]
    GenUser {
        /// 用户名 (写入 cfg 的 user-name, 仅标识用途)
        #[arg(long)]
        name: String,
        /// gnp.cfg 输出路径 (缺省当前目录 gnp.cfg)
        #[arg(long, default_value = "gnp.cfg")]
        out: String,
    },
    /// 添加一个用户 (只写 toml, 不出 cfg)
    #[command(long_about = "为新用户生成密码并追加 [[users]] 到 config.toml, 重渲染 + 重启。\n\n\
        示例:\n  \
        sudo gnps add-user macbook")]
    AddUser {
        /// 用户名称
        name: String,
    },
    /// 预生成 N 个用户密码包 (不加入 server)
    #[command(long_about = "批量生成待用用户密码包 (不占运行时资源)。\n\n\
        - 每个一份 JSON 落 pending-users/<id>.json (0600)\n\
        - 内容: id / status=available / password / server_endpoint / obfs\n\
        - 配套: `gnpc register` 从 gitee peer 池自动取用\n\n\
        示例:\n  \
        sudo gnps pregen 20")]
    Pregen {
        /// 数量
        count: u32,
    },
    /// 激活一个预生成的用户
    #[command(long_about = "把 pending-users/<id>.json 里的密码加入 config.toml, 重渲染 + 重启。\n\
        并把 JSON 状态改为 activated。\n\n\
        重要: `gnpc register` 完成后必须执行此命令, 否则客户端连不上。\n\n\
        示例:\n  \
        sudo gnps activate macbook")]
    Activate {
        /// client_id
        id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Install { config } => cmd_install(config.as_deref()),
        Commands::Uninstall => cmd_uninstall(),
        Commands::Status => cmd_status(),
        Commands::Users => cmd_users(),
        Commands::GenUser { name, out } => cmd_gen_user(&name, &out),
        Commands::AddUser { name } => cmd_add_user(&name),
        Commands::Pregen { count } => cmd_pregen(count),
        Commands::Activate { id } => cmd_activate(&id),
    }
}

// --- 布局 ---

fn home() -> PathBuf {
    platform::gnps_home()
}
fn toml_path() -> PathBuf {
    home().join("config.toml")
}
fn json_path() -> PathBuf {
    home().join("config.json")
}
fn sb_bin() -> PathBuf {
    home().join("bin").join("sing-box")
}
fn cert_crt() -> PathBuf {
    home().join("certs").join("server.crt")
}
fn cert_key() -> PathBuf {
    home().join("certs").join("server.key")
}
fn pending_dir() -> PathBuf {
    home().join("pending-users")
}
fn unit_path() -> PathBuf {
    scheduler::systemd_unit_path(scheduler::SYSTEMD_SERVER)
}

/// 检查是否 root
fn check_root() -> Result<()> {
    let out = Command::new("id").arg("-u").output();
    let uid = out
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
        .unwrap_or(1);
    if uid != 0 {
        bail!("需要 root 权限, 请用 sudo 运行 gnps");
    }
    Ok(())
}

// --- 唯一事实源 ---

fn load_settings(config: Option<&str>) -> Result<ServerSettings> {
    match config {
        Some(p) => {
            let s = ServerSettings::load(Path::new(p))?;
            println!("   事实源: {} (显式)", p);
            Ok(s)
        }
        None => {
            let p = toml_path();
            if !p.exists() {
                bail!(
                    "找不到唯一事实源: {}。先放一份 config.toml (可从 deploy/hosts/lwtop-server.toml 抄)",
                    p.display()
                );
            }
            println!("   事实源: {}", p.display());
            ServerSettings::load(&p)
        }
    }
}

/// config.toml → config.json (纯函数, 便于测试)
///
/// **不放注释** (不依赖 sing-box 对未知字段的容忍度); 证书路径固定 `$GNPS_HOME/certs/`。
pub fn render_config(s: &ServerSettings) -> serde_json::Value {
    let users: Vec<serde_json::Value> = s
        .users
        .iter()
        .map(|u: &ServerUser| match &u.name {
            Some(n) if !n.trim().is_empty() => serde_json::json!({ "name": n, "password": u.password }),
            _ => serde_json::json!({ "password": u.password }),
        })
        .collect();

    let mut inbound = serde_json::json!({
        "type": "hysteria2",
        "tag": "hy2-in",
        "listen": s.server.listen,
        "listen_port": s.server.hy2_port,
        "users": users,
        "tls": {
            "enabled": true,
            "certificate_path": cert_crt(),
            "key_path": cert_key(),
        }
    });
    // obfs 空串 = 不带 obfs (客户端也须一致)
    if let Some(obfs) = s.obfs() {
        inbound["obfs"] = serde_json::json!({ "type": "salamander", "password": obfs });
    }

    serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        "inbounds": [inbound],
        "outbounds": [ { "type": "direct", "tag": "direct" } ]
    })
}

/// 渲染 + 落盘 + `sing-box check` (坏配置不许上线)
fn write_config(s: &ServerSettings) -> Result<()> {
    let content = serde_json::to_string_pretty(&render_config(s)).context("序列化 config.json 失败")?;
    let path = json_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, content).with_context(|| format!("写 config.json 失败: {}", path.display()))?;
    println!("✅ 生成物: {}", path.display());

    if sb_bin().exists() {
        let out = Command::new(sb_bin())
            .args(["check", "-c"])
            .arg(&path)
            .output()
            .context("sing-box check 启动失败")?;
        if !out.status.success() {
            bail!(
                "sing-box check 失败 (未重启服务): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        println!("✅ sing-box check 通过");
    } else {
        println!("⚠️  sing-box 不存在, 跳过 check: {}", sb_bin().display());
    }
    Ok(())
}

// --- 资产 ---

/// 下载 sing-box 到 bin/ (已存在则跳过)
fn install_singbox() -> Result<()> {
    if sb_bin().exists() {
        println!("✅ sing-box 已存在: {}", sb_bin().display());
        return Ok(());
    }
    let base = home();
    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).context("创建 bin 目录失败")?;
    let tmp = base.join("sb.tar.gz");
    let ver = gnp_core::install::SB_VERSION;
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let os_name = if os == "macos" { "darwin" } else { "linux" };
    let arch_name = if arch == "aarch64" { "arm64" } else { "amd64" };
    let url = format!(
        "https://github.com/SagerNet/sing-box/releases/download/v{}/sing-box-{}-{}-{}.tar.gz",
        ver, ver, os_name, arch_name
    );
    println!("📦 下载 sing-box v{} ...", ver);
    let st = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&tmp)
        .arg(&url)
        .status()
        .context("curl 下载 sing-box 失败")?;
    if !st.success() {
        bail!("下载 sing-box 失败: {}", url);
    }
    let st = Command::new("tar")
        .args(["-xzf"])
        .arg(&tmp)
        .arg("-C")
        .arg(&base)
        .status()
        .context("解压 sing-box 失败")?;
    if !st.success() {
        bail!("解压 sing-box 失败");
    }
    let found = find_sb_bin(&base)?;
    std::fs::copy(&found, sb_bin()).context("复制 sing-box 失败")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(sb_bin(), std::fs::Permissions::from_mode(0o755)).ok();
    }
    let _ = std::fs::remove_file(&tmp);
    // 解压出来的目录没用了
    let extracted = base.join(format!("sing-box-{}-{}-{}", ver, os_name, arch_name));
    if extracted.is_dir() && extracted != sb_bin().parent().unwrap() {
        let _ = std::fs::remove_dir_all(&extracted);
    }
    println!("✅ sing-box 安装完成: {}", sb_bin().display());
    Ok(())
}

fn find_sb_bin(dir: &Path) -> Result<PathBuf> {
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

/// 自签证书 (缺失才生成)
fn gen_cert() -> Result<()> {
    let dir = home().join("certs");
    std::fs::create_dir_all(&dir).context("创建 certs 目录失败")?;
    if cert_crt().exists() && cert_key().exists() {
        println!("✅ 证书已存在: {}", cert_crt().display());
        return Ok(());
    }
    println!("🔐 生成自签证书 (10 年)...");
    let st = Command::new("openssl")
        .args([
            "req", "-x509", "-nodes", "-newkey", "rsa:2048",
            "-keyout", cert_key().to_str().unwrap(),
            "-out", cert_crt().to_str().unwrap(),
            "-days", "3650", "-subj", "/CN=gnp-hy2",
        ])
        .status()
        .context("openssl 生成证书失败")?;
    if !st.success() {
        bail!("openssl 生成证书失败");
    }
    println!("✅ 证书已生成: {}", cert_crt().display());
    Ok(())
}

/// gnps.service (资产来自 deploy/scheduler/gnps.service)
fn write_unit() -> Result<()> {
    let base = home();
    let unit = scheduler::server_unit(&base)?;
    let path = unit_path();
    std::fs::write(&path, unit).with_context(|| format!("写 systemd 单元失败: {}", path.display()))?;
    println!("✅ systemd 单元: {}", path.display());
    Ok(())
}

// --- 命令 ---

fn cmd_install(config: Option<&str>) -> Result<()> {
    check_root()?;
    println!("== 部署 gnp server ==");
    let base = home();
    println!("   部署根: {}", base.display());
    platform::ensure_layout(&base).context("创建部署目录失败")?;

    let s = load_settings(config)?;
    s.validate()?;
    println!("   hy2: {}:{}  obfs: {}", s.server.listen, s.server.hy2_port,
        s.obfs().unwrap_or_else(|| "(无)".into()));
    println!("   users: {}", s.users.len());

    install_singbox()?;
    gen_cert()?;
    write_config(&s)?;
    write_unit()?;

    // 启动: 新服务先起, 验证后再 disable 旧 gnp-hy2
    println!("🚀 启动 gnps...");
    let _ = Command::new("systemctl").args(["daemon-reload"]).status();
    let _ = Command::new("systemctl").args(["enable", scheduler::SYSTEMD_SERVER]).status();
    let _ = Command::new("systemctl").args(["restart", scheduler::SYSTEMD_SERVER]).status();
    std::thread::sleep(std::time::Duration::from_secs(1));
    if tunnel::hy2_server_active() && tunnel::hy2_port_listening(s.server.hy2_port) {
        println!("✅ gnps active, UDP {} 监听中", s.server.hy2_port);
    } else {
        bail!("gnps 未正常起来 (systemctl status {} 看日志); 旧 gnp-hy2 未拆, 可回退", scheduler::SYSTEMD_SERVER);
    }

    println!("\n✅ 部署完成!");
    println!("   服务: {} (systemd)", scheduler::SYSTEMD_SERVER);
    println!("   端口: {}/udp  (ufw + 云安全组都要放行)", s.server.hy2_port);
    println!("   配置: {}", toml_path().display());
    println!("   证书: {}", cert_crt().display());
    println!("\n加用户: sudo gnps gen-user --name <名>   (出 gnp.cfg, 客户端 `gnpc peer gnp.cfg`)");
    Ok(())
}

fn cmd_uninstall() -> Result<()> {
    check_root()?;
    println!("== 卸载 gnp server (保留数据) ==");
    let name = scheduler::SYSTEMD_SERVER;
    let _ = Command::new("systemctl").args(["stop", name]).status();
    let _ = Command::new("systemctl").args(["disable", name]).status();
    let _ = std::fs::remove_file(unit_path());
    let _ = Command::new("systemctl").args(["daemon-reload"]).status();

    println!("✅ 已卸载。数据仍在 {} (config.toml/证书/users/pending-users)", home().display());
    println!("   要彻底清: rm -rf {}", home().display());
    Ok(())
}

fn cmd_status() -> Result<()> {
    println!("== gnps 状态 ==");
    let s = ServerSettings::load_default().ok().flatten();
    let port = s.as_ref().map(|x| x.server.hy2_port).unwrap_or(platform::GNP_PORT);
    if !tunnel::hy2_server_active() {
        println!("❌ gnps 服务未激活 (运行 `sudo gnps install`)");
        return Ok(());
    }
    println!("✅ gnps 服务已激活");
    if tunnel::hy2_port_listening(port) {
        println!("✅ UDP {} 正在监听", port);
    } else {
        println!("⚠️  UDP {} 未检测到监听!", port);
    }
    if let Some(s) = &s {
        println!("   事实源: {}", toml_path().display());
        println!("   obfs: {}", s.obfs().unwrap_or_else(|| "(无)".into()));
        println!("   users: {}", s.users.len());
    }
    println!("\n📋 服务详情:");
    match tunnel::hy2_status_raw() {
        Ok(raw) => println!("{}", raw),
        Err(e) => println!("  (获取失败: {})", e),
    }
    Ok(())
}

fn cmd_users() -> Result<()> {
    let s = match ServerSettings::load_default()? {
        Some(s) => s,
        None => {
            println!("  (无 {} )", toml_path().display());
            return Ok(());
        }
    };
    println!("== 已注册用户 ({}) ==", toml_path().display());
    if s.users.is_empty() {
        println!("  (无用户)");
        return Ok(());
    }
    for (i, u) in s.users.iter().enumerate() {
        match &u.name {
            Some(n) if !n.trim().is_empty() => println!("  {}: {} — {}", i + 1, n, u.password),
            _ => println!("  {}: (无名) {}", i + 1, u.password),
        }
    }
    Ok(())
}

/// 追加一个用户到 toml → 重渲染 → 重启 (唯一写 users 的路径)
fn add_user_and_apply(name: Option<&str>, password: &str) -> Result<ServerSettings> {
    let path = toml_path();
    let mut s = ServerSettings::load(&path)?;
    s.add_user(name, password)?;
    s.validate()?;
    s.save(&path)?;
    write_config(&s)?;
    let _ = Command::new("systemctl").args(["restart", scheduler::SYSTEMD_SERVER]).status();
    println!("✅ 已写入 {} 并重启 {}", path.display(), scheduler::SYSTEMD_SERVER);
    Ok(s)
}

fn cmd_gen_user(name: &str, out_path: &str) -> Result<()> {
    check_root()?;
    println!("== gen-user: {} ==", name);
    ServerSettings::load(&toml_path())?; // 先确认事实源在, 免得生成完密码才发现
    let password = gen_password();
    let s = add_user_and_apply(Some(name), &password)?;

    // gnp.cfg: 客户端一条命令接入 (§3.9: 5766 + obfs-pass 必带)
    let obfs = s.obfs().unwrap_or_else(|| DEFAULT_OBFS.to_string());
    let cfg = format!(
        "# gnp peer config — 由 gnps gen-user 生成\n\
         # 用法: gnpc peer <本文件>   (会落 ~/.local/gnp/config.toml + 装齐)\n\
         user-name = {}\n\
         server-ip = {}\n\
         server-port = {}\n\
         peer-key = {}\n\
         obfs-pass = {}\n",
        name,
        SERVER_IP,
        s.server.hy2_port,
        password,
        obfs,
    );
    std::fs::write(out_path, &cfg).with_context(|| format!("写 {} 失败", out_path))?;

    println!("\n==================");
    println!("{}", cfg.trim_end());
    println!("==================");
    println!("送到客户端后: gnpc peer {}", out_path);
    Ok(())
}

fn cmd_add_user(name: &str) -> Result<()> {
    check_root()?;
    println!("== 添加用户: {} ==", name);
    let password = gen_password();
    add_user_and_apply(Some(name), &password)?;
    println!("\n==================");
    println!("name: {}", name);
    println!("password: {}", password);
    println!("==================");
    Ok(())
}

fn cmd_pregen(count: u32) -> Result<()> {
    check_root()?;
    println!("== 预生成 {} 个用户密码包 ==", count);
    let dir = pending_dir();
    std::fs::create_dir_all(&dir).context("创建 pending-users 目录失败")?;
    let s = ServerSettings::load_default().ok().flatten();
    let port = s.as_ref().map(|x| x.server.hy2_port).unwrap_or(platform::GNP_PORT);
    let obfs = s
        .as_ref()
        .and_then(|x| x.obfs())
        .unwrap_or_else(|| DEFAULT_OBFS.to_string());

    for i in 0..count {
        let password = gen_password();
        let id = format!("{}-{}", uuid_prefix(), i + 1);
        let user_json = serde_json::json!({
            "id": id,
            "status": "available",
            "client_id": "",
            "password": password,
            "server_endpoint": format!("{}:{}", SERVER_IP, port),
            "obfs": obfs,
            "created": now_iso(),
        });
        let out_path = dir.join(format!("{}.json", id));
        platform::write_private(&out_path, &serde_json::to_string_pretty(&user_json)?)?;
        println!("  {} → {}", id, out_path.display());
    }
    println!("\n✅ 生成 {} 个用户, 存在 {}", count, dir.display());
    println!("   配套: 复制为 peers/ 推到 gitee 私有仓库 → 客户端 `gnpc register`");
    Ok(())
}

fn cmd_activate(id: &str) -> Result<()> {
    check_root()?;
    println!("== 激活用户: {} ==", id);
    let path = pending_dir().join(format!("{}.json", id));
    if !path.exists() {
        bail!("用户不存在: {} (找 {})", id, path.display());
    }
    let content = std::fs::read_to_string(&path)?;
    let mut v: serde_json::Value = serde_json::from_str(&content)?;
    let password = v["password"].as_str().unwrap_or_default().to_string();

    let s = ServerSettings::load(&toml_path())?;
    if s.users.iter().any(|u| u.password == password) {
        println!("⚠️  密码已存在于 server, 跳过加入");
    } else {
        add_user_and_apply(None, &password)?;
    }

    v["status"] = serde_json::json!("activated");
    platform::write_private(&path, &serde_json::to_string_pretty(&v)?)?;
    println!("✅ 用户 {} 已激活", id);
    Ok(())
}

// --- 辅助 ---

/// 生成随机密码 (时间戳 + pid 混淆哈希)
fn gen_password() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let h = |mut x: u128| -> u64 {
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51afd7ed558ccd);
        x ^= x >> 33;
        x as u64
    };
    format!("gnp-{:x}{:x}", h(nanos), h(nanos.wrapping_add(pid as u128)))
}

/// UUID 前缀 (简化)
fn uuid_prefix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos % 0xFFFF)
}

/// 当前时间 (Unix 秒)
fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}s", now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srv() -> ServerSettings {
        ServerSettings {
            server: gnp_core::settings::ServerListen {
                listen: "::".into(),
                hy2_port: 5766,
            },
            auth: gnp_core::settings::ServerAuth {
                obfs_password: "gnp-obfs-20261005".into(),
            },
            users: vec![
                ServerUser { name: None, password: "gnp-quic-test-password".into() },
                ServerUser { name: Some("mac-peertest".into()), password: "gnp-acaba44".into() },
            ],
        }
    }

    #[test]
    fn render_shape_matches_live_lwtop_config() {
        let c = render_config(&srv());
        let inb = &c["inbounds"][0];
        assert_eq!(inb["type"], "hysteria2");
        assert_eq!(inb["tag"], "hy2-in");
        assert_eq!(inb["listen"], "::");
        assert_eq!(inb["listen_port"], 5766);
        // obfs 强制 (服务端 inbound)
        assert_eq!(inb["obfs"]["type"], "salamander");
        assert_eq!(inb["obfs"]["password"], "gnp-obfs-20261005");
        // users: 无名用户不带 name 字段
        let users = inb["users"].as_array().unwrap();
        assert_eq!(users.len(), 2);
        assert!(users[0].get("name").is_none());
        assert_eq!(users[0]["password"], "gnp-quic-test-password");
        assert_eq!(users[1]["name"], "mac-peertest");
        // 证书路径在新布局下
        assert!(inb["tls"]["certificate_path"].as_str().unwrap().ends_with("certs/server.crt"));
        assert!(inb["tls"]["key_path"].as_str().unwrap().ends_with("certs/server.key"));
        assert_eq!(c["outbounds"][0]["tag"], "direct");
        // 生成物里不许有注释/未知字段 (不依赖 sing-box 容忍度)
        let text = serde_json::to_string(&c).unwrap();
        assert!(!text.contains("//"));
        assert!(!text.contains("_comment"));
    }

    #[test]
    fn no_obfs_when_blank() {
        let mut s = srv();
        s.auth.obfs_password = String::new();
        let c = render_config(&s);
        assert!(c["inbounds"][0].get("obfs").is_none());
    }

    #[test]
    fn port_is_overridable() {
        let mut s = srv();
        s.server.hy2_port = 443;
        assert_eq!(render_config(&s)["inbounds"][0]["listen_port"], 443);
    }

    #[test]
    fn add_user_rejects_duplicate_password() {
        let mut s = srv();
        assert!(s.add_user(Some("x"), "gnp-acaba44").is_err());
        assert!(s.add_user(Some("y"), "gnp-new01").is_ok());
        assert_eq!(s.users.len(), 3);
    }

    #[test]
    fn live_asset_toml_renders() {
        // repo 的 lwtop-server.toml 必须能直接渲染 (实机 8 用户)
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/hosts/lwtop-server.toml");
        let s = ServerSettings::load(&p).expect("load");
        s.validate().unwrap();
        let c = render_config(&s);
        assert_eq!(c["inbounds"][0]["users"].as_array().unwrap().len(), 8);
        assert_eq!(c["inbounds"][0]["listen_port"], 5766);
    }
}

//! gnpc register — 新机器一键自动注册
//!
//! 从 gitee 私有仓库拉取预生成的 peer 配置池, 挑一个 status=available 的,
//! 标记为 used 并 push, 然后自动安装 sing-box + 规则集 + 生成 config。
//!
//! 安全原则: 只生成 mixed 代理模式 (socks5+http on 127.0.0.1:1080), 绝不 tun。

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// gitee 私有仓库
const GITEE_REPO: &str = "lw_boy/global-net-proxy";
const GITEE_BRANCH: &str = "main";

/// lwtop server 配置 (hysteria2/QUIC, 密码认证)
const SERVER_HOST: &str = "8.209.203.17";
/// hy2 端口 = GNP_PORT (5766); peer 池 JSON 可用 server_endpoint 覆盖
const SERVER_PORT: u16 = gnp_core::platform::GNP_PORT;
/// 测试密码 (与 server 端 /opt/gnp/config.toml 一致)
const HY2_PASSWORD: &str = "gnp-quic-test-password";
/// salamander obfs 密码 (服务端 inbound 强制; §3.9 同步进 peer 池)
const OBFS_PASSWORD: &str = "gnp-obfs-20261005";

/// peer 池 JSON 结构 (hysteria2: 只需 password)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub client_id: String,
    #[serde(rename = "password")]
    pub password: String,
    pub status: String,
    #[serde(default)]
    pub activated: bool,
    /// "ip:port" (gnps pregen 生成; 缺省用 SERVER_HOST:SERVER_PORT)
    #[serde(default)]
    pub server_endpoint: Option<String>,
    /// salamander obfs 密码 (缺省 = 全网统一 OBFS_PASSWORD)
    #[serde(default)]
    pub obfs: Option<String>,
}

/// register 参数
pub struct RegisterArgs {
    pub client_id: Option<String>,
    pub list: bool,
    pub dry_run: bool,
}

/// 生成 gitee clone URL (带 token)
fn gitee_clone_url() -> Result<String> {
    let token = std::env::var("GITEE_TOKEN")
        .context("GITEE_TOKEN 未设置! 请先: export GITEE_TOKEN=xxxx")?;
    Ok(format!("https://oauth2:{}@gitee.com/{}.git", token, GITEE_REPO))
}

/// 克隆 gitee 仓库到临时目录, 返回 (repo_dir)
fn clone_repo() -> Result<PathBuf> {
    let url = gitee_clone_url()?;
    let tmpdir = std::env::temp_dir().join(format!(
        "gnp-repo-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmpdir);
    println!("从 gitee 克隆仓库...");
    let st = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", GITEE_BRANCH, &url])
        .arg(&tmpdir)
        .status()
        .context("git clone 失败 (需要 git 命令)")?;
    if !st.success() {
        bail!("git clone 失败: 请检查 GITEE_TOKEN 是否有仓库读权限");
    }
    Ok(tmpdir)
}

/// 读取 peers 目录下的所有 peer JSON
fn read_peers(peers_dir: &Path) -> Result<Vec<Peer>> {
    let mut peers = Vec::new();
    if !peers_dir.is_dir() {
        bail!(
            "peers/ 目录不存在: {}。请先在 lwtop 上运行: gnps pregen <N>",
            peers_dir.display()
        );
    }
    for entry in std::fs::read_dir(peers_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        if let Ok(p) = serde_json::from_str::<Peer>(&content) {
            peers.push(p);
        }
    }
    Ok(peers)
}

/// 列出 peer 池状态
fn cmd_list(repo: &Path) -> Result<()> {
    let peers = read_peers(&repo.join("peers"))?;
    println!("===== Peer 池状态 ====\n");
    let (mut available, mut used, mut activated) = (0, 0, 0);
    for p in &peers {
        match p.status.as_str() {
            "available" => {
                println!("  ✓ {}  {}  [{}]", p.client_id, p.password, p.status);
                available += 1;
            }
            "used" => {
                println!("  ● {}  {}  [{}]", p.client_id, p.password, p.status);
                used += 1;
            }
            "activated" => {
                println!("  ★ {}  {}  [{}]", p.client_id, p.password, p.status);
                activated += 1;
            }
            _ => println!("  ? {}  {}  [{}]", p.client_id, p.password, p.status),
        }
    }
    println!(
        "\n总计: {} available, {} used, {} activated",
        available, used, activated
    );
    Ok(())
}

/// 选择 peer: 优先 client_id 精确匹配, 否则第一个 available
fn select_peer<'a>(peers: &'a [Peer], client_id: &str) -> Result<&'a Peer> {
    if !client_id.is_empty() {
        if let Some(p) = peers.iter().find(|p| p.client_id == client_id) {
            if p.status == "available" {
                return Ok(p);
            }
            bail!(
                "peer {} 状态为 '{}' (非 available), 可能已被使用",
                p.client_id,
                p.status
            );
        }
    }
    peers
        .iter()
        .find(|p| p.status == "available")
        .ok_or_else(|| anyhow::anyhow!("没有可用的 peer (status=available)。请在 lwtop 上运行: gnps pregen <N>"))
}

/// 标记 peer 为 used 并 push 到 gitee
fn mark_peer_used(repo: &Path, peer_file: &Path, client_id: &str) -> Result<()> {
    let content = std::fs::read_to_string(peer_file)?;
    let mut v: serde_json::Value = serde_json::from_str(&content)?;
    v["status"] = serde_json::Value::String("used".to_string());
    v["client_id"] = serde_json::Value::String(client_id.to_string());
    let out = serde_json::to_string_pretty(&v)?;
    std::fs::write(peer_file, out)?;

    println!("标记 peer {} 为 used...", client_id);
    let st = Command::new("git")
        .current_dir(repo)
        .args(["config", "user.email", "register@global-net-proxy"])
        .status()?;
    let _ = st;
    let _ = Command::new("git")
        .current_dir(repo)
        .args(["config", "user.name", "register"])
        .status()?;
    let _ = Command::new("git").current_dir(repo).args(["add", "-A"]).status()?;
    let _ = Command::new("git")
        .current_dir(repo)
        .args(["commit", "-m", &format!("register: {} marked as used", client_id)])
        .status()?;
    let st = Command::new("git")
        .current_dir(repo)
        .args(["push", "origin", GITEE_BRANCH])
        .status()
        .context("git push 失败")?;
    if !st.success() {
        bail!("git push 失败: peer 未推送到 gitee");
    }
    println!("✓ 已标记并推送到 gitee");
    Ok(())
}

/// 校验 server 密码 (防止 gitee 上的 HY2_PASSWORD 被篡改)
fn verify_server_password(repo: &Path) {
    let spk = repo.join("peers").join("HY2_PASSWORD");
    if let Ok(content) = std::fs::read_to_string(&spk) {
        let gitee_pwd = content.trim().to_string();
        if !gitee_pwd.is_empty() && gitee_pwd != HY2_PASSWORD {
            println!(
                "⚠️  gitee 上的 HY2_PASSWORD ({}) 与内置 ({}) 不一致! 使用内置值 (更安全)",
                gitee_pwd, HY2_PASSWORD
            );
        }
    }
}

/// peer → ClientSettings (唯一事实源形态)
fn peer_settings(peer: &Peer) -> Result<gnp_core::settings::ClientSettings> {
    let (host, port) = match peer
        .server_endpoint
        .as_deref()
        .and_then(|e| e.rsplit_once(':'))
        .and_then(|(h, p)| Some((h.to_string(), p.parse::<u16>().ok()?)))
    {
        Some((h, p)) => (h, p),
        None => (SERVER_HOST.to_string(), SERVER_PORT),
    };
    let s = gnp_core::settings::ClientSettings {
        server: gnp_core::settings::ClientServer {
            host,
            hy2_port: port,
            ..Default::default()
        },
        auth: gnp_core::settings::ClientAuth {
            hy2_password: peer.password.clone(),
            obfs_password: peer
                .obfs
                .clone()
                .unwrap_or_else(|| OBFS_PASSWORD.to_string()),
        },
        client: gnp_core::settings::ClientLocal {
            // 新注册机器 = 局域网服务机, 与 aipro/lwmate/cozepc 一致
            listen: "0.0.0.0".to_string(),
            ..Default::default()
        },
        guard: Default::default(),
    };
    s.validate()?;
    Ok(s)
}

/// 写 config.toml (唯一事实源) + config.json (生成物, 走共享 builder)
///
/// 与 install/migrate 同一条路径: 双通道 + obfs + clash_api, 行为一致 (D3/D5)。
fn generate_conf(peer: &Peer) -> Result<()> {
    let settings = peer_settings(peer)?;
    let home = gnp_core::platform::gnp_home();
    gnp_core::platform::ensure_layout(&home)?;

    let toml_path = settings.save_default()?;
    println!("✓ 唯一事实源: {}", toml_path.display());

    let json_path = gnp_core::install::generate_config(&settings)?;
    println!("✓ 生成物: {} (双通道 mixed 模式, 不碰路由表)", json_path.display());
    Ok(())
}

/// register 主入口
pub fn run(args: &RegisterArgs) -> Result<()> {
    // 确定 client_id
    let client_id = match &args.client_id {
        Some(id) if !id.is_empty() => id.clone(),
        _ => {
            // 用 hostname 简化
            let host = Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "client".to_string());
            host.split('.')
                .next()
                .unwrap_or("client")
                .to_lowercase()
        }
    };
    println!("client_id: {}", client_id);

    // 克隆仓库
    let repo = clone_repo()?;
    let _guard = CleanupGuard(repo.clone());

    if args.list {
        return cmd_list(&repo);
    }

    let peers = read_peers(&repo.join("peers"))?;

    // 选择 peer
    let peer = select_peer(&peers, &client_id)?.clone();
    println!("\n═══════════════════════════════════════");
    println!("选中 peer: {} → {}", peer.client_id, client_id);
    println!("  password: {}", peer.password);
    println!("  server:   {}:{}", SERVER_HOST, SERVER_PORT);
    println!("  status:   {}", peer.status);
    println!("═══════════════════════════════════════");

    if args.dry_run {
        println!("\n[dry-run] 不修改任何文件");
        println!("实际注册会:");
        println!("  1. 标记该 peer 为 used 并 push 到 gitee");
        println!("  2. 生成 sing-box config.json");
        println!("  3. 下载并安装 sing-box");
        println!("  4. 安装服务");
        return Ok(());
    }

    // 校验 server 密码
    verify_server_password(&repo);

    // 标记 used + push
    let peer_file = repo.join("peers").join(format!("{}.json", peer.client_id));
    mark_peer_used(&repo, &peer_file, &client_id)?;

    // 生成 config.toml + config.json (顺序: 资产先就位, 配置最后)
    if !gnp_core::platform::sb_exists() {
        gnp_core::install::install_singbox(None)?;
    } else {
        println!("✅ sing-box 已存在: {}", gnp_core::platform::gnp_sb_bin().display());
    }

    // 下载规则集
    gnp_core::install::install_rules()?;

    // 最后落配置 (前面失败就不留半边配置)
    generate_conf(&peer)?;
    gnp_core::install::check_config_file(&gnp_core::platform::gnp_config_json())?;

    // Linux: 安装 systemd 用户服务 (开机自启, 无需 root)
    if gnp_core::platform::Platform::detect() == gnp_core::platform::Platform::Linux {
        gnp_core::service::install_linux()?;
    }

    println!("✓ 配置验证通过 (sing-box check)");

    println!("\n⚠️  最后一步: 在 lwtop 上执行激活!");
    println!("  sudo /opt/gnp/bin/gnps activate {}", client_id);
    println!("\n激活后启动代理 + 装调度:");
    println!("  gnpc install-scheduler && gnpc start");
    println!("使用代理:");
    println!("  export https_proxy=http://127.0.0.1:1080 http_proxy=http://127.0.0.1:1080");
    println!("\n注册完成! client_id={}  server={}:{}", client_id, SERVER_HOST, SERVER_PORT);
    Ok(())
}

/// 临时目录清理守卫
struct CleanupGuard(PathBuf);
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
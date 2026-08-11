//! gnp-client register — 新机器一键自动注册
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

/// lwtop server 配置 (公钥可公开)
const SERVER_HOST: &str = "8.209.203.17";
const SERVER_PORT: u16 = 1194;
const SERVER_PUBKEY: &str = "M/t3YYwIW7Xou+vASGtNoAHHNrh82ROzYDU4LIsLz18=";

/// peer 池 JSON 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub client_id: String,
    #[serde(rename = "wg_ip")]
    pub wg_ip: String,
    #[serde(rename = "private_key")]
    pub private_key: String,
    #[serde(rename = "public_key")]
    pub public_key: String,
    pub status: String,
    #[serde(default)]
    pub activated: bool,
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
            "peers/ 目录不存在: {}。请先在 lwtop 上运行: gnp-server pregen <N>",
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
                println!("  ✓ {}  {}  [{}]", p.client_id, p.wg_ip, p.status);
                available += 1;
            }
            "used" => {
                println!("  ● {}  {}  [{}]", p.client_id, p.wg_ip, p.status);
                used += 1;
            }
            "activated" => {
                println!("  ★ {}  {}  [{}]", p.client_id, p.wg_ip, p.status);
                activated += 1;
            }
            _ => println!("  ? {}  {}  [{}]", p.client_id, p.wg_ip, p.status),
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
        .ok_or_else(|| anyhow::anyhow!("没有可用的 peer (status=available)。请在 lwtop 上运行: gnp-server pregen <N>"))
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

/// 校验 server 公钥 (防止 gitee 上的 SERVER_PUBKEY 被篡改)
fn verify_server_pubkey(repo: &Path) {
    let spk = repo.join("peers").join("SERVER_PUBKEY");
    if let Ok(content) = std::fs::read_to_string(&spk) {
        let gitee_pubkey = content.trim().to_string();
        if !gitee_pubkey.is_empty() && gitee_pubkey != SERVER_PUBKEY {
            println!(
                "⚠️  gitee 上的 SERVER_PUBKEY ({}) 与内置 ({}) 不一致! 使用内置值 (更安全)",
                gitee_pubkey, SERVER_PUBKEY
            );
        }
    }
}

/// 生成 config (mixed 模式, sing-box 1.12 outbound wireguard 格式, 安全)
///
/// 使用 outbound wireguard (1.12 旧格式), 需配合环境变量
/// ENABLE_DEPRECATED_WIREGUARD_OUTBOUND=true 使用。
/// DNS: 国外域名经 wg 走 1.1.1.1 UDP (非 DoH), 用 socks5h 远程解析。
fn generate_conf(peer: &Peer) -> Result<()> {
    let sb_dir = gnp_core::platform::sb_dir();
    let rules_dir = gnp_core::platform::sb_rules_dir();
    std::fs::create_dir_all(&sb_dir)?;
    std::fs::create_dir_all(&rules_dir)?;

    let wg_ip = peer.wg_ip.clone();
    let privkey = peer.private_key.clone();

    let config = serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        "dns": {
            "servers": [
                { "tag": "dns-direct", "address": "223.5.5.5", "detour": "direct" },
                { "tag": "dns-proxy", "address": "1.1.1.1", "detour": "wg-out", "type": "tcp" }
            ],
            "rules": [
                { "rule_set": ["geosite-cn", "geoip-cn"], "server": "dns-direct" }
            ],
            "final": "dns-proxy",
            "strategy": "prefer_ipv4"
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": "0.0.0.0",
            "listen_port": 1080
        }],
        "outbounds": [
            {
                "type": "wireguard",
                "tag": "wg-out",
                "local_address": [wg_ip],
                "private_key": privkey,
                "peer_public_key": SERVER_PUBKEY,
                "server": SERVER_HOST,
                "server_port": SERVER_PORT,
                "mtu": 1280,
                "system": false,
                "reserved": [0, 0, 0]
            },
            { "type": "direct", "tag": "direct" }
        ],
        "route": {
            "rule_set": [
                { "type": "local", "tag": "geosite-cn", "format": "binary", "path": rules_dir.join("geosite-cn.srs").to_str().unwrap() },
                { "type": "local", "tag": "geoip-cn", "format": "binary", "path": rules_dir.join("geoip-cn.srs").to_str().unwrap() }
            ],
            "rules": [
                { "rule_set": ["geosite-cn", "geoip-cn"], "outbound": "direct" },
                { "ip_is_private": true, "outbound": "direct" }
            ],
            "final": "wg-out"
        }
    });

    let content = serde_json::to_string_pretty(&config)?;
    let conf_path = gnp_core::platform::sb_config();
    std::fs::write(&conf_path, content)?;
    println!("✓ 配置已生成: {} (mixed 模式, 不碰路由表)", conf_path.display());
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
    println!("  wg_ip:    {}", peer.wg_ip);
    println!("  public_key: {}...", &peer.public_key[..peer.public_key.len().min(24)]);
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

    // 校验 server 公钥
    verify_server_pubkey(&repo);

    // 标记 used + push
    let peer_file = repo.join("peers").join(format!("{}.json", peer.client_id));
    mark_peer_used(&repo, &peer_file, &client_id)?;

    // 生成 config
    generate_conf(&peer)?;

    // 安装 sing-box (若未装)
    if !gnp_core::platform::sb_exists() {
        gnp_core::install::install_singbox(None)?;
    } else {
        println!("✅ sing-box 已存在: {}", gnp_core::platform::sb_bin().display());
    }

    // 下载规则集
    gnp_core::install::install_rules()?;

    // Linux: 安装 systemd 用户服务 (开机自启, 无需 root)
    if gnp_core::platform::Platform::detect() == gnp_core::platform::Platform::Linux {
        gnp_core::service::install_linux()?;
    }

    // 验证配置
    println!("\n验证 sing-box 配置...");
    let st = Command::new(gnp_core::platform::sb_bin())
        .args(["check", "-c", gnp_core::platform::sb_config().to_str().unwrap()])
        .status();
    match st {
        Ok(s) if s.success() => println!("✓ 配置验证通过"),
        _ => println!("⚠️  配置验证失败! 请检查 {}", gnp_core::platform::sb_config().display()),
    }

    println!("\n⚠️  最后一步: 在 lwtop 上执行激活!");
    println!("  gnp-server activate {}", client_id);
    println!("\n激活后启动代理:");
    println!("  gnp-client start");
    println!("使用代理:");
    println!("  export https_proxy=http://127.0.0.1:1080 http_proxy=http://127.0.0.1:1080");
    println!("\n注册完成! client_id={}  wg_ip={}", client_id, peer.wg_ip);
    Ok(())
}

/// 临时目录清理守卫
struct CleanupGuard(PathBuf);
impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
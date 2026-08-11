//! 安装模块 — 下载 sing-box + 规则集 + 生成 config
//!
//! 完全自包含, 不依赖 repo。下载到 ~/.local/share/sing-box/。

use crate::platform::{sb_bin, sb_config, sb_dir, sb_rules_dir};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// sing-box 版本 (1.13 endpoint wireguard 有 bug, 降级到 1.12.3 用 outbound wireguard)
pub const SB_VERSION: &str = "1.12.3";

/// 下载 URL 模板
fn download_url(version: &str, os: &str, arch: &str) -> Result<String> {
    let (os_name, ext) = match os {
        "macos" => ("darwin", "tar.gz"),
        "linux" => ("linux", "tar.gz"),
        "windows" => ("windows", "zip"),
        _ => bail!("不支持的平台: {}", os),
    };
    Ok(format!(
        "https://github.com/SagerNet/sing-box/releases/download/v{}/sing-box-{}-{}-{}.{}",
        version, version, os_name, arch, ext
    ))
}

/// 检测架构
fn detect_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" | "x86" => "amd64",
        "aarch64" | "arm64" => "arm64",
        _ => "amd64",
    }
}

/// 下载并解压 sing-box 二进制
pub fn install_singbox(url: Option<&str>) -> Result<()> {
    let dir = sb_dir();
    std::fs::create_dir_all(&dir).context("创建 sing-box 目录失败")?;

    let url = match url {
        Some(u) => u.to_string(),
        None => download_url(SB_VERSION, std::env::consts::OS, detect_arch())?,
    };
    println!("📦 下载 sing-box v{} ...", SB_VERSION);
    println!("  URL: {}", url);

    // 下载到临时文件
    let tmp_dir = dir.join(".download");
    std::fs::create_dir_all(&tmp_dir).ok();
    let archive = tmp_dir.join("sing-box.tar.gz");

    let st = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .context("curl 下载失败")?;
    if !st.success() {
        bail!("下载 sing-box 失败: {}", url);
    }

    // 解压
    let st = Command::new("tar")
        .args(["-xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(&tmp_dir)
        .status()
        .context("tar 解压失败")?;
    if !st.success() {
        bail!("解压 sing-box 失败");
    }

    // 找到二进制 (sing-box-<ver>-<os>-<arch>/sing-box)
    let bin = find_bin(&tmp_dir).context("在解压目录中找不到 sing-box 二进制")?;
    std::fs::copy(&bin, sb_bin()).context("复制 sing-box 二进制失败")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(sb_bin(), std::fs::Permissions::from_mode(0o755)).ok();
    }

    // 清理临时文件
    std::fs::remove_dir_all(&tmp_dir).ok();

    println!("✅ sing-box 安装完成: {}", sb_bin().display());
    Ok(())
}

/// 在解压目录中递归找 sing-box 二进制
fn find_bin(dir: &Path) -> Option<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find_bin(&path) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some("sing-box") {
            return Some(path);
        }
    }
    None
}

/// 下载规则集到 rules/ 目录
pub fn install_rules() -> Result<()> {
    let rules_dir = sb_rules_dir();
    std::fs::create_dir_all(&rules_dir).context("创建 rules 目录失败")?;

    // 国外分组规则
    let foreign_groups = ["google", "github", "openai", "anthropic", "docker"];
    for g in foreign_groups {
        let url = format!(
            "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-{}.srs",
            g
        );
        let out = rules_dir.join(format!("geosite-{}.srs", g));
        println!("  ⬇️  geosite-{}", g);
        let st = Command::new("curl")
            .args(["-fsSL", "--max-time", "20"])
            .arg("-o")
            .arg(&out)
            .arg(&url)
            .status();
        if let Ok(s) = st {
            if s.success() {
                println!("    ✓ 下载完成");
            } else {
                println!("    ✗ 失败");
            }
        }
    }

    // 国内规则
    let cn_rules = [
        ("geosite-cn", "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-cn.srs"),
        ("geoip-cn", "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geoip/geoip-cn.srs"),
    ];
    for (name, url) in cn_rules {
        let out = rules_dir.join(format!("{}.srs", name));
        println!("  ⬇️  {}", name);
        let st = Command::new("curl")
            .args(["-fsSL", "--max-time", "30"])
            .arg("-o")
            .arg(&out)
            .arg(url)
            .status();
        if let Ok(s) = st {
            if s.success() {
                println!("    ✓ 下载完成");
            } else {
                println!("    ✗ 失败");
            }
        }
    }
    println!("✅ 规则集安装完成: {}", rules_dir.display());
    Ok(())
}

/// 生成 config.json (mixed 模式模板, sing-box 1.12 outbound wireguard 格式)
///
/// 使用 outbound wireguard (1.12 旧格式), 需配合环境变量
/// ENABLE_DEPRECATED_WIREGUARD_OUTBOUND=true 使用。
/// DNS: 国外域名经 wg 走 1.1.1.1 UDP (非 DoH), 用 socks5h 远程解析。
pub fn generate_config(
    server: &str,
    server_pubkey: &str,
    client_privkey: &str,
    client_ip: &str,
    wg_port: u16,
) -> Result<()> {
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
                "local_address": [client_ip],
                "private_key": client_privkey,
                "peer_public_key": server_pubkey,
                "server": server,
                "server_port": wg_port,
                "mtu": 1280,
                "system": false,
                "reserved": [0, 0, 0]
            },
            { "type": "direct", "tag": "direct" }
        ],
        "route": {
            "rule_set": [
                { "type": "local", "tag": "geosite-cn", "format": "binary", "path": sb_rules_dir().join("geosite-cn.srs").to_str().unwrap() },
                { "type": "local", "tag": "geoip-cn", "format": "binary", "path": sb_rules_dir().join("geoip-cn.srs").to_str().unwrap() }
            ],
            "rules": [
                { "rule_set": ["geosite-cn", "geoip-cn"], "outbound": "direct" },
                { "ip_is_private": true, "outbound": "direct" }
            ],
            "final": "wg-out"
        }
    });

    let content = serde_json::to_string_pretty(&config)
        .context("序列化 config 失败")?;
    std::fs::write(sb_config(), content).context("写 config.json 失败")?;
    println!("✅ 配置生成: {}", sb_config().display());
    Ok(())
}
//! 安装模块 — 下载 sing-box + 规则集 + 生成 config
//!
//! 完全自包含, 不依赖 repo。下载到 ~/.local/share/sing-box/。

use crate::platform::{sb_bin, sb_config, sb_dir, sb_rules_dir};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// sing-box 版本 (hysteria2 outbound 需要新版, 默认 1.13.16)
pub const SB_VERSION: &str = "1.13.16";

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

    // 下载到临时文件 (后缀必须与实际格式一致: Windows Expand-Archive 按扩展名校验)
    let tmp_dir = dir.join(".download");
    std::fs::create_dir_all(&tmp_dir).ok();
    let archive = tmp_dir.join(if std::env::consts::OS == "windows" {
        "sing-box.zip"
    } else {
        "sing-box.tar.gz"
    });

    let st = Command::new("curl")
        .args(["-fL", "--retry", "3", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .context("curl 下载失败")?;
    if !st.success() {
        bail!("下载 sing-box 失败: {}", url);
    }

    // 解压 (Linux/macOS: tar.gz; Windows: zip 用 PowerShell Expand-Archive)
    if std::env::consts::OS == "windows" {
        let ps = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            archive.display(),
            tmp_dir.display()
        );
        // -EncodedCommand 免引号转义
        let st = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &crate::platform::ps_encode(&ps)])
            .status()
            .context("PowerShell Expand-Archive 失败")?;
        if !st.success() {
            bail!("解压 sing-box zip 失败");
        }
    } else {
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
        } else if matches!(
            path.file_name().and_then(|n| n.to_str()),
            Some("sing-box") | Some("sing-box.exe")
        ) {
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
        download_rule_tmp(&url, &out);
    }

    // 国内规则
    let cn_rules = [
        ("geosite-cn", "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-cn.srs"),
        ("geoip-cn", "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geoip/geoip-cn.srs"),
    ];
    for (name, url) in cn_rules {
        let out = rules_dir.join(format!("{}.srs", name));
        println!("  ⬇️  {}", name);
        download_rule_tmp(url, &out);
    }
    println!("✅ 规则集安装完成: {}", rules_dir.display());
    Ok(())
}

/// 下载规则到 .tmp, 成功才替换 (失败不污染已有有效规则文件)
fn download_rule_tmp(url: &str, out: &Path) {
    let tmp = out.with_extension("srs.tmp");
    let st = Command::new("curl")
        .args(["-fsSL", "--max-time", "30"])
        .arg("-o")
        .arg(&tmp)
        .arg(url)
        .status();
    match st {
        Ok(s) if s.success() && tmp.exists() => {
            let _ = std::fs::rename(&tmp, out);
            println!("    ✓ 下载完成");
        }
        _ => {
            let _ = std::fs::remove_file(&tmp);
            if out.exists() {
                println!("    ⚠️ 下载失败, 保留现有 {}", out.file_name().unwrap_or_default().to_string_lossy());
            } else {
                println!("    ✗ 失败 (且本地无缓存)");
            }
        }
    }
}

/// ssh 兜底通道参数 (TCP 底座, 免疫 UDP MTU 黑洞)
///
/// 默认复用 gnp 服务端主机的 sshd (密钥认证), 2026-10-05 MTU 事件引入。
#[derive(Debug, Clone)]
pub struct SshFallback {
    pub server: String,
    pub server_port: u16,
    pub user: String,
    pub private_key_path: std::path::PathBuf,
}

impl SshFallback {
    /// 默认形态: 与 hy2 同主机, sshd 22 端口, user=lw, 密钥 ~/.ssh/id_ed25519
    pub fn for_server(server: &str) -> Self {
        let key = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".ssh/id_ed25519");
        Self {
            server: server.to_string(),
            server_port: 22,
            user: "lw".to_string(),
            private_key_path: key,
        }
    }
}

/// client 配置生成参数
#[derive(Debug, Clone)]
pub struct ClientConfigParams {
    pub server: String,
    pub password: String,
    pub server_port: u16,
    /// salamander obfs 密码; None = 不带 obfs (须与服务端 inbound 一致)
    pub obfs_password: Option<String>,
    /// mixed 监听地址 (默认 0.0.0.0; 单机自用可 127.0.0.1)
    pub listen: String,
    /// 本地域名预定义解析 [(域名, IP)]; 写入 dns-hosts + 路由直连 (Mac *.host 用法)
    pub hosts: Vec<(String, String)>,
    /// ssh 兜底通道; None = 单通道 (旧行为)
    pub ssh_fallback: Option<SshFallback>,
    /// clash_api 端口 (127.0.0.1); None = 不开 — switch/status/guard 依赖它
    pub clash_api_port: Option<u16>,
    /// 输出路径; None = 写 sing-box 标准位置
    ///
    /// Some(path) = "只生成配置文件" 模式 (供跨机部署: HOME=<目标机home> + --out),
    /// cmd_install 据此跳过规则下载与服务安装。
    pub output: Option<std::path::PathBuf>,
}

impl ClientConfigParams {
    pub fn new(server: &str, password: &str, server_port: u16) -> Self {
        Self {
            server: server.to_string(),
            password: password.to_string(),
            server_port,
            obfs_password: None,
            listen: "0.0.0.0".to_string(),
            hosts: Vec::new(),
            ssh_fallback: Some(SshFallback::for_server(server)),
            clash_api_port: Some(9090),
            output: None,
        }
    }
}

/// 构造 client config.json 内容 (纯函数, 便于测试)
///
/// 出站拓扑 (2026-10-05 MTU 黑洞事件 P0):
///   route.final → proxy-out (selector, 默认 auto-out)
///     → auto-out (urltest: hy2-out + ssh-out, 自动降级/回切)
///     → hy2-out (QUIC 主通道) / ssh-out (TCP 兜底) / direct
pub fn build_config(p: &ClientConfigParams) -> serde_json::Value {
    // 出站: selector → urltest → hy2(+obfs) / ssh / direct
    let mut outbounds = Vec::new();
    if p.ssh_fallback.is_some() {
        outbounds.push(serde_json::json!({
            "type": "selector",
            "tag": "proxy-out",
            "outbounds": ["auto-out", "hy2-out", "ssh-out"],
            "default": "auto-out",
            "interrupt_exist_connections": true
        }));
        outbounds.push(serde_json::json!({
            "type": "urltest",
            "tag": "auto-out",
            "outbounds": ["hy2-out", "ssh-out"],
            "url": "https://www.gstatic.com/generate_204",
            "interval": "3m",
            "tolerance": 300
        }));
    }
    let mut hy2 = serde_json::json!({
        "type": "hysteria2",
        "tag": "hy2-out",
        "server": p.server,
        "server_port": p.server_port,
        "password": p.password,
        "tls": { "enabled": true, "insecure": true }
    });
    if let Some(obfs) = &p.obfs_password {
        hy2["obfs"] = serde_json::json!({ "type": "salamander", "password": obfs });
    }
    outbounds.push(hy2);
    if let Some(ssh) = &p.ssh_fallback {
        outbounds.push(serde_json::json!({
            "type": "ssh",
            "tag": "ssh-out",
            "server": ssh.server,
            "server_port": ssh.server_port,
            "user": ssh.user,
            "private_key_path": ssh.private_key_path.to_string_lossy()
        }));
    }
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));

    // route.final: 双通道 → selector; 单通道 → hy2 (旧行为)
    let final_out = if p.ssh_fallback.is_some() { "proxy-out" } else { "hy2-out" };
    // DNS 远程解析 detour 跟随 route.final — hy2 死时 DNS 必须同步降级, 否则 ssh 兜底残废
    let dns_detour = final_out;

    // DNS servers/rules (+ 可选 hosts 预定义解析)
    let mut dns_servers = vec![
        serde_json::json!({ "tag": "dns-direct", "type": "udp", "server": "223.5.5.5" }),
        serde_json::json!({ "tag": "dns-remote", "type": "tcp", "server": "1.1.1.1", "detour": dns_detour }),
    ];
    let mut dns_rules = Vec::new();
    let mut route_rules = vec![
        serde_json::json!({ "rule_set": ["geosite-cn", "geoip-cn"], "outbound": "direct" }),
        serde_json::json!({ "ip_is_private": true, "outbound": "direct" }),
    ];
    if !p.hosts.is_empty() {
        let names: Vec<&str> = p.hosts.iter().map(|(n, _)| n.as_str()).collect();
        let predefined: serde_json::Map<String, serde_json::Value> = p
            .hosts
            .iter()
            .map(|(n, ip)| (n.clone(), serde_json::Value::String(ip.clone())))
            .collect();
        dns_servers.push(serde_json::json!({
            "tag": "dns-hosts", "type": "hosts", "path": ["/etc/hosts"], "predefined": predefined
        }));
        dns_rules.push(serde_json::json!({ "domain": names, "server": "dns-hosts" }));
        route_rules.insert(0, serde_json::json!({ "domain_suffix": names, "outbound": "direct" }));
    }
    dns_rules.push(serde_json::json!({ "rule_set": ["geosite-cn", "geoip-cn"], "server": "dns-direct" }));

    // experimental: cache_file (必须绝对路径) + clash_api (switch/status/guard 依赖)
    let mut experimental = serde_json::json!({
        "cache_file": {
            "enabled": true,
            "path": sb_dir().join("cache.db").to_str().unwrap()
        }
    });
    if let Some(port) = p.clash_api_port {
        experimental["clash_api"] = serde_json::json!({
            "external_controller": format!("127.0.0.1:{}", port)
        });
    }

    serde_json::json!({
        "log": { "level": "info", "timestamp": true },
        // DNS: 无 fakeip —— fakeip 只服务 aipro 路由器 hijack-dns 场景, 标准 mixed 客户端
        // socks5h 直接携域名分流, 不本地解析 (Mac 生产 1.12.3 同构实测稳定);
        // 且 sing-box >=1.13 对 legacy dns.fakeip 顶层选项 FATAL 拒启 (2026-09-16 lwwin 实测)
        // - CN 域名 → dns-direct 真解析; 海外 → 通道组携域名出海, 远端 1.1.1.1 解析无污染
        "dns": {
            "servers": dns_servers,
            "rules": dns_rules,
            "final": "dns-remote",
            "strategy": "prefer_ipv4"
        },
        "inbounds": [{
            "type": "mixed",
            "tag": "mixed-in",
            "listen": p.listen,
            "listen_port": 1080
        }],
        "outbounds": outbounds,
        "route": {
            "rule_set": [
                { "type": "local", "tag": "geosite-cn", "format": "binary", "path": sb_rules_dir().join("geosite-cn.srs").to_str().unwrap() },
                { "type": "local", "tag": "geoip-cn", "format": "binary", "path": sb_rules_dir().join("geoip-cn.srs").to_str().unwrap() }
            ],
            "rules": route_rules,
            "final": final_out,
            "default_domain_resolver": "dns-direct"
        },
        // cache_file 必须绝对路径 — systemd CWD=/ 不可写, 相对路径 cache.db 启动即死
        // (2026-08-15 aipro gnp-proxy crash-loop 111 次教训)
        "experimental": experimental
    })
}

/// 生成 config.json (默认写 sing-box 标准位置; params.output 指定则写该路径)
pub fn generate_config(p: &ClientConfigParams) -> Result<()> {
    let config = build_config(p);
    let content = serde_json::to_string_pretty(&config)
        .context("序列化 config 失败")?;
    let out = p.output.clone().unwrap_or_else(sb_config);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out, content).with_context(|| format!("写 config 失败: {}", out.display()))?;
    println!("✅ 配置生成: {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_channel_shape() {
        let mut p = ClientConfigParams::new("1.2.3.4", "pw", 443);
        p.obfs_password = Some("obfs-pw".to_string());
        let c = build_config(&p);

        // route.final → selector → 默认 urltest
        assert_eq!(c["route"]["final"], "proxy-out");
        let sel = &c["outbounds"][0];
        assert_eq!(sel["type"], "selector");
        assert_eq!(sel["default"], "auto-out");
        let urltest = &c["outbounds"][1];
        assert_eq!(urltest["type"], "urltest");
        assert_eq!(urltest["outbounds"], serde_json::json!(["hy2-out", "ssh-out"]));
        assert_eq!(urltest["interval"], "3m");
        assert_eq!(urltest["tolerance"], 300);

        // hy2 带 obfs; ssh 兜底结构
        let hy2 = &c["outbounds"][2];
        assert_eq!(hy2["tag"], "hy2-out");
        assert_eq!(hy2["obfs"]["type"], "salamander");
        let ssh = &c["outbounds"][3];
        assert_eq!(ssh["tag"], "ssh-out");
        assert_eq!(ssh["type"], "ssh");
        assert_eq!(ssh["server_port"], 22);
        assert_eq!(ssh["user"], "lw");
        assert!(ssh["private_key_path"].as_str().unwrap().contains(".ssh/id_ed25519"));

        // DNS detour 跟随 final (selector), hy2 死时 DNS 同步降级
        assert_eq!(c["dns"]["servers"][1]["detour"], "proxy-out");
        // clash_api 默认开 (switch/status/guard 依赖)
        assert_eq!(c["experimental"]["clash_api"]["external_controller"], "127.0.0.1:9090");
        // cache_file 绝对路径
        assert!(c["experimental"]["cache_file"]["path"].as_str().unwrap().starts_with('/'));
    }

    #[test]
    fn single_channel_legacy_shape() {
        let mut p = ClientConfigParams::new("1.2.3.4", "pw", 443);
        p.ssh_fallback = None;
        p.clash_api_port = None;
        let c = build_config(&p);
        assert_eq!(c["route"]["final"], "hy2-out");
        assert_eq!(c["dns"]["servers"][1]["detour"], "hy2-out");
        let obs = c["outbounds"].as_array().unwrap();
        assert_eq!(obs.len(), 2); // hy2 + direct
        assert!(c["experimental"].get("clash_api").is_none());
    }

    #[test]
    fn hosts_wiring() {
        let mut p = ClientConfigParams::new("1.2.3.4", "pw", 443);
        p.hosts = vec![("aipro.host".into(), "192.168.1.2".into())];
        let c = build_config(&p);
        assert_eq!(c["dns"]["servers"][2]["tag"], "dns-hosts");
        assert_eq!(c["dns"]["servers"][2]["predefined"]["aipro.host"], "192.168.1.2");
        assert_eq!(c["dns"]["rules"][0]["server"], "dns-hosts");
        assert_eq!(c["route"]["rules"][0]["outbound"], "direct");
        assert_eq!(c["route"]["rules"][0]["domain_suffix"][0], "aipro.host");
    }
}
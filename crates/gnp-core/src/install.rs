//! 安装模块 — 下载 sing-box + 规则集 + 生成 config
//!
//! 目录收敛到 `~/.local/gnp/` (客户端) — bin/ rules/ var/ secrets/ backups/。
//! 配置形态: config.toml (唯一事实源) → config.json (生成物)。

use crate::platform::{gnp_config_json, gnp_rules_dir, gnp_sb_bin, gnp_var_dir};
use crate::settings::ClientSettings;
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

/// 本地代理地址 (curl 走它下 GitHub)
pub const LOCAL_PROXY: &str = "socks5h://127.0.0.1:1080";

/// 下载一个文件: 先直连, 直连不通再走本地代理 (127.0.0.1:1080)
///
/// 为什么要代理兜底: raw.githubusercontent.com / github release 在国内直连不通,
/// 而这些机器**唯一稳定的出站路径就是自己的代理**。直连失败就借代理,
/// 否则 tick 每晚 rules-update 都会 WARN (§7.1 验收 3 要求"无持续 WARN")。
/// 返回实际用的通道 ("direct" / "proxy"), 供日志显示。
pub fn fetch(url: &str, out: &Path, max_time_s: u64) -> Result<&'static str> {
    let try_direct = || -> bool {
        Command::new("curl")
            .args(["-fsSL", "--max-time", &max_time_s.to_string(), "-o"])
            .arg(out)
            .arg(url)
            .status()
            .map(|s| s.success())
            .unwrap_or(false) && out.exists()
    };
    if try_direct() {
        return Ok("direct");
    }
    if crate::platform::port_open(1080) {
        let ok = Command::new("curl")
            .args([
                "-fsSL",
                "--max-time",
                &max_time_s.to_string(),
                "-x",
                LOCAL_PROXY,
                "-o",
            ])
            .arg(out)
            .arg(url)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && out.exists();
        if ok {
            return Ok("proxy");
        }
    }
    Ok("failed")
}

/// 下载并解压 sing-box 二进制
pub fn install_singbox(url: Option<&str>) -> Result<()> {
    let dir = crate::platform::gnp_bin_dir();
    std::fs::create_dir_all(&dir).context("创建 bin 目录失败")?;

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

    match fetch(&url, &archive, 300)? {
        "direct" => println!("  ↘ 直连"),
        "proxy" => println!("  ↘ 直连不通, 经本地代理 {} 下载", LOCAL_PROXY),
        _ => bail!("下载 sing-box 失败 (直连与代理都不通): {}", url),
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
    std::fs::copy(&bin, gnp_sb_bin()).context("复制 sing-box 二进制失败")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(gnp_sb_bin(), std::fs::Permissions::from_mode(0o755)).ok();
    }

    // 清理临时文件
    std::fs::remove_dir_all(&tmp_dir).ok();

    println!("✅ sing-box 安装完成: {}", gnp_sb_bin().display());
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
    let rules_dir = gnp_rules_dir();
    std::fs::create_dir_all(&rules_dir).context("创建 rules 目录失败")?;

    // 国外分组规则
    let foreign_groups = ["google", "github", "openai", "anthropic", "docker"];
    let mut ok = 0;
    let mut total = 0;
    for g in foreign_groups {
        let url = format!(
            "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set-geosite/geosite-{}.srs",
            g
        );
        let out = rules_dir.join(format!("geosite-{}.srs", g));
        println!("  ⬇️  geosite-{}", g);
        total += 1;
        if download_rule_tmp(&url, &out) {
            ok += 1;
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
        total += 1;
        if download_rule_tmp(url, &out) {
            ok += 1;
        }
    }
    println!("✅ 规则集安装完成: {}/{} 成功 — {}", ok, total, rules_dir.display());
    if ok == 0 {
        // 全军覆没: tick 会把非零退出落成 WARN, 这是对的事实 (规则彻底不新鲜)
        bail!("规则集全部下载失败 (直连与本地代理 {} 都不通)", LOCAL_PROXY);
    }
    Ok(())
}

/// 下载规则到 .tmp, 成功才替换 (失败不污染已有有效规则文件)
///
/// 通道: 直连优先, 不通则经本地代理 (见 [`fetch`]) —— 否则每晚 tick 都会 WARN。
/// 返回是否拿到新文件。
fn download_rule_tmp(url: &str, out: &Path) -> bool {
    let tmp = out.with_extension("srs.tmp");
    match fetch(url, &tmp, 60) {
        Ok("direct") => {
            let _ = std::fs::rename(&tmp, out);
            println!("    ✓ 下载完成 (直连)");
            true
        }
        Ok("proxy") => {
            let _ = std::fs::rename(&tmp, out);
            println!("    ✓ 下载完成 (经本地代理 {})", LOCAL_PROXY);
            true
        }
        _ => {
            let _ = std::fs::remove_file(&tmp);
            if out.exists() {
                println!(
                    "    ⚠️ 下载失败 (直连与代理都不通), 保留现有 {}",
                    out.file_name().unwrap_or_default().to_string_lossy()
                );
            } else {
                println!("    ✗ 失败 (且本地无缓存)");
            }
            false
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

/// 构造 client config.json 内容 (纯函数, 便于测试)
///
/// 入参 = config.toml 唯一事实源 (`ClientSettings`)。**生成逻辑零变更**:
/// 双通道/clash_api/hosts/dns-detour 跟随组等 2026-10-05 已实测形态, 勿动。
///
/// 出站拓扑 (2026-10-05 MTU 黑洞事件 P0):
///   route.final → proxy-out (selector, 默认 auto-out)
///     → auto-out (urltest: hy2-out + ssh-out, 自动降级/回切)
///     → hy2-out (QUIC 主通道) / ssh-out (TCP 兜底) / direct
pub fn build_config(p: &ClientSettings) -> serde_json::Value {
    let ssh_fallback = p.ssh_fallback();
    let hosts = p.hosts();

    // 出站: selector → urltest → hy2(+obfs) / ssh / direct
    let mut outbounds = Vec::new();
    if ssh_fallback.is_some() {
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
        "server": p.server.host,
        "server_port": p.server.hy2_port,
        "password": p.auth.hy2_password,
        "tls": { "enabled": true, "insecure": true }
    });
    if let Some(obfs) = p.obfs() {
        hy2["obfs"] = serde_json::json!({ "type": "salamander", "password": obfs });
    }
    outbounds.push(hy2);
    if let Some(ssh) = &ssh_fallback {
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
    let final_out = if ssh_fallback.is_some() { "proxy-out" } else { "hy2-out" };
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
    if !hosts.is_empty() {
        let names: Vec<&str> = hosts.iter().map(|(n, _)| n.as_str()).collect();
        let predefined: serde_json::Map<String, serde_json::Value> = hosts
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
            "path": gnp_var_dir().join("cache.db").to_str().unwrap()
        }
    });
    if let Some(port) = p.clash_api() {
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
            "listen": p.client.listen,
            "listen_port": 1080
        }],
        "outbounds": outbounds,
        "route": {
            "rule_set": [
                { "type": "local", "tag": "geosite-cn", "format": "binary", "path": gnp_rules_dir().join("geosite-cn.srs").to_str().unwrap() },
                { "type": "local", "tag": "geoip-cn", "format": "binary", "path": gnp_rules_dir().join("geoip-cn.srs").to_str().unwrap() }
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

/// 渲染 config.json 文本 (纯函数)
pub fn render_config(p: &ClientSettings) -> Result<String> {
    let config = build_config(p);
    serde_json::to_string_pretty(&config).context("序列化 config 失败")
}

/// 生成 config.json 到 `out` (缺省 `$GNP_HOME/config.json` = 生成物)
pub fn generate_config_to(p: &ClientSettings, out: Option<&Path>) -> Result<std::path::PathBuf> {
    let content = render_config(p)?;
    let out = out
        .map(|x| x.to_path_buf())
        .unwrap_or_else(gnp_config_json);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&out, content).with_context(|| format!("写 config 失败: {}", out.display()))?;
    Ok(out)
}

/// 生成 config.json (默认写生成物位置)
pub fn generate_config(p: &ClientSettings) -> Result<std::path::PathBuf> {
    let out = generate_config_to(p, None)?;
    println!("✅ 配置生成: {}", out.display());
    Ok(out)
}

/// `sing-box check` 校验 config.json (迁移/安装必须过 — 坏配置不许顶掉旧服务)
pub fn check_config_file(cfg: &Path) -> Result<()> {
    let sb = crate::platform::gnp_sb_bin();
    if !sb.exists() {
        bail!("sing-box 不存在, 无法 check: {}", sb.display());
    }
    let out = Command::new(&sb)
        .args(["check", "-c"])
        .arg(cfg)
        .output()
        .context("sing-box check 启动失败")?;
    if !out.status.success() {
        bail!(
            "sing-box check 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ClientAuth, ClientLocal, ClientServer};
    use std::collections::BTreeMap;

    /// 双通道默认 settings (server 8.209.203.17:5766 + obfs + ssh 兜底)
    fn dual() -> ClientSettings {
        ClientSettings {
            server: ClientServer {
                host: "1.2.3.4".to_string(),
                hy2_port: 5766,
                ssh_port: 22,
                ssh_user: "lw".to_string(),
                ssh_key: "~/.ssh/id_ed25519".to_string(),
            },
            auth: ClientAuth {
                hy2_password: "pw".to_string(),
                obfs_password: "obfs-pw".to_string(),
            },
            client: ClientLocal::default(),
            guard: Default::default(),
        }
    }

    #[test]
    fn dual_channel_shape() {
        let c = build_config(&dual());

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
        assert_eq!(hy2["server"], "1.2.3.4");
        assert_eq!(hy2["server_port"], 5766);
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
        // cache_file 绝对路径 (坑清单 #1)
        assert!(c["experimental"]["cache_file"]["path"].as_str().unwrap().starts_with('/'));
        // cache_file 在 var/ 下
        assert!(c["experimental"]["cache_file"]["path"]
            .as_str()
            .unwrap()
            .ends_with("var/cache.db"));
    }

    #[test]
    fn single_channel_legacy_shape() {
        let mut p = dual();
        p.server.ssh_user = String::new(); // 空 ssh_user = 关兜底
        p.client.clash_api_port = 0; // 0 = 关 clash_api
        let c = build_config(&p);
        assert_eq!(c["route"]["final"], "hy2-out");
        assert_eq!(c["dns"]["servers"][1]["detour"], "hy2-out");
        let obs = c["outbounds"].as_array().unwrap();
        assert_eq!(obs.len(), 2); // hy2 + direct
        assert!(c["experimental"].get("clash_api").is_none());
    }

    #[test]
    fn hosts_wiring() {
        let mut p = dual();
        let mut hosts = BTreeMap::new();
        hosts.insert("aipro.host".to_string(), "192.168.1.2".to_string());
        p.client.hosts = hosts;
        let c = build_config(&p);
        assert_eq!(c["dns"]["servers"][2]["tag"], "dns-hosts");
        assert_eq!(c["dns"]["servers"][2]["predefined"]["aipro.host"], "192.168.1.2");
        assert_eq!(c["dns"]["rules"][0]["server"], "dns-hosts");
        assert_eq!(c["route"]["rules"][0]["outbound"], "direct");
        assert_eq!(c["route"]["rules"][0]["domain_suffix"][0], "aipro.host");
    }

    #[test]
    fn listen_and_port_from_settings() {
        let mut p = dual();
        p.client.listen = "127.0.0.1".to_string();
        p.server.hy2_port = 443;
        let c = build_config(&p);
        assert_eq!(c["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(c["inbounds"][0]["listen_port"], 1080);
        assert_eq!(c["outbounds"][2]["server_port"], 443);
    }

    #[test]
    fn no_obfs_when_blank() {
        let mut p = dual();
        p.auth.obfs_password = String::new();
        let c = build_config(&p);
        assert!(c["outbounds"][2].get("obfs").is_none());
    }
}

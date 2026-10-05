//! Hysteria2 (QUIC) 隧道诊断
//!
//! 诊断信息来自:
//! - client 端: 通过代理访问出口 IP 检测 (curl 到 ipinfo.io)
//! - server 端: 检查 sing-box hysteria2 服务 (systemd `gnps`) + UDP hy2 端口监听
//!
//! v2: 服务名 `gnps` (旧名 gnp-hy2), 端口 `GNP_PORT`=5766 (旧 443)。
//!
//! client 端 sing-box 是 userspace hysteria2 outbound, 没有内核接口,
//! 所以只能通过 HTTP 检测出口 IP。

use anyhow::{Context, Result};
use std::process::Command;

/// 通过代理检测出口 IP
/// 返回 (出口IP, 延迟ms)
pub fn detect_exit_ip(proxy: &str, timeout_s: u64) -> Result<(String, u64)> {
    let start = std::time::Instant::now();
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            &timeout_s.to_string(),
            "-x",
            proxy,
            "https://ipinfo.io/ip",
        ])
        .output()
        .context("curl 失败 (需要 curl 命令)")?;
    let elapsed = start.elapsed().as_millis() as u64;
    let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if ip.is_empty() || !ip.contains('.') {
        anyhow::bail!("代理出口检测失败 (无输出或非 IP)");
    }
    Ok((ip, elapsed))
}

/// 测试代理是否可用 (返回 HTTP 状态码)
pub fn test_proxy(proxy: &str, url: &str, timeout_s: u64) -> Result<(String, u64)> {
    let start = std::time::Instant::now();
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            &timeout_s.to_string(),
            "-x",
            proxy,
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            url,
        ])
        .output()
        .context("curl 失败")?;
    let elapsed = start.elapsed().as_millis() as u64;
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((code, elapsed))
}

/// 检查 server 端 sing-box hysteria2 服务是否激活
///
/// systemd `gnps` 状态, 或 (迁移窗口内) 旧名 `gnp-hy2`。
///
/// **只认 systemd**: pgrep 兜底会误判 —— 客户端 sing-box 也在跑, 会让
/// "服务端活着" 显示成 ✅ (Mac 上尤其明显)。服务端判定必须看服务状态。
pub fn hy2_server_active() -> bool {
    for name in [crate::scheduler::SYSTEMD_SERVER, "gnp-hy2"] {
        if let Ok(out) = Command::new("systemctl").args(["is-active", name]).output() {
            if String::from_utf8_lossy(&out.stdout).trim() == "active" {
                return true;
            }
        }
    }
    false
}

/// 运行 `systemctl status <name>` 获取原始输出 (server 端)
pub fn hy2_status_raw() -> Result<String> {
    let name = crate::scheduler::SYSTEMD_SERVER;
    let out = Command::new("systemctl")
        .args(["status", name, "--no-pager", "-l"])
        .output()
        .context(format!("systemctl status {} 失败 (需要 root)", name))?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl status {} 失败: {}",
            name,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 检查 server 端 UDP hy2 端口是否监听 (默认 GNP_PORT=5766)
pub fn hy2_port_listening(port: u16) -> bool {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("ss -ulnp | grep -q ':{} '", port))
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// 将 Unix 时间戳转为可读时间
pub fn ts_to_readable(ts: u64) -> String {
    if ts == 0 {
        return "从未握手".to_string();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ago = now.saturating_sub(ts);
    format!("{} 秒前 ({}s)", ago, ts)
}
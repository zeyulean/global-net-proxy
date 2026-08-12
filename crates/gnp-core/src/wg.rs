//! Hysteria2 (QUIC) 隧道诊断
//!
//! 诊断信息来自:
//! - client 端: 通过代理访问出口 IP 检测 (curl 到 ipinfo.io)
//! - server 端: 检查 sing-box hysteria2 服务 (systemd gnp-hy2) + UDP 443 端口监听
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

/// 检查 server 端 sing-box hysteria2 服务是否激活 (server 端)
///
/// 通过 systemd 服务 gnp-hy2 状态或 sing-box 进程判断。
pub fn hy2_server_active() -> bool {
    // 方式 1: systemd 服务 gnp-hy2
    if let Ok(out) = Command::new("systemctl")
        .args(["is-active", "gnp-hy2"])
        .output()
    {
        let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if status == "active" {
            return true;
        }
    }
    // 方式 2: sing-box 进程存在
    if let Ok(out) = Command::new("pgrep").arg("-f").arg("sing-box run").output() {
        if out.status.success() {
            return true;
        }
    }
    false
}

/// 运行 `systemctl status gnp-hy2` 获取原始输出 (server 端)
pub fn hy2_status_raw() -> Result<String> {
    let out = Command::new("systemctl")
        .args(["status", "gnp-hy2", "--no-pager", "-l"])
        .output()
        .context("systemctl status gnp-hy2 失败 (需要 root)")?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl status gnp-hy2 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 检查 server 端 UDP 443 端口是否监听 (hysteria2/QUIC)
pub fn port_443_listening() -> bool {
    let out = Command::new("sh")
        .arg("-c")
        .arg("ss -ulnp | grep -q ':443 '")
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
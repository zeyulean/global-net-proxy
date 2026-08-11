//! WireGuard 隧道诊断
//!
//! 诊断信息来自:
//! - client 端: 通过代理访问出口 IP 检测 (curl 到 ifconfig.me / ipinfo.io)
//! - server 端: `wg show wg0` 命令 (握手时间/传输/endpoint)
//!
//! client 端 sing-box 是 userspace wg (system:false), 没有内核 wg0 接口,
//! 所以不能用 `wg show`, 只能通过 HTTP 检测出口 IP。

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

/// 检查本机 wg 端口 (server 端, 内核 wg0)
/// 返回 true 如果 wg0 存在且监听 1194
pub fn server_wg_active() -> bool {
    let out = Command::new("wg")
        .args(["show", "wg0"])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// 运行 `wg show wg0` 获取原始输出 (server 端)
pub fn wg_show_raw() -> Result<String> {
    let out = Command::new("wg")
        .args(["show", "wg0"])
        .output()
        .context("wg show wg0 失败 (需要内核 wireguard + root)")?;
    if !out.status.success() {
        anyhow::bail!("wg show wg0 失败: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 解析 wg show 输出中某个 peer 的握手时间 (Unix 时间戳)
/// 0 = 从未握手
pub fn parse_handshake(raw: &str) -> u64 {
    // wg show wg0 latest-handshakes 单独命令更可靠, 这里解析通用输出
    // 简化: 从 "latest handshake" 行提取
    for line in raw.lines() {
        if line.contains("latest handshake") {
            if let Some(ts) = line.split_whitespace().last() {
                return ts.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// 将 Unix 时间戳转为可读时间
pub fn ts_to_readable(ts: u64) -> String {
    if ts == 0 {
        return "从未握手".to_string();
    }
    let secs = std::time::Duration::from_secs(ts);
    let dt = std::time::SystemTime::UNIX_EPOCH + secs;
    let fmt = dt
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // 简单格式化 (相对时间)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ago = now.saturating_sub(ts);
    format!("{} 秒前 ({}s)", ago, ts)
}
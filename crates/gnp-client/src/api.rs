//! clash_api 交互 (sing-box experimental.clash_api, 默认 127.0.0.1:9090)
//!
//! switch / status / guard 共用。curl 实现 (与 tunnel/proxy 模块同风格, 免新依赖)。

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::process::Command;

/// 从 config.json 读 clash_api 地址; 未开启返回 None (旧配置)
pub fn controller_addr() -> Option<String> {
    let cfg = gnp_core::platform::sb_config();
    if !cfg.exists() {
        return None;
    }
    gnp_core::config::load(&cfg)
        .ok()
        .and_then(|v| gnp_core::config::clash_api_addr(&v))
}

pub fn api_get(addr: &str, path: &str, timeout_s: u64) -> Result<String> {
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            &timeout_s.to_string(),
            &format!("http://{}/{}", addr.trim_end_matches('/'), path.trim_start_matches('/')),
        ])
        .output()
        .context("curl 失败")?;
    if !out.status.success() {
        bail!("clash_api GET {} 失败 (curl 退出码 {:?})", path, out.status.code());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// 解析 JSON, 失败带上下文 (API down 时报错可读)
pub fn api_get_json(addr: &str, path: &str, timeout_s: u64) -> Result<Value> {
    let body = api_get(addr, path, timeout_s)?;
    serde_json::from_str(&body).with_context(|| format!("clash_api {} 返回非 JSON: {}", path, body))
}

/// PUT (selector 热切换用); 非 2xx 报错
pub fn api_put(addr: &str, path: &str, body: &str) -> Result<()> {
    let out = Command::new("curl")
        .args([
            "-s",
            "-m",
            "3",
            "-X",
            "PUT",
            "-d",
            body,
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            &format!("http://{}/{}", addr.trim_end_matches('/'), path.trim_start_matches('/')),
        ])
        .output()
        .context("curl 失败")?;
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !code.starts_with('2') {
        bail!("clash_api PUT {} 失败 (HTTP {})", path, code);
    }
    Ok(())
}

/// 探测某个出站的实时延迟 (clash_api delay 接口), 成功返回 ms
///
/// 失败 = 该出站当前不可用 (超时/握手失败)。
pub fn probe_delay(addr: &str, outbound_tag: &str, timeout_ms: u64) -> Result<u64> {
    let url = "https%3A%2F%2Fwww.gstatic.com%2Fgenerate_204";
    let path = format!(
        "/proxies/{}/delay?timeout={}&url={}",
        outbound_tag, timeout_ms, url
    );
    let body = api_get(addr, &path, (timeout_ms / 1000) + 3)?;
    let v: Value = serde_json::from_str(&body)
        .with_context(|| format!("delay 返回非 JSON: {}", body))?;
    v.get("delay")
        .and_then(|d| d.as_u64())
        .context("delay 响应缺 delay 字段")
}

//! clash_api 交互 (sing-box experimental.clash_api, 默认 127.0.0.1:9090)
//!
//! switch / status / guard 共用。curl 实现 (与 tunnel/proxy 模块同风格, 免新依赖)。

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::process::Command;

/// 从 config.json 读 clash_api 地址; 未开启返回 None (旧配置)
pub fn controller_addr() -> Option<String> {
    let cfg = gnp_core::platform::gnp_config_json();
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

/// 取 clash_api `/proxies` 的**内层** map (顶层是 `{"proxies": {...}}`)
///
/// ⚠️ 2026-10-05 修: 之前各处都拿顶层对象直接 `get("proxy-out")` —— 永远 None,
/// 于是 `status` 的通道显示、`switch` 的当前通道、以及 guard 的
/// "hy2 健康但 urltest 卡在 ssh → 强制 hy2" (坑清单 #9) 全部静默失效。
/// 一律走这个函数, 别再手撸 api_get_json("/proxies")。
pub fn proxies_map(addr: &str, timeout_s: u64) -> Result<Map<String, Value>> {
    let body = api_get_json(addr, "/proxies", timeout_s)?;
    body.get("proxies")
        .and_then(|p| p.as_object())
        .cloned()
        .with_context(|| format!("/proxies 响应缺少 proxies 对象: {}", body))
}

/// 某个出站当前生效的目标 (selector/urltest 的 `now`)
pub fn proxy_now(proxies: &Map<String, Value>, tag: &str) -> Option<String> {
    proxies
        .get(tag)
        .and_then(|p| p.get("now"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

/// 某个出站最近一次探测延迟 ms (没有历史 = 未探测/失败)
pub fn proxy_delay_ms(proxies: &Map<String, Value>, tag: &str) -> Option<u64> {
    proxies
        .get(tag)
        .and_then(|p| p.get("history"))
        .and_then(|h| h.as_array())
        .and_then(|arr| arr.last())
        .and_then(|e| e.get("delay"))
        .and_then(|d| d.as_u64())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// clash_api 真实响应形状 (顶层裹一层 "proxies")
    const SAMPLE: &str = r#"{"proxies":{
        "GLOBAL":{"type":"Selector","now":"proxy-out"},
        "proxy-out":{"type":"Selector","now":"auto-out"},
        "auto-out":{"type":"URLTest","now":"hy2-out"},
        "hy2-out":{"type":"Hysteria2","history":[{"delay":125}]},
        "ssh-out":{"type":"Ssh","history":[{"delay":1309}]}
    }}"#;

    #[test]
    fn proxies_are_nested_one_level() {
        let v: Value = serde_json::from_str(SAMPLE).unwrap();
        // 旧写法 (顶层直接 get) 必然 None —— 这就是那个 bug
        assert!(v.get("proxy-out").is_none());
        let m = v.get("proxies").and_then(|p| p.as_object()).unwrap();
        assert_eq!(proxy_now(m, "proxy-out").as_deref(), Some("auto-out"));
        assert_eq!(proxy_now(m, "auto-out").as_deref(), Some("hy2-out"));
        assert_eq!(proxy_delay_ms(m, "hy2-out"), Some(125));
        assert_eq!(proxy_delay_ms(m, "ssh-out"), Some(1309));
        // 没有 history = 未探测/失败 → None (不是 0)
        assert_eq!(proxy_delay_ms(m, "auto-out"), None);
    }
}

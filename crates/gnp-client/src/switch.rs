//! gnp-client switch — 手动通道切换 (manual override)
//!
//! urltest(auto-out) 按"最快"自动选路, 不保证 hy2 优先; switch 提供 manual override:
//!   auto → 交还 urltest 自动选路 (默认)
//!   hy2  → 强制 hy2-out (QUIC)
//!   ssh  → 强制 ssh-out (TCP 兜底)
//!
//! 实现 = clash_api 热切换 (立即生效) + 写回 config selector default (重启后仍生效,
//! cache_file 未开 store_selected, config default 是重启后的权威值)。
//! API 不可用且 sing-box 未运行时, 退化为改配置 + 重启。

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::api;
use gnp_core::platform;

/// switch 目标 → 出站 tag
pub fn resolve_target(word: &str) -> Result<&'static str> {
    match word.to_ascii_lowercase().as_str() {
        "auto" => Ok("auto-out"),
        "hy2" | "hysteria2" | "quic" => Ok("hy2-out"),
        "ssh" => Ok("ssh-out"),
        other => bail!("未知通道 '{}' (可选: auto | hy2 | ssh)", other),
    }
}

/// 找 selector 出站 tag (默认 proxy-out)
fn selector_tag(v: &serde_json::Value) -> Result<String> {
    gnp_core::config::find_outbound(v, "selector")
        .and_then(|ob| ob.get("tag").and_then(|t| t.as_str()))
        .map(|s| s.to_string())
        .context("配置中没有 selector 出站 (旧配置? 重刷: gnp-client install)")
}

/// 切换 selector 到目标出站: API 热切换 + config default 写回
///
/// 返回实际生效方式 ("api+config" / "config+restart")。
pub fn set_selector(target: &str) -> Result<String> {
    let cfg_path = platform::sb_config();
    let mut v = gnp_core::config::load(&cfg_path)?;
    let sel_tag = selector_tag(&v)?;

    // 校验目标在 selector 组里
    let members: Vec<String> = gnp_core::config::find_outbound(&v, &sel_tag)
        .and_then(|ob| ob.get("outbounds").and_then(|o| o.as_array()))
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if !members.iter().any(|m| m == target) {
        bail!("{} 不在 selector {} 组内 (成员: {:?})", target, sel_tag, members);
    }

    // 1) 热切换 (立即生效)
    let mut mode = None;
    if let Some(addr) = api::controller_addr() {
        if api::api_put(&addr, &format!("/proxies/{}", sel_tag), &json!({ "name": target }).to_string()).is_ok() {
            mode = Some("api");
            println!("🔥 热切换生效: {} → {} (clash_api)", sel_tag, target);
        }
    }

    // 2) 写回 config default (重启后仍生效)
    if let Some(sel) = gnp_core::config::find_outbound_mut(&mut v, &sel_tag) {
        sel["default"] = json!(target);
    }
    gnp_core::config::save(&cfg_path, &v)?;
    println!("💾 config selector default 已写回: {}", target);

    match mode {
        Some(_) => Ok("api+config".to_string()),
        None => {
            // API 不可用: 若进程也没跑, 改完配置重启即可; 若在跑则是旧配置 (无 clash_api)
            let p = platform::ensure_supported()?;
            if platform::config_exists() && !g_core_running(p) {
                println!("⚠️  clash_api 不可用且 sing-box 未运行 → 改配置后重启");
                gnp_core::service::start(p)?;
                Ok("config+restart".to_string())
            } else {
                bail!(
                    "clash_api 不可用 (旧配置无 clash_api 或 sing-box 异常)。\
                     重刷配置后重试: gnp-client install"
                )
            }
        }
    }
}

fn g_core_running(p: platform::Platform) -> bool {
    gnp_core::service::is_running(p).unwrap_or(false)
}

/// 显示当前通道选择状态
pub fn show_current() -> Result<()> {
    let cfg_path = platform::sb_config();
    let v = gnp_core::config::load(&cfg_path)?;
    let sel_tag = selector_tag(&v)?;
    let sel = gnp_core::config::find_outbound(&v, &sel_tag);
    let default = sel
        .and_then(|s| s.get("default").and_then(|d| d.as_str()))
        .unwrap_or("?");

    println!("== 通道切换 ==");
    println!("  route.final: {}", gnp_core::config::final_outbound(&v).unwrap_or_default());
    println!("  {} 默认: {} (auto=自动选路, hy2=强制 QUIC, ssh=强制 TCP 兜底)", sel_tag, default);

    if let Some(addr) = api::controller_addr() {
        if let Ok(proxies) = api::api_get_json(&addr, "/proxies", 2) {
            if let Some(now) = proxies
                .get(&sel_tag)
                .and_then(|p| p.get("now"))
                .and_then(|n| n.as_str())
            {
                println!("  当前生效 (热): {}", now);
            }
            if let Some(auto) = proxies.get("auto-out").and_then(|p| p.get("now")).and_then(|n| n.as_str()) {
                println!("  urltest(auto-out) 选中: {}", auto);
            }
        }
    } else {
        println!("  (旧配置无 clash_api, 热状态不可见 — 重刷: gnp-client install)");
    }
    println!("\n用法: gnp-client switch <auto|hy2|ssh>");
    Ok(())
}

/// 入口
pub fn run(target: Option<String>) -> Result<()> {
    match target {
        None => show_current(),
        Some(word) => {
            let tag = resolve_target(&word)?;
            println!("== 切换通道: {} → {} ==", word, tag);
            set_selector(tag)?;
            println!("✅ 完成 (switch auto 可交还 urltest 自动选路)");
            Ok(())
        }
    }
}
